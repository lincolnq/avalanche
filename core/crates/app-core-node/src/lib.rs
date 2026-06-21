//! Node.js bindings for `app-core` via napi-rs.
//!
//! Mirrors the UniFFI surface from `app-core::lib.rs` and `app-core::groups`.
//! Every method is `async fn` on the JS side; internally we run the sync
//! app-core FFI calls on `tokio::task::spawn_blocking` so we never block
//! napi-rs's tokio reactor.

#![deny(clippy::all)]

use std::sync::Arc;

use napi::bindgen_prelude::{Buffer, Error as NapiError};

use napi_derive::napi;

use app_core::error::AppErrorFfi;
use app_core::{
    self as core, AccountInfoFfi, ConnectionState, ContactRowFfi, ConversationSummaryFfi,
    CreatedGroupFfi, DecryptedMessage, DeliveryStatusUpdate, GroupMemberFfi, GroupPendingFfi,
    AdminEvent, GroupSummaryFfi, IncomingEvent, InviteInfo, JoinResultFfi, MessageTarget,
    ProjectInfoFfi, StoredMessageFfi,
};

// ── Error mapping ───────────────────────────────────────────────────────────

fn to_napi(e: AppErrorFfi) -> NapiError {
    NapiError::from_reason(e.to_string())
}

fn join_err(e: tokio::task::JoinError) -> NapiError {
    NapiError::from_reason(format!("background task panicked: {e}"))
}

// ── Plain object types (mirror app-core FFI records) ────────────────────────

#[napi(object)]
pub struct ProjectInfoJs {
    pub name: String,
    pub url: String,
    pub description: String,
}

impl From<ProjectInfoFfi> for ProjectInfoJs {
    fn from(p: ProjectInfoFfi) -> Self {
        Self { name: p.name, url: p.url, description: p.description }
    }
}

#[napi(object)]
pub struct DecryptedMessageJs {
    pub server_id: i64,
    pub sender_did: String,
    pub sender_device_id: u32,
    pub plaintext: Buffer,
    pub sent_at_ms: Option<i64>,
    pub group_id: Option<String>,
    /// Sender's profile key from the envelope, if any. app-core does not cache
    /// the profile automatically; a bot that wants the display name passes this
    /// to `fetchAndCacheProfile`.
    pub profile_key: Option<Buffer>,
    /// True for an inbound DM that the message-request gate treats as a request
    /// (docs/12 §1). Bots typically ignore this; a bot that tracks requests
    /// calls `setPendingRequest`.
    pub is_request: bool,
}

impl From<DecryptedMessage> for DecryptedMessageJs {
    fn from(m: DecryptedMessage) -> Self {
        Self {
            server_id: m.server_id,
            sender_did: m.sender_did,
            sender_device_id: m.sender_device_id,
            plaintext: m.plaintext.into(),
            sent_at_ms: m.sent_at_ms,
            group_id: m.group_id,
            profile_key: m.profile_key.map(Into::into),
            is_request: m.is_request,
        }
    }
}

#[napi(object)]
pub struct StoredMessageJs {
    pub id: String,
    pub conversation_id: String,
    pub sender_did: String,
    pub body: String,
    pub sent_at_ms: i64,
    pub edited_at_ms: Option<i64>,
    pub read_at_ms: Option<i64>,
    pub delivery_status: u32,
    /// 0 = normal chat; >0 = system/metadata timeline entry (docs/03 §3.6).
    pub kind: i64,
    /// JSON for system rows (event kind + actor/target DIDs); `None` otherwise.
    pub metadata: Option<String>,
}

impl From<StoredMessageFfi> for StoredMessageJs {
    fn from(m: StoredMessageFfi) -> Self {
        Self {
            id: m.id,
            conversation_id: m.conversation_id,
            sender_did: m.sender_did,
            body: m.body,
            sent_at_ms: m.sent_at_ms,
            edited_at_ms: m.edited_at_ms,
            read_at_ms: m.read_at_ms,
            delivery_status: m.delivery_status as u32,
            kind: m.kind,
            metadata: m.metadata,
        }
    }
}

impl From<StoredMessageJs> for StoredMessageFfi {
    fn from(m: StoredMessageJs) -> Self {
        Self {
            id: m.id,
            conversation_id: m.conversation_id,
            sender_did: m.sender_did,
            body: m.body,
            sent_at_ms: m.sent_at_ms,
            edited_at_ms: m.edited_at_ms,
            read_at_ms: m.read_at_ms,
            delivery_status: m.delivery_status as u8,
            // Edit/delete state is managed by app-core, not the JS layer.
            edit_count: 0,
            deleted: false,
            // The JS layer only ever saves normal chat messages; system rows
            // are produced by app-core's group-event path.
            kind: 0,
            metadata: None,
            // Disappearing-messages timers are a substrate concern stamped by
            // the send path; bots don't set them via the JS save path.
            expire_timer_secs: 0,
            expire_at_ms: None,
        }
    }
}

