//! HTTP and WebSocket client for the actnet homeserver API.
//!
//! This crate provides a typed client for every homeserver endpoint. It handles
//! JSON serialization, base64 encoding of key material and ciphertext, and
//! bearer token authentication. `app-core` uses this to talk to the server;
//! the crypto and store crates remain I/O-free.
//!
//! The `ws` module provides a WebSocket client for real-time message delivery
//! via `GET /v1/ws?token=<session_token>`.
//!
//! ## Authentication
//!
//! Authenticated requests use lazy challenge/response. Callers configure a
//! [`Signer`] via [`Client::with_signer`]; the client performs the
//! challenge/response handshake on the first authenticated call (and again
//! transparently on any HTTP 401 response). Token state is internal to the
//! `Client`.

pub mod error;
pub mod groups;
pub mod types;
pub mod ws;

/// Generated protobuf types for the `/v1/ws` framing.
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/actnet.ws.rs"));
}

use std::sync::{Arc, Mutex};

use base64::prelude::*;
use error::NetError;
use types::*;

/// Produces Ed25519 signatures over server-issued challenge nonces.
///
/// The implementation lives in `app-core` (`IdentitySigner`) and captures the
/// device's identity private key. Kept abstract here so `net` doesn't depend
/// on crypto/store types.
pub trait Signer: Send + Sync {
    /// Sign a challenge nonce. The nonce is the raw decoded bytes returned by
    /// the server's `/v1/auth/challenge` endpoint.
    fn sign(&self, nonce: &[u8]) -> Result<Vec<u8>, String>;
}

/// Per-account authentication state held by the `Client`.
struct AuthState {
    did: String,
    device_id: u32,
    signer: Arc<dyn Signer>,
    token: Option<String>,
}

/// HTTP client for a single homeserver.
pub struct Client {
    http: reqwest::Client,
    server_url: String,
    /// `None` until `with_signer` is called. The inner `Option<String>` is the
    /// session token, populated lazily by `ensure_authenticated`.
    ///
    /// We use `std::sync::Mutex` (not `tokio::sync::Mutex`) because we never
    /// hold the lock across an `await` — the double-check re-auth pattern
    /// releases it during network I/O.
    auth: Mutex<Option<AuthState>>,
}

impl Client {
    /// Create a new client. Use `.with_signer(...)` to enable authenticated
    /// requests. Unauthenticated endpoints (register, resolve_did, etc.) work
    /// without a signer.
    pub fn new(server_url: &str) -> Self {
        // Bound both the TCP connect and the overall request so a stale
        // network path (typical after an app resumes from suspension) can't
        // leave an authenticated call — e.g. the lazy challenge/response in
        // `ensure_authenticated`, which gates WS connect — hung indefinitely.
        // Defense-in-depth alongside the connect timeout in
        // `app-core::connection::try_connect_ws`.
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self {
            http,
            server_url: server_url.trim_end_matches('/').to_string(),
            auth: Mutex::new(None),
        }
    }

    /// Configure this client for authenticated requests. Lazy: challenge/
    /// response runs on the first authenticated call. Pass a pre-fetched
    /// `initial_token` (e.g. from `register`) to skip the first round trip.
    pub fn with_signer(
        self,
        did: String,
        device_id: u32,
        signer: Arc<dyn Signer>,
        initial_token: Option<String>,
    ) -> Self {
        *self.auth.lock().unwrap() = Some(AuthState {
            did,
            device_id,
            signer,
            token: initial_token,
        });
        self
    }

    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    /// Borrow the underlying reqwest client. Crate-private; only the
    /// `groups` module needs it to build presentation-authenticated
    /// requests that bypass the bearer-token plumbing.
    pub(crate) fn http_client(&self) -> &reqwest::Client {
        &self.http
    }

    /// Get the current session token, if set. Used by the WebSocket client.
    pub fn token(&self) -> Option<String> {
        self.auth.lock().unwrap().as_ref().and_then(|a| a.token.clone())
    }

    /// Acquire a session token via challenge/response if we don't have one.
    /// Idempotent. Safe under concurrent callers (the double-check pattern
    /// means at most one wasted nonce request per race).
    pub async fn ensure_authenticated(&self) -> Result<(), NetError> {
        // Fast path: token already present.
        {
            let auth = self.auth.lock().unwrap();
            match auth.as_ref() {
                Some(a) if a.token.is_some() => return Ok(()),
                None => return Err(NetError::NoSigner),
                _ => {}
            }
        }
        // Slow path: extract what we need under the lock, release, then do I/O.
        let (did, device_id, signer) = {
            let auth = self.auth.lock().unwrap();
            let a = auth.as_ref().ok_or(NetError::NoSigner)?;
            if a.token.is_some() { return Ok(()); }
            (a.did.clone(), a.device_id, a.signer.clone())
        };

        let nonce = self.challenge(&did, device_id as i32).await?;
        let nonce_bytes = BASE64_URL_SAFE_NO_PAD
            .decode(&nonce)
            .map_err(|e| NetError::Base64(e.to_string()))?;
        let sig = signer.sign(&nonce_bytes).map_err(NetError::Signing)?;
        let resp = self.authenticate(&did, device_id as i32, &nonce, &sig).await?;

        let mut auth = self.auth.lock().unwrap();
        if let Some(a) = auth.as_mut() {
            // Another caller may have set it concurrently; first-write wins.
            if a.token.is_none() {
                a.token = Some(resp.session_token);
            }
        }
        Ok(())
    }

