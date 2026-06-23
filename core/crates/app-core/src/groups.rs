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
use std::collections::HashMap;
use store::groups::{GroupRow, PolicyRow};
use types::Timestamp;

use crate::error::{AppError, AppErrorFfi};
use crate::proto::{self, content_message::Body, groups as gproto, ContentMessage};
use crate::{
    ffi_runtime, AppCore, CreatedGroupFfi, GroupSummaryFfi, JoinResultFfi,
    summary_to_ffi,
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
        crypto::groups::did_to_service_id_string(did),
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

/// Kind of a derived group timeline entry (docs/03 §3.6), reconstructed from a
/// `GroupChange`'s actions plus the surrounding state snapshots. Keep this list
/// **append-only** — the numeric `kind_code` is persisted in
/// `message_history.kind`.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupEventKind {
    /// A pending invitee accepted (promoted themselves into the group).
    MemberJoined,
    /// Someone joined directly via an open invite link.
    MemberJoinedViaLink,
    /// Someone requested to join via a request-to-join link (awaits approval).
    MemberRequestedToJoin,
    /// An admin invited someone (they're now pending).
    MemberInvited,
    /// A member removed themselves.
    MemberLeft,
    /// An admin removed another member.
    MemberRemoved,
    /// An admin approved a pending join request.
    JoinRequestApproved,
    /// An admin denied a pending join request.
    JoinRequestDenied,
    /// An invitee declined an invitation.
    InviteDeclined,
    /// A requester cancelled their own join request.
    JoinRequestCancelled,
    /// A member was promoted to admin.
    RoleChangedToAdmin,
    /// A member's admin role was removed.
    RoleChangedToMember,
    /// The group title changed.
    TitleChanged,
    /// The group description changed.
    DescriptionChanged,
    /// The disappearing-message timer changed.
    ExpiryChanged,
    /// The group policy (join policy / announcement-only / link password) changed.
    PolicyChanged,
}

/// Stable numeric code for a [`GroupEventKind`], persisted in
/// `message_history.kind` (offset by +1 so 0 stays "normal chat message").
pub fn kind_code(kind: GroupEventKind) -> i64 {
    match kind {
        GroupEventKind::MemberJoined => 1,
        GroupEventKind::MemberJoinedViaLink => 2,
        GroupEventKind::MemberRequestedToJoin => 3,
        GroupEventKind::MemberInvited => 4,
        GroupEventKind::MemberLeft => 5,
        GroupEventKind::MemberRemoved => 6,
        GroupEventKind::JoinRequestApproved => 7,
        GroupEventKind::JoinRequestDenied => 8,
        GroupEventKind::InviteDeclined => 9,
        GroupEventKind::JoinRequestCancelled => 10,
        GroupEventKind::RoleChangedToAdmin => 11,
        GroupEventKind::RoleChangedToMember => 12,
        GroupEventKind::TitleChanged => 13,
        GroupEventKind::DescriptionChanged => 14,
        GroupEventKind::ExpiryChanged => 15,
        GroupEventKind::PolicyChanged => 16,
    }
}

/// One derived chat-timeline entry describing a membership/metadata change
/// (docs/03 §3.6). Persisted as a system row in `message_history` and surfaced
/// to the UI via `IncomingEvent::GroupMetadataChanged`.
#[derive(uniffi::Record, Debug, Clone, PartialEq)]
pub struct GroupMetadataEvent {
    /// URL-safe-no-pad base64 group_id this entry belongs to.
    pub group_id: String,
    /// Revision this change produced (the post-change revision).
    pub revision: i64,
    pub kind: GroupEventKind,
    /// DID of the member who performed the change, from the sub-encrypted
    /// `change_meta`. Empty when attribution was unavailable (pre-§3.6 change).
    pub actor_did: String,
    /// DID of the member the change is about, when resolvable from the
    /// surrounding state. Empty when the target's DID isn't known to us (e.g. a
    /// still-pending invitee whose DID we never learned).
    pub target_did: String,
    /// base64 encrypted_member_id of the target, when the action names one.
    pub target_emi: String,
    /// Best-effort millis the change occurred — pulled from a relevant state
    /// timestamp (join/invite/request) where available, else fill time.
    pub occurred_at_ms: i64,
    /// For `ExpiryChanged`, the new disappearing-message timer in seconds
    /// (`0` = off). Lets the UI render "set disappearing messages to 4 weeks"
    /// without a separate state lookup. `0` for all other kinds.
    pub expiry_seconds: u32,
    /// For `TitleChanged`, the new group name, so the UI can render
    /// "changed the group name to 'X'". Empty for all other kinds.
    pub new_title: String,
    /// Pre-rendered English one-liner (using DIDs, not display names) for
    /// headless consumers / logging. UIs should render from the structured
    /// fields instead, resolving DIDs to display names.
    pub summary: String,
}

// ── helpers ──────────────────────────────────────────────────────────────

/// Fetch and cache the server's zkgroup public params + sender-cert trust
/// root, reusing the local cached copy if its `version` matches what the
/// server advertises.
pub async fn ensure_server_params(
    store: &store::DeviceStore,
    client: &net::Client,
    server_url: &str,
) -> Result<ServerPublicParams, AppError> {
    // Fast path: trust the cached params. zkgroup server params are
    // version-pinned and effectively immutable for the life of a server
    // (rotating them would invalidate every outstanding credential and group),
    // so we don't pay a round trip on every group operation just to re-confirm
    // the version — that confirmation is what made simply opening a group hit
    // the network. Mirrors `load_sender_cert_trust_root`, which already trusts
    // this same cache without a version check.
    if let Some((_version, cached_bytes, _trust_root)) =
        store.load_group_server_params(server_url).await?
    {
        return Ok(ServerPublicParams::from_bytes(&cached_bytes)?);
    }
    // Cold cache — fetch once and persist. (A genuine param rotation would only
    // surface as a crypto verification failure; refreshing on that is a separate
    // change, not handled here.)
    let fresh = client.get_group_server_params().await?;
    store
        .save_group_server_params(
            server_url,
            fresh.version,
            &fresh.params,
            &fresh.sender_cert_trust_root,
        )
        .await?;
    Ok(ServerPublicParams::from_bytes(&fresh.params)?)
}

/// Load the pinned sender-cert trust-root public key for a given homeserver,
/// populating the cache via [`ensure_server_params`] if it isn't there yet.
/// Used by the sealed-sender group decrypt path to validate sender certs.
pub async fn load_sender_cert_trust_root(
    store: &store::DeviceStore,
    client: &net::Client,
    server_url: &str,
) -> Result<Vec<u8>, AppError> {
    if let Some((_v, _bytes, trust_root)) = store.load_group_server_params(server_url).await? {
        return Ok(trust_root);
    }
    // Cold cache — populate it.
    let _ = ensure_server_params(store, client, server_url).await?;
    let (_v, _bytes, trust_root) = store
        .load_group_server_params(server_url)
        .await?
        .expect("just populated cache");
    Ok(trust_root)
}

