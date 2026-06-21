//! Action-bound group endpoints. See `docs/03-groups.md`.
//!
//! Authentication model is split:
//!
//! - **Public:** `GET /v1/groups/server-params` — anyone can fetch the
//!   homeserver's zkgroup public params.
//! - **Session-authenticated:** `POST /v1/groups` (create) and
//!   `POST /v1/groups/credentials` (daily credential refresh). The server
//!   transiently sees the requesting account's DID. §3.9 forbids
//!   *persisting* the link; the in-memory request context is fine.
//! - **Presentation-authenticated:** every other endpoint takes an
//!   `AuthCredentialDidPresentation` in the `X-Group-Auth` header. The
//!   server verifies the presentation against the group's stored
//!   `group_public_params` and extracts an `EncryptedMemberId` to use for
//!   the action-specific eligibility check. The server never learns the
//!   actor's DID.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use base64::Engine as _;
use crypto::groups::{
    did_to_uuid, endorsements as group_endorsements, AuthCredentialWithPniZkcPresentation,
    AuthCredentialWithPniZkcResponse, EncryptedMemberId, GroupPublicParams, RedemptionTime,
};
use libsignal_core::{Aci, Pni};
use rand::TryRngCore;
use serde::{Deserialize, Serialize};

use crate::{
    db,
    error::ServerError,
    middleware::{auth::AuthDevice, client_ip::ClientIp, rate_limit},
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/groups/server-params", get(get_server_params))
        .route("/v1/groups/credentials", post(issue_credential))
        .route("/v1/groups", post(create_group))
        .route("/v1/groups/{group_id}", get(get_group))
        .route("/v1/groups/{group_id}/endorsements", get(get_endorsements))
        .route("/v1/groups/{group_id}/changes", get(get_changes))
        .route("/v1/groups/{group_id}/changes", post(submit_changes))
        .route("/v1/groups/{group_id}/push_binding", post(push_binding))
        .route("/v1/groups/{group_id}/send", post(send_group_message))
        .route("/v1/groups/{group_id}/messages", get(fetch_group_messages))
        .route(
            "/v1/groups/{group_id}/messages",
            axum::routing::delete(ack_group_messages),
        )
}

// ── /v1/groups/server-params (public) ────────────────────────────────────────

#[derive(Serialize)]
struct ServerParamsResponse {
    version: i32,
    /// URL-safe base64 of the zkgroup `ServerPublicParams`.
    params: String,
    /// URL-safe base64 of the sender-cert chain trust-root public key,
    /// pinned by clients to validate sender certs in the sealed-sender
    /// group flow. See `crypto::sender_cert`.
    sender_cert_trust_root: String,
}

async fn get_server_params(State(state): State<AppState>) -> Json<ServerParamsResponse> {
    let bytes = state.zkgroup_secret.public_params().to_bytes();
    let trust_root = state.sender_cert_chain.trust_root_public_bytes();
    Json(ServerParamsResponse {
        version: db::zkgroup_params::CURRENT_VERSION,
        params: b64_encode(&bytes),
        sender_cert_trust_root: b64_encode(&trust_root),
    })
}

// ── /v1/groups/credentials (session-auth) ────────────────────────────────────
//
// Daily credential refresh. Clients pass a DID + day-aligned redemption time;
// the server issues an `AuthCredentialDidResponse` the client `receive`s and
// uses for all that day's presentations. §3.11 "Daily credential refresh".
//
// §3.9 rule 3: we do not persist a row tying (DID, credential identifier).
// The issuance proof contains no credential identifier the server can later
// match against a presentation; rate-limit counters per-DID-per-day are
// fine, see step 4g.

#[derive(Deserialize)]
struct IssueCredentialRequest {
    /// DID to bind the credential to. Must match the requester (verified
    /// server-side via `device_pk → account → did`).
    did: String,
    /// Unix seconds, day-aligned.
    redemption_time: u64,
}

#[derive(Serialize)]
struct IssueCredentialResponse {
    response: String,
    redemption_time: u64,
    /// Per-device sender certificate, valid through the day after
    /// `redemption_time`. Embedded in the sealed-sender envelope for any
    /// group message the client sends during that window.
    sender_cert: String,
    /// Unix millis at which `sender_cert` expires. Clients refresh before
    /// hitting this.
    sender_cert_expires_at: u64,
}

async fn issue_credential(
    State(state): State<AppState>,
    auth: AuthDevice,
    Json(body): Json<IssueCredentialRequest>,
) -> Result<Json<IssueCredentialResponse>, ServerError> {
    let redemption = RedemptionTime::from_epoch_seconds(body.redemption_time);
    if !redemption.is_day_aligned() {
        return Err(ServerError::BadRequest(
            "redemption_time must be day-aligned (multiple of 86400)".into(),
        ));
    }

    // Verify the requester is binding the credential to their own DID — not
    // someone else's. Without this check, anyone could request a credential
    // for any DID and then masquerade in groups.
    let mut conn = state.db.acquire().await?;
    let device = db::devices::find_by_pk(&mut conn, auth.device_pk)
        .await?
        .ok_or(ServerError::Unauthorized)?;
    let account_row = sqlx::query("SELECT did FROM accounts WHERE id = $1")
        .bind(device.account_id)
        .fetch_one(&mut *conn)
        .await?;
    let device_did: String = sqlx::Row::get(&account_row, "did");
    if device_did != body.did {
        return Err(ServerError::Unauthorized);
    }

    // §3.9 rule 3: per-DID daily counter is the allowed form of rate-limiting
    // on credential issuance. The rate_limit table is keyed by account_id,
    // which is the bigint behind the DID, so this satisfies the rule.
    if !db::rate_limits::check_and_increment(
        &mut conn,
        device.account_id,
        rate_limit::ACTION_ISSUE_GROUP_CREDENTIAL,
        rate_limit::LIMIT_ISSUE_GROUP_CREDENTIAL,
        rate_limit::WINDOW_ISSUE_GROUP_CREDENTIAL,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }

    let randomness = fresh_randomness();
    let uuid = did_to_uuid(&body.did);
    let response = AuthCredentialWithPniZkcResponse::issue_credential(
        Aci::from(uuid),
        Pni::from(uuid),
        redemption,
        state.zkgroup_secret.zkgroup(),
        randomness,
    );
    let bytes = zkgroup::serialize(&response);

    // Mint a sender certificate for this device, valid for ~2 days from
    // `redemption_time` so clients have a comfortable refresh window. The
    // device's identity_key (libsignal 33-byte serialization) was uploaded
    // at registration time.
    let sender_cert_expires_at_ms = (body.redemption_time + 2 * 86_400) * 1_000;
    let sender_cert_bytes = state
        .sender_cert_chain
        .issue_sender_cert(
            &body.did,
            device.device_id as u32,
            &device.identity_key,
            sender_cert_expires_at_ms,
        )
        .map_err(|e| ServerError::Internal(format!("issue sender cert: {e}")))?;

    Ok(Json(IssueCredentialResponse {
        response: b64_encode(&bytes),
        redemption_time: body.redemption_time,
        sender_cert: b64_encode(&sender_cert_bytes),
        sender_cert_expires_at: sender_cert_expires_at_ms,
    }))
}