#[napi(object)]
pub struct ConversationSummaryJs {
    pub conversation_id: String,
    pub last_message: Option<StoredMessageJs>,
}

impl From<ConversationSummaryFfi> for ConversationSummaryJs {
    fn from(c: ConversationSummaryFfi) -> Self {
        Self {
            conversation_id: c.conversation_id,
            last_message: c.last_message.map(Into::into),
        }
    }
}

#[napi(object)]
pub struct DeliveryStatusUpdateJs {
    pub conversation_id: String,
    pub sent_at_ms: i64,
    pub delivery_status: u32,
}

impl From<DeliveryStatusUpdate> for DeliveryStatusUpdateJs {
    fn from(u: DeliveryStatusUpdate) -> Self {
        Self {
            conversation_id: u.conversation_id,
            sent_at_ms: u.sent_at_ms,
            delivery_status: u.delivery_status as u32,
        }
    }
}

#[napi(object)]
pub struct CreatedGroupJs {
    pub group_id: String,
    pub master_key: Buffer,
}

impl From<CreatedGroupFfi> for CreatedGroupJs {
    fn from(g: CreatedGroupFfi) -> Self {
        Self { group_id: g.group_id, master_key: g.master_key.into() }
    }
}

#[napi(object)]
pub struct GroupMemberJs {
    pub did: String,
    pub encrypted_member_id: String,
    /// 0 = Member, 1 = Admin.
    pub role: i32,
    pub joined_at_ms: i64,
}

impl From<GroupMemberFfi> for GroupMemberJs {
    fn from(m: GroupMemberFfi) -> Self {
        Self {
            did: m.did,
            encrypted_member_id: m.encrypted_member_id,
            role: m.role as i32,
            joined_at_ms: m.joined_at_ms,
        }
    }
}

#[napi(object)]
pub struct GroupPendingJs {
    pub encrypted_member_id: String,
    pub timestamp_ms: i64,
}

impl From<GroupPendingFfi> for GroupPendingJs {
    fn from(p: GroupPendingFfi) -> Self {
        Self {
            encrypted_member_id: p.encrypted_member_id,
            timestamp_ms: p.timestamp_ms,
        }
    }
}

#[napi(object)]
pub struct GroupSummaryJs {
    pub group_id: String,
    pub master_key: Buffer,
    pub revision: i64,
    pub title: String,
    pub description: String,
    pub expiry_seconds: u32,
    pub members: Vec<GroupMemberJs>,
    pub pending_invites: Vec<GroupPendingJs>,
    pub pending_approvals: Vec<GroupPendingJs>,
}

impl From<GroupSummaryFfi> for GroupSummaryJs {
    fn from(s: GroupSummaryFfi) -> Self {
        Self {
            group_id: s.group_id,
            master_key: s.master_key.into(),
            revision: s.revision,
            title: s.title,
            description: s.description,
            expiry_seconds: s.expiry_seconds,
            members: s.members.into_iter().map(Into::into).collect(),
            pending_invites: s.pending_invites.into_iter().map(Into::into).collect(),
            pending_approvals: s.pending_approvals.into_iter().map(Into::into).collect(),
        }
    }
}

#[napi(object)]
pub struct ContactRowJs {
    pub did: String,
    pub display_name: String,
    pub is_curated: bool,
    pub last_interaction_at_ms: i64,
}

impl From<ContactRowFfi> for ContactRowJs {
    fn from(c: ContactRowFfi) -> Self {
        Self {
            did: c.did,
            display_name: c.display_name,
            is_curated: c.is_curated,
            last_interaction_at_ms: c.last_interaction_at_ms,
        }
    }
}

#[napi(object)]
pub struct AccountInfoJs {
    pub did: String,
    pub display_name: Option<String>,
    pub is_bot: bool,
}

impl From<AccountInfoFfi> for AccountInfoJs {
    fn from(a: AccountInfoFfi) -> Self {
        Self { did: a.did, display_name: a.display_name, is_bot: a.is_bot }
    }
}

#[napi(object)]
pub struct InviteInfoJs {
    pub server_url: String,
    pub server_name: String,
    pub inviter_did: Option<String>,
    pub post_onboarding_redirect: Option<String>,
    pub inviter_display_name: Option<String>,
    pub inviter_profile_key: Option<Buffer>,
}

impl From<InviteInfo> for InviteInfoJs {
    fn from(i: InviteInfo) -> Self {
        Self {
            server_url: i.server_url,
            server_name: i.server_name,
            inviter_did: i.inviter_did,
            post_onboarding_redirect: i.post_onboarding_redirect,
            inviter_display_name: i.inviter_display_name,
            inviter_profile_key: i.inviter_profile_key.map(Into::into),
        }
    }
}

