//! Client-side action-bound group flows (docs/03-groups.md §5).
//!
//! Maps the high-level FFI surface (`create_group`, `invite_member`,
//! `accept_invite`, …) onto the `crypto::groups` primitives and the
//! `net::Client` group endpoints. State is cached in `store::groups`.
//!
//! Encryption boundaries:
//! - **Group state blob** (server-opaque): client serializes
//!   `proto::groups::GroupState`, encrypts with `GroupKey::encrypt_state`,
//!   uploads as `encrypted_state`. Server stores bytes only.
//! - **Auth presentations** (per-action): client picks today's
//!   `AuthCredentialDid`, calls `present(public, group_key, rnd)`, ships
//!   the bincode-serialized presentation in the `X-Group-Auth` header.
//! - **GroupContext DM** (admin → invitee): a `ContentMessage` with the
//!   `group_context` body variant, sent over the standard sealed-sender
//!   substrate (PreKey if no session, Whisper if there is one).
//!
//! Everything here is `async` and lives inside `AppCoreInner`'s
//! tokio runtime; the FFI wrappers in `lib.rs` block on the global
//! runtime per the project's sync-FFI convention.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use prost::Message as _;
use rand::TryRngCore as _;

use crypto::groups::{
    did_to_uuid, AuthCredentialWithPniZkc, AuthCredentialWithPniZkcResponse, GroupKey,
    RedemptionTime, ServerPublicParams,
};
use libsignal_core::{Aci, Pni};
use crypto::sender_keys;
use libsignal_protocol::{DeviceId, ProtocolAddress};
use uuid::Uuid;
use net::groups::{
    CreateGroupRequest as NetCreateGroupRequest, GroupActionsWire, GroupPolicyWire,
    InviteMemberWire, JoinViaLinkWire, PromoteSelfWire, RoleAssignmentWire, SubmitChangeRequest,
};
use store::groups::{GroupRow, PolicyRow};
use types::Timestamp;

use crate::error::{AppError, AppErrorFfi};
use crate::proto::{self, content_message::Body, groups as gproto, ContentMessage};
use crate::{
    ffi_runtime, AppCore, CreatedGroupFfi, GroupSummaryFfi, JoinResultFfi, summary_to_ffi,
};

/// Seconds per day; redemption times are day-aligned (§3.11).
const DAY: u64 = 86_400;

pub const ROLE_MEMBER: i16 = 0;
pub const ROLE_ADMIN: i16 = 1;
pub const JOIN_POLICY_CLOSED: i16 = 0;
pub const JOIN_POLICY_REQUEST_TO_JOIN: i16 = 1;
pub const JOIN_POLICY_OPEN_LINK: i16 = 2;

pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn b64d(s: &str) -> Result<Vec<u8>, AppError> {
    URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .map_err(|e| AppError::Protocol(format!("base64 decode: {e}")))
}

/// Derive the libsignal `distribution_id` for a group, deterministically
/// from its zkgroup master key. All members compute the same value, so we
/// don't need to negotiate or remember per-sender state; each sender's
/// own ratchet is keyed on `(sender_address, distribution_id)` inside
/// the `SenderKeyStore`.
///
/// `distribution_id` is a 16-byte opaque blob from libsignal's
/// perspective. We derive it as the first 16 bytes of
/// `SHA-256(DOMAIN || master_key)` and pack into a `Uuid`. The domain
/// string is a fixed prefix so a hypothetical future deterministic UUID
/// over the same master key (some other purpose) can't collide.
pub fn distribution_id_for(master_key: &[u8; 32]) -> Uuid {
    use sha2::{Digest as _, Sha256};
    const DOMAIN: &[u8] = b"actnet-sk-distribution-id-v1";
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    hasher.update(master_key);
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

fn sender_protocol_address(did: &str, device_id: u32) -> ProtocolAddress {
    ProtocolAddress::new(
        did.to_string(),
        DeviceId::try_from(device_id).expect("device_id must be > 0"),
    )
}

fn day_aligned_now() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now - (now % DAY)
}

fn fresh_randomness() -> [u8; zkcredential::RANDOMNESS_LEN] {
    let mut r = [0u8; zkcredential::RANDOMNESS_LEN];
    rand::rngs::OsRng
        .try_fill_bytes(&mut r)
        .expect("OS RNG failed");
    r
}

fn fresh_pseudonym() -> Vec<u8> {
    let mut r = [0u8; 24];
    rand::rngs::OsRng
        .try_fill_bytes(&mut r)
        .expect("OS RNG failed");
    r.to_vec()
}

fn role_from_i16(role: i16) -> gproto::Role {
    match role {
        ROLE_ADMIN => gproto::Role::Admin,
        _ => gproto::Role::Member,
    }
}

fn role_to_i16(role: gproto::Role) -> i16 {
    match role {
        gproto::Role::Admin => ROLE_ADMIN,
        _ => ROLE_MEMBER,
    }
}

/// Summary of a group's decrypted state, returned to the UI.
#[derive(Debug, Clone)]
pub struct GroupSummary {
    pub group_id: String,
    /// Master key bytes (32). Caller can stash this in invite tokens.
    pub master_key: Vec<u8>,
    pub revision: i64,
    pub title: String,
    pub description: String,
    pub expiry_seconds: u32,
    pub members: Vec<GroupMember>,
    pub pending_invites: Vec<GroupPending>,
    pub pending_approvals: Vec<GroupPending>,
}