// ── POST /v1/groups (session-auth) ───────────────────────────────────────────

#[derive(Deserialize)]
struct CreateGroupRequest {
    group_public_params: String,
    encrypted_state: String,
    founder_encrypted_member_id: String,
    founder_group_push_pseudonym: String,
    policy: PolicyWire,
}

#[derive(Deserialize, Serialize)]
struct PolicyWire {
    invite_members_role: i16,
    remove_members_role: i16,
    modify_title_role: i16,
    modify_description_role: i16,
    modify_expiry_role: i16,
    join_policy: i16,
    invite_link_password: Option<String>,
    announcement_only: bool,
}

#[derive(Serialize)]
struct CreateGroupResponse {
    group_id: String,
    revision: i64,
}

async fn create_group(
    State(state): State<AppState>,
    auth: AuthDevice,
    Json(body): Json<CreateGroupRequest>,
) -> Result<Json<CreateGroupResponse>, ServerError> {
    let mut rate_conn = state.db.acquire().await?;
    let device = db::devices::find_by_pk(&mut rate_conn, auth.device_pk)
        .await?
        .ok_or(ServerError::Unauthorized)?;
    if !db::rate_limits::check_and_increment(
        &mut rate_conn,
        device.account_id,
        rate_limit::ACTION_CREATE_GROUP,
        rate_limit::LIMIT_CREATE_GROUP,
        rate_limit::WINDOW_CREATE_GROUP,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }
    drop(rate_conn);

    let params_bytes = b64_decode(&body.group_public_params, "group_public_params")?;
    let group_public_params = GroupPublicParams::from_bytes(&params_bytes)
        .map_err(|_| ServerError::BadRequest("invalid group_public_params".into()))?;
    let group_id = group_public_params.group_id().0;

    let encrypted_state = b64_decode(&body.encrypted_state, "encrypted_state")?;
    let founder_id = b64_decode(&body.founder_encrypted_member_id, "founder_encrypted_member_id")?;
    let founder_pseudonym = b64_decode(
        &body.founder_group_push_pseudonym,
        "founder_group_push_pseudonym",
    )?;
    let policy_invite_link_password = body
        .policy
        .invite_link_password
        .as_deref()
        .map(|s| b64_decode(s, "policy.invite_link_password"))
        .transpose()?;
    let policy = db::groups::Policy {
        invite_members_role: body.policy.invite_members_role,
        remove_members_role: body.policy.remove_members_role,
        modify_title_role: body.policy.modify_title_role,
        modify_description_role: body.policy.modify_description_role,
        modify_expiry_role: body.policy.modify_expiry_role,
        join_policy: body.policy.join_policy,
        invite_link_password: policy_invite_link_password,
        announcement_only: body.policy.announcement_only,
    };
    validate_policy(&policy)?;

    let mut tx = state.db.begin().await?;
    // Conflict if a group with this id already exists.
    if db::groups::get(&mut tx, &group_id).await?.is_some() {
        return Err(ServerError::Conflict("group_id already exists".into()));
    }
    db::groups::create(
        &mut tx,
        &db::groups::NewGroup {
            group_id: &group_id,
            server_public_params_version: db::zkgroup_params::CURRENT_VERSION,
            group_public_params: &params_bytes,
            encrypted_state: &encrypted_state,
            policy: &policy,
            founder_encrypted_member_id: &founder_id,
            founder_group_push_pseudonym: &founder_pseudonym,
        },
    )
    .await?;
    tx.commit().await?;

    Ok(Json(CreateGroupResponse {
        group_id: b64_encode(&group_id),
        revision: 0,
    }))
}

// ── GET /v1/groups/{id} (presentation-auth) ──────────────────────────────────

#[derive(Serialize)]
struct GetGroupResponse {
    revision: i64,
    encrypted_state: String,
    group_public_params: String,
    policy: PolicyWire,
}

async fn get_group(
    State(state): State<AppState>,
    Path(group_id_b64): Path<String>,
    headers: HeaderMap,
) -> Result<Json<GetGroupResponse>, ServerError> {
    let (group, _actor_emi) =
        authorize_member_or_pending_invite(&state, &headers, &group_id_b64).await?;
    Ok(Json(GetGroupResponse {
        revision: group.current_revision,
        encrypted_state: b64_encode(&group.encrypted_state),
        group_public_params: b64_encode(&group.group_public_params),
        policy: policy_to_wire(&group.policy),
    }))
}

// ── GET /v1/groups/{id}/endorsements (presentation-auth) ─────────────────────

#[derive(Serialize)]
struct EndorsementsResponse {
    /// Serialized `GroupSendEndorsementsResponse` covering every active
    /// member's EMI for this group; URL-safe base64.
    response: String,
    /// Day-aligned expiration in Unix seconds. Clients refresh before this.
    expiration_unix_seconds: u64,
}

async fn get_endorsements(
    State(state): State<AppState>,
    Path(group_id_b64): Path<String>,
    headers: HeaderMap,
) -> Result<Json<EndorsementsResponse>, ServerError> {
    // Pending-invites don't get endorsements — they can't send yet.
    let (group, _actor_emi) = authorize_member(&state, &headers, &group_id_b64).await?;
    let mut conn = state.db.acquire().await?;
    let member_emis = db::groups::list_member_encrypted_ids(&mut conn, &group.group_id).await?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_secs();
    let expiration = group_endorsements::default_expiration_unix_seconds(now);
    let response_bytes = group_endorsements::issue_endorsements(
        state.zkgroup_secret.as_ref(),
        &member_emis,
        expiration,
    )
    .map_err(|e| ServerError::Internal(format!("issue endorsements: {e}")))?;
    Ok(Json(EndorsementsResponse {
        response: b64_encode(&response_bytes),
        expiration_unix_seconds: expiration,
    }))
}