/// Connection liveness. `state` is one of `"disconnected" | "connecting" |
/// "connected" | "reconnecting"`. `nextAttemptAtMs` is set only for
/// `"reconnecting"`.
#[napi(object)]
pub struct ConnectionStateJs {
    pub state: String,
    pub next_attempt_at_ms: Option<i64>,
}

impl From<ConnectionState> for ConnectionStateJs {
    fn from(s: ConnectionState) -> Self {
        match s {
            ConnectionState::Disconnected => Self { state: "disconnected".into(), next_attempt_at_ms: None },
            ConnectionState::Connecting => Self { state: "connecting".into(), next_attempt_at_ms: None },
            ConnectionState::Connected => Self { state: "connected".into(), next_attempt_at_ms: None },
            ConnectionState::Reconnecting { next_attempt_at_ms } => Self {
                state: "reconnecting".into(),
                next_attempt_at_ms: Some(next_attempt_at_ms),
            },
        }
    }
}

impl ConnectionStateJs {
    fn into_ffi(self) -> napi::Result<ConnectionState> {
        match self.state.as_str() {
            "disconnected" => Ok(ConnectionState::Disconnected),
            "connecting" => Ok(ConnectionState::Connecting),
            "connected" => Ok(ConnectionState::Connected),
            "reconnecting" => Ok(ConnectionState::Reconnecting {
                next_attempt_at_ms: self.next_attempt_at_ms.unwrap_or(0),
            }),
            other => Err(NapiError::from_reason(format!("unknown connection state: {other}"))),
        }
    }
}

/// Where a plain-text send is directed. `kind` is `"dm"` or `"group"`;
/// `recipientDid` is set for `"dm"`, `groupId` for `"group"`.
#[napi(object)]
pub struct MessageTargetJs {
    pub kind: String,
    pub recipient_did: Option<String>,
    pub group_id: Option<String>,
}

impl MessageTargetJs {
    fn into_ffi(self) -> napi::Result<MessageTarget> {
        match self.kind.as_str() {
            "dm" => Ok(MessageTarget::Dm {
                recipient_did: self
                    .recipient_did
                    .ok_or_else(|| NapiError::from_reason("dm target requires recipientDid"))?,
            }),
            "group" => Ok(MessageTarget::Group {
                group_id: self
                    .group_id
                    .ok_or_else(|| NapiError::from_reason("group target requires groupId"))?,
            }),
            other => Err(NapiError::from_reason(format!(
                "unknown message target kind: {other}"
            ))),
        }
    }
}

/// A single event from the receive loop. `kind` is one of `"message"`,
/// `"receipt"`, or `"groupInvite"`. Exactly one of `message` / `receipt` /
/// `groupInvite` is set, matching `kind`.
#[napi(object)]
pub struct IncomingEventJs {
    pub kind: String,
    pub message: Option<DecryptedMessageJs>,
    pub receipt: Option<DeliveryStatusUpdateJs>,
    pub group_invite: Option<GroupInviteJs>,
    pub group_metadata: Option<GroupMetadataChangedJs>,
}

/// A `groupMetadataChanged` payload: a membership/metadata change derived from
/// the group change log (docs/03 §3.6). Lets a bot (e.g. adminbot) observe who
/// joined/left/was-added without rendering a UI; `summary` is a ready-to-log
/// English line, the structured fields drive any custom handling.
#[napi(object)]
pub struct GroupMetadataChangedJs {
    pub group_id: String,
    pub revision: i64,
    /// camelCase kind name, e.g. "memberJoined", "memberRemoved".
    pub kind: String,
    pub actor_did: String,
    pub target_did: String,
    pub target_emi: String,
    pub occurred_at_ms: i64,
    pub summary: String,
}

fn group_event_kind_name(kind: app_core::groups::GroupEventKind) -> &'static str {
    use app_core::groups::GroupEventKind as K;
    match kind {
        K::MemberJoined => "memberJoined",
        K::MemberJoinedViaLink => "memberJoinedViaLink",
        K::MemberRequestedToJoin => "memberRequestedToJoin",
        K::MemberInvited => "memberInvited",
        K::MemberLeft => "memberLeft",
        K::MemberRemoved => "memberRemoved",
        K::JoinRequestApproved => "joinRequestApproved",
        K::JoinRequestDenied => "joinRequestDenied",
        K::InviteDeclined => "inviteDeclined",
        K::JoinRequestCancelled => "joinRequestCancelled",
        K::RoleChangedToAdmin => "roleChangedToAdmin",
        K::RoleChangedToMember => "roleChangedToMember",
        K::TitleChanged => "titleChanged",
        K::DescriptionChanged => "descriptionChanged",
        K::ExpiryChanged => "expiryChanged",
        K::PolicyChanged => "policyChanged",
    }
}