#[derive(Debug, Clone)]
pub struct GroupMember {
    pub did: String,
    pub encrypted_member_id: String,
    pub role: i16,
    pub joined_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct GroupPending {
    pub encrypted_member_id: String,
    pub timestamp_ms: i64,
}

/// Result of `join_via_link`.
#[derive(Debug, Clone, Copy)]
pub enum JoinResult {
    /// Server placed the requester directly in `member_credentials`.
    Member,
    /// Server placed the requester in `members_pending_approval`; the
    /// admins need to act before the requester is fully a member.
    Pending,
}

// ── helpers ──────────────────────────────────────────────────────────────

/// Fetch and cache the server's zkgroup public params, reusing the local
/// cached copy if its `version` matches what the server advertises.
pub async fn ensure_server_params(
    store: &store::Store,
    client: &net::Client,
    server_url: &str,
) -> Result<ServerPublicParams, AppError> {
    let fresh = client.get_group_server_params().await?;
    if let Some((cached_version, cached_bytes)) =
        store.load_group_server_params(server_url).await?
    {
        if cached_version == fresh.version && cached_bytes == fresh.params {
            return Ok(ServerPublicParams::from_bytes(&cached_bytes)?);
        }
    }
    store
        .save_group_server_params(server_url, fresh.version, &fresh.params)
        .await?;
    Ok(ServerPublicParams::from_bytes(&fresh.params)?)
}

/// Fetch today's credential (or reuse a cached one). Uses stock
/// `zkgroup::auth::AuthCredentialWithPniZkc` per §2.3; the carried
/// identity is `Aci::from(UUID(did))`.
pub async fn ensure_credential(
    store: &store::Store,
    client: &net::Client,
    server_url: &str,
    did: &str,
    public: &ServerPublicParams,
) -> Result<AuthCredentialWithPniZkc, AppError> {
    let today = day_aligned_now();
    let uuid = did_to_uuid(did);
    let aci = Aci::from(uuid);
    let pni = Pni::from(uuid);
    let redemption = RedemptionTime::from_epoch_seconds(today);

    if let Some(bytes) = store.load_group_credential(server_url, did, today).await? {
        if let Ok(cred) = zkgroup::deserialize::<AuthCredentialWithPniZkc>(&bytes) {
            return Ok(cred);
        }
    }
    let issued = client.issue_group_credential(did, today).await?;
    let response: AuthCredentialWithPniZkcResponse = zkgroup::deserialize(&issued.bytes)
        .map_err(|e| AppError::Protocol(format!("decode credential response: {e}")))?;
    let cred = response
        .receive(aci, pni, redemption, public.zkgroup())
        .map_err(|_| AppError::Protocol("credential response verification failed".into()))?;
    let cred_bytes = zkgroup::serialize(&cred);
    store
        .save_group_credential(server_url, did, today, &cred_bytes)
        .await?;
    // Drop yesterday's leftovers opportunistically.
    let _ = store.prune_group_credentials(today.saturating_sub(2 * DAY)).await;
    Ok(cred)
}

/// Build a fresh presentation for `(group_key, credential)`. zkgroup-
/// serialized; the `net::Client` base64s it for the `X-Group-Auth` header.
pub fn build_presentation_bytes(
    public: &ServerPublicParams,
    credential: &AuthCredentialWithPniZkc,
    group_key: &GroupKey,
) -> Result<Vec<u8>, AppError> {
    let presentation = credential.present(public.zkgroup(), group_key.zkgroup_secret(), fresh_randomness());
    Ok(zkgroup::serialize(&presentation))
}

fn policy_row_from_wire(policy: &GroupPolicyWire) -> Result<PolicyRow, AppError> {
    let pw = policy
        .invite_link_password
        .as_deref()
        .map(b64d)
        .transpose()?;
    Ok(PolicyRow {
        invite_members_role: policy.invite_members_role,
        remove_members_role: policy.remove_members_role,
        modify_title_role: policy.modify_title_role,
        modify_description_role: policy.modify_description_role,
        modify_expiry_role: policy.modify_expiry_role,
        join_policy: policy.join_policy,
        invite_link_password: pw,
        announcement_only: policy.announcement_only,
    })
}

fn policy_wire_from_row(p: &PolicyRow) -> GroupPolicyWire {
    GroupPolicyWire {
        invite_members_role: p.invite_members_role,
        remove_members_role: p.remove_members_role,
        modify_title_role: p.modify_title_role,
        modify_description_role: p.modify_description_role,
        modify_expiry_role: p.modify_expiry_role,
        join_policy: p.join_policy,
        invite_link_password: p.invite_link_password.as_deref().map(b64),
        announcement_only: p.announcement_only,
    }
}

fn proto_policy_from_row(p: &PolicyRow) -> gproto::Policy {
    gproto::Policy {
        invite_members_role: role_from_i16(p.invite_members_role) as i32,
        remove_members_role: role_from_i16(p.remove_members_role) as i32,
        modify_title_role: role_from_i16(p.modify_title_role) as i32,
        modify_description_role: role_from_i16(p.modify_description_role) as i32,
        modify_expiry_role: role_from_i16(p.modify_expiry_role) as i32,
        join_policy: match p.join_policy {
            JOIN_POLICY_OPEN_LINK => gproto::JoinPolicy::OpenLink as i32,
            JOIN_POLICY_REQUEST_TO_JOIN => gproto::JoinPolicy::RequestToJoin as i32,
            _ => gproto::JoinPolicy::Closed as i32,
        },
        invite_link_password: p.invite_link_password.clone().unwrap_or_default(),
        announcement_only: p.announcement_only,
    }
}

fn summary_from_state(
    row: &GroupRow,
    state: &gproto::GroupState,
) -> GroupSummary {
    GroupSummary {
        group_id: row.group_id.clone(),
        master_key: row.master_key.clone(),
        revision: row.revision,
        title: state.title.clone(),
        description: state.description.clone(),
        expiry_seconds: state.expiry_seconds,
        members: state
            .members
            .iter()
            .map(|m| GroupMember {
                did: m.did.clone(),
                encrypted_member_id: b64(&m.encrypted_member_id),
                role: role_to_i16(gproto::Role::try_from(m.role).unwrap_or(gproto::Role::Member)),
                joined_at_ms: m.joined_at_ms,
            })
            .collect(),
        pending_invites: state
            .pending_invites
            .iter()
            .map(|p| GroupPending {
                encrypted_member_id: b64(&p.encrypted_member_id),
                timestamp_ms: p.invited_at_ms,
            })
            .collect(),
        pending_approvals: state
            .pending_approvals
            .iter()
            .map(|p| GroupPending {
                encrypted_member_id: b64(&p.encrypted_member_id),
                timestamp_ms: p.requested_at_ms,
            })
            .collect(),
    }
}

// ── create group ─────────────────────────────────────────────────────────

pub struct CreatedGroup {
    pub group_id: String,
    pub master_key: Vec<u8>,
}

/// Build the founder's initial encrypted state + upload via `POST /v1/groups`.
/// The founder is themselves the only member; the group starts in `Closed`
/// state (admin-only) with no link password.
pub async fn create_group(
    store: &store::Store,
    client: &net::Client,
    server_url: &str,
    founder_did: &str,
    title: &str,
    description: &str,
    expiry_seconds: u32,
) -> Result<CreatedGroup, AppError> {
    let group_key = GroupKey::generate();
    let group_id_bytes = group_key.group_id().0;

    let founder_emi = group_key.encrypt_member_id(founder_did);
    let founder_emi_bytes = zkgroup::serialize(&founder_emi);
    let founder_pseudonym = fresh_pseudonym();
    let now_ms = Timestamp::now().as_millis();

    let founder_profile_key = store
        .load_own_profile()
        .await?
        .map(|p| p.profile_key)
        .unwrap_or_default();

    let policy = PolicyRow::default_admin_only();
    let state = gproto::GroupState {
        group_id: group_id_bytes.to_vec(),
        created_at_ms: now_ms,
        creator_did: founder_did.to_string(),
        revision: 0,
        title: title.to_string(),
        description: description.to_string(),
        expiry_seconds,
        members: vec![gproto::Member {
            did: founder_did.to_string(),
            encrypted_member_id: founder_emi_bytes.clone(),
            role: gproto::Role::Admin as i32,
            joined_at_ms: now_ms,
            profile_key: founder_profile_key,
        }],
        pending_invites: vec![],
        pending_approvals: vec![],
        policy: Some(proto_policy_from_row(&policy)),
    };
    let plaintext = state.encode_to_vec();
    let encrypted_state = group_key.encrypt_state(&plaintext);

    let req = NetCreateGroupRequest {
        group_public_params: group_key.public_params().to_bytes(),
        encrypted_state,
        founder_encrypted_member_id: founder_emi_bytes,
        founder_group_push_pseudonym: founder_pseudonym.clone(),
        policy: policy_wire_from_row(&policy),
    };
    let resp = client.create_group(&req).await?;

    // Persist locally.
    let row = GroupRow {
        group_id: resp.group_id.clone(),
        master_key: group_key.to_bytes().to_vec(),
        hosting_server_url: server_url.to_string(),
        revision: resp.revision,
        encrypted_state_plaintext: plaintext,
        policy,
        group_push_pseudonym: Some(founder_pseudonym),
        created_at: Timestamp::now(),
    };
    store.save_group(&row).await?;

    Ok(CreatedGroup {
        group_id: resp.group_id,
        master_key: group_key.to_bytes().to_vec(),
    })
}

// ── fetch group state ────────────────────────────────────────────────────

/// Pull the current encrypted state, decrypt, and update the local cache.
pub async fn fetch_group_state(
    store: &store::Store,
    client: &net::Client,
    server_url: &str,
    did: &str,
    group_id_b64_s: &str,
) -> Result<GroupSummary, AppError> {
    let row = store
        .load_group(group_id_b64_s)
        .await?
        .ok_or_else(|| AppError::Protocol("group not found in local store".into()))?;
    let group_key = GroupKey::from_bytes(
        row.master_key
            .clone()
            .try_into()
            .map_err(|_| AppError::Protocol("master_key length != 32".into()))?,
    );

    let public = ensure_server_params(store, client, server_url).await?;
    let credential = ensure_credential(store, client, server_url, did, &public).await?;
    let presentation = build_presentation_bytes(&public, &credential, &group_key)?;

    let resp = client.get_group(group_id_b64_s, &presentation).await?;
    let plaintext = group_key.decrypt_state(&resp.encrypted_state)?;
    let state = gproto::GroupState::decode(plaintext.as_slice())
        .map_err(|e| AppError::Protocol(format!("decode GroupState: {e}")))?;

    let policy = policy_row_from_wire(&resp.policy)?;
    store
        .update_group_state(group_id_b64_s, resp.revision, plaintext, policy.clone())
        .await?;

    let row = GroupRow {
        revision: resp.revision,
        policy,
        ..row
    };
    Ok(summary_from_state(&row, &state))
}

// ── action submission helpers ────────────────────────────────────────────

/// Load the group row + key from the store, fetch credential/presentation,
/// and call `submit_group_changes` with the supplied actions.
async fn submit_actions(
    store: &store::Store,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
    apply_to_state: impl FnOnce(&mut gproto::GroupState, &GroupKey) -> Result<GroupActionsWire, AppError>,
) -> Result<net::groups::SubmitChangeResponse, AppError> {
    let row = store
        .load_group(group_id_b64_s)
        .await?
        .ok_or_else(|| AppError::Protocol("group not found in local store".into()))?;
    let group_key = GroupKey::from_bytes(
        row.master_key
            .clone()
            .try_into()
            .map_err(|_| AppError::Protocol("master_key length != 32".into()))?,
    );

    let public = ensure_server_params(store, client, &row.hosting_server_url).await?;
    let credential =
        ensure_credential(store, client, &row.hosting_server_url, did, &public).await?;
    let presentation = build_presentation_bytes(&public, &credential, &group_key)?;

    // Compute optimistic new state from the cached plaintext.
    let mut state = if row.encrypted_state_plaintext.is_empty() {
        return Err(AppError::Protocol(
            "no cached group state; call fetch_group_state first".into(),
        ));
    } else {
        gproto::GroupState::decode(row.encrypted_state_plaintext.as_slice())
            .map_err(|e| AppError::Protocol(format!("decode cached GroupState: {e}")))?
    };
    let actions = apply_to_state(&mut state, &group_key)?;
    state.revision = (row.revision as u64) + 1;
    let new_plaintext = state.encode_to_vec();
    let new_encrypted_state = group_key.encrypt_state(&new_plaintext);

    let req = SubmitChangeRequest {
        revision: row.revision + 1,
        new_encrypted_state: b64(&new_encrypted_state),
        actions,
    };
    let resp = client
        .submit_group_changes(group_id_b64_s, &req, &presentation)
        .await?;

    // Persist the optimistic state on success.
    store
        .update_group_state(group_id_b64_s, resp.revision, new_plaintext, row.policy)
        .await?;

    Ok(resp)
}

// ── invite / accept / decline ────────────────────────────────────────────

pub async fn invite_member(
    store: &store::Store,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
    recipient_did: &str,
    role: i16,
) -> Result<(), AppError> {
    let _ = submit_actions(
        store,
        client,
        did,
        group_id_b64_s,
        |state, group_key| {
            let emi = group_key.encrypt_member_id(recipient_did);
            let emi_bytes = zkgroup::serialize(&emi);
            // Optimistic state update: append to pending_invites.
            state.pending_invites.push(gproto::PendingInvite {
                encrypted_member_id: emi_bytes.clone(),
                role: role_from_i16(role) as i32,
                inviter_did: did.to_string(),
                invited_at_ms: Timestamp::now().as_millis(),
            });
            Ok(GroupActionsWire {
                invite_members: vec![InviteMemberWire {
                    encrypted_member_id: b64(&emi_bytes),
                    role,
                }],
                ..Default::default()
            })
        },
    )
    .await?;
    Ok(())
}

pub async fn accept_invite(
    store: &store::Store,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
) -> Result<(), AppError> {
    let pseudonym = fresh_pseudonym();
    let pseudonym_for_state = pseudonym.clone();
    let own_profile_key = store
        .load_own_profile()
        .await?
        .map(|p| p.profile_key)
        .unwrap_or_default();

    let _ = submit_actions(
        store,
        client,
        did,
        group_id_b64_s,
        |state, group_key| {
            let self_emi = zkgroup::serialize(&group_key.encrypt_member_id(did));
            // Optimistic: remove from pending_invites and append to members.
            let role = state
                .pending_invites
                .iter()
                .find(|p| p.encrypted_member_id == self_emi)
                .map(|p| p.role)
                .unwrap_or(gproto::Role::Member as i32);
            state
                .pending_invites
                .retain(|p| p.encrypted_member_id != self_emi);
            state.members.push(gproto::Member {
                did: did.to_string(),
                encrypted_member_id: self_emi,
                role,
                joined_at_ms: Timestamp::now().as_millis(),
                profile_key: own_profile_key.clone(),
            });
            Ok(GroupActionsWire {
                promote_pending_members: Some(PromoteSelfWire {
                    encrypted_profile_key: b64(&own_profile_key),
                    group_push_pseudonym: b64(&pseudonym_for_state),
                }),
                ..Default::default()
            })
        },
    )
    .await?;
    store.set_group_push_pseudonym(group_id_b64_s, &pseudonym).await?;
    Ok(())
}

pub async fn decline_invite(
    store: &store::Store,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
) -> Result<(), AppError> {
    submit_actions(
        store,
        client,
        did,
        group_id_b64_s,
        |state, group_key| {
            let emi = zkgroup::serialize(&group_key.encrypt_member_id(did));
            state
                .pending_invites
                .retain(|p| p.encrypted_member_id != emi);
            Ok(GroupActionsWire {
                decline_invite: Some(b64(&emi)),
                ..Default::default()
            })
        },
    )
    .await?;
    Ok(())
}

// ── join via link ────────────────────────────────────────────────────────

/// Join an existing group via its master key + link password. The caller
/// must persist the group row locally *before* calling this so the
/// presentation can be built against the right group key.
pub async fn join_via_link(
    store: &store::Store,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
    password_bytes: &[u8],
) -> Result<JoinResult, AppError> {
    let pseudonym = fresh_pseudonym();
    let pseudonym_for_state = pseudonym.clone();
    let own_profile_key = store
        .load_own_profile()
        .await?
        .map(|p| p.profile_key)
        .unwrap_or_default();
    let pw_b64 = b64(password_bytes);

    let resp = submit_actions(
        store,
        client,
        did,
        group_id_b64_s,
        |_state, _group_key| {
            Ok(GroupActionsWire {
                join_via_link: Some(JoinViaLinkWire {
                    encrypted_profile_key: b64(&own_profile_key),
                    group_push_pseudonym: b64(&pseudonym_for_state),
                    invite_link_password: pw_b64,
                }),
                ..Default::default()
            })
        },
    )
    .await?;
    store.set_group_push_pseudonym(group_id_b64_s, &pseudonym).await?;
    Ok(match resp.join_landed_as_member {
        Some(true) => JoinResult::Member,
        Some(false) => JoinResult::Pending,
        None => {
            return Err(AppError::Protocol(
                "server did not return join_result for join_via_link".into(),
            ))
        }
    })
}

pub async fn cancel_join_request(
    store: &store::Store,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
) -> Result<(), AppError> {
    submit_actions(
        store,
        client,
        did,
        group_id_b64_s,
        |state, group_key| {
            let emi = zkgroup::serialize(&group_key.encrypt_member_id(did));
            state
                .pending_approvals
                .retain(|p| p.encrypted_member_id != emi);
            Ok(GroupActionsWire {
                cancel_join_request: Some(b64(&emi)),
                ..Default::default()
            })
        },
    )
    .await?;
    Ok(())
}

// ── admin-class actions ──────────────────────────────────────────────────

pub async fn approve_join_request(
    store: &store::Store,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
    encrypted_member_id_b64: &str,
) -> Result<(), AppError> {
    let emi = b64d(encrypted_member_id_b64)?;
    submit_actions(
        store,
        client,
        did,
        group_id_b64_s,
        |state, _group_key| {
            // Optimistic: move from pending_approvals → members (role Member).
            state.pending_approvals.retain(|p| p.encrypted_member_id != emi);
            state.members.push(gproto::Member {
                did: String::new(), // unknown to admin until the requester DMs them
                encrypted_member_id: emi.clone(),
                role: gproto::Role::Member as i32,
                joined_at_ms: Timestamp::now().as_millis(),
                profile_key: Vec::new(),
            });
            Ok(GroupActionsWire {
                approve_join_request: vec![b64(&emi)],
                ..Default::default()
            })
        },
    )
    .await?;
    Ok(())
}

pub async fn deny_join_request(
    store: &store::Store,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
    encrypted_member_id_b64: &str,
) -> Result<(), AppError> {
    let emi = b64d(encrypted_member_id_b64)?;
    submit_actions(
        store,
        client,
        did,
        group_id_b64_s,
        |state, _group_key| {
            state.pending_approvals.retain(|p| p.encrypted_member_id != emi);
            Ok(GroupActionsWire {
                deny_join_request: vec![b64(&emi)],
                ..Default::default()
            })
        },
    )
    .await?;
    Ok(())
}

pub async fn remove_member(
    store: &store::Store,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
    encrypted_member_id_b64: &str,
) -> Result<(), AppError> {
    let emi = b64d(encrypted_member_id_b64)?;
    submit_actions(
        store,
        client,
        did,
        group_id_b64_s,
        |state, _group_key| {
            state.members.retain(|m| m.encrypted_member_id != emi);
            state.pending_invites.retain(|p| p.encrypted_member_id != emi);
            state.pending_approvals.retain(|p| p.encrypted_member_id != emi);
            Ok(GroupActionsWire {
                remove_members: vec![b64(&emi)],
                ..Default::default()
            })
        },
    )
    .await?;
    Ok(())
}

pub async fn change_role(
    store: &store::Store,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
    encrypted_member_id_b64: &str,
    new_role: i16,
) -> Result<(), AppError> {
    let emi = b64d(encrypted_member_id_b64)?;
    submit_actions(
        store,
        client,
        did,
        group_id_b64_s,
        |state, _group_key| {
            for m in &mut state.members {
                if m.encrypted_member_id == emi {
                    m.role = role_from_i16(new_role) as i32;
                }
            }
            Ok(GroupActionsWire {
                modify_member_role: vec![RoleAssignmentWire {
                    encrypted_member_id: b64(&emi),
                    role: new_role,
                }],
                ..Default::default()
            })
        },
    )
    .await?;
    Ok(())
}

// ── apply_pending_changes / rotate ───────────────────────────────────────

/// Pull `/changes` since the last applied revision, decrypt each blob, and
/// fast-forward the local cache.
pub async fn apply_pending_changes(
    store: &store::Store,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
) -> Result<i64, AppError> {
    let row = store
        .load_group(group_id_b64_s)
        .await?
        .ok_or_else(|| AppError::Protocol("group not found".into()))?;
    let group_key = GroupKey::from_bytes(
        row.master_key
            .clone()
            .try_into()
            .map_err(|_| AppError::Protocol("master_key length != 32".into()))?,
    );

    let public = ensure_server_params(store, client, &row.hosting_server_url).await?;
    let credential =
        ensure_credential(store, client, &row.hosting_server_url, did, &public).await?;
    let presentation = build_presentation_bytes(&public, &credential, &group_key)?;

    let resp = client
        .get_group_changes(group_id_b64_s, row.revision, &presentation)
        .await?;

    let Some(last) = resp.changes.last() else {
        return Ok(row.revision);
    };
    let plaintext = group_key.decrypt_state(&last.encrypted_state)?;
    // We don't re-fetch the policy on a /changes pull — the server
    // includes policy changes in actions; a follow-up fetch_group_state
    // call is the simplest way to re-sync the policy mirror. For now we
    // keep the cached policy as-is. (The server still enforces against its
    // authoritative copy.)
    store
        .update_group_state(group_id_b64_s, last.revision, plaintext, row.policy)
        .await?;
    Ok(last.revision)
}

/// Generate a fresh `group_push_pseudonym` and rotate it on the server.
/// (§3.7 rotation.) Returns the new pseudonym bytes for caller-side relay
/// registration.
pub async fn rotate_group_pseudonym(
    store: &store::Store,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
) -> Result<Vec<u8>, AppError> {
    let row = store
        .load_group(group_id_b64_s)
        .await?
        .ok_or_else(|| AppError::Protocol("group not found".into()))?;
    let group_key = GroupKey::from_bytes(
        row.master_key
            .clone()
            .try_into()
            .map_err(|_| AppError::Protocol("master_key length != 32".into()))?,
    );
    let public = ensure_server_params(store, client, &row.hosting_server_url).await?;
    let credential =
        ensure_credential(store, client, &row.hosting_server_url, did, &public).await?;
    let presentation = build_presentation_bytes(&public, &credential, &group_key)?;
    let new_pseudonym = fresh_pseudonym();
    client
        .rotate_group_push_binding(group_id_b64_s, &new_pseudonym, &presentation)
        .await?;
    store
        .set_group_push_pseudonym(group_id_b64_s, &new_pseudonym)
        .await?;
    Ok(new_pseudonym)
}

// ── Sender Keys: SKDM exchange + content encrypt/decrypt ────────────────
//
// All four entry points below are thin wrappers around `crypto::sender_keys`
// that translate between our `(did, device_id, master_key)` model and
// libsignal's `(ProtocolAddress, distribution_id)` API. They take the
// store directly so callers can sequence DM I/O around them without
// holding the mutex across the libsignal calls.

/// Generate (or refresh) the local sender key for this group; return
/// the wire bytes the caller should ship to every other member.
/// Idempotent — repeated calls within one chain return matching bytes.
pub async fn seed_own_sender_key(
    store: &mut store::Store,
    did: &str,
    device_id: u32,
    master_key: &[u8; 32],
) -> Result<Vec<u8>, AppError> {
    let dist_id = distribution_id_for(master_key);
    let sender = sender_protocol_address(did, device_id);
    let skdm = sender_keys::create_skdm(store, &sender, dist_id).await?;
    Ok(skdm)
}

/// Install a peer's distribution message into the local
/// `SenderKeyStore`. After this completes, `decrypt_group_content` calls
/// for messages from `(sender_did, sender_device_id)` will succeed.
pub async fn process_inbound_skdm(
    store: &mut store::Store,
    sender_did: &str,
    sender_device_id: u32,
    skdm_bytes: &[u8],
) -> Result<(), AppError> {
    let sender = sender_protocol_address(sender_did, sender_device_id);
    sender_keys::process_skdm(store, &sender, skdm_bytes).await?;
    Ok(())
}

/// Encrypt `plaintext` under our own sender key for the group. Output is
/// a serialized `SenderKeyMessage` ready to ship inside a
/// `proto::GroupMessage`.
pub async fn encrypt_group_content(
    store: &mut store::Store,
    did: &str,
    device_id: u32,
    master_key: &[u8; 32],
    plaintext: &[u8],
) -> Result<Vec<u8>, AppError> {
    let dist_id = distribution_id_for(master_key);
    let sender = sender_protocol_address(did, device_id);
    let ct = sender_keys::group_encrypt(store, &sender, dist_id, plaintext).await?;
    Ok(ct)
}

/// Decrypt a `SenderKeyMessage` (received inside a `proto::GroupMessage`)
/// using the locally cached sender key for `(sender_did, sender_device_id)`.
pub async fn decrypt_group_content(
    store: &mut store::Store,
    sender_did: &str,
    sender_device_id: u32,
    ciphertext: &[u8],
) -> Result<Vec<u8>, AppError> {
    let sender = sender_protocol_address(sender_did, sender_device_id);
    let plaintext = sender_keys::group_decrypt(store, &sender, ciphertext).await?;
    Ok(plaintext)
}

/// Read the cached `GroupState` plaintext for `group_id_b64_s` and
/// return the DIDs of every current full member except `excluding_did`.
/// Used to enumerate SKDM-distribution and group-send recipients.
pub async fn other_member_dids(
    store: &store::Store,
    group_id_b64_s: &str,
    excluding_did: &str,
) -> Result<Vec<String>, AppError> {
    let row = store
        .load_group(group_id_b64_s)
        .await?
        .ok_or_else(|| AppError::Protocol("group not found".into()))?;
    if row.encrypted_state_plaintext.is_empty() {
        return Ok(Vec::new());
    }
    let state = gproto::GroupState::decode(row.encrypted_state_plaintext.as_slice())
        .map_err(|e| AppError::Protocol(format!("decode cached GroupState: {e}")))?;
    Ok(state
        .members
        .into_iter()
        .filter_map(|m| {
            if !m.did.is_empty() && m.did != excluding_did {
                Some(m.did)
            } else {
                None
            }
        })
        .collect())
}

/// Master-key bytes for a stored group. Convenience: most call sites need
/// the 32-byte array form, not the raw `Vec`.
pub async fn master_key_for(
    store: &store::Store,
    group_id_b64_s: &str,
) -> Result<[u8; 32], AppError> {
    let row = store
        .load_group(group_id_b64_s)
        .await?
        .ok_or_else(|| AppError::Protocol("group not found".into()))?;
    row.master_key
        .try_into()
        .map_err(|_| AppError::Protocol("master_key length != 32".into()))
}

// ── inbound GroupContext DM ──────────────────────────────────────────────

/// Persist a freshly received `GroupContext` DM. The caller has already
/// decrypted the message envelope; this records the master key locally so
/// `fetch_group_state` works.
///
/// Does NOT fetch state — the caller follows up with `fetch_group_state`
/// once it's ready to render the pending list. (Keeping these separate
/// lets the inbound message hot path stay synchronous-ish.)
pub async fn store_inbound_group_context(
    store: &store::Store,
    master_key: &[u8],
    hosting_server_url: &str,
) -> Result<String, AppError> {
    if master_key.len() != 32 {
        return Err(AppError::Protocol("master_key length != 32".into()));
    }
    let mut mk = [0u8; 32];
    mk.copy_from_slice(master_key);
    let group_key = GroupKey::from_bytes(mk);
    let group_id_b64_s = b64(&group_key.group_id().0);

    if store.load_group(&group_id_b64_s).await?.is_some() {
        // Already known — silently drop. The invitee may have stored the
        // master key on a sibling device and synced over.
        return Ok(group_id_b64_s);
    }

    let row = GroupRow {
        group_id: group_id_b64_s.clone(),
        master_key: master_key.to_vec(),
        hosting_server_url: hosting_server_url.to_string(),
        revision: 0,
        encrypted_state_plaintext: Vec::new(),
        policy: PolicyRow::default_admin_only(),
        group_push_pseudonym: None,
        created_at: Timestamp::now(),
    };
    store.save_group(&row).await?;
    Ok(group_id_b64_s)
}

// ── FFI surface ─────────────────────────────────────────────────────────
//
// Sync wrappers that block on the global tokio runtime. Each one is a
// thin shim around the `async` business logic above; keep new logic out
// of this section.

#[uniffi::export]
impl AppCore {
    /// Create a new action-bound group. Returns the server-visible
    /// `group_id` (URL-safe-no-pad base64) and the 32-byte master key the
    /// founder should distribute to invitees.
    pub fn create_group(
        &self,
        title: String,
        description: String,
        expiry_seconds: u32,
    ) -> Result<CreatedGroupFfi, AppErrorFfi> {
        ffi_runtime().block_on(async {
            let mut inner = self.inner.lock().await;
            let did = inner.did.clone();
            let device_id = inner.device_id;
            let server_url = inner.client.server_url().to_string();
            let created = create_group(
                &inner.store,
                &inner.client,
                &server_url,
                &did,
                &title,
                &description,
                expiry_seconds,
            )
            .await?;
            // Seed our own sender key for this group locally. No other
            // members exist yet, so there's no SKDM to ship — the
            // recipients of future invites will receive it as part of
            // the invite-flow DM.
            let mk: [u8; 32] = created
                .master_key
                .clone()
                .try_into()
                .map_err(|_| AppError::Protocol("master_key length != 32".into()))?;
            let _ = seed_own_sender_key(&mut inner.store, &did, device_id, &mk).await?;
            Ok::<_, AppError>(CreatedGroupFfi {
                group_id: created.group_id,
                master_key: created.master_key,
            })
        })
        .map_err(AppErrorFfi::from)
    }