    /// Drop the cached token. Called internally on 401 before re-auth.
    fn clear_token(&self) {
        if let Some(a) = self.auth.lock().unwrap().as_mut() {
            a.token = None;
        }
    }

    /// Issue an authenticated request. Builds the request via `build`, sends
    /// it, and on HTTP 401 transparently re-authenticates and retries once.
    ///
    /// `build` receives a fresh `RequestBuilder` with the bearer token already
    /// applied; the closure adds headers, body, query params, etc.
    async fn send_authed<F>(
        &self,
        method: reqwest::Method,
        path: &str,
        build: F,
    ) -> Result<reqwest::Response, NetError>
    where
        F: Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    {
        let url = format!("{}{}", self.server_url, path);

        self.ensure_authenticated().await?;
        let token = self.token().ok_or(NetError::NoSigner)?;
        let resp = build(self.http.request(method.clone(), &url).bearer_auth(&token))
            .send()
            .await?;
        if resp.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Ok(resp);
        }

        // Token rejected — drop, re-auth, retry exactly once.
        self.clear_token();
        self.ensure_authenticated().await?;
        let token = self.token().ok_or(NetError::NoSigner)?;
        Ok(build(self.http.request(method, &url).bearer_auth(&token))
            .send()
            .await?)
    }

    // ── Account ──────────────────────────────────────────────────────────

    /// Register a new account. Returns DID and session token.
    pub async fn register(&self, req: &RegisterRequest) -> Result<RegisterResponse, NetError> {
        let body = serde_json::json!({
            "did": req.did,
            "identity_key": BASE64_STANDARD.encode(&req.identity_key),
            "registration_id": req.registration_id,
            "device_id": req.device_id,
            "signed_prekey": {
                "id": req.signed_prekey_id,
                "public_key": BASE64_STANDARD.encode(&req.signed_prekey_public),
                "signature": BASE64_STANDARD.encode(&req.signed_prekey_signature),
            },
            "one_time_prekeys": req.one_time_prekeys.iter().map(|(id, pk)| {
                serde_json::json!({"id": id, "public_key": BASE64_STANDARD.encode(pk)})
            }).collect::<Vec<_>>(),
            "kyber_prekey": {
                "id": req.kyber_prekey_id,
                "public_key": BASE64_STANDARD.encode(&req.kyber_prekey_public),
                "signature": BASE64_STANDARD.encode(&req.kyber_prekey_signature),
            },
            "display_name": req.display_name,
            "is_bot": req.is_bot,
            "did_suffix": req.did_suffix,
            "recovery_blob": req.recovery_blob.as_ref().map(|b| BASE64_STANDARD.encode(b)),
            "encrypted_profile": req.encrypted_profile.as_ref().map(|b| BASE64_STANDARD.encode(b)),
            "identity_key_signature": req.identity_key_signature,
            "invite_token": req.invite_token,
        });

        let resp = self.http
            .post(format!("{}/v1/accounts", self.server_url))
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(resp.json().await?)
    }

    /// Permanently delete the authenticated account on this server
    /// (`DELETE /v1/accounts`). Server hard-deletes the account and all its data
    /// (devices, prekeys, queue, DID document, profile, …) and returns 204.
    /// Used by the Leave-server / Delete-identity flows (docs/53).
    pub async fn delete_account(&self) -> Result<(), NetError> {
        let resp = self
            .send_authed(reqwest::Method::DELETE, "/v1/accounts", |b| b)
            .await?;

        if !resp.status().is_success() {
            return Err(NetError::Server(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(())
    }

    /// Validate an invite token against the server.
    pub async fn validate_invite(&self, token: &str) -> Result<InviteValidationResponse, NetError> {
        let resp = self.http
            .get(format!("{}/v1/invites/{}", self.server_url, token))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(resp.json().await?)
    }

    /// Step 1 of authentication: request a challenge nonce from the server.
    pub async fn challenge(&self, did: &str, device_id: i32) -> Result<String, NetError> {
        let resp = self.http
            .post(format!("{}/v1/auth/challenge", self.server_url))
            .json(&serde_json::json!({"did": did, "device_id": device_id}))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        let body: ChallengeResponse = resp.json().await?;
        Ok(body.nonce)
    }

    /// Step 2 of authentication: exchange a signed nonce for a session token.
    /// `nonce` is the base64url string returned by `challenge()`.
    /// `signature` is the raw Ed25519 signature bytes over the decoded nonce bytes.
    pub async fn authenticate(&self, did: &str, device_id: i32, nonce: &str, signature: &[u8]) -> Result<AuthResponse, NetError> {
        let resp = self.http
            .post(format!("{}/v1/auth/token", self.server_url))
            .json(&serde_json::json!({
                "did": did,
                "device_id": device_id,
                "nonce": nonce,
                "signature": BASE64_URL_SAFE_NO_PAD.encode(signature),
            }))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(resp.json().await?)
    }

    // ── Prekeys ──────────────────────────────────────────────────────────

    /// Upload prekeys for the authenticated device.
    pub async fn upload_prekeys(&self, req: &UploadPrekeysRequest) -> Result<(), NetError> {
        let mut body = serde_json::Map::new();

        if let Some(spk) = &req.signed_prekey {
            body.insert("signed_prekey".into(), serde_json::json!({
                "id": spk.0,
                "public_key": BASE64_STANDARD.encode(&spk.1),
                "signature": BASE64_STANDARD.encode(&spk.2),
            }));
        }

        if let Some(otpks) = &req.one_time_prekeys {
            body.insert("one_time_prekeys".into(), serde_json::json!(
                otpks.iter().map(|(id, pk)| {
                    serde_json::json!({"id": id, "public_key": BASE64_STANDARD.encode(pk)})
                }).collect::<Vec<_>>()
            ));
        }

        if let Some(kpk) = &req.kyber_prekey {
            body.insert("kyber_prekey".into(), serde_json::json!({
                "id": kpk.0,
                "public_key": BASE64_STANDARD.encode(&kpk.1),
                "signature": BASE64_STANDARD.encode(&kpk.2),
            }));
        }

        if let Some(otkpks) = &req.one_time_kyber_prekeys {
            body.insert("one_time_kyber_prekeys".into(), serde_json::json!(
                otkpks.iter().map(|(id, pk, sig)| {
                    serde_json::json!({
                        "id": id,
                        "public_key": BASE64_STANDARD.encode(pk),
                        "signature": BASE64_STANDARD.encode(sig),
                    })
                }).collect::<Vec<_>>()
            ));
        }

        let resp = self
            .send_authed(reqwest::Method::PUT, "/v1/prekeys", |b| b.json(&body))
            .await?;

        if !resp.status().is_success() {
            return Err(NetError::Server(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(())
    }

    /// Fetch a device's prekey bundle for session initiation.
    pub async fn fetch_prekey_bundle(
        &self,
        did: &str,
        device_id: i32,
    ) -> Result<PreKeyBundleResponse, NetError> {
        let path = format!("/v1/prekeys/{}/{}", did, device_id);
        let resp = self
            .send_authed(reqwest::Method::GET, &path, |b| b)
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        let raw: RawPreKeyBundleResponse = resp.json().await?;
        raw.decode()
    }

    /// Check remaining prekey pool counts.
    pub async fn prekey_status(&self) -> Result<PrekeyStatusResponse, NetError> {
        let resp = self
            .send_authed(reqwest::Method::GET, "/v1/prekeys/status", |b| b)
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(resp.json().await?)
    }

    // ── Messages ─────────────────────────────────────────────────────────

    /// Send encrypted messages to recipient devices.
    pub async fn send_messages(&self, messages: &[OutboundMessage]) -> Result<Vec<i64>, NetError> {
        let wire: Vec<_> = messages.iter().map(|m| {
            let mut obj = serde_json::json!({
                "recipient_did": m.recipient_did,
                "recipient_device_id": m.recipient_device_id,
                "destination_registration_id": m.destination_registration_id,
                "ciphertext": BASE64_STANDARD.encode(&m.ciphertext),
                "message_kind": m.message_kind,
            });
            if let Some(secs) = m.expiry_secs {
                obj["expiry_secs"] = serde_json::json!(secs);
            }
            obj
        }).collect();
        let body = serde_json::json!({"messages": wire});

        let resp = self
            .send_authed(reqwest::Method::POST, "/v1/messages", |b| b.json(&body))
            .await?;

        let status = resp.status();
        if status == reqwest::StatusCode::CONFLICT {
            let text = resp.text().await.unwrap_or_default();
            if let Ok(body) = serde_json::from_str::<serde_json::Value>(&text) {
                if body.get("error").and_then(|v| v.as_str()) == Some("stale_device") {
                    let stale_devices: Vec<crate::error::StaleDevice> = body["stale_devices"]
                        .as_array()
                        .unwrap_or(&vec![])
                        .iter()
                        .filter_map(|v| serde_json::from_value(v.clone()).ok())
                        .collect();
                    return Err(NetError::StaleDevice { stale_devices });
                }
            }
            return Err(NetError::Server(status.as_u16(), text));
        }
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        let body: serde_json::Value = resp.json().await?;
        Ok(body["sent"].as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_i64())
            .collect())
    }

    /// Fetch queued messages for the authenticated device.
    pub async fn fetch_messages(&self) -> Result<Vec<InboundMessage>, NetError> {
        let resp = self
            .send_authed(reqwest::Method::GET, "/v1/messages", |b| b)
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        let body: RawFetchResponse = resp.json().await?;
        body.decode()
    }

    /// Acknowledge (delete) delivered messages.
    pub async fn ack_messages(&self, message_ids: &[i64]) -> Result<(), NetError> {
        let body = serde_json::json!({"message_ids": message_ids});
        let resp = self
            .send_authed(reqwest::Method::DELETE, "/v1/messages", |b| b.json(&body))
            .await?;

        if !resp.status().is_success() {
            return Err(NetError::Server(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(())
    }

    // ── Projects ─────────────────────────────────────────────────────────

    /// Fetch the list of Projects installed on the homeserver.
    pub async fn fetch_projects(&self) -> Result<Vec<ProjectInfo>, NetError> {
        let resp = self.http
            .get(format!("{}/v1/projects", self.server_url))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(resp.json().await?)
    }

    /// Request a Project token for opening a Project webview.
    pub async fn request_project_token(
        &self,
        project_url: &str,
    ) -> Result<ProjectTokenResponse, NetError> {
        let body = serde_json::json!({"project_url": project_url});
        let resp = self
            .send_authed(reqwest::Method::POST, "/v1/project-token", |b| b.json(&body))
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(resp.json().await?)
    }

    // ── Project login / OAuth (docs/25) ──────────────────────────────────

    /// Mint an OAuth authorization code after the user consents on this device
    /// (same-device front-end). Returns the `code` the app hands back to the
    /// Project via the registered `redirect_uri`.
    pub async fn oauth_issue_code(
        &self,
        client_id: &str,
        redirect_uri: &str,
        code_challenge: &str,
        code_challenge_method: &str,
        scope: Option<&str>,
    ) -> Result<String, NetError> {
        let body = serde_json::json!({
            "client_id": client_id,
            "redirect_uri": redirect_uri,
            "code_challenge": code_challenge,
            "code_challenge_method": code_challenge_method,
            "scope": scope,
        });
        let resp = self
            .send_authed(reqwest::Method::POST, "/v1/oauth/authorize-code", |b| b.json(&body))
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        let parsed: OauthAuthorizeCodeResponse = resp.json().await?;
        Ok(parsed.code)
    }

    /// Approve a cross-device (device-grant) login after the user consents on
    /// this device (cross-device front-end). Binds this account to the pending
    /// `user_code` and mints the token the polling Project collects. Returns the
    /// Project URL the user just signed in to.
    pub async fn oauth_approve_device(
        &self,
        user_code: &str,
        client_id: &str,
    ) -> Result<String, NetError> {
        let body = serde_json::json!({
            "user_code": user_code,
            "client_id": client_id,
        });
        let resp = self
            .send_authed(reqwest::Method::POST, "/v1/oauth/device/approve", |b| b.json(&body))
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        let parsed: OauthDeviceApproveResponse = resp.json().await?;
        Ok(parsed.project_url)
    }

    // ── Abuse ────────────────────────────────────────────────────────────

    /// Submit an abuse report to the caller's own homeserver (docs/12 §3). The
    /// report carries no message content — only the reported DID and a reason
    /// enum (`spam` | `harassment` | `impersonation` | `other`). The server
    /// authenticates and rate-limits the reporter, then persists the report for
    /// operator review.
    pub async fn report_abuse(&self, reported_did: &str, reason: &str) -> Result<(), NetError> {
        let body = serde_json::json!({"reported_did": reported_did, "reason": reason});
        let resp = self
            .send_authed(reqwest::Method::POST, "/v1/abuse/report", |b| b.json(&body))
            .await?;

        if !resp.status().is_success() {
            return Err(NetError::Server(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(())
    }

    // ── Push ─────────────────────────────────────────────────────────────

    /// Register a push pseudonym with the homeserver.
    pub async fn register_push_pseudonym(&self, pseudonym: &str) -> Result<(), NetError> {
        let body = serde_json::json!({"pseudonym": pseudonym});
        let resp = self
            .send_authed(reqwest::Method::POST, "/v1/push/register", |b| b.json(&body))
            .await?;

        if !resp.status().is_success() {
            return Err(NetError::Server(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(())
    }

    /// Unregister a push pseudonym (e.g. on rotation or logout).
    pub async fn unregister_push_pseudonym(&self, pseudonym: &str) -> Result<(), NetError> {
        let body = serde_json::json!({"pseudonym": pseudonym});
        let resp = self
            .send_authed(reqwest::Method::POST, "/v1/push/unregister", |b| b.json(&body))
            .await?;

        if !resp.status().is_success() {
            return Err(NetError::Server(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(())
    }

    /// Register `(pseudonym, device_token, platform, environment)` directly
    /// with the push relay. The relay does not require auth — pseudonym is
    /// the opaque rendezvous identifier the homeserver later uses to
    /// trigger wakeups.
    pub async fn register_push_with_relay(
        &self,
        relay_url: &str,
        pseudonym: &str,
        device_token: &str,
        platform: &str,
        environment: &str,
    ) -> Result<(), NetError> {
        let body = serde_json::json!({
            "pseudonym": pseudonym,
            "device_token": device_token,
            "platform": platform,
            "environment": environment,
        });
        let resp = self
            .http
            .post(format!("{}/v1/register", relay_url.trim_end_matches('/')))
            .json(&body)
            .send()
            .await
            .map_err(NetError::Http)?;
        if !resp.status().is_success() {
            return Err(NetError::Server(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }

    /// Unregister a pseudonym from the relay (rotation / logout).
    pub async fn unregister_push_with_relay(
        &self,
        relay_url: &str,
        pseudonym: &str,
    ) -> Result<(), NetError> {
        let body = serde_json::json!({ "pseudonym": pseudonym });
        let resp = self
            .http
            .post(format!("{}/v1/unregister", relay_url.trim_end_matches('/')))
            .json(&body)
            .send()
            .await
            .map_err(NetError::Http)?;
        if !resp.status().is_success() {
            return Err(NetError::Server(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }

    // ── Account info ─────────────────────────────────────────────────────

    /// Look up an account's display name and bot flag.
    pub async fn get_account_info(&self, did: &str) -> Result<AccountInfoResponse, NetError> {
        let path = format!("/v1/accounts/{}", did);
        let resp = self
            .send_authed(reqwest::Method::GET, &path, |b| b)
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(resp.json().await?)
    }

    /// List the active device_ids for an account. Used by senders to fan-out
    /// encrypted message envelopes across all of a recipient's devices.
    pub async fn fetch_devices(&self, did: &str) -> Result<Vec<i32>, NetError> {
        let path = format!("/v1/accounts/{}/devices", did);
        let resp = self
            .send_authed(reqwest::Method::GET, &path, |b| b)
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        #[derive(serde::Deserialize)]
        struct Resp {
            device_ids: Vec<i32>,
        }
        let body: Resp = resp.json().await?;
        Ok(body.device_ids)
    }

    /// Like [`fetch_devices`], but returns each device paired with its current
    /// `registration_id`. The group sealed-sender send path uses this to
    /// reconcile stale sessions before fanning out (the send endpoint can't
    /// report a stale device, since sealed sender hides the sender).
    pub async fn fetch_device_registrations(
        &self,
        did: &str,
    ) -> Result<Vec<crate::types::DeviceRegistration>, NetError> {
        let path = format!("/v1/accounts/{}/devices", did);
        let resp = self
            .send_authed(reqwest::Method::GET, &path, |b| b)
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        #[derive(serde::Deserialize)]
        struct Resp {
            devices: Vec<crate::types::DeviceRegistration>,
        }
        let body: Resp = resp.json().await?;
        Ok(body.devices)
    }

    // ── DID ──────────────────────────────────────────────────────────────

    /// Resolve a DID document (public, no auth needed).
    pub async fn resolve_did(&self, did: &str) -> Result<serde_json::Value, NetError> {
        let resp = self.http
            .get(format!("{}/.well-known/did/{}", self.server_url, did))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(resp.json().await?)
    }

    // ── Recovery ─────────────────────────────────────────────────────────

    /// Download the encrypted recovery blob and the account's current
    /// device list (unauthenticated). The device list is needed during
    /// recovery to target the existing device for replacement; bundling it
    /// with the blob avoids an extra authenticated round-trip.
    pub async fn get_recovery_blob(&self, did: &str) -> Result<crate::types::RecoveryBundle, NetError> {
        let resp = self.http
            .get(format!("{}/v1/recovery/{}", self.server_url, did))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        let body: RecoveryBlobResponse = resp.json().await?;
        let blob = BASE64_STANDARD
            .decode(&body.recovery_blob)
            .map_err(|e| NetError::Base64(e.to_string()))?;
        Ok(crate::types::RecoveryBundle {
            blob,
            device_ids: body.device_ids,
        })
    }

    /// Update the encrypted recovery blob (authenticated).
    pub async fn update_recovery_blob(&self, blob: &[u8]) -> Result<(), NetError> {
        let body = serde_json::json!({
            "recovery_blob": BASE64_STANDARD.encode(blob),
        });
        let resp = self
            .send_authed(reqwest::Method::PUT, "/v1/recovery", |b| b.json(&body))
            .await?;

        if !resp.status().is_success() {
            return Err(NetError::Server(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(())
    }

    /// Replace a device (authenticated by rotation key signature, not session token).
    pub async fn replace_device(&self, req: &ReplaceDeviceRequest) -> Result<ReplaceDeviceResponse, NetError> {
        let body = serde_json::json!({
            "did": req.did,
            "old_device_id": req.old_device_id,
            "new_device_id": req.new_device_id,
            "new_identity_key": BASE64_STANDARD.encode(&req.new_identity_key),
            "new_registration_id": req.new_registration_id,
            "nonce": req.nonce,
            "rotation_key_signature": BASE64_STANDARD.encode(&req.rotation_key_signature),
            "rotation_key": BASE64_STANDARD.encode(&req.rotation_key),
            "signed_prekey": {
                "id": req.signed_prekey_id,
                "public_key": BASE64_STANDARD.encode(&req.signed_prekey_public),
                "signature": BASE64_STANDARD.encode(&req.signed_prekey_signature),
            },
            "one_time_prekeys": req.one_time_prekeys.iter().map(|(id, pk)| {
                serde_json::json!({"id": id, "public_key": BASE64_STANDARD.encode(pk)})
            }).collect::<Vec<_>>(),
            "kyber_prekey": {
                "id": req.kyber_prekey_id,
                "public_key": BASE64_STANDARD.encode(&req.kyber_prekey_public),
                "signature": BASE64_STANDARD.encode(&req.kyber_prekey_signature),
            },
            "recovery_blob": req.recovery_blob.as_ref().map(|b| BASE64_STANDARD.encode(b)),
        });

        let resp = self.http
            .post(format!("{}/v1/devices/replace", self.server_url))
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(resp.json().await?)
    }

    // ── Device linking / provisioning mailbox (docs/04 §4) ────────────────
    //
    // These are unauthenticated. The mailbox endpoints may target a *different*
    // server than this client's home server (the mailbox host named in the
    // pairing code), so callers construct a `Client::new(mailbox_url)` for them;
    // `link_device` targets the DID's home server.

    /// Create an ephemeral provisioning session on this client's server.
    pub async fn create_provisioning_session(&self) -> Result<ProvisioningSession, NetError> {
        let resp = self
            .http
            .post(format!("{}/v1/provisioning/sessions", self.server_url))
            .json(&serde_json::json!({}))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(resp.json().await?)
    }

    /// Write opaque ciphertext to a provisioning slot.
    pub async fn put_provisioning_slot(
        &self,
        session_id: &str,
        slot: &str,
        ciphertext: &[u8],
    ) -> Result<(), NetError> {
        let body = serde_json::json!({ "ciphertext": BASE64_STANDARD.encode(ciphertext) });
        let resp = self
            .http
            .put(format!("{}/v1/provisioning/{}/{}", self.server_url, session_id, slot))
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }

    /// Read a provisioning slot. Returns `None` if the slot has not been written
    /// yet or the session is missing/expired (HTTP 404) — callers poll on `None`.
    pub async fn get_provisioning_slot(
        &self,
        session_id: &str,
        slot: &str,
    ) -> Result<Option<Vec<u8>>, NetError> {
        let resp = self
            .http
            .get(format!("{}/v1/provisioning/{}/{}", self.server_url, session_id, slot))
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }
        #[derive(serde::Deserialize)]
        struct SlotResponse {
            ciphertext: String,
        }
        let body: SlotResponse = resp.json().await?;
        let bytes = BASE64_STANDARD
            .decode(&body.ciphertext)
            .map_err(|e| NetError::Base64(e.to_string()))?;
        Ok(Some(bytes))
    }

    /// Link an additive new device to an existing identity (rotation-key
    /// authorized). Targets the DID's home server.
    pub async fn link_device(&self, req: &LinkDeviceRequest) -> Result<LinkDeviceResponse, NetError> {
        let body = serde_json::json!({
            "did": req.did,
            "new_device_id": req.new_device_id,
            "new_identity_key": BASE64_STANDARD.encode(&req.new_identity_key),
            "new_registration_id": req.new_registration_id,
            "nonce": req.nonce,
            "rotation_key_signature": BASE64_STANDARD.encode(&req.rotation_key_signature),
            "rotation_key": BASE64_STANDARD.encode(&req.rotation_key),
            "signed_prekey": {
                "id": req.signed_prekey_id,
                "public_key": BASE64_STANDARD.encode(&req.signed_prekey_public),
                "signature": BASE64_STANDARD.encode(&req.signed_prekey_signature),
            },
            "one_time_prekeys": req.one_time_prekeys.iter().map(|(id, pk)| {
                serde_json::json!({"id": id, "public_key": BASE64_STANDARD.encode(pk)})
            }).collect::<Vec<_>>(),
            "kyber_prekey": {
                "id": req.kyber_prekey_id,
                "public_key": BASE64_STANDARD.encode(&req.kyber_prekey_public),
                "signature": BASE64_STANDARD.encode(&req.kyber_prekey_signature),
            },
        });

        let resp = self
            .http
            .post(format!("{}/v1/devices/link", self.server_url))
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(resp.json().await?)
    }

    // ── Profile ──────────────────────────────────────────────────────────

    /// Upload the caller's encrypted profile blob.
    pub async fn put_profile(&self, encrypted_blob: &[u8]) -> Result<(), NetError> {
        let body = serde_json::json!({
            "encrypted_blob": BASE64_STANDARD.encode(encrypted_blob),
        });
        let resp = self
            .send_authed(reqwest::Method::PUT, "/v1/profile", |b| b.json(&body))
            .await?;

        if !resp.status().is_success() {
            return Err(NetError::Server(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }

    /// Fetch a contact's encrypted profile blob. Returns `Ok(None)` on 404
    /// (which is returned identically whether the DID is unknown or simply
    /// has no profile, so callers can't distinguish those cases).
    pub async fn get_profile(&self, did: &str) -> Result<Option<Vec<u8>>, NetError> {
        let path = format!("/v1/profile/{}", did);
        let resp = self
            .send_authed(reqwest::Method::GET, &path, |b| b)
            .await?;

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        #[derive(serde::Deserialize)]
        struct R { encrypted_blob: String }
        let body: R = resp.json().await?;
        let blob = BASE64_STANDARD
            .decode(&body.encrypted_blob)
            .map_err(|e| NetError::Base64(e.to_string()))?;
        Ok(Some(blob))
    }

    // ── Attachments (docs/35-attachments.md) ──────────────────────────────

    /// Allocate an upload slot for an attachment blob of `ciphertext_size`
    /// bytes. Returns the server-assigned id, the *upload descriptor* (where and
    /// how to PUT the bytes — see [`upload_attachment_blob`]), the absolute
    /// download URL for the E2E pointer, and the blob TTL deadline.
    ///
    /// The server enforces the per-attachment size cap and per-account upload
    /// quota here (before any bytes are sent).
    pub async fn allocate_attachment_upload(
        &self,
        ciphertext_size: u64,
    ) -> Result<AttachmentUploadSlot, NetError> {
        let body = serde_json::json!({ "size_bytes": ciphertext_size });
        let resp = self
            .send_authed(reqwest::Method::POST, "/v1/attachments", |b| b.json(&body))
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        #[derive(serde::Deserialize)]
        struct Upload {
            url: String,
            method: String,
            headers: Vec<(String, String)>,
        }
        #[derive(serde::Deserialize)]
        struct R {
            attachment_id: String,
            upload: Upload,
            download_url: String,
            expires_at_ms: i64,
        }
        let r: R = resp.json().await?;
        Ok(AttachmentUploadSlot {
            attachment_id: r.attachment_id,
            upload_url: r.upload.url,
            upload_method: r.upload.method,
            upload_headers: r.upload.headers,
            download_url: r.download_url,
            expires_at_ms: r.expires_at_ms,
        })
    }

    /// Upload the encrypted blob to a previously-allocated `slot` by replaying
    /// the server's upload descriptor verbatim: a `slot.upload_method` request
    /// to the absolute `slot.upload_url` carrying exactly `slot.upload_headers`
    /// plus the body. The client is backend-blind — the descriptor names where
    /// the bytes go and what auth/headers to send. For the LocalFs backend the
    /// URL is the homeserver's own route and the headers carry the bearer; for a
    /// future S3 backend the URL is a presigned PUT and the headers are the
    /// signed ones, with no client change.
    pub async fn upload_attachment_blob(
        &self,
        slot: &AttachmentUploadSlot,
        ciphertext: Vec<u8>,
    ) -> Result<(), NetError> {
        let method = reqwest::Method::from_bytes(slot.upload_method.as_bytes())
            .map_err(|_| NetError::Server(0, format!("bad upload method: {}", slot.upload_method)))?;
        let mut rb = self.http.request(method, &slot.upload_url).body(ciphertext);
        for (name, value) in &slot.upload_headers {
            rb = rb.header(name, value);
        }
        let resp = rb.send().await?;
        if !resp.status().is_success() {
            return Err(NetError::Server(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(())
    }

    /// Download an attachment blob by its (absolute) pointer URL, optionally
    /// requesting a byte `range` (`(start, end)` inclusive). Returns the raw
    /// ciphertext bytes; the caller verifies the digest and decrypts.
    ///
    /// The download route is **unauthenticated** (docs/35): the unguessable id
    /// in the URL is the capability, so no bearer token is attached. This is
    /// what lets a recipient fetch a blob hosted on a homeserver it has no
    /// account on (e.g. the sender's, pre-federation), and what a future S3
    /// presigned-URL pointer needs.
    pub async fn download_attachment(
        &self,
        url: &str,
        range: Option<(u64, u64)>,
    ) -> Result<Vec<u8>, NetError> {
        let mut rb = self.http.get(url);
        if let Some((start, end)) = range {
            rb = rb.header(reqwest::header::RANGE, format!("bytes={start}-{end}"));
        }
        let resp = rb.send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }
        Ok(resp.bytes().await?.to_vec())
    }

    // ── Storage service (docs/05-device-data-sync.md §5) ──────────────────

    /// Delta-pull durable-state records with `seq > since` (authenticated).
    pub async fn storage_pull(&self, since: i64, limit: i64) -> Result<StoragePullPage, NetError> {
        let path = format!("/v1/storage/items?since={since}&limit={limit}");
        let resp = self.send_authed(reqwest::Method::GET, &path, |b| b).await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        #[derive(serde::Deserialize)]
        struct RawItem {
            record_id: String,
            version: i64,
            seq: i64,
            deleted: bool,
            ciphertext: String,
        }
        #[derive(serde::Deserialize)]
        struct Raw {
            items: Vec<RawItem>,
            next_cursor: i64,
            has_more: bool,
        }
        let body: Raw = resp.json().await?;
        let mut items = Vec::with_capacity(body.items.len());
        for it in body.items {
            items.push(StorageItem {
                record_id: BASE64_STANDARD
                    .decode(&it.record_id)
                    .map_err(|e| NetError::Base64(e.to_string()))?,
                version: it.version,
                seq: it.seq,
                deleted: it.deleted,
                ciphertext: BASE64_STANDARD
                    .decode(&it.ciphertext)
                    .map_err(|e| NetError::Base64(e.to_string()))?,
            });
        }
        Ok(StoragePullPage {
            items,
            next_cursor: body.next_cursor,
            has_more: body.has_more,
        })
    }

    /// Batch-write durable-state records with per-item CAS (authenticated).
    pub async fn storage_push(
        &self,
        writes: &[StorageWrite],
    ) -> Result<StoragePushResult, NetError> {
        let body = serde_json::json!({
            "writes": writes.iter().map(|w| serde_json::json!({
                "record_id": BASE64_STANDARD.encode(&w.record_id),
                "expected_version": w.expected_version,
                "deleted": w.deleted,
                "ciphertext": BASE64_STANDARD.encode(&w.ciphertext),
            })).collect::<Vec<_>>(),
        });
        let resp = self
            .send_authed(reqwest::Method::PUT, "/v1/storage/items", |b| b.json(&body))
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        #[derive(serde::Deserialize)]
        struct RawApplied {
            record_id: String,
            version: i64,
            seq: i64,
        }
        #[derive(serde::Deserialize)]
        struct RawConflict {
            record_id: String,
            current_version: i64,
        }
        #[derive(serde::Deserialize)]
        struct Raw {
            applied: Vec<RawApplied>,
            conflicts: Vec<RawConflict>,
        }
        let body: Raw = resp.json().await?;
        let decode = |s: &str| BASE64_STANDARD.decode(s).map_err(|e| NetError::Base64(e.to_string()));
        let mut applied = Vec::with_capacity(body.applied.len());
        for a in &body.applied {
            applied.push(StorageApplied {
                record_id: decode(&a.record_id)?,
                version: a.version,
                seq: a.seq,
            });
        }
        let mut conflicts = Vec::with_capacity(body.conflicts.len());
        for c in &body.conflicts {
            conflicts.push(StorageConflict {
                record_id: decode(&c.record_id)?,
                current_version: c.current_version,
            });
        }
        Ok(StoragePushResult { applied, conflicts })
    }
}

// ── Storage service value types ──────────────────────────────────────────────

/// One record returned by a delta pull. Binary fields are already base64-decoded.
pub struct StorageItem {
    pub record_id: Vec<u8>,
    pub version: i64,
    pub seq: i64,
    pub deleted: bool,
    pub ciphertext: Vec<u8>,
}

/// A reserved attachment upload slot (docs/35-attachments.md). The `upload_*`
/// fields are the server's descriptor for where/how to PUT the ciphertext; the
/// client replays them verbatim so it stays backend-agnostic (LocalFs vs S3).
pub struct AttachmentUploadSlot {
    /// Server-assigned opaque id (storage key, internal to the server).
    pub attachment_id: String,
    /// Absolute URL to PUT the ciphertext to.
    pub upload_url: String,
    /// HTTP method for the upload (e.g. `PUT`).
    pub upload_method: String,
    /// Headers to replay verbatim on the upload request (auth, content-type).
    pub upload_headers: Vec<(String, String)>,
    /// Absolute download URL to embed in the E2E pointer.
    pub download_url: String,
    /// Unix-millis blob TTL deadline.
    pub expires_at_ms: i64,
}

/// A page of pulled records plus the cursor to resume from.
pub struct StoragePullPage {
    pub items: Vec<StorageItem>,
    pub next_cursor: i64,
    pub has_more: bool,
}

/// A single CAS write. `expected_version = 0` means create-if-absent;
/// `deleted = true` is a tombstone (ciphertext should be empty).
pub struct StorageWrite {
    pub record_id: Vec<u8>,
    pub expected_version: i64,
    pub deleted: bool,
    pub ciphertext: Vec<u8>,
}

/// A write the server accepted, with its freshly assigned version/seq.
pub struct StorageApplied {
    pub record_id: Vec<u8>,
    pub version: i64,
    pub seq: i64,
}

/// A write the server rejected on CAS; carries the current server version.
pub struct StorageConflict {
    pub record_id: Vec<u8>,
    pub current_version: i64,
}

/// Result of a batch push: applied writes and CAS conflicts, split per §5.
pub struct StoragePushResult {
    pub applied: Vec<StorageApplied>,
    pub conflicts: Vec<StorageConflict>,
}