/// A `groupInvite` payload: we received a `GroupContext` DM for `group_id`
/// (master key already persisted locally). UI should refresh its chat list.
#[napi(object)]
pub struct GroupInviteJs {
    pub group_id: String,
    pub hosting_server_url: String,
    pub inviter_did: String,
}

/// Adminbot-only push: a new account just registered on the homeserver.
#[napi(object)]
pub struct AccountJoinedJs {
    pub did: String,
    pub joined_at_ms: i64,
}

impl From<IncomingEvent> for IncomingEventJs {
    fn from(e: IncomingEvent) -> Self {
        match e {
            IncomingEvent::Message { msg } => Self {
                kind: "message".into(),
                message: Some(msg.into()),
                receipt: None,
                group_invite: None,
                group_metadata: None,
            },
            IncomingEvent::ReceiptUpdate { update } => Self {
                kind: "receipt".into(),
                message: None,
                receipt: Some(update.into()),
                group_invite: None,
                group_metadata: None,
            },
            IncomingEvent::GroupInvite {
                group_id,
                hosting_server_url,
                inviter_did,
            } => Self {
                kind: "groupInvite".into(),
                message: None,
                receipt: None,
                group_invite: Some(GroupInviteJs {
                    group_id,
                    hosting_server_url,
                    inviter_did,
                }),
                group_metadata: None,
            },
            // Editing/deletion/reactions (docs/33, docs/36): the store is
            // already updated by app-core. JS admin consumers don't act on
            // these today, so surface a bare kind with no payload.
            IncomingEvent::MessageEdited { .. } => Self {
                kind: "messageEdited".into(),
                message: None,
                receipt: None,
                group_invite: None,
                group_metadata: None,
            },
            IncomingEvent::MessageDeleted { .. } => Self {
                kind: "messageDeleted".into(),
                message: None,
                receipt: None,
                group_invite: None,
                group_metadata: None,
            },
            IncomingEvent::ReactionUpdated { .. } => Self {
                kind: "reactionUpdated".into(),
                message: None,
                receipt: None,
                group_invite: None,
                group_metadata: None,
            },
            // A background storage sync applied remote records. Bots don't
            // render conversation lists (and opt out of storage sync), so this
            // is surfaced as a bare kind for completeness/parity only.
            IncomingEvent::StorageSynced => Self {
                kind: "storageSynced".into(),
                message: None,
                receipt: None,
                group_invite: None,
                group_metadata: None,
            },
            // A membership/metadata change derived from the group change log
            // (docs/03 §3.6). Carries the full structured payload so bots can
            // act on who-did-what without a DB read.
            IncomingEvent::GroupMetadataChanged { event } => Self {
                kind: "groupMetadataChanged".into(),
                message: None,
                receipt: None,
                group_invite: None,
                group_metadata: Some(GroupMetadataChangedJs {
                    group_id: event.group_id,
                    revision: event.revision,
                    kind: group_event_kind_name(event.kind).into(),
                    actor_did: event.actor_did,
                    target_did: event.target_did,
                    target_emi: event.target_emi,
                    occurred_at_ms: event.occurred_at_ms,
                    summary: event.summary,
                }),
            },
            // Disappearing-messages reaper deleted timed-out messages. Bots
            // don't render conversations, so surface a bare kind for parity.
            IncomingEvent::MessagesExpired { .. } => Self {
                kind: "messagesExpired".into(),
                message: None,
                receipt: None,
                group_invite: None,
                group_metadata: None,
            },
        }
    }
}

/// An admin-only event from the receive loop, drained via
/// `nextAdminEvents`. `kind` is currently always `"accountJoined"`; future
/// variants (e.g. server build events) will land here.
#[napi(object)]
pub struct AdminEventJs {
    pub kind: String,
    pub account_joined: Option<AccountJoinedJs>,
}

impl From<AdminEvent> for AdminEventJs {
    fn from(e: AdminEvent) -> Self {
        match e {
            AdminEvent::AccountJoined { did, joined_at_ms } => Self {
                kind: "accountJoined".into(),
                account_joined: Some(AccountJoinedJs { did, joined_at_ms }),
            },
        }
    }
}

#[napi]
pub enum JoinResultJs {
    Member,
    Pending,
}

impl From<JoinResultFfi> for JoinResultJs {
    fn from(r: JoinResultFfi) -> Self {
        match r {
            JoinResultFfi::Member => JoinResultJs::Member,
            JoinResultFfi::Pending => JoinResultJs::Pending,
        }
    }
}

// ── PreparedAccount wrapper ─────────────────────────────────────────────────

#[napi]
pub struct PreparedAccount {
    inner: Arc<core::PreparedAccount>,
}

#[napi]
impl PreparedAccount {
    /// Generate identity + rotation keys and derive a `did:plc` locally.
    /// Does not contact the server. Consumed by `AppCore.finalizeAccount`.
    #[napi]
    pub async fn create(
        server_url: String,
        prf_output: Buffer,
    ) -> napi::Result<PreparedAccount> {
        let prf = prf_output.to_vec();
        let inner = tokio::task::spawn_blocking(move || core::PreparedAccount::new(server_url, prf))
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(PreparedAccount { inner })
    }