    pub fn fetch_group_state(
        &self,
        group_id: String,
    ) -> Result<GroupSummaryFfi, AppErrorFfi> {
        ffi_runtime().block_on(async {
            let inner = self.inner.lock().await;
            let did = inner.did.clone();
            let server_url = inner.client.server_url().to_string();
            let summary =
                fetch_group_state(&inner.store, &inner.client, &server_url, &did, &group_id)
                    .await?;
            Ok::<_, AppError>(summary_to_ffi(summary))
        })
        .map_err(AppErrorFfi::from)
    }

    /// Submit `invite_members` and send the per-recipient `GroupContext`
    /// substrate DM (§3.10) plus an SKDM so the invitee can decrypt our
    /// future group messages. Both DMs are best-effort on the wire —
    /// the server-side pending row is the recoverable receipt; if a DM
    /// is lost the invitee's client surfaces "this invite looks stale".
    pub fn invite_member(
        &self,
        group_id: String,
        recipient_did: String,
        role: i16,
    ) -> Result<(), AppErrorFfi> {
        ffi_runtime().block_on(async {
            let ws = self.ws.lock().expect("ws mutex poisoned").clone();
            let mut inner = self.inner.lock().await;
            let did = inner.did.clone();
            let device_id = inner.device_id;

            // Resolve hosting server URL from the local row before issuing
            // any actions; the GroupContext DM needs it.
            let row = inner
                .store
                .load_group(&group_id)
                .await?
                .ok_or_else(|| AppError::Protocol("group not found".into()))?;
            let hosting_server_url = row.hosting_server_url.clone();
            let master_key_vec = row.master_key.clone();
            let mk: [u8; 32] = master_key_vec
                .clone()
                .try_into()
                .map_err(|_| AppError::Protocol("master_key length != 32".into()))?;
            let group_id_bytes = b64d(&group_id)?;

            invite_member(
                &inner.store,
                &inner.client,
                &did,
                &group_id,
                &recipient_did,
                role,
            )
            .await?;

            // Generate (or refresh) our own SKDM for this group, so we
            // can ship it alongside the GroupContext.
            let skdm = seed_own_sender_key(&mut inner.store, &did, device_id, &mk).await?;

            // Send GroupContext to the invitee. Errors are logged but not
            // propagated — the pending row is the receipt.
            let ctx = ContentMessage {
                body: Some(Body::GroupContext(proto::GroupContext {
                    group_master_key: master_key_vec,
                    hosting_server_url,
                    inviter_did: did.clone(),
                    invited_at_ms: Timestamp::now().as_millis(),
                    invite_link_password: Vec::new(),
                })),
                timestamp_ms: Timestamp::now().as_millis() as u64,
                profile_key: inner.own_profile_key().await,
            };
            if let Err(e) = inner
                .send_dm(ws.as_ref(), &recipient_did, &ctx.encode_to_vec(), None)
                .await
            {
                tracing::warn!("[groups] GroupContext DM to {recipient_did} failed: {e}");
            }

            // Send our SKDM as a separate DM. Same best-effort handling —
            // if it's lost, the invitee will see our future group
            // messages as undecryptable, and we re-send on demand (TODO:
            // missing-key recovery path).
            let skdm_msg = ContentMessage {
                body: Some(Body::SenderKeyDistribution(proto::SenderKeyDistribution {
                    group_id: group_id_bytes,
                    distribution_id: distribution_id_for(&mk).as_bytes().to_vec(),
                    skdm,
                })),
                timestamp_ms: Timestamp::now().as_millis() as u64,
                profile_key: Vec::new(),
            };
            if let Err(e) = inner
                .send_dm(ws.as_ref(), &recipient_did, &skdm_msg.encode_to_vec(), None)
                .await
            {
                tracing::warn!("[groups] SKDM DM to {recipient_did} failed: {e}");
            }

            Ok::<_, AppError>(())
        })
        .map_err(AppErrorFfi::from)
    }

