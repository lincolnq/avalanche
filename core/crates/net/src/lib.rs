//! HTTP and WebSocket client for the actnet homeserver API.
//!
//! This crate provides a typed client for every homeserver endpoint. It handles
//! JSON serialization, base64 encoding of key material and ciphertext, and
//! bearer token authentication. `app-core` uses this to talk to the server;
//! the crypto and store crates remain I/O-free.
//!
//! The `ws` module provides a WebSocket client for real-time message delivery
//! via `GET /v1/ws?token=<session_token>`.

pub mod error;
pub mod types;
pub mod ws;

use base64::prelude::*;
use error::NetError;
use types::*;

/// HTTP client for a single homeserver.
pub struct Client {
    http: reqwest::Client,
    server_url: String,
    token: Option<String>,
}

impl Client {
    /// Create an unauthenticated client (for registration).
    pub fn new(server_url: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            server_url: server_url.trim_end_matches('/').to_string(),
            token: None,
        }
    }

    /// Create an authenticated client with a session token.
    pub fn with_token(server_url: &str, token: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            server_url: server_url.trim_end_matches('/').to_string(),
            token: Some(token),
        }
    }

    /// Set or replace the session token.
    pub fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }

    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    /// Get the current session token, if set.
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    // ── Account ──────────────────────────────────────────────────────────

    /// Register a new account. Returns DID and session token.
    pub async fn register(&self, req: &RegisterRequest) -> Result<RegisterResponse, NetError> {
        let body = serde_json::json!({
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

        let resp = self.authed_request(reqwest::Method::PUT, "/v1/prekeys")
            .json(&body)
            .send()
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
        let resp = self.authed_request(
            reqwest::Method::GET,
            &format!("/v1/prekeys/{}/{}", did, device_id),
        )
        .send()
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
        let resp = self.authed_request(reqwest::Method::GET, "/v1/prekeys/status")
            .send()
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
                "ciphertext": BASE64_STANDARD.encode(&m.ciphertext),
                "message_kind": m.message_kind,
            });
            if let Some(secs) = m.expiry_secs {
                obj["expiry_secs"] = serde_json::json!(secs);
            }
            obj
        }).collect();

        let resp = self.authed_request(reqwest::Method::POST, "/v1/messages")
            .json(&serde_json::json!({"messages": wire}))
            .send()
            .await?;

        let status = resp.status();
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
        let resp = self.authed_request(reqwest::Method::GET, "/v1/messages")
            .send()
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
        let resp = self.authed_request(reqwest::Method::DELETE, "/v1/messages")
            .json(&serde_json::json!({"message_ids": message_ids}))
            .send()
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
        let resp = self.authed_request(reqwest::Method::POST, "/v1/project-token")
            .json(&serde_json::json!({"project_url": project_url}))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(resp.json().await?)
    }

    // ── Push ─────────────────────────────────────────────────────────────

    /// Register a push pseudonym with the homeserver.
    pub async fn register_push_pseudonym(&self, pseudonym: &str) -> Result<(), NetError> {
        let resp = self.authed_request(reqwest::Method::POST, "/v1/push/register")
            .json(&serde_json::json!({"pseudonym": pseudonym}))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(NetError::Server(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(())
    }

    /// Unregister a push pseudonym (e.g. on rotation or logout).
    pub async fn unregister_push_pseudonym(&self, pseudonym: &str) -> Result<(), NetError> {
        let resp = self.authed_request(reqwest::Method::POST, "/v1/push/unregister")
            .json(&serde_json::json!({"pseudonym": pseudonym}))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(NetError::Server(resp.status().as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(())
    }

    // ── Account info ─────────────────────────────────────────────────────

    /// Look up an account's display name and bot flag.
    pub async fn get_account_info(&self, did: &str) -> Result<AccountInfoResponse, NetError> {
        let resp = self
            .authed_request(reqwest::Method::GET, &format!("/v1/accounts/{}", did))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(NetError::Server(status.as_u16(), resp.text().await.unwrap_or_default()));
        }

        Ok(resp.json().await?)
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

    // ── Helpers ──────────────────────────────────────────────────────────

    fn authed_request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut req = self.http.request(method, format!("{}{}", self.server_url, path));
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        req
    }
}