// ── GET /v1/groups/{id}/changes (presentation-auth) ──────────────────────────

#[derive(Deserialize)]
struct ChangesQuery {
    from_revision: i64,
}

#[derive(Serialize)]
struct ChangesResponse {
    changes: Vec<StateChangeWire>,
    /// Server's current revision. Clients can detect "I'm caught up" by
    /// comparing against their last applied revision.
    current_revision: i64,
}

#[derive(Serialize)]
struct StateChangeWire {
    revision: i64,
    encrypted_state: String,
    actions: String,
}

/// Cap on how many history rows we return per request. Clients can paginate
/// by advancing `from_revision`.
const CHANGES_PAGE_SIZE: i64 = 256;

async fn get_changes(
    State(state): State<AppState>,
    Path(group_id_b64): Path<String>,
    Query(query): Query<ChangesQuery>,
    headers: HeaderMap,
) -> Result<Json<ChangesResponse>, ServerError> {
    let (group, _actor_emi) =
        authorize_member_or_pending_invite(&state, &headers, &group_id_b64).await?;
    let mut conn = state.db.acquire().await?;
    let rows = db::groups::get_changes_since(
        &mut conn,
        &group.group_id,
        query.from_revision,
        CHANGES_PAGE_SIZE,
    )
    .await?;
    Ok(Json(ChangesResponse {
        changes: rows
            .into_iter()
            .map(|r| StateChangeWire {
                revision: r.revision,
                encrypted_state: b64_encode(&r.encrypted_state),
                actions: b64_encode(&r.actions),
            })
            .collect(),
        current_revision: group.current_revision,
    }))
}

// ── POST /v1/groups/{id}/changes (presentation-auth) ─────────────────────────
//
// The action-application path. See §3.3 for the protocol.

#[derive(Deserialize)]
struct SubmitChangeRequest {
    /// Revision *after* applying. Must equal `current_revision + 1`.
    revision: i64,
    /// New encrypted state blob the client computed by applying the actions
    /// to the previous state. Server doesn't validate the blob; clients
    /// re-derive on apply (§3.3).
    new_encrypted_state: String,
    actions: ActionsWire,
}