    pub fn accept_invite(&self, group_id: String) -> Result<(), AppErrorFfi> {
        ffi_runtime().block_on(async {
            let ws = self.ws.lock().expect("ws mutex poisoned").clone();
            let mut inner = self.inner.lock().await;
            let did = inner.did.clone();
            let device_id = inner.device_id;
            accept_invite(&inner.store, &inner.client, &did, &group_id).await?;

            // Now that we're a full member, generate our own sender key
            // and distribute it to every existing member so they can
            // decrypt our future group messages.
            let mk = master_key_for(&inner.store, &group_id).await?;
            let skdm = seed_own_sender_key(&mut inner.store, &did, device_id, &mk).await?;
            let group_id_bytes = b64d(&group_id)?;
            let recipients = other_member_dids(&inner.store, &group_id, &did).await?;
            for rdid in recipients {
                let skdm_msg = ContentMessage {
                    body: Some(Body::SenderKeyDistribution(
                        proto::SenderKeyDistribution {
                            group_id: group_id_bytes.clone(),
                            distribution_id: distribution_id_for(&mk).as_bytes().to_vec(),
                            skdm: skdm.clone(),
                        },
                    )),
                    timestamp_ms: Timestamp::now().as_millis() as u64,
                    profile_key: Vec::new(),
                };
                if let Err(e) = inner
                    .send_dm(ws.as_ref(), &rdid, &skdm_msg.encode_to_vec(), None)
                    .await
                {
                    tracing::warn!("[groups] SKDM DM to {rdid} failed: {e}");
                }
            }
            Ok::<_, AppError>(())
        })
        .map_err(AppErrorFfi::from)
    }

