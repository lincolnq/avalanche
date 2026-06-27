//! HTTP client methods for the `/v1/groups` endpoints
//! (`docs/03-groups.md` §3.3–§3.4).
//!
//! Two authentication models live here:
//!
//! - **Session-auth** (bearer token via [`Client::send_authed`]): the
//!   `server-params`, `credentials`, and `POST /v1/groups` (create)
//!   endpoints — the server may transiently see the requester's DID.
//! - **Presentation-auth** (`X-Group-Auth` header, no bearer token): every
//!   other group endpoint. The header carries an
//!   `AuthCredentialDidPresentation` the server verifies against the
//!   group's stored `group_public_params`; no DID is ever sent or seen.
//!
//! All payloads use URL-safe-no-pad base64 — see the convention pinned in
//! `core/crates/server/src/routes/groups.rs`.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::error::NetError;
use crate::Client;

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn b64d(s: &str) -> Result<Vec<u8>, NetError> {
    URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .map_err(|e| NetError::Base64(e.to_string()))
}

// ── server params ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GroupServerParams {
    pub version: i32,
    pub params: Vec<u8>,
    /// Sender-cert chain trust-root public key — pinned by the client to
    /// validate sender certs in the sealed-sender group flow.
    pub sender_cert_trust_root: Vec<u8>,
}

#[derive(Deserialize)]
struct GroupServerParamsRaw {
    version: i32,
    params: String,
    sender_cert_trust_root: String,
}

// ── credentials ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IssuedGroupCredential {
    pub bytes: Vec<u8>,
    pub redemption_time: u64,
    /// Serialized libsignal `SenderCertificate` valid through
    /// `sender_cert_expires_at_unix_millis`. Embedded in the sealed-sender
    /// envelope on every outgoing group message.
    pub sender_cert: Vec<u8>,
    pub sender_cert_expires_at_unix_millis: u64,
}

#[derive(Deserialize)]
struct IssueCredentialRaw {
    response: String,
    redemption_time: u64,
    sender_cert: String,
    sender_cert_expires_at: u64,
}

// ── endorsements ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GroupEndorsements {
    pub response: Vec<u8>,
    pub expiration_unix_seconds: u64,
}

#[derive(Deserialize)]
struct GroupEndorsementsRaw {
    response: String,
    expiration_unix_seconds: u64,
}

// ── group send / fetch ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GroupSendRequest {
    pub envelope: Vec<u8>,
    pub token: Vec<u8>,
    pub recipients: Vec<GroupSendRecipient>,
    pub expiry_secs: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct GroupSendRecipient {
    pub service_id_fixed_width: Vec<u8>,
    pub encrypted_member_id: Vec<u8>,
}

#[derive(Deserialize)]
struct GroupSendResponseRaw {
    message_ids: Vec<i64>,
}

#[derive(Debug, Clone)]
pub struct QueuedGroupMessage {
    pub id: i64,
    pub ciphertext: Vec<u8>,
    pub enqueued_at: String,
}

#[derive(Deserialize)]
struct FetchGroupMessagesRaw {
    messages: Vec<QueuedGroupMessageRaw>,
}

#[derive(Deserialize)]
struct QueuedGroupMessageRaw {
    id: i64,
    ciphertext: String,
    enqueued_at: String,
}

// ── create group ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CreateGroupRequest {
    pub group_public_params: Vec<u8>,
    pub encrypted_state: Vec<u8>,
    pub founder_encrypted_member_id: Vec<u8>,
    pub founder_group_push_pseudonym: Vec<u8>,
    pub policy: GroupPolicyWire,
}

#[derive(Debug, Clone)]
pub struct CreateGroupResponse {
    /// 32-byte group_id, URL-safe-no-pad-base64 (so the caller can drop
    /// it straight into path-shaped URLs without re-encoding).
    pub group_id: String,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupPolicyWire {
    pub invite_members_role: i16,
    pub remove_members_role: i16,
    pub modify_title_role: i16,
    pub modify_description_role: i16,
    pub modify_expiry_role: i16,
    pub join_policy: i16,
    /// Optional invite-link password (base64, URL-safe-no-pad).
    pub invite_link_password: Option<String>,
    pub announcement_only: bool,
}

#[derive(Deserialize)]
struct CreateGroupResponseRaw {
    group_id: String,
    revision: i64,
}

// ── get group / get changes ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GetGroupResponse {
    pub revision: i64,
    pub encrypted_state: Vec<u8>,
    pub group_public_params: Vec<u8>,
    pub policy: GroupPolicyWire,
}