    #[napi]
    pub fn did(&self) -> String {
        self.inner.did()
    }
}

// ── AppCore wrapper ─────────────────────────────────────────────────────────

#[napi]
pub struct AppCore {
    inner: Arc<core::AppCore>,
}

#[napi]
impl AppCore {
    // ── constructors ────────────────────────────────────────────────────────

    #[napi]
    pub async fn create_account(
        server_url: String,
        db_path: String,
        db_key: String,
        prf_output: Buffer,
        display_name: String,
        invite_token: Option<String>,
    ) -> napi::Result<AppCore> {
        let prf = prf_output.to_vec();
        let inner = tokio::task::spawn_blocking(move || {
            core::AppCore::create_account(server_url, db_path, db_key, prf, display_name, invite_token)
        })
        .await
        .map_err(join_err)?
        .map_err(to_napi)?;
        Ok(AppCore { inner })
    }

    /// Register a new bot account on the server. Bot accounts skip the
    /// PLC directory and receive a `did:local:...` DID. `displayName` is
    /// stored as plaintext on the server (bot names aren't encrypted into
    /// profile blobs). No recovery blob is uploaded.
    #[napi]
    pub async fn create_bot_account(
        server_url: String,
        db_path: String,
        db_key: String,
        display_name: String,
        did_suffix: Option<String>,
        invite_token: Option<String>,
    ) -> napi::Result<AppCore> {
        let inner = tokio::task::spawn_blocking(move || {
            core::AppCore::create_bot_account(server_url, db_path, db_key, display_name, did_suffix, invite_token)
        })
        .await
        .map_err(join_err)?
        .map_err(to_napi)?;
        Ok(AppCore { inner })
    }

    /// Open a bot account, registering it on first run and re-logging-in
    /// thereafter. Logs in when the store at `dbPath` already holds an account
    /// (ignoring `serverUrl` / `displayName` / `didSuffix`, which are fixed by
    /// the original registration); otherwise registers a new bot account. The
    /// empty-store branch is detected internally — callers no longer inspect
    /// error strings to tell "fresh deploy" from "real failure".
    #[napi]
    pub async fn login_or_create_bot(
        server_url: String,
        db_path: String,
        db_key: String,
        display_name: String,
        did_suffix: Option<String>,
        invite_token: Option<String>,
    ) -> napi::Result<AppCore> {
        let inner = tokio::task::spawn_blocking(move || {
            core::AppCore::login_or_create_bot(server_url, db_path, db_key, display_name, did_suffix, invite_token)
        })
        .await
        .map_err(join_err)?
        .map_err(to_napi)?;
        Ok(AppCore { inner })
    }

    #[napi]
    pub async fn finalize_account(
        prepared: &PreparedAccount,
        db_path: String,
        db_key: String,
        display_name: String,
        invite_token: Option<String>,
    ) -> napi::Result<AppCore> {
        let prepared = prepared.inner.clone();
        let inner = tokio::task::spawn_blocking(move || {
            core::AppCore::finalize_account(prepared, db_path, db_key, display_name, invite_token)
        })
        .await
        .map_err(join_err)?
        .map_err(to_napi)?;
        Ok(AppCore { inner })
    }

    #[napi]
    pub async fn recover_from_blob(
        server_url: String,
        did: String,
        prf_output: Buffer,
        db_path: String,
        db_key: String,
        display_name: String,
    ) -> napi::Result<AppCore> {
        let prf = prf_output.to_vec();
        let inner = tokio::task::spawn_blocking(move || {
            core::AppCore::recover_from_blob(server_url, did, prf, db_path, db_key, display_name)
        })
        .await
        .map_err(join_err)?
        .map_err(to_napi)?;
        Ok(AppCore { inner })
    }

    #[napi]
    pub async fn login(db_path: String, db_key: String) -> napi::Result<AppCore> {
        let inner = tokio::task::spawn_blocking(move || core::AppCore::login(db_path, db_key))
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(AppCore { inner })
    }

    // ── identity ────────────────────────────────────────────────────────────

    #[napi]
    pub fn did(&self) -> String {
        self.inner.did()
    }

    #[napi]
    pub fn device_id(&self) -> u32 {
        self.inner.device_id()
    }

    // ── messaging ───────────────────────────────────────────────────────────