    pub fn decline_invite(&self, group_id: String) -> Result<(), AppErrorFfi> {
        ffi_runtime().block_on(async {
            let inner = self.inner.lock().await;
            let did = inner.did.clone();
            decline_invite(&inner.store, &inner.client, &did, &group_id).await
        })
        .map_err(AppErrorFfi::from)
    }

    /// Join via an invite link. `master_key` must be the 32-byte zkgroup
    /// master key from the link; `hosting_server_url` is the homeserver
    /// hosting the group; `password` is the optional link password (empty
    /// vec when none is required).
    pub fn join_via_link(
        &self,
        master_key: Vec<u8>,
        hosting_server_url: String,
        password: Vec<u8>,
    ) -> Result<JoinResultFfi, AppErrorFfi> {
        ffi_runtime().block_on(async {
            let inner = self.inner.lock().await;
            let did = inner.did.clone();
            let group_id =
                store_inbound_group_context(&inner.store, &master_key, &hosting_server_url)
                    .await?;
            let result =
                join_via_link(&inner.store, &inner.client, &did, &group_id, &password).await?;
            Ok::<_, AppError>(match result {
                JoinResult::Member => JoinResultFfi::Member,
                JoinResult::Pending => JoinResultFfi::Pending,
            })
        })
        .map_err(AppErrorFfi::from)
    }