#[derive(Deserialize)]
struct GetGroupResponseRaw {
    revision: i64,
    encrypted_state: String,
    group_public_params: String,
    policy: GroupPolicyWire,
}

#[derive(Debug, Clone)]
pub struct GroupChange {
    pub revision: i64,
    pub encrypted_state: Vec<u8>,
    /// JSON-encoded actions blob; opaque from this layer.
    pub actions: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ChangesResponse {
    pub changes: Vec<GroupChange>,
    pub current_revision: i64,
}

#[derive(Deserialize)]
struct ChangesResponseRaw {
    changes: Vec<GroupChangeRaw>,
    current_revision: i64,
}

#[derive(Deserialize)]
struct GroupChangeRaw {
    revision: i64,
    encrypted_state: String,
    actions: String,
}

// ── submit changes ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct SubmitChangeRequest {
    pub revision: i64,
    /// Base64 URL-safe-no-pad of the new encrypted state blob.
    pub new_encrypted_state: String,
    pub actions: GroupActionsWire,
}

/// Mirrors `ActionsWire` in the server. Base64 fields are URL-safe-no-pad.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GroupActionsWire {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invite_members: Vec<InviteMemberWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promote_pending_members: Option<PromoteSelfWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decline_invite: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_members: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modify_member_role: Vec<RoleAssignmentWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_via_link: Option<JoinViaLinkWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_join_request: Option<String>,
    /// Self-class: the actor leaves the group, naming their own
    /// encrypted_member_id (docs/53). Must be the sole action in the change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leave: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approve_join_request: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny_join_request: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modify_policy: Option<GroupPolicyWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modify_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modify_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modify_expiry: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteMemberWire {
    pub encrypted_member_id: String,
    pub role: i16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromoteSelfWire {
    pub encrypted_profile_key: String,
    pub group_push_pseudonym: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssignmentWire {
    pub encrypted_member_id: String,
    pub role: i16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinViaLinkWire {
    pub encrypted_profile_key: String,
    pub group_push_pseudonym: String,
    pub invite_link_password: String,
}

#[derive(Debug, Clone)]
pub struct SubmitChangeResponse {
    pub revision: i64,
    /// `Some(true)` = added directly as member (OpenLink),
    /// `Some(false)` = added to pending-approval queue,
    /// `None` = action was not a `join_via_link`.
    pub join_landed_as_member: Option<bool>,
}

#[derive(Deserialize)]
struct SubmitChangeResponseRaw {
    revision: i64,
    join_result: Option<String>,
}

impl Client {
    // ── public: server params ───────────────────────────────────────────

    /// Fetch the server's zkgroup public params (no auth).
    pub async fn get_group_server_params(&self) -> Result<GroupServerParams, NetError> {
        let url = format!("{}/v1/groups/server-params", self.server_url());
        let resp = self.http_get_public(&url).await?;
        let raw: GroupServerParamsRaw = resp.json().await?;
        Ok(GroupServerParams {
            version: raw.version,
            params: b64d(&raw.params)?,
            sender_cert_trust_root: b64d(&raw.sender_cert_trust_root)?,
        })
    }

    // ── session-auth: issue credential ──────────────────────────────────

    /// Daily credential refresh.
    pub async fn issue_group_credential(
        &self,
        did: &str,
        redemption_time: u64,
    ) -> Result<IssuedGroupCredential, NetError> {
        let body = serde_json::json!({
            "did": did,
            "redemption_time": redemption_time,
        });
        let resp = self
            .send_authed(reqwest::Method::POST, "/v1/groups/credentials", |b| b.json(&body))
            .await?;
        let raw: IssueCredentialRaw = check_json(resp).await?;
        Ok(IssuedGroupCredential {
            bytes: b64d(&raw.response)?,
            redemption_time: raw.redemption_time,
            sender_cert: b64d(&raw.sender_cert)?,
            sender_cert_expires_at_unix_millis: raw.sender_cert_expires_at,
        })
    }

    /// Fetch the per-group endorsement bundle (one MAC over the whole
    /// member set). Caller validates and per-member-slices it via
    /// `crypto::groups::endorsements::receive_endorsements`.
    pub async fn get_group_endorsements(
        &self,
        group_id_b64: &str,
        presentation: &[u8],
    ) -> Result<GroupEndorsements, NetError> {
        let url = format!(
            "{}/v1/groups/{}/endorsements",
            self.server_url(),
            group_id_b64,
        );
        let resp = self
            .presentation_request(reqwest::Method::GET, &url, presentation, None)
            .await?;
        let raw: GroupEndorsementsRaw = check_json(resp).await?;
        Ok(GroupEndorsements {
            response: b64d(&raw.response)?,
            expiration_unix_seconds: raw.expiration_unix_seconds,
        })
    }

    // ── sealed-sender group send (no auth header) ───────────────────────

    pub async fn send_group_message(
        &self,
        group_id_b64: &str,
        req: &GroupSendRequest,
    ) -> Result<Vec<i64>, NetError> {
        let url = format!("{}/v1/groups/{}/send", self.server_url(), group_id_b64);
        let body = serde_json::json!({
            "envelope": b64(&req.envelope),
            "token": b64(&req.token),
            "recipients": req
                .recipients
                .iter()
                .map(|r| serde_json::json!({
                    "service_id_fixed_width": b64(&r.service_id_fixed_width),
                    "encrypted_member_id":     b64(&r.encrypted_member_id),
                }))
                .collect::<Vec<_>>(),
            "expiry_secs": req.expiry_secs,
        });
        let resp = self.http_client().post(&url).json(&body).send().await?;
        if !resp.status().is_success() {
            return Err(NetError::Server(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        let raw: GroupSendResponseRaw = check_json(resp).await?;
        Ok(raw.message_ids)
    }

    // ── presentation-auth: drain queued group messages (offline pickup) ──

    pub async fn fetch_group_messages(
        &self,
        group_id_b64: &str,
        pseudonym: &[u8],
        presentation: &[u8],
    ) -> Result<Vec<QueuedGroupMessage>, NetError> {
        // The device names its own pseudonym so the server drains the right
        // per-device queue (docs/04 multi-device groups). `b64` is URL-safe
        // (no `+`/`/`/`=`), so it needs no further query-string escaping.
        let url = format!(
            "{}/v1/groups/{}/messages?pseudonym={}",
            self.server_url(),
            group_id_b64,
            b64(pseudonym),
        );
        let resp = self
            .presentation_request(reqwest::Method::GET, &url, presentation, None)
            .await?;
        let raw: FetchGroupMessagesRaw = check_json(resp).await?;
        let mut out = Vec::with_capacity(raw.messages.len());
        for m in raw.messages {
            out.push(QueuedGroupMessage {
                id: m.id,
                ciphertext: b64d(&m.ciphertext)?,
                enqueued_at: m.enqueued_at,
            });
        }
        Ok(out)
    }

    pub async fn ack_group_messages(
        &self,
        group_id_b64: &str,
        pseudonym: &[u8],
        message_ids: Vec<i64>,
        presentation: &[u8],
    ) -> Result<(), NetError> {
        let url = format!("{}/v1/groups/{}/messages", self.server_url(), group_id_b64);
        let body = serde_json::json!({
            "pseudonym": b64(pseudonym),
            "message_ids": message_ids,
        });
        let resp = self
            .presentation_request(reqwest::Method::DELETE, &url, presentation, Some(body))
            .await?;
        if !resp.status().is_success() {
            return Err(NetError::Server(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }

    // ── session-auth: create group ──────────────────────────────────────

    pub async fn create_group(
        &self,
        req: &CreateGroupRequest,
    ) -> Result<CreateGroupResponse, NetError> {
        let body = serde_json::json!({
            "group_public_params": b64(&req.group_public_params),
            "encrypted_state": b64(&req.encrypted_state),
            "founder_encrypted_member_id": b64(&req.founder_encrypted_member_id),
            "founder_group_push_pseudonym": b64(&req.founder_group_push_pseudonym),
            "policy": req.policy,
        });
        let resp = self
            .send_authed(reqwest::Method::POST, "/v1/groups", |b| b.json(&body))
            .await?;
        let raw: CreateGroupResponseRaw = check_json(resp).await?;
        Ok(CreateGroupResponse {
            group_id: raw.group_id,
            revision: raw.revision,
        })
    }

    // ── presentation-auth: get group / changes ──────────────────────────

    pub async fn get_group(
        &self,
        group_id_b64: &str,
        presentation: &[u8],
    ) -> Result<GetGroupResponse, NetError> {
        let url = format!("{}/v1/groups/{}", self.server_url(), group_id_b64);
        let resp = self.presentation_request(reqwest::Method::GET, &url, presentation, None).await?;
        let raw: GetGroupResponseRaw = check_json(resp).await?;
        Ok(GetGroupResponse {
            revision: raw.revision,
            encrypted_state: b64d(&raw.encrypted_state)?,
            group_public_params: b64d(&raw.group_public_params)?,
            policy: raw.policy,
        })
    }

    pub async fn get_group_changes(
        &self,
        group_id_b64: &str,
        from_revision: i64,
        presentation: &[u8],
    ) -> Result<ChangesResponse, NetError> {
        let url = format!(
            "{}/v1/groups/{}/changes?from_revision={}",
            self.server_url(),
            group_id_b64,
            from_revision,
        );
        let resp = self.presentation_request(reqwest::Method::GET, &url, presentation, None).await?;
        let raw: ChangesResponseRaw = check_json(resp).await?;
        Ok(ChangesResponse {
            changes: raw
                .changes
                .into_iter()
                .map(|c| {
                    Ok::<_, NetError>(GroupChange {
                        revision: c.revision,
                        encrypted_state: b64d(&c.encrypted_state)?,
                        actions: b64d(&c.actions)?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            current_revision: raw.current_revision,
        })
    }

    // ── presentation-auth: submit changes ───────────────────────────────

    pub async fn submit_group_changes(
        &self,
        group_id_b64: &str,
        req: &SubmitChangeRequest,
        presentation: &[u8],
    ) -> Result<SubmitChangeResponse, NetError> {
        let url = format!("{}/v1/groups/{}/changes", self.server_url(), group_id_b64);
        let body = serde_json::to_value(req).map_err(|e| NetError::Base64(e.to_string()))?;
        let resp = self
            .presentation_request(reqwest::Method::POST, &url, presentation, Some(body))
            .await?;
        let raw: SubmitChangeResponseRaw = check_json(resp).await?;
        Ok(SubmitChangeResponse {
            revision: raw.revision,
            join_landed_as_member: raw.join_result.as_deref().map(|s| s == "member"),
        })
    }

    // ── presentation-auth: rotate push binding ──────────────────────────

    /// Register or rotate this device's group push pseudonym (docs/04
    /// multi-device groups). Pass `old_pseudonym = Some(..)` to rotate this
    /// device's existing binding in place (7-day cadence); pass `None` for a
    /// first registration (e.g. a freshly linked device), which adds a pseudonym
    /// without disturbing sibling devices' bindings.
    pub async fn rotate_group_push_binding(
        &self,
        group_id_b64: &str,
        old_pseudonym: Option<&[u8]>,
        new_pseudonym: &[u8],
        presentation: &[u8],
    ) -> Result<(), NetError> {
        let url = format!(
            "{}/v1/groups/{}/push_binding",
            self.server_url(),
            group_id_b64,
        );
        let mut body = serde_json::json!({
            "new_group_push_pseudonym": b64(new_pseudonym),
        });
        if let Some(old) = old_pseudonym {
            body["old_group_push_pseudonym"] = serde_json::Value::String(b64(old));
        }
        let resp = self
            .presentation_request(reqwest::Method::POST, &url, presentation, Some(body))
            .await?;
        if !resp.status().is_success() {
            return Err(NetError::Server(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }

    // ── helpers (only used by groups) ───────────────────────────────────

    async fn http_get_public(&self, url: &str) -> Result<reqwest::Response, NetError> {
        let resp = self.http_client().get(url).send().await?;
        if !resp.status().is_success() {
            return Err(NetError::Server(
                resp.status().as_u16(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(resp)
    }

    async fn presentation_request(
        &self,
        method: reqwest::Method,
        url: &str,
        presentation: &[u8],
        json_body: Option<serde_json::Value>,
    ) -> Result<reqwest::Response, NetError> {
        let mut req = self
            .http_client()
            .request(method, url)
            .header("x-group-auth", b64(presentation));
        if let Some(body) = json_body {
            req = req.json(&body);
        }
        Ok(req.send().await?)
    }
}

async fn check_json<T: for<'de> Deserialize<'de>>(
    resp: reqwest::Response,
) -> Result<T, NetError> {
    let status = resp.status();
    if !status.is_success() {
        return Err(NetError::Server(
            status.as_u16(),
            resp.text().await.unwrap_or_default(),
        ));
    }
    Ok(resp.json().await?)
}