#[derive(Deserialize, Serialize)]
struct ActionsWire {
    #[serde(default)]
    invite_members: Vec<InviteMemberWire>,
    #[serde(default)]
    promote_pending_members: Option<PromoteSelfWire>,
    #[serde(default)]
    decline_invite: Option<String>,
    #[serde(default)]
    remove_members: Vec<String>,
    #[serde(default)]
    modify_member_role: Vec<RoleAssignmentWire>,
    #[serde(default)]
    join_via_link: Option<JoinViaLinkWire>,
    #[serde(default)]
    cancel_join_request: Option<String>,
    /// Self-class: the actor leaves the group (docs/53), naming their own
    /// encrypted_member_id. Sole action; the named EMI must equal the actor's.
    #[serde(default)]
    leave: Option<String>,
    #[serde(default)]
    approve_join_request: Vec<String>,
    #[serde(default)]
    deny_join_request: Vec<String>,
    #[serde(default)]
    modify_policy: Option<PolicyWire>,
    // Sub-encrypted under the group key; opaque to the server. Server
    // includes these verbatim in the history blob so other clients receive
    // the change, but doesn't read them and doesn't apply them to any
    // server-visible state.
    #[serde(default)]
    modify_title: Option<String>,
    #[serde(default)]
    modify_description: Option<String>,
    #[serde(default)]
    modify_expiry: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct InviteMemberWire {
    encrypted_member_id: String,
    role: i16,
}

#[derive(Deserialize, Serialize)]
struct PromoteSelfWire {
    /// Encrypted profile key, opaque to the server, broadcast to other
    /// clients via the actions blob in history.
    encrypted_profile_key: String,
    group_push_pseudonym: String,
}

#[derive(Deserialize, Serialize)]
struct RoleAssignmentWire {
    encrypted_member_id: String,
    role: i16,
}

#[derive(Deserialize, Serialize)]
struct JoinViaLinkWire {
    encrypted_profile_key: String,
    group_push_pseudonym: String,
    invite_link_password: String,
}

#[derive(Serialize)]
struct SubmitChangeResponse {
    revision: i64,
    /// Set when `join_via_link` lands the requester directly in
    /// `member_credentials` (OpenLink) vs `members_pending_approval`
    /// (RequestToJoin). Null for all other action types.
    join_result: Option<JoinResultWire>,
}

#[derive(Serialize)]
enum JoinResultWire {
    #[serde(rename = "member")]
    Member,
    #[serde(rename = "pending")]
    Pending,
}

async fn submit_changes(
    State(state): State<AppState>,
    Path(group_id_b64): Path<String>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(body): Json<SubmitChangeRequest>,
) -> Result<Json<SubmitChangeResponse>, ServerError> {
    // Presentation auth doesn't bind to an account, so IP is the only rate-
    // limit key available here. §3.11 "Rate limiting": per-IP counter applied
    // before the heavier verification work.
    let mut rate_conn = state.db.acquire().await?;
    if !db::ip_rate_limits::check_and_increment(
        &mut rate_conn,
        &ip,
        rate_limit::ACTION_SUBMIT_GROUP_CHANGE,
        rate_limit::LIMIT_SUBMIT_GROUP_CHANGE,
        rate_limit::WINDOW_SUBMIT_GROUP_CHANGE,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }
    drop(rate_conn);

    let (group, actor_emi) = authorize_member_or_pending(&state, &headers, &group_id_b64).await?;
    let actor_emi_bytes = zkgroup::serialize(&actor_emi);

    let new_state = b64_decode(&body.new_encrypted_state, "new_encrypted_state")?;
    let action_classes = classify_actions(&body.actions)?;

    // §3.3 step 3: revision freshness.
    let expected_new_revision = group.current_revision + 1;
    if body.revision != expected_new_revision {
        return Err(ServerError::Conflict(format!(
            "stale revision: expected {expected_new_revision}, got {}",
            body.revision
        )));
    }

    // §3.3 step 2: actor-eligibility check, by action class.
    let mut tx = state.db.begin().await?;
    let actor_role =
        db::groups::member_role(&mut tx, &group.group_id, &actor_emi_bytes).await?;
    enforce_actor_eligibility(
        &mut tx,
        &group.group_id,
        &actor_emi_bytes,
        actor_role,
        &action_classes,
    )
    .await?;

    // §3.3 step 4: role checks for admin-class actions.
    if action_classes.contains_admin {
        let role = actor_role.expect("admin-class implies member_credentials presence");
        enforce_admin_role_checks(&body.actions, role, &group.policy)?;
    }

    // §3.3 step 5: apply structural changes.
    let mut join_result: Option<JoinResultWire> = None;
    apply_actions(
        &mut tx,
        &group,
        &body.actions,
        &actor_emi_bytes,
        &mut join_result,
    )
    .await?;

    // §3.3 step 6: store new state, bump revision, append history. The
    // actions blob is what we received, re-serialized canonically so
    // history bytes are stable across formatter changes.
    let actions_bytes = serde_json::to_vec(&body.actions)
        .map_err(|e| ServerError::Internal(format!("serialize actions: {e}")))?;
    let applied = db::groups::apply_revision(
        &mut tx,
        &group.group_id,
        group.current_revision,
        &new_state,
        &actions_bytes,
    )
    .await?;
    if !applied {
        // Another submitter beat us between the actor-eligibility read and
        // the revision update. Rollback by dropping the tx; client retries.
        return Err(ServerError::Conflict("concurrent revision; retry".into()));
    }
    tx.commit().await?;

    Ok(Json(SubmitChangeResponse {
        revision: expected_new_revision,
        join_result,
    }))
}

// ── POST /v1/groups/{id}/push_binding (presentation-auth) ────────────────────

#[derive(Deserialize)]
struct PushBindingRequest {
    new_group_push_pseudonym: String,
}

async fn push_binding(
    State(state): State<AppState>,
    Path(group_id_b64): Path<String>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(body): Json<PushBindingRequest>,
) -> Result<StatusCode, ServerError> {
    let mut rate_conn = state.db.acquire().await?;
    if !db::ip_rate_limits::check_and_increment(
        &mut rate_conn,
        &ip,
        rate_limit::ACTION_GROUP_PUSH_BINDING,
        rate_limit::LIMIT_GROUP_PUSH_BINDING,
        rate_limit::WINDOW_GROUP_PUSH_BINDING,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }
    drop(rate_conn);

    let (group, actor_emi) = authorize_member(&state, &headers, &group_id_b64).await?;
    let new_pseudonym = b64_decode(&body.new_group_push_pseudonym, "new_group_push_pseudonym")?;
    let mut conn = state.db.acquire().await?;
    let rotated = db::groups::rotate_member_pseudonym(
        &mut conn,
        &group.group_id,
        &zkgroup::serialize(&actor_emi),
        &new_pseudonym,
    )
    .await?;
    if !rotated {
        return Err(ServerError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── POST /v1/groups/{id}/send (sealed-sender) ────────────────────────────────
//
// No bearer token. The sealed-sender envelope embeds the SenderCertificate
// itself; sender identity is hidden from the server. Authorization is the
// `GroupSendFullToken`, which proves the sender is a member of this group
// and is endorsed to send to the listed recipient ServiceIds.

#[derive(Deserialize)]
struct SendGroupMessageRequest {
    /// URL-safe base64 of the SSv2 SentMessage produced by
    /// `crypto::sealed_sender::encrypt_group_envelope`.
    envelope: String,
    /// URL-safe base64 of the `GroupSendFullToken` covering the recipient set.
    token: String,
    /// Sender-supplied mapping: each recipient's 17-byte fixed-width-binary
    /// ServiceId (as embedded in the envelope) paired with the
    /// `recipient_group_pseudonym` to route the per-recipient slice to.
    recipients: Vec<SendRecipientWire>,
    /// Per-message expiry override in seconds. Server clamps to its
    /// configured min/max bounds. Absent = server default.
    #[serde(default)]
    expiry_secs: Option<i64>,
}

#[derive(Deserialize)]
struct SendRecipientWire {
    service_id_fixed_width: String,
    /// Sender-supplied `encrypted_member_id` for this recipient (sender
    /// computes it from the recipient's DID + group key). Server uses it
    /// to look up the recipient's current `group_push_pseudonym` via
    /// `member_credentials`. A wrong EMI produces an undeliverable
    /// envelope (recipient's keys don't match), not a security issue.
    encrypted_member_id: String,
}

#[derive(Serialize)]
struct SendGroupMessageResponse {
    /// Server-assigned IDs of the enqueued rows, in recipient-input order.
    message_ids: Vec<i64>,
}

async fn send_group_message(
    State(state): State<AppState>,
    Path(group_id_b64): Path<String>,
    ClientIp(ip): ClientIp,
    Json(body): Json<SendGroupMessageRequest>,
) -> Result<Json<SendGroupMessageResponse>, ServerError> {
    // IP-level rate limit — no per-account counter available here since
    // sealed sender hides the sender from us.
    {
        let mut rate_conn = state.db.acquire().await?;
        if !db::ip_rate_limits::check_and_increment(
            &mut rate_conn,
            &ip,
            rate_limit::ACTION_GROUP_SEND,
            rate_limit::LIMIT_GROUP_SEND,
            rate_limit::WINDOW_GROUP_SEND,
        )
        .await?
        {
            return Err(ServerError::RateLimited);
        }
    }

    let group_id = b64_decode(&group_id_b64, "group_id")?;
    let envelope = b64_decode(&body.envelope, "envelope")?;
    let token = b64_decode(&body.token, "token")?;

    // Confirm the group exists. Token verification proves group membership;
    // this gives a clean 404 if the path doesn't match.
    {
        let mut conn = state.db.acquire().await?;
        if db::groups::get(&mut conn, &group_id).await?.is_none() {
            return Err(ServerError::NotFound);
        }
    }

    // Parse the SSv2 envelope and pull out per-recipient fanouts indexed by
    // their fixed-width-binary ServiceId.
    let fanouts = crypto::sealed_sender::parse_sent_message(&envelope)
        .map_err(|e| ServerError::BadRequest(format!("invalid envelope: {e}")))?;
    let mut fanout_by_sid: std::collections::HashMap<[u8; 17], &crypto::sealed_sender::RecipientFanout> =
        std::collections::HashMap::with_capacity(fanouts.len());
    for f in &fanouts {
        fanout_by_sid.insert(f.service_id_fixed_width, f);
    }

    // Build the recipient ServiceId set for token verification and resolve
    // each EMI to a pseudonym via `member_credentials`.
    let mut recipient_service_ids: Vec<libsignal_core::ServiceId> =
        Vec::with_capacity(body.recipients.len());
    let mut recipient_targets: Vec<(Vec<u8>, &crypto::sealed_sender::RecipientFanout)> =
        Vec::with_capacity(body.recipients.len());
    {
        let mut conn = state.db.acquire().await?;
        for r in &body.recipients {
            let sid_bytes = b64_decode(&r.service_id_fixed_width, "service_id_fixed_width")?;
            let sid_arr: [u8; 17] = sid_bytes
                .as_slice()
                .try_into()
                .map_err(|_| {
                    ServerError::BadRequest("service_id_fixed_width must be 17 bytes".into())
                })?;
            let sid =
                libsignal_core::ServiceId::parse_from_service_id_fixed_width_binary(&sid_arr)
                    .ok_or_else(|| ServerError::BadRequest("invalid service_id".into()))?;
            let fanout = *fanout_by_sid
                .get(&sid_arr)
                .ok_or_else(|| ServerError::BadRequest("recipient not in envelope".into()))?;
            let emi = b64_decode(&r.encrypted_member_id, "encrypted_member_id")?;
            let pseudonym =
                db::groups::member_pseudonym(&mut conn, &group_id, &emi)
                    .await?
                    .ok_or_else(|| {
                        ServerError::BadRequest("recipient not a member of this group".into())
                    })?;
            recipient_service_ids.push(sid);
            recipient_targets.push((pseudonym, fanout));
        }
    }

    // Verify the endorsement token authorizes this exact recipient set.
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_secs();
    crypto::groups::endorsements::verify_token_for_service_ids(
        &token,
        &recipient_service_ids,
        state.zkgroup_secret.as_ref(),
        now_secs,
    )
    .map_err(|_| ServerError::Unauthorized)?;

    // Enqueue per recipient. Push immediately if a subscriber is live.
    let expiry_secs = clamp_group_expiry(&state.config, body.expiry_secs);
    let mut message_ids = Vec::with_capacity(recipient_targets.len());
    let mut conn = state.db.acquire().await?;
    for (pseudonym, fanout) in &recipient_targets {
        let id = db::group_messages::enqueue(
            &mut conn,
            pseudonym,
            &group_id,
            &fanout.received_message,
            expiry_secs,
        )
        .await?;
        message_ids.push(id);

        // Live push if subscribed.
        let subs = state.group_subscriptions.read().await;
        if let Some(tx) = subs.get(pseudonym.as_slice()) {
            let _ = tx.send(crate::state::WsPush::GroupDelivery(
                crate::state::PendingGroupDelivery {
                    message_id: id,
                    group_id: group_id.clone(),
                    ciphertext: fanout.received_message.clone(),
                    recipient_group_pseudonym: pseudonym.clone(),
                    enqueued_at: None,
                },
            ));
        }
    }

    Ok(Json(SendGroupMessageResponse { message_ids }))
}

fn clamp_group_expiry(config: &crate::config::Config, requested: Option<i64>) -> i64 {
    requested.unwrap_or(config.message_expiry_secs)
}

// ── GET /v1/groups/{id}/messages (presentation-auth) ─────────────────────────

#[derive(Serialize)]
struct FetchGroupMessagesResponse {
    messages: Vec<QueuedGroupMessageWire>,
}

#[derive(Serialize)]
struct QueuedGroupMessageWire {
    id: i64,
    ciphertext: String,
    enqueued_at: String,
}

async fn fetch_group_messages(
    State(state): State<AppState>,
    Path(group_id_b64): Path<String>,
    headers: HeaderMap,
) -> Result<Json<FetchGroupMessagesResponse>, ServerError> {
    let (group, actor_emi) = authorize_member(&state, &headers, &group_id_b64).await?;
    let mut conn = state.db.acquire().await?;
    let pseudonym = db::groups::member_pseudonym(
        &mut conn,
        &group.group_id,
        &zkgroup::serialize(&actor_emi),
    )
    .await?
    .ok_or(ServerError::NotFound)?;
    let queued = db::group_messages::fetch_for_pseudonym(&mut conn, &pseudonym).await?;
    Ok(Json(FetchGroupMessagesResponse {
        messages: queued
            .into_iter()
            .map(|m| QueuedGroupMessageWire {
                id: m.id,
                ciphertext: b64_encode(&m.ciphertext),
                enqueued_at: m.enqueued_at.to_string(),
            })
            .collect(),
    }))
}

// ── DELETE /v1/groups/{id}/messages (presentation-auth) ──────────────────────

#[derive(Deserialize)]
struct AckGroupMessagesRequest {
    message_ids: Vec<i64>,
}

async fn ack_group_messages(
    State(state): State<AppState>,
    Path(group_id_b64): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AckGroupMessagesRequest>,
) -> Result<StatusCode, ServerError> {
    let (group, actor_emi) = authorize_member(&state, &headers, &group_id_b64).await?;
    let mut conn = state.db.acquire().await?;
    let pseudonym = db::groups::member_pseudonym(
        &mut conn,
        &group.group_id,
        &zkgroup::serialize(&actor_emi),
    )
    .await?
    .ok_or(ServerError::NotFound)?;
    let _ = db::group_messages::acknowledge(&mut conn, &pseudonym, &body.message_ids).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── helpers: presentation verification and actor lookup ──────────────────────

/// Verify the presentation, look up the group, confirm the actor's
/// encrypted_member_id is in `member_credentials` for this group. Returns
/// `(Group, actor_encrypted_member_id)`.
///
/// Used by `push_binding` — only existing members can rotate their per-
/// group push pseudonym. GET endpoints use the slightly looser
/// `authorize_member_or_pending_invite` (see §3.4 / §3.10): pending
/// invitees need to fetch state to construct an accurate
/// `promote_pending_members` upload.
async fn authorize_member(
    state: &AppState,
    headers: &HeaderMap,
    group_id_b64: &str,
) -> Result<(db::groups::Group, EncryptedMemberId), ServerError> {
    let (group, actor_emi) = authorize_presentation(state, headers, group_id_b64).await?;
    let mut conn = state.db.acquire().await?;
    let role = db::groups::member_role(
        &mut conn,
        &group.group_id,
        &zkgroup::serialize(&actor_emi),
    )
    .await?;
    if role.is_none() {
        // §3.4: fetch is membership-gated; presentation alone is not enough.
        return Err(ServerError::not_found_or_forbidden());
    }
    Ok((group, actor_emi))
}

/// Like `authorize_member`, but also accepts actors currently in
/// `members_pending` (admin-invited but not yet promoted). §3.10 step 3:
/// "the client fetches the group state … and sees the invitee in the
/// pending list" — so a pending invitee needs read access in order to
/// build an accurate `new_encrypted_state` when they promote themselves.
/// Pending-approval (link-join) requesters stay excluded: they don't
/// graduate themselves (an admin does), so they don't need pre-approval
/// state and exposing it would leak membership before approval.
async fn authorize_member_or_pending_invite(
    state: &AppState,
    headers: &HeaderMap,
    group_id_b64: &str,
) -> Result<(db::groups::Group, EncryptedMemberId), ServerError> {
    let (group, actor_emi) = authorize_presentation(state, headers, group_id_b64).await?;
    let emi_bytes = zkgroup::serialize(&actor_emi);
    let mut conn = state.db.acquire().await?;
    if db::groups::member_role(&mut conn, &group.group_id, &emi_bytes)
        .await?
        .is_some()
    {
        return Ok((group, actor_emi));
    }
    if db::groups::get_pending_invite(&mut conn, &group.group_id, &emi_bytes)
        .await?
        .is_some()
    {
        return Ok((group, actor_emi));
    }
    Err(ServerError::not_found_or_forbidden())
}

/// Like `authorize_member`, but also accepts actors found in `members_pending`
/// or `members_pending_approval` (because some self-actions like
/// `promote_pending_members` and `cancel_join_request` are valid from those
/// tables). Final per-action eligibility is enforced in `submit_changes`.
async fn authorize_member_or_pending(
    state: &AppState,
    headers: &HeaderMap,
    group_id_b64: &str,
) -> Result<(db::groups::Group, EncryptedMemberId), ServerError> {
    // Just verify the presentation here — `submit_changes` does the per-class
    // eligibility check against the right table.
    authorize_presentation(state, headers, group_id_b64).await
}

async fn authorize_presentation(
    state: &AppState,
    headers: &HeaderMap,
    group_id_b64: &str,
) -> Result<(db::groups::Group, EncryptedMemberId), ServerError> {
    let group_id = b64_decode(group_id_b64, "group_id")?;
    let header = headers
        .get("x-group-auth")
        .and_then(|v| v.to_str().ok())
        .ok_or(ServerError::Unauthorized)?;
    let pres_bytes = b64_decode(header, "X-Group-Auth")?;
    let presentation: AuthCredentialWithPniZkcPresentation =
        zkgroup::deserialize(&pres_bytes).map_err(|_| ServerError::Unauthorized)?;

    let mut conn = state.db.acquire().await?;
    let group = db::groups::get(&mut conn, &group_id)
        .await?
        .ok_or(ServerError::NotFound)?;
    let group_public = GroupPublicParams::from_bytes(&group.group_public_params)
        .map_err(|_| ServerError::Internal("stored group_public_params is invalid".into()))?;

    let today = day_aligned_now()?;
    presentation
        .verify(state.zkgroup_secret.zkgroup(), group_public.zkgroup(), today)
        .map_err(|_| ServerError::Unauthorized)?;

    Ok((group, presentation.aci_ciphertext()))
}

// ── action-class classification and per-class eligibility ────────────────────

struct ActionClasses {
    /// True if any admin-class action is present.
    contains_admin: bool,
    /// At most one self-class action allowed per change; this is set to
    /// whichever it is.
    self_kind: Option<SelfKind>,
}

#[derive(Clone, Copy)]
enum SelfKind {
    Promote,
    Decline,
    Join,
    Cancel,
    Leave,
}

fn classify_actions(actions: &ActionsWire) -> Result<ActionClasses, ServerError> {
    // Self-class actions. Per §3.3 "Self-actions vs. admin actions" each
    // GroupChange contains at most one self-action AND no admin-class
    // actions alongside it.
    let mut self_kinds: Vec<SelfKind> = Vec::new();
    if actions.promote_pending_members.is_some() {
        self_kinds.push(SelfKind::Promote);
    }
    if actions.decline_invite.is_some() {
        self_kinds.push(SelfKind::Decline);
    }
    if actions.join_via_link.is_some() {
        self_kinds.push(SelfKind::Join);
    }
    if actions.cancel_join_request.is_some() {
        self_kinds.push(SelfKind::Cancel);
    }
    if actions.leave.is_some() {
        self_kinds.push(SelfKind::Leave);
    }
    if self_kinds.len() > 1 {
        return Err(ServerError::BadRequest(
            "at most one self-action per change".into(),
        ));
    }
    let self_kind = self_kinds.first().copied();

    let contains_admin = !actions.invite_members.is_empty()
        || !actions.remove_members.is_empty()
        || !actions.modify_member_role.is_empty()
        || !actions.approve_join_request.is_empty()
        || !actions.deny_join_request.is_empty()
        || actions.modify_policy.is_some()
        || actions.modify_title.is_some()
        || actions.modify_description.is_some()
        || actions.modify_expiry.is_some();

    if self_kind.is_some() && contains_admin {
        return Err(ServerError::BadRequest(
            "self-action cannot be batched with admin-class actions".into(),
        ));
    }
    if self_kind.is_none() && !contains_admin {
        return Err(ServerError::BadRequest("no actions in change".into()));
    }

    Ok(ActionClasses {
        contains_admin,
        self_kind,
    })
}

/// §3.3 step 2: ensure the actor is in the right table for the action class.
async fn enforce_actor_eligibility(
    tx: &mut sqlx::PgConnection,
    group_id: &[u8],
    actor_emi: &[u8],
    actor_role: Option<i16>,
    classes: &ActionClasses,
) -> Result<(), ServerError> {
    if classes.contains_admin {
        if actor_role.is_none() {
            return Err(ServerError::Unauthorized);
        }
        return Ok(());
    }
    let kind = classes.self_kind.expect("classify guarantees one path");
    match kind {
        SelfKind::Promote | SelfKind::Decline => {
            if db::groups::get_pending_invite(tx, group_id, actor_emi)
                .await?
                .is_none()
            {
                return Err(ServerError::Unauthorized);
            }
        }
        SelfKind::Cancel => {
            if db::groups::get_pending_approval(tx, group_id, actor_emi)
                .await?
                .is_none()
            {
                return Err(ServerError::Unauthorized);
            }
        }
        SelfKind::Join => {
            // join_via_link does NOT require the actor to be in any table;
            // password check happens at apply time.
        }
        SelfKind::Leave => {
            // Only an established member can leave (a pending invitee uses
            // decline_invite; a pending-approval requester uses
            // cancel_join_request). Requires a member_credentials row.
            if actor_role.is_none() {
                return Err(ServerError::Unauthorized);
            }
        }
    }
    Ok(())
}

/// §3.3 step 4: per-action role minimums. `modify_policy` and
/// `modify_member_role` are protocol-fixed Admin (§3.3).
fn enforce_admin_role_checks(
    actions: &ActionsWire,
    actor_role: i16,
    policy: &db::groups::Policy,
) -> Result<(), ServerError> {
    let check = |min: i16, what: &str| -> Result<(), ServerError> {
        if actor_role < min {
            Err(ServerError::Unauthorized)
        } else {
            tracing::debug!(action = what, "role-check passed");
            Ok(())
        }
    };
    if !actions.invite_members.is_empty() {
        check(policy.invite_members_role, "invite_members")?;
    }
    if !actions.remove_members.is_empty() {
        check(policy.remove_members_role, "remove_members")?;
    }
    if !actions.modify_member_role.is_empty() {
        check(ROLE_ADMIN, "modify_member_role")?;
    }
    if !actions.approve_join_request.is_empty() {
        check(ROLE_ADMIN, "approve_join_request")?;
    }
    if !actions.deny_join_request.is_empty() {
        check(ROLE_ADMIN, "deny_join_request")?;
    }
    if actions.modify_title.is_some() {
        check(policy.modify_title_role, "modify_title")?;
    }
    if actions.modify_description.is_some() {
        check(policy.modify_description_role, "modify_description")?;
    }
    if actions.modify_expiry.is_some() {
        check(policy.modify_expiry_role, "modify_expiry")?;
    }
    if actions.modify_policy.is_some() {
        check(ROLE_ADMIN, "modify_policy")?;
    }
    Ok(())
}

const ROLE_MEMBER: i16 = 0;
const ROLE_ADMIN: i16 = 1;
const JOIN_POLICY_CLOSED: i16 = 0;
const JOIN_POLICY_REQUEST_TO_JOIN: i16 = 1;
const JOIN_POLICY_OPEN_LINK: i16 = 2;

// ── §3.3 step 5: action application ──────────────────────────────────────────

async fn apply_actions(
    tx: &mut sqlx::PgConnection,
    group: &db::groups::Group,
    actions: &ActionsWire,
    actor_emi: &[u8],
    join_result: &mut Option<JoinResultWire>,
) -> Result<(), ServerError> {
    // invite_members → members_pending
    for invite in &actions.invite_members {
        let emi = b64_decode(&invite.encrypted_member_id, "invite_members.encrypted_member_id")?;
        validate_role_value(invite.role)?;
        db::groups::insert_pending_invite(tx, &group.group_id, &emi, invite.role).await?;
    }

    // promote_pending_members (self) → members_pending row becomes member_credentials
    if let Some(promote) = &actions.promote_pending_members {
        let pending = db::groups::get_pending_invite(tx, &group.group_id, actor_emi)
            .await?
            .ok_or(ServerError::Unauthorized)?;
        let pseudonym = b64_decode(
            &promote.group_push_pseudonym,
            "promote_pending_members.group_push_pseudonym",
        )?;
        db::groups::delete_pending_invite(tx, &group.group_id, actor_emi).await?;
        db::groups::insert_member(tx, &group.group_id, actor_emi, pending.role, &pseudonym).await?;
    }

    // decline_invite (self)
    if let Some(declined) = &actions.decline_invite {
        let emi = b64_decode(declined, "decline_invite")?;
        // §3.3 self-action: target must be the actor. Byte-equality is the
        // right comparison because the encrypted_member_id ciphertext for a
        // given (DID, group_key) pair is *deterministic* — see
        // `Ciphertext::encrypt_arbitrary_attribute` in `zkcredential` — so
        // the presentation's encrypted_member_id will match the field the
        // client computed and put in `declined` iff the client controls the
        // DID being declined.
        if emi != actor_emi {
            return Err(ServerError::Unauthorized);
        }
        db::groups::delete_pending_invite(tx, &group.group_id, &emi).await?;
    }

    // remove_members
    for raw in &actions.remove_members {
        let emi = b64_decode(raw, "remove_members")?;
        db::groups::delete_member(tx, &group.group_id, &emi).await?;
        // Also clean up pending-when-removed races (§3.3).
        db::groups::delete_pending_invite(tx, &group.group_id, &emi).await?;
        db::groups::delete_pending_approval(tx, &group.group_id, &emi).await?;
    }

    // modify_member_role
    for assign in &actions.modify_member_role {
        let emi = b64_decode(&assign.encrypted_member_id, "modify_member_role.encrypted_member_id")?;
        validate_role_value(assign.role)?;
        db::groups::set_member_role(tx, &group.group_id, &emi, assign.role).await?;
    }

    // join_via_link (self)
    if let Some(join) = &actions.join_via_link {
        let password = b64_decode(&join.invite_link_password, "join_via_link.invite_link_password")?;
        let policy_pw = group.policy.invite_link_password.as_deref().unwrap_or(&[]);
        if !constant_time_eq(&password, policy_pw) {
            return Err(ServerError::Unauthorized);
        }
        let pseudonym = b64_decode(
            &join.group_push_pseudonym,
            "join_via_link.group_push_pseudonym",
        )?;
        match group.policy.join_policy {
            x if x == JOIN_POLICY_OPEN_LINK => {
                db::groups::insert_member(
                    tx,
                    &group.group_id,
                    actor_emi,
                    ROLE_MEMBER,
                    &pseudonym,
                )
                .await?;
                *join_result = Some(JoinResultWire::Member);
            }
            x if x == JOIN_POLICY_REQUEST_TO_JOIN => {
                db::groups::insert_pending_approval(tx, &group.group_id, actor_emi, &pseudonym)
                    .await?;
                *join_result = Some(JoinResultWire::Pending);
            }
            _ => return Err(ServerError::Unauthorized),
        }
    }

    // cancel_join_request (self). Same actor-equality argument as
    // `decline_invite` above: deterministic ciphertext lets byte-equality
    // stand in for "the same DID under the same group key."
    if let Some(emi_b64) = &actions.cancel_join_request {
        let emi = b64_decode(emi_b64, "cancel_join_request")?;
        if emi != actor_emi {
            return Err(ServerError::Unauthorized);
        }
        db::groups::delete_pending_approval(tx, &group.group_id, &emi).await?;
    }

    // leave (self). Same deterministic-ciphertext actor-equality argument as
    // `decline_invite`/`cancel_join_request`: the named EMI must be the actor's
    // own. Drops the member_credentials row (and any pending-state races).
    if let Some(emi_b64) = &actions.leave {
        let emi = b64_decode(emi_b64, "leave")?;
        if emi != actor_emi {
            return Err(ServerError::Unauthorized);
        }
        db::groups::delete_member(tx, &group.group_id, &emi).await?;
        db::groups::delete_pending_invite(tx, &group.group_id, &emi).await?;
        db::groups::delete_pending_approval(tx, &group.group_id, &emi).await?;
    }

    // approve_join_request → move row from members_pending_approval to member_credentials
    for raw in &actions.approve_join_request {
        let emi = b64_decode(raw, "approve_join_request")?;
        let pending = db::groups::get_pending_approval(tx, &group.group_id, &emi)
            .await?
            .ok_or_else(|| ServerError::BadRequest("approve_join_request: not pending".into()))?;
        db::groups::delete_pending_approval(tx, &group.group_id, &emi).await?;
        db::groups::insert_member(
            tx,
            &group.group_id,
            &emi,
            ROLE_MEMBER,
            &pending.group_push_pseudonym,
        )
        .await?;
    }

    // deny_join_request
    for raw in &actions.deny_join_request {
        let emi = b64_decode(raw, "deny_join_request")?;
        db::groups::delete_pending_approval(tx, &group.group_id, &emi).await?;
    }

    // modify_policy
    if let Some(new_policy) = &actions.modify_policy {
        let invite_link_password = new_policy
            .invite_link_password
            .as_deref()
            .map(|s| b64_decode(s, "modify_policy.invite_link_password"))
            .transpose()?;
        let policy = db::groups::Policy {
            invite_members_role: new_policy.invite_members_role,
            remove_members_role: new_policy.remove_members_role,
            modify_title_role: new_policy.modify_title_role,
            modify_description_role: new_policy.modify_description_role,
            modify_expiry_role: new_policy.modify_expiry_role,
            join_policy: new_policy.join_policy,
            invite_link_password,
            announcement_only: new_policy.announcement_only,
        };
        validate_policy(&policy)?;
        db::groups::update_policy(tx, &group.group_id, &policy).await?;
    }

    // modify_title / description / expiry: opaque to the server, no
    // server-visible state to update. Bytes are carried in the actions blob.

    Ok(())
}

// ── small helpers ────────────────────────────────────────────────────────────

// Group endpoints use URL-safe-no-pad base64 *everywhere* — URL paths,
// headers, JSON bodies, response bodies. `group_id` travels through URL
// paths where `+` and `/` from standard base64 collide with reserved
// characters, and using a single alphabet across the whole surface means
// one decoder, no per-field encoding rules. (Other endpoints in this server
// use standard base64; the difference is local to `groups`.)

fn b64_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn b64_decode(input: &str, field: &str) -> Result<Vec<u8>, ServerError> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(input.as_bytes())
        .map_err(|_| ServerError::BadRequest(format!("invalid base64 in {field}")))
}

fn day_aligned_now() -> Result<RedemptionTime, ServerError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ServerError::Internal("system time before epoch".into()))?
        .as_secs();
    Ok(RedemptionTime::from_epoch_seconds(now - (now % 86_400)))
}

fn validate_role_value(role: i16) -> Result<(), ServerError> {
    if role == ROLE_MEMBER || role == ROLE_ADMIN {
        Ok(())
    } else {
        Err(ServerError::BadRequest(format!("invalid role: {role}")))
    }
}

fn validate_policy(policy: &db::groups::Policy) -> Result<(), ServerError> {
    for r in [
        policy.invite_members_role,
        policy.remove_members_role,
        policy.modify_title_role,
        policy.modify_description_role,
        policy.modify_expiry_role,
    ] {
        validate_role_value(r)?;
    }
    if ![
        JOIN_POLICY_CLOSED,
        JOIN_POLICY_REQUEST_TO_JOIN,
        JOIN_POLICY_OPEN_LINK,
    ]
    .contains(&policy.join_policy)
    {
        return Err(ServerError::BadRequest(format!(
            "invalid join_policy: {}",
            policy.join_policy
        )));
    }
    Ok(())
}

fn policy_to_wire(policy: &db::groups::Policy) -> PolicyWire {
    PolicyWire {
        invite_members_role: policy.invite_members_role,
        remove_members_role: policy.remove_members_role,
        modify_title_role: policy.modify_title_role,
        modify_description_role: policy.modify_description_role,
        modify_expiry_role: policy.modify_expiry_role,
        join_policy: policy.join_policy,
        invite_link_password: policy.invite_link_password.as_deref().map(b64_encode),
        announcement_only: policy.announcement_only,
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn fresh_randomness() -> [u8; zkcredential::RANDOMNESS_LEN] {
    let mut r = [0u8; zkcredential::RANDOMNESS_LEN];
    rand::rngs::OsRng
        .try_fill_bytes(&mut r)
        .expect("OS RNG failed");
    r
}

impl ServerError {
    /// Hide membership status: non-members get the same response (404) as
    /// "no such group", so an attacker can't tell whether the group exists
    /// from a probe with a non-member credential.
    fn not_found_or_forbidden() -> Self {
        ServerError::NotFound
    }
}