    pub fn cancel_join_request(&self, group_id: String) -> Result<(), AppErrorFfi> {
        ffi_runtime().block_on(async {
            let inner = self.inner.lock().await;
            let did = inner.did.clone();
            cancel_join_request(&inner.store, &inner.client, &did, &group_id).await
        })
        .map_err(AppErrorFfi::from)
    }

    pub fn approve_join_request(
        &self,
        group_id: String,
        encrypted_member_id: String,
    ) -> Result<(), AppErrorFfi> {
        ffi_runtime().block_on(async {
            let inner = self.inner.lock().await;
            let did = inner.did.clone();
            approve_join_request(
                &inner.store,
                &inner.client,
                &did,
                &group_id,
                &encrypted_member_id,
            )
            .await
        })
        .map_err(AppErrorFfi::from)
    }

    pub fn deny_join_request(
        &self,
        group_id: String,
        encrypted_member_id: String,
    ) -> Result<(), AppErrorFfi> {
        ffi_runtime().block_on(async {
            let inner = self.inner.lock().await;
            let did = inner.did.clone();
            deny_join_request(
                &inner.store,
                &inner.client,
                &did,
                &group_id,
                &encrypted_member_id,
            )
            .await
        })
        .map_err(AppErrorFfi::from)
    }

    pub fn remove_member(
        &self,
        group_id: String,
        encrypted_member_id: String,
    ) -> Result<(), AppErrorFfi> {
        ffi_runtime().block_on(async {
            let inner = self.inner.lock().await;
            let did = inner.did.clone();
            remove_member(
                &inner.store,
                &inner.client,
                &did,
                &group_id,
                &encrypted_member_id,
            )
            .await
        })
        .map_err(AppErrorFfi::from)
    }