    #[napi]
    pub async fn send_dm(
        &self,
        recipient_did: String,
        plaintext: Buffer,
        sent_at_ms: i64,
    ) -> napi::Result<()> {
        let core = self.inner.clone();
        let plaintext = plaintext.to_vec();
        tokio::task::spawn_blocking(move || core.send_dm(recipient_did, plaintext, sent_at_ms))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    /// Send a plain-text message to a DM or group target without the caller
    /// choosing the transport. Mirrors `sendDm` / `sendGroupMessage` exactly
    /// for the matching target; folds the fork into one call.
    #[napi]
    pub async fn send_message(
        &self,
        target: MessageTargetJs,
        plaintext: Buffer,
        sent_at_ms: i64,
    ) -> napi::Result<()> {
        let core = self.inner.clone();
        let target = target.into_ffi()?;
        let pt = plaintext.to_vec();
        tokio::task::spawn_blocking(move || core.send_message(target, pt, sent_at_ms))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    /// React to a message (docs/33) in a DM or group. `remove = true` clears
    /// this account's reaction on the target; otherwise `emoji` replaces any
    /// prior one. `targetAuthor` is the DID of the reacted-to message's author;
    /// `targetSentAtMs` identifies that message; `sentAtMs` is this op's clock.
    #[napi]
    pub async fn send_reaction(
        &self,
        target: MessageTargetJs,
        target_author: String,
        target_sent_at_ms: i64,
        emoji: String,
        remove: bool,
        sent_at_ms: i64,
    ) -> napi::Result<()> {
        let core = self.inner.clone();
        let target = target.into_ffi()?;
        tokio::task::spawn_blocking(move || {
            core.send_reaction(target, target_author, target_sent_at_ms, emoji, remove, sent_at_ms)
        })
        .await
        .map_err(join_err)?
        .map_err(to_napi)
    }

    #[napi]
    pub async fn receive_messages(&self) -> napi::Result<Vec<DecryptedMessageJs>> {
        let core = self.inner.clone();
        let msgs = tokio::task::spawn_blocking(move || core.receive_messages())
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(msgs.into_iter().map(Into::into).collect())
    }

    #[napi]
    pub async fn send_read_receipt(
        &self,
        recipient_did: String,
        timestamps: Vec<i64>,
    ) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.send_read_receipt(recipient_did, timestamps))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    // ── connection lifecycle ────────────────────────────────────────────────

    #[napi]
    pub fn connection_state(&self) -> ConnectionStateJs {
        self.inner.connection_state().into()
    }

    /// Blocks (off the event loop) until the connection state differs from
    /// `last`, then returns the new value.
    #[napi]
    pub async fn wait_for_connection_state_change(
        &self,
        last: ConnectionStateJs,
    ) -> napi::Result<ConnectionStateJs> {
        let last = last.into_ffi()?;
        let core = self.inner.clone();
        let new_state = tokio::task::spawn_blocking(move || core.wait_for_connection_state_change(last))
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(new_state.into())
    }

    /// Block until at least one event is available; drain the queue and
    /// return the batch.
    #[napi]
    pub async fn next_events(&self) -> napi::Result<Vec<IncomingEventJs>> {
        let core = self.inner.clone();
        let events = tokio::task::spawn_blocking(move || core.next_events())
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(events.into_iter().map(Into::into).collect())
    }

    /// Block until at least one admin event is available; drain the admin
    /// queue and return the batch. Only adminbot sessions ever receive
    /// admin events — for any other session this future pends indefinitely.
    #[napi]
    pub async fn next_admin_events(&self) -> napi::Result<Vec<AdminEventJs>> {
        let core = self.inner.clone();
        let events = core
            .next_admin_events_async()
            .await
            .map_err(|e| to_napi(AppErrorFfi::from(e)))?;
        Ok(events.into_iter().map(Into::into).collect())
    }

    // ── projects ────────────────────────────────────────────────────────────

    #[napi]
    pub async fn fetch_projects(&self) -> napi::Result<Vec<ProjectInfoJs>> {
        let core = self.inner.clone();
        let projects = tokio::task::spawn_blocking(move || core.fetch_projects())
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(projects.into_iter().map(Into::into).collect())
    }

    #[napi]
    pub async fn request_project_token(&self, project_url: String) -> napi::Result<String> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.request_project_token(project_url))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    // ── message history (local) ─────────────────────────────────────────────