/// Fetch today's credential (or reuse a cached one). Uses stock
/// `zkgroup::auth::AuthCredentialWithPniZkc` per §2.3; the carried
/// identity is `Aci::from(UUID(did))`.
pub async fn ensure_credential(
    store: &store::DeviceStore,
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

    if let Some((bytes, _sender_cert, _exp)) =
        store.load_group_credential(server_url, did, today).await?
    {
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
        .save_group_credential(
            server_url,
            did,
            today,
            &cred_bytes,
            &issued.sender_cert,
            issued.sender_cert_expires_at_unix_millis,
        )
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
    store: &store::DeviceStore,
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
        // No prior change produced revision 0 — it's the founder's creation.
        last_change_actor_did: String::new(),
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
    store: &store::DeviceStore,
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

    // Populate contact_profiles for fellow members from the profile keys carried
    // in the group state, so the member list can show display names. A human's
    // name is only knowable via their profile key, and for someone we've never
    // DMed the *only* place we see that key is the group roster — without this,
    // such members render as "Unknown" (e.g. a second account joining #admins
    // sees the first member with no name).
    prime_member_profiles(store, client, did, &state.members).await;

    let row = GroupRow {
        revision: resp.revision,
        policy,
        ..row
    };
    Ok(summary_from_state(&row, &state))
}

/// Build a `GroupSummary` from the locally-cached state only — no network.
/// Used to show last-known group info for a group we've left, where a server
/// fetch is membership-gated and would 404 (docs/53 §Leave). Returns `None` if
/// the group isn't in the local store or has no cached state yet.
pub async fn cached_group_state(
    store: &store::DeviceStore,
    group_id_b64_s: &str,
) -> Result<Option<GroupSummary>, AppError> {
    let Some(row) = store.load_group(group_id_b64_s).await? else {
        return Ok(None);
    };
    if row.encrypted_state_plaintext.is_empty() {
        return Ok(None);
    }
    let state = gproto::GroupState::decode(row.encrypted_state_plaintext.as_slice())
        .map_err(|e| AppError::Protocol(format!("decode cached GroupState: {e}")))?;
    Ok(Some(summary_from_state(&row, &state)))
}

/// Best-effort: for each group member other than ourselves whose roster entry
/// carries a profile key we don't already have cached, fetch + decrypt their
/// profile blob and upsert it into `contact_profiles`. Mirrors
/// `AppCoreInner::handle_inbound_profile_key` (the DM path) but sourced from the
/// group roster instead of an inbound message. Errors are swallowed — a missing
/// or undecryptable profile just leaves that member unresolved, never blocking
/// the roster load.
async fn prime_member_profiles(
    store: &store::DeviceStore,
    client: &net::Client,
    self_did: &str,
    members: &[gproto::Member],
) {
    for m in members {
        if m.did == self_did || m.profile_key.len() != crate::profile::PROFILE_KEY_LEN {
            continue;
        }
        // Skip if we already hold this exact key (the name is cached with it).
        match store.load_contact_profile_key(&m.did).await {
            Ok(Some(cached)) if cached == m.profile_key => continue,
            Ok(_) => {}
            Err(_) => continue,
        }
        let blob = match client.get_profile(&m.did).await {
            Ok(Some(b)) => b,
            _ => continue,
        };
        let mut key = [0u8; crate::profile::PROFILE_KEY_LEN];
        key.copy_from_slice(&m.profile_key);
        let plaintext = match crate::profile::decrypt_profile(&blob, &key) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let _ = store
            .upsert_contact_profile(&store::profiles::ContactProfile {
                did: m.did.clone(),
                display_name: plaintext.display_name,
                profile_key: m.profile_key.clone(),
                fetched_at: Timestamp::now(),
            })
            .await;
    }
}

// ── action submission helpers ────────────────────────────────────────────

/// Load the group row + key from the store, fetch credential/presentation,
/// and call `submit_group_changes` with the supplied actions.
async fn submit_actions(
    store: &store::DeviceStore,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
    apply_to_state: impl Fn(&mut gproto::GroupState, &GroupKey) -> Result<GroupActionsWire, AppError>,
) -> Result<(net::groups::SubmitChangeResponse, Vec<GroupMetadataEvent>), AppError> {
    // We retry once on stale-revision (HTTP 409). The cached revision can
    // lag the server's whenever another party submits a change (e.g. a
    // new member accepts an invite while we're inviting somebody else).
    // After the retry, if we still race, the second 409 is propagated and
    // the caller can re-invoke.
    let mut attempt = 0;
    loop {
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
        // Snapshot the pre-change state so we can derive the actor's own
        // timeline entry below (the actor never re-pulls their own change via
        // `/changes`, so without this they'd never see "You made Bob an admin").
        let pre_state = state.clone();
        let actions = apply_to_state(&mut state, &group_key)?;
        // Attribute this change to the submitter (§3.6) so other clients can
        // render "Alice added Bob" / "Bob was removed by Carol". Carried inside
        // the encrypted state blob (stored opaquely + never re-serialized by the
        // server), so it survives regardless of server version.
        state.last_change_actor_did = did.to_string();
        state.revision = (row.revision as u64) + 1;
        let new_plaintext = state.encode_to_vec();
        let new_encrypted_state = group_key.encrypt_state(&new_plaintext);

        let req = SubmitChangeRequest {
            revision: row.revision + 1,
            new_encrypted_state: b64(&new_encrypted_state),
            actions: actions.clone(),
        };
        let resp = client
            .submit_group_changes(group_id_b64_s, &req, &presentation)
            .await;

        match resp {
            Ok(resp) => {
                store
                    .update_group_state(
                        group_id_b64_s,
                        resp.revision,
                        new_plaintext,
                        row.policy,
                    )
                    .await?;
                // Derive + persist the actor's own change as a system timeline
                // entry so it shows immediately on their device (idempotent with
                // the copy other members derive when they pull `/changes`).
                let now_ms = Timestamp::now().as_millis();
                let events = derive_change_events(
                    &ChangeContext {
                        group_id_b64: group_id_b64_s,
                        new_revision: resp.revision,
                        actor_did: did,
                        group_key: &group_key,
                        pre: &pre_state,
                        post: &state,
                        now_ms,
                    },
                    &actions,
                );
                for ev in &events {
                    persist_group_event(store, ev).await?;
                }
                return Ok((resp, events));
            }
            Err(net::error::NetError::Server(409, ref body)) if attempt == 0 => {
                attempt += 1;
                tracing::info!(
                    "[groups] stale revision on submit (body={body}); \
                     refreshing state and retrying once"
                );
                fetch_group_state(
                    store,
                    client,
                    &row.hosting_server_url,
                    did,
                    group_id_b64_s,
                )
                .await?;
                continue;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

// ── invite / accept / decline ────────────────────────────────────────────

pub async fn invite_member(
    store: &store::DeviceStore,
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
                invited_did: recipient_did.to_string(),
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
    store: &store::DeviceStore,
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
    store: &store::DeviceStore,
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
    store: &store::DeviceStore,
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

    let (resp, _events) = submit_actions(
        store,
        client,
        did,
        group_id_b64_s,
        |_state, _group_key| {
            Ok(GroupActionsWire {
                join_via_link: Some(JoinViaLinkWire {
                    encrypted_profile_key: b64(&own_profile_key),
                    group_push_pseudonym: b64(&pseudonym_for_state),
                    invite_link_password: pw_b64.clone(),
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
    store: &store::DeviceStore,
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
    store: &store::DeviceStore,
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
    store: &store::DeviceStore,
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
    store: &store::DeviceStore,
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

/// Leave a group (docs/53 §Leave). Self-class action: we compute our own
/// encrypted_member_id deterministically from the group key (no roster scan
/// needed — the ciphertext for `(did, group_key)` is stable) and submit a sole
/// `leave` action naming it. Unlike `remove_member` (admin-class), any member
/// may leave themselves.
///
/// We deliberately **keep** the local group row and message history: leaving is
/// a tombstone-in-place, not a delete. `submit_actions` persists the updated
/// state with us removed from `members` (so `is_group_member` reports false and
/// the UI hides the composer) and records a "You left the group" system event
/// as the last timeline entry. The conversation stays in the inbox, read-only.
pub async fn leave_group(
    store: &store::DeviceStore,
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
            let emi = group_key.encrypt_member_id(did);
            let emi_bytes = zkgroup::serialize(&emi);
            state.members.retain(|m| m.encrypted_member_id != emi_bytes);
            state.pending_invites.retain(|p| p.encrypted_member_id != emi_bytes);
            state.pending_approvals.retain(|p| p.encrypted_member_id != emi_bytes);
            Ok(GroupActionsWire {
                leave: Some(b64(&emi_bytes)),
                ..Default::default()
            })
        },
    )
    .await?;
    Ok(())
}

/// Whether `did` is currently a member of the locally-cached group state. Reads
/// the cached `encrypted_state_plaintext` only (no network) so it works after a
/// leave, when a server fetch would 404. Returns `false` if the group isn't in
/// the local store. Drives the conversation composer gate (docs/53 §Leave).
pub async fn is_group_member(
    store: &store::DeviceStore,
    did: &str,
    group_id_b64_s: &str,
) -> Result<bool, AppError> {
    let Some(row) = store.load_group(group_id_b64_s).await? else {
        return Ok(false);
    };
    if row.encrypted_state_plaintext.is_empty() {
        // No cached state yet (e.g. a freshly received invite we haven't
        // fetched) — treat as not-yet-a-member; the composer stays gated until
        // state is fetched and we appear in `members`.
        return Ok(false);
    }
    let state = gproto::GroupState::decode(row.encrypted_state_plaintext.as_slice())
        .map_err(|e| AppError::Protocol(format!("decode cached GroupState: {e}")))?;
    Ok(state.members.iter().any(|m| m.did == did))
}

pub async fn change_role(
    store: &store::DeviceStore,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
    encrypted_member_id_b64: &str,
    new_role: i16,
) -> Result<Vec<GroupMetadataEvent>, AppError> {
    let emi = b64d(encrypted_member_id_b64)?;
    let (_, events) = submit_actions(
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
    Ok(events)
}

/// Rename the group (§3.3 `modify_title`). Updates `title` in the encrypted
/// state and emits the sub-encrypted action (opaque to the server — only the
/// fact that the title changed is server-visible, gated by `modify_title_role`).
/// Returns the derived timeline entries.
pub async fn set_title(
    store: &store::DeviceStore,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
    new_title: &str,
) -> Result<Vec<GroupMetadataEvent>, AppError> {
    // Group names must not be empty.
    let new_title = new_title.trim();
    if new_title.is_empty() {
        return Err(AppError::Protocol("group name cannot be empty".into()));
    }
    let (_, events) = submit_actions(
        store,
        client,
        did,
        group_id_b64_s,
        |state, group_key| {
            state.title = new_title.to_string();
            let sub = gproto::ModifyTitle {
                new_title: new_title.to_string(),
            };
            Ok(GroupActionsWire {
                modify_title: Some(b64(&group_key.encrypt_state(&sub.encode_to_vec()))),
                ..Default::default()
            })
        },
    )
    .await?;
    Ok(events)
}

/// Set the group's disappearing-message timer (§3.3 `modify_expiry`). Updates
/// `expiry_seconds` in the encrypted state and emits the sub-encrypted action
/// (opaque to the server — only the fact that expiry changed is server-visible,
/// gated by `modify_expiry_role`). Returns the derived timeline entries.
pub async fn set_expiry(
    store: &store::DeviceStore,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
    expiry_seconds: u32,
) -> Result<Vec<GroupMetadataEvent>, AppError> {
    let (_, events) = submit_actions(
        store,
        client,
        did,
        group_id_b64_s,
        |state, group_key| {
            state.expiry_seconds = expiry_seconds;
            // Carry the new value sub-encrypted under the group key so the
            // server stores it opaquely (it doesn't enforce expiry); the
            // authoritative value also rides in the new state blob.
            let sub = gproto::ModifyExpiry {
                new_expiry_seconds: expiry_seconds,
            };
            Ok(GroupActionsWire {
                modify_expiry: Some(b64(&group_key.encrypt_state(&sub.encode_to_vec()))),
                ..Default::default()
            })
        },
    )
    .await?;
    Ok(events)
}

// ── apply_pending_changes / rotate ───────────────────────────────────────

// ── change → timeline-entry derivation (docs/03 §3.6) ────────────────────

/// DID of the member carrying `emi_b64` in `state`, or empty if not present /
/// not yet known (a roster entry can carry an empty DID, e.g. an approved
/// link-joiner before they DM us).
fn emi_b64_to_did(state: &gproto::GroupState, emi_b64: &str) -> String {
    state
        .members
        .iter()
        .find(|m| b64(&m.encrypted_member_id) == emi_b64)
        .map(|m| m.did.clone())
        .filter(|d| !d.is_empty())
        .unwrap_or_default()
}

/// DID of a pending invitee carrying `emi_b64`, from the cleartext `invited_did`
/// the inviter stamped (§3.6). Empty if not found / not known.
fn pending_invited_did(state: &gproto::GroupState, emi_b64: &str) -> String {
    state
        .pending_invites
        .iter()
        .find(|p| b64(&p.encrypted_member_id) == emi_b64)
        .map(|p| p.invited_did.clone())
        .filter(|d| !d.is_empty())
        .unwrap_or_default()
}

/// The member present in `post` but not `pre` (by encrypted_member_id) — i.e.
/// the one a join/promote added. Returns its `(did, emi_b64)`. Used to attribute
/// "X joined" robustly from the membership diff, independent of whether the
/// joiner's client stamped `last_change_actor_did`.
fn newly_added_member(
    pre: &gproto::GroupState,
    post: &gproto::GroupState,
) -> Option<(String, String)> {
    let pre_emis: std::collections::HashSet<Vec<u8>> = pre
        .members
        .iter()
        .map(|m| m.encrypted_member_id.clone())
        .collect();
    post.members
        .iter()
        .find(|m| !pre_emis.contains(&m.encrypted_member_id))
        .map(|m| (m.did.clone(), b64(&m.encrypted_member_id)))
}

fn member_joined_at(state: &gproto::GroupState, emi_b64: &str) -> Option<i64> {
    state
        .members
        .iter()
        .find(|m| b64(&m.encrypted_member_id) == emi_b64)
        .map(|m| m.joined_at_ms)
}

fn pending_invited_at(state: &gproto::GroupState, emi_b64: &str) -> Option<i64> {
    state
        .pending_invites
        .iter()
        .find(|p| b64(&p.encrypted_member_id) == emi_b64)
        .map(|p| p.invited_at_ms)
}

fn pending_requested_at(state: &gproto::GroupState, emi_b64: &str) -> Option<i64> {
    state
        .pending_approvals
        .iter()
        .find(|p| b64(&p.encrypted_member_id) == emi_b64)
        .map(|p| p.requested_at_ms)
}

fn actor_label(actor_did: &str) -> &str {
    if actor_did.is_empty() {
        "Someone"
    } else {
        actor_did
    }
}

/// Human label for a disappearing-message duration, for the headless `summary`
/// string. Mirrors the canonical options the iOS picker offers; falls back to a
/// raw seconds count for any off-grid value. (The UI does its own localized
/// rendering from `expiry_seconds`.)
fn expiry_label(seconds: u32) -> String {
    match seconds {
        0 => "off".to_string(),
        30 => "30 seconds".to_string(),
        300 => "5 minutes".to_string(),
        3_600 => "1 hour".to_string(),
        28_800 => "8 hours".to_string(),
        86_400 => "1 day".to_string(),
        604_800 => "1 week".to_string(),
        2_419_200 => "4 weeks".to_string(),
        s => format!("{s} seconds"),
    }
}

/// Inputs to [`derive_change_events`] for one change. `pre` is the state before
/// the change (still contains removed members, so "X was removed" resolves);
/// `post` is the state after (contains added/promoted members, so "X joined"
/// resolves). `actor_did` is the change author from the sub-encrypted
/// `change_meta`. `new_revision` is the revision this change produced.
#[derive(Clone, Copy)]
struct ChangeContext<'a> {
    group_id_b64: &'a str,
    new_revision: i64,
    actor_did: &'a str,
    group_key: &'a GroupKey,
    pre: &'a gproto::GroupState,
    post: &'a gproto::GroupState,
    now_ms: i64,
}

/// Reconstruct the timeline entries for a single change.
fn derive_change_events(ctx: &ChangeContext, actions: &GroupActionsWire) -> Vec<GroupMetadataEvent> {
    // Destructure into the same local names the body uses (all fields are Copy
    // — references + i64), so the derivation logic below reads unchanged.
    let ChangeContext {
        group_id_b64,
        new_revision,
        actor_did,
        group_key,
        pre,
        post,
        now_ms,
    } = *ctx;
    let mut out = Vec::new();
    let actor_emi_b64 = if actor_did.is_empty() {
        String::new()
    } else {
        b64(&zkgroup::serialize(&group_key.encrypt_member_id(actor_did)))
    };
    let make = |kind, target_did: String, target_emi: String, occurred_at_ms: i64, summary: String| {
        GroupMetadataEvent {
            group_id: group_id_b64.to_string(),
            revision: new_revision,
            kind,
            actor_did: actor_did.to_string(),
            target_did,
            target_emi,
            occurred_at_ms,
            expiry_seconds: 0,
            new_title: String::new(),
            summary,
        }
    };
    // For a self-action (join/promote) the actor and target are the same member,
    // identified from the membership diff rather than the captured `actor_did`.
    let make_self = |kind, joiner_did: String, joiner_emi: String, occurred_at_ms: i64, summary: String| {
        GroupMetadataEvent {
            group_id: group_id_b64.to_string(),
            revision: new_revision,
            kind,
            actor_did: joiner_did.clone(),
            target_did: joiner_did,
            target_emi: joiner_emi,
            occurred_at_ms,
            expiry_seconds: 0,
            new_title: String::new(),
            summary,
        }
    };
    let resolve = |emi: &str| {
        let d = emi_b64_to_did(post, emi);
        if d.is_empty() {
            emi_b64_to_did(pre, emi)
        } else {
            d
        }
    };
    let who = |did: &str, fallback: &str| {
        if did.is_empty() {
            fallback.to_string()
        } else {
            did.to_string()
        }
    };

    for inv in &actions.invite_members {
        let emi = inv.encrypted_member_id.clone();
        // The invitee isn't a member yet, so resolve from the stamped
        // `invited_did` (the inviter knows the DID), then fall back to the
        // members list.
        let tdid = {
            let d = pending_invited_did(post, &emi);
            if d.is_empty() { resolve(&emi) } else { d }
        };
        let ts = pending_invited_at(post, &emi).unwrap_or(now_ms);
        let summary = format!("{} invited {}", actor_label(actor_did), who(&tdid, "a new member"));
        out.push(make(GroupEventKind::MemberInvited, tdid, emi, ts, summary));
    }
    // The membership diff is authoritative for self-joins, but only when we
    // actually have the pre-change state — an empty `pre` (a decode gap) would
    // make every post member look "new", so fall back to the change-actor stamp.
    let joiner = if pre.members.is_empty() {
        None
    } else {
        newly_added_member(pre, post)
    };
    if actions.promote_pending_members.is_some() {
        // A promote is a self-action: the joiner is the member it added, which
        // we read from the membership diff (robust even if the joiner's client
        // didn't stamp `last_change_actor_did`).
        let (jd, je) = joiner
            .clone()
            .unwrap_or_else(|| (actor_did.to_string(), actor_emi_b64.clone()));
        let ts = member_joined_at(post, &je).unwrap_or(now_ms);
        let summary = format!("{} joined the group", actor_label(&jd));
        out.push(make_self(GroupEventKind::MemberJoined, jd, je, ts, summary));
    }
    if actions.join_via_link.is_some() {
        if let Some((jd, je)) = joiner.clone() {
            // Landed directly as a member.
            let ts = member_joined_at(post, &je).unwrap_or(now_ms);
            let summary = format!("{} joined via invite link", actor_label(&jd));
            out.push(make_self(GroupEventKind::MemberJoinedViaLink, jd, je, ts, summary));
        } else {
            // Landed in the pending-approval queue (request-to-join). The
            // requester isn't a member, so fall back to the change actor.
            let ts = pending_requested_at(post, &actor_emi_b64).unwrap_or(now_ms);
            let summary = format!("{} requested to join", actor_label(actor_did));
            out.push(make(GroupEventKind::MemberRequestedToJoin, actor_did.to_string(), actor_emi_b64.clone(), ts, summary));
        }
    }
    if let Some(emi) = &actions.decline_invite {
        let summary = format!("{} declined the invitation", actor_label(actor_did));
        out.push(make(GroupEventKind::InviteDeclined, actor_did.to_string(), emi.clone(), now_ms, summary));
    }
    if let Some(emi) = &actions.cancel_join_request {
        let summary = format!("{} cancelled their join request", actor_label(actor_did));
        out.push(make(GroupEventKind::JoinRequestCancelled, actor_did.to_string(), emi.clone(), now_ms, summary));
    }
    if let Some(emi) = &actions.leave {
        let summary = format!("{} left the group", actor_label(actor_did));
        out.push(make(GroupEventKind::MemberLeft, actor_did.to_string(), emi.clone(), now_ms, summary));
    }
    for emi in &actions.approve_join_request {
        let tdid = emi_b64_to_did(post, emi);
        let ts = member_joined_at(post, emi).unwrap_or(now_ms);
        let summary = format!("{} approved {}'s join request", actor_label(actor_did), who(&tdid, "a member"));
        out.push(make(GroupEventKind::JoinRequestApproved, tdid, emi.clone(), ts, summary));
    }
    for emi in &actions.deny_join_request {
        let summary = format!("{} declined a join request", actor_label(actor_did));
        out.push(make(GroupEventKind::JoinRequestDenied, String::new(), emi.clone(), now_ms, summary));
    }
    for emi in &actions.remove_members {
        if !actor_emi_b64.is_empty() && *emi == actor_emi_b64 {
            let summary = format!("{} left the group", actor_label(actor_did));
            out.push(make(GroupEventKind::MemberLeft, actor_did.to_string(), emi.clone(), now_ms, summary));
        } else {
            let tdid = emi_b64_to_did(pre, emi);
            let summary = format!("{} removed {}", actor_label(actor_did), who(&tdid, "a member"));
            out.push(make(GroupEventKind::MemberRemoved, tdid, emi.clone(), now_ms, summary));
        }
    }
    for ra in &actions.modify_member_role {
        let emi = ra.encrypted_member_id.clone();
        let tdid = resolve(&emi);
        if ra.role == ROLE_ADMIN {
            let summary = format!("{} made {} an admin", actor_label(actor_did), who(&tdid, "a member"));
            out.push(make(GroupEventKind::RoleChangedToAdmin, tdid, emi, now_ms, summary));
        } else {
            let summary = format!("{} removed admin from {}", actor_label(actor_did), who(&tdid, "a member"));
            out.push(make(GroupEventKind::RoleChangedToMember, tdid, emi, now_ms, summary));
        }
    }
    if actions.modify_title.is_some() {
        // Group names are never empty (enforced in `set_title`); the empty
        // arm is only a defensive fallback for any legacy/foreign change.
        let summary = if post.title.is_empty() {
            format!("{} changed the group name", actor_label(actor_did))
        } else {
            format!("{} changed the group name to '{}'", actor_label(actor_did), post.title)
        };
        let mut ev = make(GroupEventKind::TitleChanged, String::new(), String::new(), now_ms, summary);
        ev.new_title = post.title.clone();
        out.push(ev);
    }
    if actions.modify_description.is_some() {
        let summary = format!("{} changed the group description", actor_label(actor_did));
        out.push(make(GroupEventKind::DescriptionChanged, String::new(), String::new(), now_ms, summary));
    }
    if actions.modify_expiry.is_some() {
        let seconds = post.expiry_seconds;
        let summary = if seconds == 0 {
            format!("{} turned off disappearing messages", actor_label(actor_did))
        } else {
            format!(
                "{} set disappearing messages to {}",
                actor_label(actor_did),
                expiry_label(seconds)
            )
        };
        let mut ev = make(GroupEventKind::ExpiryChanged, String::new(), String::new(), now_ms, summary);
        ev.expiry_seconds = seconds;
        out.push(ev);
    }
    if actions.modify_policy.is_some() {
        let summary = format!("{} changed the group settings", actor_label(actor_did));
        out.push(make(GroupEventKind::PolicyChanged, String::new(), String::new(), now_ms, summary));
    }
    out
}

/// Persist a derived event as a system row in `message_history`. Idempotent on
/// a deterministic id so re-applying the same `/changes` page is a no-op.
async fn persist_group_event(
    store: &store::DeviceStore,
    ev: &GroupMetadataEvent,
) -> Result<(), AppError> {
    let conversation_id = format!("group-{}", ev.group_id);
    let id = format!(
        "grpevt-{}-{}-{}-{}",
        ev.group_id,
        ev.revision,
        kind_code(ev.kind),
        ev.target_emi
    );
    let metadata = serde_json::json!({
        "event": kind_code(ev.kind),
        "actor_did": ev.actor_did,
        "target_did": ev.target_did,
        "target_emi": ev.target_emi,
        "expiry_seconds": ev.expiry_seconds,
        "new_title": ev.new_title,
    })
    .to_string();
    store
        .save_group_event(&store::messages::HistoryMessage {
            id,
            conversation_id,
            sender_did: ev.actor_did.clone(),
            body: ev.summary.clone(),
            sent_at: Timestamp(ev.occurred_at_ms),
            // save_group_event marks the row read at sent_at; these are ignored.
            edited_at: None,
            read_at: Some(Timestamp(ev.occurred_at_ms)),
            delivery_status: 1,
            edit_count: 0,
            deleted_at: None,
            kind: kind_code(ev.kind),
            metadata: Some(metadata),
            // System/metadata rows never disappear (they persist like Signal's
            // update messages).
            expire_timer_secs: 0,
            expire_at: None,
        })
        .await?;
    Ok(())
}

/// Pull `/changes` since the last applied revision, derive the membership /
/// metadata timeline entries (§3.6), persist them as system rows, fast-forward
/// the local cached state, and return `(new_revision, derived_events)`.
///
/// The newest state (revision == server's current) lives only in the `groups`
/// table, never in the `/changes` history (each history row stores the state
/// *before* its actions). So after reading the deltas we call
/// `fetch_group_state` once to (a) resolve members added by the final change
/// and (b) re-sync the cached state + policy mirror to the authoritative copy.
pub async fn apply_pending_changes(
    store: &store::DeviceStore,
    client: &net::Client,
    did: &str,
    group_id_b64_s: &str,
) -> Result<(i64, Vec<GroupMetadataEvent>), AppError> {
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

    if resp.changes.is_empty() {
        return Ok((row.revision, Vec::new()));
    }

    // Decrypt each history row's pre-change state: row revision R carries the
    // state S_R that existed *before* its actions transformed it to S_{R+1}.
    let mut states: std::collections::BTreeMap<i64, gproto::GroupState> =
        std::collections::BTreeMap::new();
    for ch in &resp.changes {
        if let Ok(pt) = group_key.decrypt_state(&ch.encrypted_state) {
            if let Ok(st) = gproto::GroupState::decode(pt.as_slice()) {
                states.insert(ch.revision, st);
            }
        }
    }

    // Fast-forward the cached state + policy to the authoritative current
    // revision, and pick up the post-state for the final change.
    let summary = fetch_group_state(
        store,
        client,
        &row.hosting_server_url,
        did,
        group_id_b64_s,
    )
    .await?;
    if let Some(cur) = store.load_group(group_id_b64_s).await? {
        if let Ok(st) = gproto::GroupState::decode(cur.encrypted_state_plaintext.as_slice()) {
            states.insert(summary.revision, st);
        }
    }

    let empty = gproto::GroupState::default();
    let now_ms = Timestamp::now().as_millis();
    let mut events = Vec::new();
    for ch in &resp.changes {
        let actions: GroupActionsWire = match serde_json::from_slice(&ch.actions) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(
                    "[groups] skipping undecodable change actions at revision {}: {e}",
                    ch.revision
                );
                continue;
            }
        };
        let pre = states.get(&ch.revision).unwrap_or(&empty);
        let post = states.get(&(ch.revision + 1)).unwrap_or(&empty);
        // The actor who produced the post-state stamped their DID into it at
        // submit time (§3.6); read it back here for attribution.
        let actor_did = post.last_change_actor_did.clone();
        events.extend(derive_change_events(
            &ChangeContext {
                group_id_b64: group_id_b64_s,
                new_revision: ch.revision + 1,
                actor_did: &actor_did,
                group_key: &group_key,
                pre,
                post,
                now_ms,
            },
            &actions,
        ));
    }

    for ev in &events {
        persist_group_event(store, ev).await?;
    }

    Ok((summary.revision, events))
}

/// Generate a fresh `group_push_pseudonym` and rotate it on the server.
/// (§3.7 rotation.) Returns the new pseudonym bytes for caller-side relay
/// registration.
pub async fn rotate_group_pseudonym(
    store: &store::DeviceStore,
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
    store: &mut store::DeviceStore,
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
    store: &mut store::DeviceStore,
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
    store: &mut store::DeviceStore,
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
    store: &mut store::DeviceStore,
    sender_did: &str,
    sender_device_id: u32,
    ciphertext: &[u8],
) -> Result<Vec<u8>, AppError> {
    let sender = sender_protocol_address(sender_did, sender_device_id);
    let plaintext = sender_keys::group_decrypt(store, &sender, ciphertext).await?;
    Ok(plaintext)
}

// ── sealed-sender group send / fetch ─────────────────────────────────────

/// A group message decrypted from a sealed-sender envelope. Carries the
/// sender's identity (validated against the pinned trust root) and the
/// inner plaintext bytes — the same bytes the sender passed to
/// [`send_group_message`].
#[derive(Debug, Clone)]
pub struct ReceivedGroupMessage {
    pub message_id: i64,
    pub group_id_b64: String,
    pub sender_did: String,
    pub sender_device_id: u32,
    pub plaintext: Vec<u8>,
}

/// Send `plaintext` to every other current member of the group via the
/// sealed-sender path. Builds the SenderKey ciphertext, the SSv2 envelope,
/// and a `GroupSendFullToken` over the recipient set, then POSTs to
/// `/v1/groups/{id}/send`. Caller-supplied `plaintext` is the inner payload
/// (typically a `ContentMessage` proto with `body = Text/Receipt/...`).
pub async fn send_group_message(
    store: &mut store::DeviceStore,
    client: &net::Client,
    server_url: &str,
    sender_did: &str,
    sender_device_id: u32,
    group_id_b64: &str,
    plaintext: &[u8],
) -> Result<Vec<i64>, AppError> {
    // Master key is stable; derive it (and the per-day auth material) once. The
    // member/recipient sets may be stale, so the endorsement step below refreshes
    // group state and retries on a whole-set MAC failure.
    let mk: [u8; 32] = {
        let row = store
            .load_group(group_id_b64)
            .await?
            .ok_or_else(|| AppError::Protocol("group not found".into()))?;
        row.master_key
            .clone()
            .try_into()
            .map_err(|_| AppError::Protocol("master_key length != 32".into()))?
    };
    let group_key = GroupKey::from_bytes(mk);

    // Today's credential + sender cert (cached together in group_credentials).
    let public = ensure_server_params(store, client, server_url).await?;
    let cred = ensure_credential(store, client, server_url, sender_did, &public).await?;
    let today = day_aligned_now();
    let (_cred_bytes, sender_cert_bytes, _exp) = store
        .load_group_credential(server_url, sender_did, today)
        .await?
        .ok_or_else(|| AppError::Protocol("sender cert not cached after refresh".into()))?;
    let presentation = build_presentation_bytes(&public, &cred, &group_key)?;

    // Validate the server's endorsement response over the group's *full* member
    // ciphertext set. EMIs come from cached state — present even for members
    // whose DID we don't know (invite-link joins are stored with an empty DID by
    // `approve_join_request`), so unlike a DID-derived set they don't silently
    // drop members. If cached membership is stale (a member we invited has since
    // accepted server-side, or one was removed), the whole-set MAC fails with
    // `InvalidCiphertext`; refresh group state from the server and retry once.
    // (Historically this surfaced as "receive endorsements: unexpected ciphertext
    // type" once a group accrued unknown-DID / not-yet-synced members.)
    let (recipient_endorsements, other_dids, expiration) = {
        let mut attempt = 0;
        loop {
            let row = store
                .load_group(group_id_b64)
                .await?
                .ok_or_else(|| AppError::Protocol("group not found".into()))?;
            if row.encrypted_state_plaintext.is_empty() {
                return Err(AppError::Protocol(
                    "no cached group state; call fetch_group_state first".into(),
                ));
            }
            let state = gproto::GroupState::decode(row.encrypted_state_plaintext.as_slice())
                .map_err(|e| AppError::Protocol(format!("decode cached GroupState: {e}")))?;
            let member_emis: Vec<Vec<u8>> = state
                .members
                .iter()
                .map(|m| m.encrypted_member_id.clone())
                .collect();
            // Addressable recipients = members whose DID we know, excluding self
            // (a sealed-sender envelope / session can only be built for those).
            let other_dids: Vec<String> = state
                .members
                .iter()
                .filter(|m| !m.did.is_empty() && m.did != sender_did)
                .map(|m| m.did.clone())
                .collect();
            if other_dids.is_empty() {
                return Ok(Vec::new());
            }

            let endo = client
                .get_group_endorsements(group_id_b64, &presentation)
                .await?;
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time before epoch")
                .as_secs();
            tracing::debug!(
                "group send: endorsing over {} members, {} addressable recipients (attempt {})",
                member_emis.len(),
                other_dids.len(),
                attempt
            );
            match crypto::groups::endorsements::receive_endorsements_by_ciphertexts(
                &endo.response,
                &member_emis,
                &public,
                now_secs,
            ) {
                // Result aligned to `member_emis` (== `state.members`) order; keep
                // only the addressable recipients' endorsements so the token's set
                // matches the sealed-sender fanout the server verifies against.
                Ok(endorsements_in_order) => {
                    let recip: Vec<Vec<u8>> = state
                        .members
                        .iter()
                        .zip(endorsements_in_order)
                        .filter(|(m, _)| !m.did.is_empty() && m.did != sender_did)
                        .map(|(_, e)| e)
                        .collect();
                    break (recip, other_dids, endo.expiration_unix_seconds);
                }
                Err(_) if attempt == 0 => {
                    attempt += 1;
                    tracing::debug!(
                        "group send: endorsement set stale at {} cached members; \
                         refreshing group state and retrying once",
                        member_emis.len()
                    );
                    fetch_group_state(store, client, server_url, sender_did, group_id_b64).await?;
                    continue;
                }
                Err(e) => {
                    return Err(AppError::Protocol(format!("receive endorsements: {e}")));
                }
            }
        }
    };

    let token = crypto::groups::endorsements::token_for_recipients(
        &recipient_endorsements,
        &group_key,
        expiration,
    )
    .map_err(|e| AppError::Protocol(format!("token_for_recipients: {e}")))?;

    // Inner SenderKey ciphertext over the caller's payload (member-independent,
    // so it's built after the endorsement set is settled to avoid re-ratcheting
    // on a refresh+retry).
    let senderkey_ct =
        encrypt_group_content(store, sender_did, sender_device_id, &mk, plaintext).await?;

    // One ProtocolAddress per (recipient, device); per-recipient wire entries
    // pair the ServiceId with the EMI we precompute so the server can resolve
    // each recipient to a `group_push_pseudonym`.
    let mut destinations: Vec<ProtocolAddress> = Vec::new();
    let mut wire_recipients: Vec<net::groups::GroupSendRecipient> = Vec::new();
    for did in &other_dids {
        let uuid = did_to_uuid(did);
        let sid = libsignal_core::ServiceId::from(Aci::from(uuid));
        let sid_name = sid.service_id_string();
        let sid_fixed = sid.service_id_fixed_width_binary().to_vec();
        let emi_bytes = zkgroup::serialize(&group_key.encrypt_member_id(did));
        // Establish/refresh a session with every device of this member before
        // we encrypt. Without this, `encrypt_group_envelope` fails with
        // `NoSession` for any member we never DM'd (e.g. someone an admin added
        // after us) or whose device set has since changed.
        let device_ids = crate::messaging::ensure_group_recipient_sessions(
            store,
            client,
            sender_did,
            sender_device_id,
            did,
        )
        .await?;
        for dev_id in device_ids {
            let dev = DeviceId::try_from(dev_id)
                .map_err(|_| AppError::Protocol("device_id 0 is reserved".into()))?;
            destinations.push(ProtocolAddress::new(sid_name.clone(), dev));
        }
        wire_recipients.push(net::groups::GroupSendRecipient {
            service_id_fixed_width: sid_fixed,
            encrypted_member_id: emi_bytes,
        });
    }

    let group_id_bytes = b64d(group_id_b64)?;
    let envelope = crypto::sealed_sender::encrypt_group_envelope(
        store,
        &sender_cert_bytes,
        Some(group_id_bytes.clone()),
        &senderkey_ct,
        &destinations,
    )
    .await?;

    let ids = client
        .send_group_message(
            group_id_b64,
            &net::groups::GroupSendRequest {
                envelope,
                token,
                recipients: wire_recipients,
                expiry_secs: None,
            },
        )
        .await?;
    Ok(ids)
}

/// Drain queued sealed-sender group messages for one group via HTTP, run
/// each through the receive pipeline (validate sender cert →
/// `sender_keys::group_decrypt`), and ack them server-side. Returns the
/// validated, decrypted messages in delivery order.
pub async fn fetch_group_messages(
    store: &mut store::DeviceStore,
    client: &net::Client,
    server_url: &str,
    recipient_did: &str,
    group_id_b64: &str,
) -> Result<Vec<ReceivedGroupMessage>, AppError> {
    let mk = master_key_for(store, group_id_b64).await?;
    let group_key = GroupKey::from_bytes(mk);
    let public = ensure_server_params(store, client, server_url).await?;
    let cred = ensure_credential(store, client, server_url, recipient_did, &public).await?;
    let presentation = build_presentation_bytes(&public, &cred, &group_key)?;

    let queued = client
        .fetch_group_messages(group_id_b64, &presentation)
        .await?;
    if queued.is_empty() {
        return Ok(Vec::new());
    }

    let trust_root = load_sender_cert_trust_root(store, client, server_url).await?;
    let now_ms = Timestamp::now().as_millis() as u64;

    let mut decoded = Vec::with_capacity(queued.len());
    let mut acked = Vec::with_capacity(queued.len());
    for msg in queued {
        let env = match crypto::sealed_sender::decrypt_envelope_to_usmc(store, &msg.ciphertext)
            .await
        {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("[groups] sealed-sender decrypt failed: {e}");
                acked.push(msg.id);
                continue;
            }
        };
        let info = match crypto::sender_cert::validate_sender_cert(
            &env.sender_cert_bytes,
            &trust_root,
            now_ms,
        ) {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!("[groups] sender cert validation failed: {e}");
                acked.push(msg.id);
                continue;
            }
        };
        let plaintext = match decrypt_group_content(
            store,
            &info.sender_did,
            info.sender_device_id,
            &env.contents,
        )
        .await
        {
            Ok(pt) => pt,
            Err(e) => {
                tracing::warn!("[groups] group_decrypt failed: {e}");
                acked.push(msg.id);
                continue;
            }
        };
        decoded.push(ReceivedGroupMessage {
            message_id: msg.id,
            group_id_b64: group_id_b64.to_string(),
            sender_did: info.sender_did,
            sender_device_id: info.sender_device_id,
            plaintext,
        });
        acked.push(msg.id);
    }

    if !acked.is_empty() {
        if let Err(e) = client
            .ack_group_messages(group_id_b64, acked, &presentation)
            .await
        {
            tracing::warn!("[groups] ack_group_messages failed: {e}");
        }
    }
    Ok(decoded)
}

/// Read the cached `GroupState` plaintext for `group_id_b64_s` and
/// return the DIDs of every current full member except `excluding_did`.
/// Used to enumerate SKDM-distribution and group-send recipients.
pub async fn other_member_dids(
    store: &store::DeviceStore,
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

/// Resolve every known group's title from locally-persisted state in a
/// single query, with no network round trip. The chat list uses this at
/// startup so group rows render with their real names immediately instead of
/// falling back to a placeholder.
///
/// Returns a map keyed by url-safe-no-pad base64 group_id. Groups whose state
/// hasn't been fetched yet (empty `encrypted_state_plaintext`, e.g. a freshly
/// received invite) are omitted; the caller falls back to `fetch_group_state`
/// for those. A group whose state fails to decode is skipped (logged) rather
/// than failing the whole list.
pub async fn local_group_titles(
    store: &store::DeviceStore,
) -> Result<HashMap<String, String>, AppError> {
    let rows = store.list_groups().await?;
    let mut out = HashMap::with_capacity(rows.len());
    for row in rows {
        if row.encrypted_state_plaintext.is_empty() {
            continue;
        }
        match gproto::GroupState::decode(row.encrypted_state_plaintext.as_slice()) {
            Ok(state) => {
                out.insert(row.group_id, state.title);
            }
            Err(e) => {
                tracing::warn!("[groups] decode GroupState for {}: {e}", row.group_id);
            }
        }
    }
    Ok(out)
}

/// The group's current disappearing-message timer (seconds; 0 = off), read
/// from cached state. Used to stamp outgoing group messages (docs/03 §5).
/// Returns 0 if the group/state isn't cached or fails to decode — i.e. no
/// expiry rather than erroring the send.
pub async fn group_expiry_seconds(store: &store::DeviceStore, group_id_b64_s: &str) -> u32 {
    match store.load_group(group_id_b64_s).await {
        Ok(Some(row)) if !row.encrypted_state_plaintext.is_empty() => {
            gproto::GroupState::decode(row.encrypted_state_plaintext.as_slice())
                .map(|s| s.expiry_seconds)
                .unwrap_or(0)
        }
        _ => 0,
    }
}

/// Master-key bytes for a stored group. Convenience: most call sites need
/// the 32-byte array form, not the raw `Vec`.
pub async fn master_key_for(
    store: &store::DeviceStore,
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
    identity: &store::IdentityStore,
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

    if identity.load_group(&group_id_b64_s).await?.is_some() {
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
    identity.save_group(&row).await?;
    Ok(group_id_b64_s)
}

// ── FFI surface ─────────────────────────────────────────────────────────
//
// Sync wrappers that block on the global tokio runtime. Each one is a
// thin shim around the `async` business logic above; keep new logic out
// of this section.

impl AppCore {
    /// Surface derived group timeline entries (§3.6) on the event channel so a
    /// foregrounded conversation refreshes live. The rows are already persisted
    /// by the action/apply path; this is the "refresh now" signal.
    fn emit_group_events(&self, events: Vec<GroupMetadataEvent>) {
        for event in events {
            let _ = self
                .event_tx
                .send(crate::IncomingEvent::GroupMetadataChanged { event });
        }
    }
}

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
            let ws = self.ws.lock().expect("ws mutex poisoned").clone();
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
            // Subscribe to the new group's push pseudonym so reply messages
            // (e.g. from invitees) get pushed live without a fetch poll.
            if let (Some(ws), Some(pseudonym)) = (
                ws.as_ref(),
                inner
                    .store
                    .load_group(&created.group_id)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|g| g.group_push_pseudonym),
            ) {
                if let Err(e) = ws.subscribe_group_pseudonyms(vec![pseudonym]) {
                    tracing::warn!(
                        "[groups] subscribe to new group pseudonym for {} failed: {e}",
                        created.group_id
                    );
                }
            }
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
            // Sync server-side blob so a future recovery can rejoin this group.
            inner.refresh_recovery_blob_best_effort().await;
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

    /// Last-known group info from the local cache, without a server round-trip
    /// (docs/53 §Leave). Returns `nil` if nothing is cached. Lets the group-info
    /// screen render for a group we've left, where `fetch_group_state` 404s.
    pub fn cached_group_state(
        &self,
        group_id: String,
    ) -> Result<Option<GroupSummaryFfi>, AppErrorFfi> {
        ffi_runtime().block_on(async {
            let inner = self.inner.lock().await;
            let summary = cached_group_state(&inner.store, &group_id).await?;
            Ok::<_, AppError>(summary.map(summary_to_ffi))
        })
        .map_err(AppErrorFfi::from)
    }

    /// Group ids (URL-safe-no-pad base64) of every group held locally —
    /// i.e. every group we have the master key for, including ones we were
    /// invited to (app-core auto-accepts invites). Reads the local group
    /// store directly, so it is independent of message history (unlike
    /// `load_conversations`, which only surfaces groups that have messages).
    /// Bots use this to enumerate their memberships; pair with
    /// `fetch_group_state` to inspect roles.
    pub fn list_groups(&self) -> Result<Vec<String>, AppErrorFfi> {
        ffi_runtime().block_on(async {
            let inner = self.inner.lock().await;
            let rows = inner.store.list_groups().await.map_err(AppError::from)?;
            Ok::<_, AppError>(rows.into_iter().map(|g| g.group_id).collect())
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

            // Inviting someone counts as a deliberate gesture — they
            // belong in the People list now (docs/35 §"What changes a row").
            let _ = inner
                .store
                .touch_contact(&recipient_did, true, Timestamp::now())
                .await;

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
                expire_timer_secs: 0,
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
                expire_timer_secs: 0,
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
            // The hosting server is recorded on the group row at invite
            // receipt; pull it back out so we don't need a caller-provided
            // URL here.
            let hosting_server_url = inner
                .store
                .load_group(&group_id)
                .await?
                .ok_or_else(|| AppError::Protocol("group not found in local store".into()))?
                .hosting_server_url;
            inner
                .complete_join_group(ws.as_ref(), &hosting_server_url, &group_id)
                .await?;
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

    /// Leave a group (docs/53 §Leave). Self-class action — works for any
    /// member, not just admins. Removes our own server-side membership and drops
    /// the group from the local store.
    pub fn leave_group(&self, group_id: String) -> Result<(), AppErrorFfi> {
        ffi_runtime()
            .block_on(self.leave_group_async(&group_id))
            .map_err(AppErrorFfi::from)
    }

    /// Whether the current identity is still a member of `group_id` per the
    /// locally-cached state (docs/53 §Leave). Read-only, no network — `false`
    /// after leaving, which the UI uses to hide the composer.
    pub fn is_group_member(&self, group_id: String) -> Result<bool, AppErrorFfi> {
        ffi_runtime()
            .block_on(self.is_group_member_async(&group_id))
            .map_err(AppErrorFfi::from)
    }

    pub fn change_member_role(
        &self,
        group_id: String,
        encrypted_member_id: String,
        new_role: i16,
    ) -> Result<(), AppErrorFfi> {
        ffi_runtime()
            .block_on(async {
                let events = {
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
                    .await?
                };
                self.emit_group_events(events);
                Ok::<_, AppError>(())
            })
            .map_err(AppErrorFfi::from)
    }

    /// The group's current disappearing-message timer (seconds; 0 = off). The
    /// client reads this to stamp the local copy of an outgoing message with the
    /// same timer the send path puts on the wire (docs/03 §5).
    pub fn group_expiry_seconds(&self, group_id: String) -> Result<u32, AppErrorFfi> {
        ffi_runtime()
            .block_on(async {
                let inner = self.inner.lock().await;
                Ok::<_, AppError>(group_expiry_seconds(&inner.store, &group_id).await)
            })
            .map_err(AppErrorFfi::from)
    }

    /// Rename the group. Admin-gated by `modify_title_role`. Emits a
    /// `GroupMetadataChanged` timeline entry ("… changed the group name to X").
    pub fn set_group_title(&self, group_id: String, new_title: String) -> Result<(), AppErrorFfi> {
        ffi_runtime()
            .block_on(async {
                let events = {
                    let inner = self.inner.lock().await;
                    let did = inner.did.clone();
                    set_title(&inner.store, &inner.client, &did, &group_id, &new_title).await?
                };
                self.emit_group_events(events);
                Ok::<_, AppError>(())
            })
            .map_err(AppErrorFfi::from)
    }

    /// Set the group's disappearing-message timer (`0` = off). Admin-gated by
    /// `modify_expiry_role`. Emits a `GroupMetadataChanged` timeline entry.
    pub fn set_group_expiry(
        &self,
        group_id: String,
        expiry_seconds: u32,
    ) -> Result<(), AppErrorFfi> {
        ffi_runtime()
            .block_on(async {
                let events = {
                    let inner = self.inner.lock().await;
                    let did = inner.did.clone();
                    set_expiry(&inner.store, &inner.client, &did, &group_id, expiry_seconds).await?
                };
                self.emit_group_events(events);
                Ok::<_, AppError>(())
            })
            .map_err(AppErrorFfi::from)
    }

    /// Pull `/changes` since the last applied revision. Returns the new
    /// revision (== previous if nothing was pending). Any derived membership /
    /// metadata timeline entries (§3.6) are persisted as system rows and
    /// surfaced individually as `IncomingEvent::GroupMetadataChanged`.
    pub fn apply_pending_group_changes(&self, group_id: String) -> Result<i64, AppErrorFfi> {
        ffi_runtime()
            .block_on(async {
                let (revision, events) = {
                    let inner = self.inner.lock().await;
                    let did = inner.did.clone();
                    apply_pending_changes(&inner.store, &inner.client, &did, &group_id).await?
                };
                self.emit_group_events(events);
                Ok::<_, AppError>(revision)
            })
            .map_err(AppErrorFfi::from)
    }

    /// Send a group text message to every other current member. The text is
    /// wrapped in a `ContentMessage` envelope (with `sent_at_ms` and our
    /// profile key), encrypted once under our Sender Key, and fanned out over
    /// the sealed-sender path. Carrying the envelope (rather than raw text) is
    /// what lets reactions/edits/deletes/receipts work in groups, and gives
    /// every member the same sender-assigned `sent_at` to target by.
    pub fn send_group_message(
        &self,
        group_id: String,
        plaintext: Vec<u8>,
        sent_at_ms: i64,
    ) -> Result<(), AppErrorFfi> {
        ffi_runtime().block_on(async {
            let ws = self.ws.lock().expect("ws mutex poisoned").clone();
            let mut inner = self.inner.lock().await;
            let body = String::from_utf8_lossy(&plaintext).into_owned();
            inner
                .send_group_content(
                    ws.as_ref(),
                    &group_id,
                    Body::Text(proto::TextMessage { body }),
                    sent_at_ms as u64,
                )
                .await
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

#[cfg(test)]
mod metadata_event_tests {
    use super::*;

    const ALICE: &str = "did:plc:alice";
    const BOB: &str = "did:plc:bob";
    const CAROL: &str = "did:plc:carol";

    /// base64 encrypted_member_id for `did` under `gk`.
    fn emi(gk: &GroupKey, did: &str) -> String {
        b64(&zkgroup::serialize(&gk.encrypt_member_id(did)))
    }

    fn member(gk: &GroupKey, did: &str, joined_at_ms: i64) -> gproto::Member {
        gproto::Member {
            did: did.to_string(),
            encrypted_member_id: zkgroup::serialize(&gk.encrypt_member_id(did)),
            role: gproto::Role::Member as i32,
            joined_at_ms,
            profile_key: Vec::new(),
        }
    }

    fn pending_invite(gk: &GroupKey, did: &str, invited_at_ms: i64) -> gproto::PendingInvite {
        gproto::PendingInvite {
            encrypted_member_id: zkgroup::serialize(&gk.encrypt_member_id(did)),
            role: gproto::Role::Member as i32,
            inviter_did: ALICE.to_string(),
            invited_at_ms,
            invited_did: did.to_string(),
        }
    }

    fn pending_approval(gk: &GroupKey, did: &str, requested_at_ms: i64) -> gproto::PendingApproval {
        gproto::PendingApproval {
            encrypted_member_id: zkgroup::serialize(&gk.encrypt_member_id(did)),
            requested_at_ms,
        }
    }

    fn derive(
        gk: &GroupKey,
        actions: GroupActionsWire,
        actor: &str,
        pre: gproto::GroupState,
        post: gproto::GroupState,
    ) -> Vec<GroupMetadataEvent> {
        derive_change_events(
            &ChangeContext {
                group_id_b64: "group-b64",
                new_revision: 5,
                actor_did: actor,
                group_key: gk,
                pre: &pre,
                post: &post,
                now_ms: 1_000,
            },
            &actions,
        )
    }

    #[test]
    fn actor_is_read_from_post_state() {
        // End-to-end of the attribution path: the submitter stamps its DID into
        // the post-state, and apply_pending_changes reads it back from there.
        let gk = GroupKey::generate();
        let actions = GroupActionsWire {
            promote_pending_members: Some(PromoteSelfWire {
                encrypted_profile_key: b64(b""),
                group_push_pseudonym: b64(b"x"),
            }),
            ..Default::default()
        };
        let post = gproto::GroupState {
            members: vec![member(&gk, BOB, 1234)],
            last_change_actor_did: BOB.to_string(),
            ..Default::default()
        };
        // Caller derives `actor` from `post.last_change_actor_did`.
        let actor = post.last_change_actor_did.clone();
        let evs = derive(&gk, actions, &actor, gproto::GroupState::default(), post);
        assert_eq!(evs[0].kind, GroupEventKind::MemberJoined);
        assert_eq!(evs[0].actor_did, BOB);
    }

    #[test]
    fn invite_attributes_actor_and_target() {
        let gk = GroupKey::generate();
        let actions = GroupActionsWire {
            invite_members: vec![InviteMemberWire {
                encrypted_member_id: emi(&gk, BOB),
                role: ROLE_MEMBER,
            }],
            ..Default::default()
        };
        let post = gproto::GroupState {
            pending_invites: vec![pending_invite(&gk, BOB, 7_777)],
            ..Default::default()
        };
        let evs = derive(&gk, actions, ALICE, gproto::GroupState::default(), post);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].kind, GroupEventKind::MemberInvited);
        assert_eq!(evs[0].actor_did, ALICE);
        // Resolved from the stamped `invited_did`, not just the EMI.
        assert_eq!(evs[0].target_did, BOB);
        assert_eq!(evs[0].target_emi, emi(&gk, BOB));
        assert_eq!(evs[0].occurred_at_ms, 7_777);
        assert!(evs[0].summary.contains("invited"));
    }

    #[test]
    fn join_attributed_from_membership_diff_without_actor_stamp() {
        // Even when the joiner's client didn't stamp `last_change_actor_did`
        // (empty actor), the joiner is identified from the membership diff.
        let gk = GroupKey::generate();
        let actions = GroupActionsWire {
            promote_pending_members: Some(PromoteSelfWire {
                encrypted_profile_key: b64(b""),
                group_push_pseudonym: b64(b"x"),
            }),
            ..Default::default()
        };
        let pre = gproto::GroupState {
            members: vec![member(&gk, ALICE, 1)],
            ..Default::default()
        };
        let post = gproto::GroupState {
            members: vec![member(&gk, ALICE, 1), member(&gk, BOB, 2)],
            // No last_change_actor_did — simulates a joiner on an older build.
            ..Default::default()
        };
        let evs = derive(&gk, actions, "", pre, post);
        assert_eq!(evs[0].kind, GroupEventKind::MemberJoined);
        assert_eq!(evs[0].actor_did, BOB, "joiner resolved from the diff");
        assert!(evs[0].summary.contains(BOB));
    }

    #[test]
    fn promote_renders_as_join() {
        let gk = GroupKey::generate();
        let actions = GroupActionsWire {
            promote_pending_members: Some(PromoteSelfWire {
                encrypted_profile_key: b64(b""),
                group_push_pseudonym: b64(b"x"),
            }),
            ..Default::default()
        };
        let post = gproto::GroupState {
            members: vec![member(&gk, BOB, 1234)],
            ..Default::default()
        };
        let evs = derive(&gk, actions, BOB, gproto::GroupState::default(), post);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].kind, GroupEventKind::MemberJoined);
        assert_eq!(evs[0].actor_did, BOB);
        assert_eq!(evs[0].occurred_at_ms, 1234);
    }

    #[test]
    fn join_via_link_open_vs_request() {
        let gk = GroupKey::generate();
        let actions = || GroupActionsWire {
            join_via_link: Some(JoinViaLinkWire {
                encrypted_profile_key: b64(b""),
                group_push_pseudonym: b64(b"x"),
                invite_link_password: b64(b""),
            }),
            ..Default::default()
        };
        // A join happens into an existing group, so `pre` always has a member.
        let pre = || gproto::GroupState {
            members: vec![member(&gk, ALICE, 1)],
            ..Default::default()
        };
        // Landed directly as a member → joined via link (resolved from the diff).
        let post_member = gproto::GroupState {
            members: vec![member(&gk, ALICE, 1), member(&gk, CAROL, 11)],
            ..Default::default()
        };
        let evs = derive(&gk, actions(), CAROL, pre(), post_member);
        assert_eq!(evs[0].kind, GroupEventKind::MemberJoinedViaLink);
        assert_eq!(evs[0].actor_did, CAROL);

        // Landed in pending approval → requested to join (not a new member).
        let post_pending = gproto::GroupState {
            members: vec![member(&gk, ALICE, 1)],
            pending_approvals: vec![pending_approval(&gk, CAROL, 22)],
            ..Default::default()
        };
        let evs = derive(&gk, actions(), CAROL, pre(), post_pending);
        assert_eq!(evs[0].kind, GroupEventKind::MemberRequestedToJoin);
        assert_eq!(evs[0].occurred_at_ms, 22);
    }

    #[test]
    fn remove_distinguishes_left_from_kicked() {
        let gk = GroupKey::generate();
        let pre = gproto::GroupState {
            members: vec![member(&gk, ALICE, 1), member(&gk, BOB, 2)],
            ..Default::default()
        };

        // Alice removes Bob → kicked, target resolved to Bob's DID.
        let kick = GroupActionsWire {
            remove_members: vec![emi(&gk, BOB)],
            ..Default::default()
        };
        let evs = derive(&gk, kick, ALICE, pre.clone(), gproto::GroupState::default());
        assert_eq!(evs[0].kind, GroupEventKind::MemberRemoved);
        assert_eq!(evs[0].target_did, BOB);
        assert!(evs[0].summary.contains("removed"));

        // Bob removes Bob → left.
        let leave = GroupActionsWire {
            remove_members: vec![emi(&gk, BOB)],
            ..Default::default()
        };
        let evs = derive(&gk, leave, BOB, pre, gproto::GroupState::default());
        assert_eq!(evs[0].kind, GroupEventKind::MemberLeft);
        assert_eq!(evs[0].actor_did, BOB);
        assert!(evs[0].summary.contains("left"));
    }

    #[test]
    fn expiry_change_names_the_duration() {
        let gk = GroupKey::generate();
        let actions = GroupActionsWire {
            modify_expiry: Some(b64(b"x")),
            ..Default::default()
        };
        let post = gproto::GroupState {
            expiry_seconds: 2_419_200,
            ..Default::default()
        };
        let evs = derive(&gk, actions, ALICE, gproto::GroupState::default(), post);
        assert_eq!(evs[0].kind, GroupEventKind::ExpiryChanged);
        assert_eq!(evs[0].expiry_seconds, 2_419_200);
        assert!(evs[0].summary.contains("4 weeks"), "{}", evs[0].summary);

        // Turning it off reads differently.
        let off_actions = GroupActionsWire {
            modify_expiry: Some(b64(b"x")),
            ..Default::default()
        };
        let off = derive(&gk, off_actions, ALICE, gproto::GroupState::default(), gproto::GroupState::default());
        assert_eq!(off[0].expiry_seconds, 0);
        assert!(off[0].summary.contains("turned off"), "{}", off[0].summary);
    }

    #[test]
    fn title_change_names_the_new_title() {
        let gk = GroupKey::generate();
        let actions = GroupActionsWire {
            modify_title: Some(b64(b"x")),
            ..Default::default()
        };
        let post = gproto::GroupState {
            title: "March Logistics".to_string(),
            ..Default::default()
        };
        let evs = derive(&gk, actions, ALICE, gproto::GroupState::default(), post);
        assert_eq!(evs[0].kind, GroupEventKind::TitleChanged);
        assert_eq!(evs[0].new_title, "March Logistics");
        assert!(evs[0].summary.contains("'March Logistics'"), "{}", evs[0].summary);
    }

    #[test]
    fn role_change_to_admin() {
        let gk = GroupKey::generate();
        let actions = GroupActionsWire {
            modify_member_role: vec![RoleAssignmentWire {
                encrypted_member_id: emi(&gk, BOB),
                role: ROLE_ADMIN,
            }],
            ..Default::default()
        };
        let post = gproto::GroupState {
            members: vec![member(&gk, BOB, 1)],
            ..Default::default()
        };
        let evs = derive(&gk, actions, ALICE, gproto::GroupState::default(), post);
        assert_eq!(evs[0].kind, GroupEventKind::RoleChangedToAdmin);
        assert_eq!(evs[0].target_did, BOB);
    }

    #[test]
    fn missing_change_meta_yields_unattributed_entry() {
        let gk = GroupKey::generate();
        let actions = GroupActionsWire {
            remove_members: vec![emi(&gk, BOB)],
            ..Default::default()
        };
        let pre = gproto::GroupState {
            members: vec![member(&gk, BOB, 2)],
            ..Default::default()
        };
        // Empty actor (no change_meta) still produces an entry, just unattributed.
        let evs = derive(&gk, actions, "", pre, gproto::GroupState::default());
        assert_eq!(evs[0].kind, GroupEventKind::MemberRemoved);
        assert_eq!(evs[0].actor_did, "");
        assert!(evs[0].summary.starts_with("Someone"));
    }
}