    pub fn change_member_role(
        &self,
        group_id: String,
        encrypted_member_id: String,
        new_role: i16,
    ) -> Result<(), AppErrorFfi> {
        ffi_runtime().block_on(async {
            let inner = self.inner.lock().await;
            let did = inner.did.clone();
            change_role(
                &inner.store,
                &inner.client,
                &did,
                &group_id,
                &encrypted_member_id,
                new_role,
            )
            .await
        })
        .map_err(AppErrorFfi::from)
    }

    /// Pull `/changes` since the last applied revision. Returns the new
    /// revision (== previous if nothing was pending).
    pub fn apply_pending_group_changes(&self, group_id: String) -> Result<i64, AppErrorFfi> {
        ffi_runtime().block_on(async {
            let inner = self.inner.lock().await;
            let did = inner.did.clone();
            apply_pending_changes(&inner.store, &inner.client, &did, &group_id).await
        })
        .map_err(AppErrorFfi::from)
    }

    /// Send a group message to every other current member. Encrypted
    /// once under our Sender Key for the group, then fanned out as a
    /// per-recipient DM carrying the same `proto::GroupMessage` body.
    ///
    /// This Stage 5 path uses the existing /v1/messages transport. The
    /// sealed-sender wrapping and dedicated send endpoint (§3.11) layer
    /// in at PR 2.
    pub fn send_group_message(
        &self,
        group_id: String,
        plaintext: Vec<u8>,
    ) -> Result<(), AppErrorFfi> {
        ffi_runtime().block_on(async {
            let ws = self.ws.lock().expect("ws mutex poisoned").clone();
            let mut inner = self.inner.lock().await;
            let did = inner.did.clone();
            let device_id = inner.device_id;
            let mk = master_key_for(&inner.store, &group_id).await?;
            let ciphertext =
                encrypt_group_content(&mut inner.store, &did, device_id, &mk, &plaintext).await?;
            let group_id_bytes = b64d(&group_id)?;
            let msg = ContentMessage {
                body: Some(Body::GroupMessage(proto::GroupMessage {
                    group_id: group_id_bytes,
                    distribution_id: distribution_id_for(&mk).as_bytes().to_vec(),
                    ciphertext,
                })),
                timestamp_ms: Timestamp::now().as_millis() as u64,
                profile_key: inner.own_profile_key().await,
            };
            let encoded = msg.encode_to_vec();
            let recipients = other_member_dids(&inner.store, &group_id, &did).await?;
            for rdid in recipients {
                if let Err(e) = inner.send_dm(ws.as_ref(), &rdid, &encoded, None).await {
                    tracing::warn!("[groups] GroupMessage DM to {rdid} failed: {e}");
                }
            }
            Ok::<_, AppError>(())
        })
        .map_err(AppErrorFfi::from)
    }

    /// Generate a fresh per-group push pseudonym and rotate it on the
    /// server. Returns the new pseudonym bytes; the caller should
    /// re-register with the relay (`register_push_with_relay`).
    pub fn rotate_group_pseudonym(&self, group_id: String) -> Result<Vec<u8>, AppErrorFfi> {
        ffi_runtime().block_on(async {
            let inner = self.inner.lock().await;
            let did = inner.did.clone();
            rotate_group_pseudonym(&inner.store, &inner.client, &did, &group_id).await
        })
        .map_err(AppErrorFfi::from)
    }
}