    #[napi]
    pub async fn save_message(&self, msg: StoredMessageJs) -> napi::Result<()> {
        let core = self.inner.clone();
        let ffi: StoredMessageFfi = msg.into();
        tokio::task::spawn_blocking(move || core.save_message(ffi))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn load_messages(
        &self,
        conversation_id: String,
    ) -> napi::Result<Vec<StoredMessageJs>> {
        let core = self.inner.clone();
        let msgs = tokio::task::spawn_blocking(move || core.load_messages(conversation_id))
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(msgs.into_iter().map(Into::into).collect())
    }

    #[napi]
    pub async fn load_conversations(&self) -> napi::Result<Vec<ConversationSummaryJs>> {
        let core = self.inner.clone();
        let rows = tokio::task::spawn_blocking(move || core.load_conversations())
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    #[napi]
    pub async fn load_last_message(
        &self,
        conversation_id: String,
    ) -> napi::Result<Option<StoredMessageJs>> {
        let core = self.inner.clone();
        let msg = tokio::task::spawn_blocking(move || core.load_last_message(conversation_id))
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(msg.map(Into::into))
    }

    #[napi]
    pub async fn mark_messages_read(
        &self,
        conversation_id: String,
        up_to_sent_at_ms: i64,
    ) -> napi::Result<i64> {
        let core = self.inner.clone();
        let n = tokio::task::spawn_blocking(move || {
            core.mark_messages_read(conversation_id, up_to_sent_at_ms)
        })
        .await
        .map_err(join_err)?
        .map_err(to_napi)?;
        Ok(n as i64)
    }

    #[napi]
    pub async fn unread_count(&self, conversation_id: String) -> napi::Result<i64> {
        let core = self.inner.clone();
        let n = tokio::task::spawn_blocking(move || core.unread_count(conversation_id))
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(n as i64)
    }

    // ── account / profile / contacts ────────────────────────────────────────

    #[napi]
    pub async fn get_account_info(&self, did: String) -> napi::Result<AccountInfoJs> {
        let core = self.inner.clone();
        let info = tokio::task::spawn_blocking(move || core.get_account_info(did))
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(info.into())
    }

    #[napi]
    pub async fn register_push_token(
        &self,
        device_token: String,
        platform: String,
        relay_url: String,
        environment: String,
    ) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            core.register_push_token(device_token, platform, relay_url, environment)
        })
        .await
        .map_err(join_err)?
        .map_err(to_napi)
    }

    #[napi]
    pub async fn update_recovery_blob(
        &self,
        prf_output: Buffer,
        servers: Vec<String>,
    ) -> napi::Result<()> {
        let core = self.inner.clone();
        let prf = prf_output.to_vec();
        tokio::task::spawn_blocking(move || core.update_recovery_blob(prf, servers))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn own_display_name(&self) -> napi::Result<String> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.own_display_name())
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn set_display_name(&self, display_name: String) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.set_display_name(display_name))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn contact_display_name(&self, did: String) -> napi::Result<String> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.contact_display_name(did))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn refresh_contact_profile(&self, did: String) -> napi::Result<bool> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.refresh_contact_profile(did))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn list_contacts(&self) -> napi::Result<Vec<ContactRowJs>> {
        let core = self.inner.clone();
        let rows = tokio::task::spawn_blocking(move || core.list_contacts())
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    #[napi]
    pub async fn touch_contact(&self, did: String, curated: bool) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.touch_contact(did, curated))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn set_pending_request(&self, did: String, pending: bool) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.set_pending_request(did, pending))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn fetch_and_cache_profile(
        &self,
        did: String,
        profile_key: Buffer,
    ) -> napi::Result<()> {
        let core = self.inner.clone();
        let pk = profile_key.to_vec();
        tokio::task::spawn_blocking(move || core.fetch_and_cache_profile(did, pk))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn has_recovery(&self) -> napi::Result<bool> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.has_recovery())
            .await
            .map_err(join_err)
    }

    #[napi]
    pub async fn prime_contact_profile(
        &self,
        did: String,
        display_name: String,
        profile_key: Buffer,
    ) -> napi::Result<()> {
        let core = self.inner.clone();
        let pk = profile_key.to_vec();
        tokio::task::spawn_blocking(move || core.prime_contact_profile(did, display_name, pk))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    // ── groups ──────────────────────────────────────────────────────────────

    #[napi]
    pub async fn create_group(
        &self,
        title: String,
        description: String,
        expiry_seconds: u32,
    ) -> napi::Result<CreatedGroupJs> {
        let core = self.inner.clone();
        let g = tokio::task::spawn_blocking(move || {
            core.create_group(title, description, expiry_seconds)
        })
        .await
        .map_err(join_err)?
        .map_err(to_napi)?;
        Ok(g.into())
    }

    #[napi]
    pub async fn fetch_group_state(&self, group_id: String) -> napi::Result<GroupSummaryJs> {
        let core = self.inner.clone();
        let s = tokio::task::spawn_blocking(move || core.fetch_group_state(group_id))
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(s.into())
    }

    #[napi]
    pub async fn list_groups(&self) -> napi::Result<Vec<String>> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.list_groups())
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn invite_member(
        &self,
        group_id: String,
        recipient_did: String,
        role: i32,
    ) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.invite_member(group_id, recipient_did, role as i16))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn accept_invite(&self, group_id: String) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.accept_invite(group_id))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn decline_invite(&self, group_id: String) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.decline_invite(group_id))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn join_via_link(
        &self,
        master_key: Buffer,
        hosting_server_url: String,
        password: Buffer,
    ) -> napi::Result<JoinResultJs> {
        let core = self.inner.clone();
        let mk = master_key.to_vec();
        let pw = password.to_vec();
        let r = tokio::task::spawn_blocking(move || core.join_via_link(mk, hosting_server_url, pw))
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(r.into())
    }

    #[napi]
    pub async fn cancel_join_request(&self, group_id: String) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.cancel_join_request(group_id))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn approve_join_request(
        &self,
        group_id: String,
        encrypted_member_id: String,
    ) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            core.approve_join_request(group_id, encrypted_member_id)
        })
        .await
        .map_err(join_err)?
        .map_err(to_napi)
    }

    #[napi]
    pub async fn deny_join_request(
        &self,
        group_id: String,
        encrypted_member_id: String,
    ) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.deny_join_request(group_id, encrypted_member_id))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn remove_member(
        &self,
        group_id: String,
        encrypted_member_id: String,
    ) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.remove_member(group_id, encrypted_member_id))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    /// Leave a group (docs/53). Self-class action: works for any member.
    #[napi]
    pub async fn leave_group(&self, group_id: String) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.leave_group(group_id))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    /// Whether the current identity is still a member of the group (docs/53).
    #[napi]
    pub async fn is_group_member(&self, group_id: String) -> napi::Result<bool> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.is_group_member(group_id))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    /// Last-known group info from the local cache, no network (docs/53). `null`
    /// if nothing is cached.
    #[napi]
    pub async fn cached_group_state(&self, group_id: String) -> napi::Result<Option<GroupSummaryJs>> {
        let core = self.inner.clone();
        let summary = tokio::task::spawn_blocking(move || core.cached_group_state(group_id))
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(summary.map(GroupSummaryJs::from))
    }

    /// Leave this server: leave-cascade hosted groups, then delete the account
    /// on the server (docs/53 §Leave).
    #[napi]
    pub async fn leave_server(&self) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.leave_server())
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    /// Delete this identity: leave-cascade every account, PLC-tombstone the DID,
    /// then wipe all local state (docs/53 §Delete identity).
    #[napi]
    pub async fn delete_identity(&self) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.delete_identity())
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn change_member_role(
        &self,
        group_id: String,
        encrypted_member_id: String,
        new_role: i32,
    ) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            core.change_member_role(group_id, encrypted_member_id, new_role as i16)
        })
        .await
        .map_err(join_err)?
        .map_err(to_napi)
    }

    #[napi]
    pub async fn set_group_expiry(&self, group_id: String, expiry_seconds: u32) -> napi::Result<()> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.set_group_expiry(group_id, expiry_seconds))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn apply_pending_group_changes(&self, group_id: String) -> napi::Result<i64> {
        let core = self.inner.clone();
        tokio::task::spawn_blocking(move || core.apply_pending_group_changes(group_id))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn send_group_message(
        &self,
        group_id: String,
        plaintext: Buffer,
    ) -> napi::Result<()> {
        let core = self.inner.clone();
        let pt = plaintext.to_vec();
        // Group messages now carry a ContentMessage envelope with a
        // sender-assigned timestamp; bots stamp it with the current time.
        let sent_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        tokio::task::spawn_blocking(move || core.send_group_message(group_id, pt, sent_at_ms))
            .await
            .map_err(join_err)?
            .map_err(to_napi)
    }

    #[napi]
    pub async fn rotate_group_pseudonym(&self, group_id: String) -> napi::Result<Buffer> {
        let core = self.inner.clone();
        let bytes = tokio::task::spawn_blocking(move || core.rotate_group_pseudonym(group_id))
            .await
            .map_err(join_err)?
            .map_err(to_napi)?;
        Ok(bytes.into())
    }
}

// ── Free functions ──────────────────────────────────────────────────────────

/// Install a stderr tracing subscriber. Idempotent. `filter` uses RUST_LOG
/// syntax (`"info"`, `"app_core=debug,net=debug"`).
#[napi]
pub fn init_logging(filter: String) {
    core::init_logging(filter);
}

#[napi]
pub async fn resolve_homeserver_from_plc(did: String) -> napi::Result<String> {
    tokio::task::spawn_blocking(move || core::resolve_homeserver_from_plc(did))
        .await
        .map_err(join_err)?
        .map_err(to_napi)
}

#[napi]
pub async fn download_recovery_blob(
    server_url: String,
    did: String,
    prf_output: Buffer,
) -> napi::Result<Vec<String>> {
    let prf = prf_output.to_vec();
    tokio::task::spawn_blocking(move || core::download_recovery_blob(server_url, did, prf))
        .await
        .map_err(join_err)?
        .map_err(to_napi)
}

#[napi]
pub async fn validate_invite(token: String) -> napi::Result<InviteInfoJs> {
    let info = tokio::task::spawn_blocking(move || core::validate_invite(token))
        .await
        .map_err(join_err)?
        .map_err(to_napi)?;
    Ok(info.into())
}
