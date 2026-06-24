// FFI types shared across Tauri commands.
// These are the single source of truth for the TS↔Rust bridge contract —
// tauri-specta generates `desktop/src/bindings.ts` from these definitions.
//
// Every type here mirrors an app-core UniFFI Record/Enum exactly (same fields,
// same types, same serde shape) so that the generated TS matches what the
// mobile platforms get via UniFFI. When app-core gains a `specta` feature we
// can drop this file and derive specta::Type directly on the originals.

use serde::{Deserialize, Serialize};
use specta::Type;

// ── Conversions from app-core types ─────────────────────────────────────────
// ffi_types mirror app-core's UniFFI types structurally but are separate Rust
// types (so we can derive specta::Type on them). These From impls bridge the
// two — app-core → ffi for return values. For the reverse direction (ffi →
// app-core, used in a handful of command params), we use inherent methods.

use app_core::MessageTarget as AppMessageTarget;
use app_core::ConnectionState as AppConnectionState;
use app_core::StoredMessageFfi as AppStoredMessageFfi;

// ── Struct conversions (app_core → ffi_types) ───────────────────────────────

impl From<app_core::DecryptedMessage> for DecryptedMessage {
    fn from(m: app_core::DecryptedMessage) -> Self {
        Self {
            server_id: m.server_id,
            sender_did: m.sender_did,
            sender_device_id: m.sender_device_id,
            plaintext: m.plaintext,
            sent_at_ms: m.sent_at_ms,
            group_id: m.group_id,
            expire_timer_secs: m.expire_timer_secs,
            profile_key: m.profile_key,
            is_request: m.is_request,
        }
    }
}

impl From<AppStoredMessageFfi> for StoredMessageFfi {
    fn from(m: AppStoredMessageFfi) -> Self {
        Self {
            id: m.id,
            conversation_id: m.conversation_id,
            sender_did: m.sender_did,
            body: m.body,
            sent_at_ms: m.sent_at_ms,
            edited_at_ms: m.edited_at_ms,
            read_at_ms: m.read_at_ms,
            delivery_status: m.delivery_status,
            edit_count: m.edit_count,
            deleted: m.deleted,
            kind: m.kind,
            metadata: m.metadata,
            expire_timer_secs: m.expire_timer_secs,
            expire_at_ms: m.expire_at_ms,
        }
    }
}

impl From<app_core::ConversationSummaryFfi> for ConversationSummaryFfi {
    fn from(s: app_core::ConversationSummaryFfi) -> Self {
        Self {
            conversation_id: s.conversation_id,
            group_title: s.group_title,
            last_message: s.last_message.map(Into::into),
            is_request: s.is_request,
            is_blocked: s.is_blocked,
        }
    }
}

impl From<app_core::AccountInfoFfi> for AccountInfoFfi {
    fn from(a: app_core::AccountInfoFfi) -> Self {
        Self { did: a.did, display_name: a.display_name, is_bot: a.is_bot }
    }
}

impl From<app_core::ProjectInfoFfi> for ProjectInfoFfi {
    fn from(p: app_core::ProjectInfoFfi) -> Self {
        Self { name: p.name, url: p.url, description: p.description }
    }
}

impl From<app_core::ContactRowFfi> for ContactRowFfi {
    fn from(c: app_core::ContactRowFfi) -> Self {
        Self {
            did: c.did,
            display_name: c.display_name,
            is_curated: c.is_curated,
            last_interaction_at_ms: c.last_interaction_at_ms,
        }
    }
}

impl From<app_core::ReactionFfi> for ReactionFfi {
    fn from(r: app_core::ReactionFfi) -> Self {
        Self {
            conversation_id: r.conversation_id,
            target_author: r.target_author,
            target_sent_at_ms: r.target_sent_at_ms,
            reactor_did: r.reactor_did,
            emoji: r.emoji,
            reacted_at_ms: r.reacted_at_ms,
        }
    }
}

impl From<app_core::MessageRevisionFfi> for MessageRevisionFfi {
    fn from(r: app_core::MessageRevisionFfi) -> Self {
        Self { body: r.body, replaced_at_ms: r.replaced_at_ms }
    }
}

impl From<app_core::DeliveryStatusUpdate> for DeliveryStatusUpdate {
    fn from(u: app_core::DeliveryStatusUpdate) -> Self {
        Self {
            conversation_id: u.conversation_id,
            sent_at_ms: u.sent_at_ms,
            delivery_status: u.delivery_status,
        }
    }
}

impl From<app_core::CreatedGroupFfi> for CreatedGroupFfi {
    fn from(g: app_core::CreatedGroupFfi) -> Self {
        Self { group_id: g.group_id, master_key: g.master_key }
    }
}

impl From<app_core::GroupMemberFfi> for GroupMemberFfi {
    fn from(m: app_core::GroupMemberFfi) -> Self {
        Self {
            did: m.did,
            encrypted_member_id: m.encrypted_member_id,
            role: m.role,
            joined_at_ms: m.joined_at_ms,
        }
    }
}

impl From<app_core::GroupPendingFfi> for GroupPendingFfi {
    fn from(p: app_core::GroupPendingFfi) -> Self {
        Self { encrypted_member_id: p.encrypted_member_id, timestamp_ms: p.timestamp_ms }
    }
}

impl From<app_core::GroupSummaryFfi> for GroupSummaryFfi {
    fn from(g: app_core::GroupSummaryFfi) -> Self {
        Self {
            group_id: g.group_id,
            master_key: g.master_key,
            revision: g.revision,
            title: g.title,
            description: g.description,
            expiry_seconds: g.expiry_seconds,
            members: g.members.into_iter().map(Into::into).collect(),
            pending_invites: g.pending_invites.into_iter().map(Into::into).collect(),
            pending_approvals: g.pending_approvals.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<app_core::InviteInfo> for InviteInfo {
    fn from(i: app_core::InviteInfo) -> Self {
        Self {
            server_url: i.server_url,
            server_name: i.server_name,
            inviter_did: i.inviter_did,
            post_onboarding_redirect: i.post_onboarding_redirect,
            inviter_display_name: i.inviter_display_name,
            inviter_profile_key: i.inviter_profile_key,
        }
    }
}

// ── Enum conversions (app_core → ffi_types) ────────────────────────────────

impl From<AppConnectionState> for ConnectionState {
    fn from(s: AppConnectionState) -> Self {
        match s {
            AppConnectionState::Disconnected => Self::Disconnected,
            AppConnectionState::Connecting => Self::Connecting,
            AppConnectionState::Connected => Self::Connected,
            AppConnectionState::Reconnecting { next_attempt_at_ms } => {
                Self::Reconnecting(ReconnectingState { next_attempt_at_ms })
            }
        }
    }
}

impl From<app_core::JoinResultFfi> for JoinResultFfi {
    fn from(j: app_core::JoinResultFfi) -> Self {
        match j {
            app_core::JoinResultFfi::Member => Self::Member,
            app_core::JoinResultFfi::Pending => Self::Pending,
        }
    }
}

impl From<app_core::groups::GroupEventKind> for GroupEventKind {
    fn from(k: app_core::groups::GroupEventKind) -> Self {
        match k {
            app_core::groups::GroupEventKind::MemberJoined => Self::MemberJoined,
            app_core::groups::GroupEventKind::MemberJoinedViaLink => Self::MemberJoinedViaLink,
            app_core::groups::GroupEventKind::MemberRequestedToJoin => Self::MemberRequestedToJoin,
            app_core::groups::GroupEventKind::MemberInvited => Self::MemberInvited,
            app_core::groups::GroupEventKind::MemberLeft => Self::MemberLeft,
            app_core::groups::GroupEventKind::MemberRemoved => Self::MemberRemoved,
            app_core::groups::GroupEventKind::JoinRequestApproved => Self::JoinRequestApproved,
            app_core::groups::GroupEventKind::JoinRequestDenied => Self::JoinRequestDenied,
            app_core::groups::GroupEventKind::InviteDeclined => Self::InviteDeclined,
            app_core::groups::GroupEventKind::JoinRequestCancelled => Self::JoinRequestCancelled,
            app_core::groups::GroupEventKind::RoleChangedToAdmin => Self::RoleChangedToAdmin,
            app_core::groups::GroupEventKind::RoleChangedToMember => Self::RoleChangedToMember,
            app_core::groups::GroupEventKind::TitleChanged => Self::TitleChanged,
            app_core::groups::GroupEventKind::DescriptionChanged => Self::DescriptionChanged,
            app_core::groups::GroupEventKind::ExpiryChanged => Self::ExpiryChanged,
            app_core::groups::GroupEventKind::PolicyChanged => Self::PolicyChanged,
        }
    }
}

impl From<app_core::groups::GroupMetadataEvent> for GroupMetadataEvent {
    fn from(e: app_core::groups::GroupMetadataEvent) -> Self {
        Self {
            group_id: e.group_id,
            revision: e.revision,
            kind: e.kind.into(),
            actor_did: e.actor_did,
            target_did: e.target_did,
            target_emi: e.target_emi,
            occurred_at_ms: e.occurred_at_ms,
            expiry_seconds: e.expiry_seconds,
            new_title: e.new_title,
            summary: e.summary,
        }
    }
}

impl From<app_core::IncomingEvent> for IncomingEvent {
    fn from(ev: app_core::IncomingEvent) -> Self {
        match ev {
            app_core::IncomingEvent::Message { msg } => Self::Message(MessageEvent { msg: msg.into() }),
            app_core::IncomingEvent::ReceiptUpdate { update } => {
                Self::ReceiptUpdate(ReceiptUpdateEvent { update: update.into() })
            }
            app_core::IncomingEvent::GroupInvite { group_id, hosting_server_url, inviter_did } => {
                Self::GroupInvite(GroupInviteEvent { group_id, hosting_server_url, inviter_did })
            }
            app_core::IncomingEvent::MessageEdited { conversation_id, author_did, sent_at_ms, new_body, edited_at_ms } => {
                Self::MessageEdited(MessageEditedEvent { conversation_id, author_did, sent_at_ms, new_body, edited_at_ms })
            }
            app_core::IncomingEvent::MessageDeleted { conversation_id, author_did, sent_at_ms } => {
                Self::MessageDeleted(MessageDeletedEvent { conversation_id, author_did, sent_at_ms })
            }
            app_core::IncomingEvent::ReactionUpdated { conversation_id, target_author, target_sent_at_ms, reactor_did, emoji, removed } => {
                Self::ReactionUpdated(ReactionUpdatedEvent { conversation_id, target_author, target_sent_at_ms, reactor_did, emoji, removed })
            }
            app_core::IncomingEvent::StorageSynced => Self::StorageSynced,
            app_core::IncomingEvent::GroupMetadataChanged { event } => {
                Self::GroupMetadataChanged(GroupMetadataChangedEvent { event: event.into() })
            }
            app_core::IncomingEvent::MessagesExpired { conversation_ids } => {
                Self::MessagesExpired(MessagesExpiredEvent { conversation_ids })
            }
        }
    }
}

// ── Reverse conversions (ffi_types → app_core) for command params ───────────

impl MessageTarget {
    pub fn into_app_core(self) -> AppMessageTarget {
        match self {
            Self::Dm(t) => AppMessageTarget::Dm { recipient_did: t.recipient_did },
            Self::Group(t) => AppMessageTarget::Group { group_id: t.group_id },
        }
    }
}

impl ConnectionState {
    pub fn into_app_core(self) -> AppConnectionState {
        match self {
            Self::Disconnected => AppConnectionState::Disconnected,
            Self::Connecting => AppConnectionState::Connecting,
            Self::Connected => AppConnectionState::Connected,
            Self::Reconnecting(s) => AppConnectionState::Reconnecting { next_attempt_at_ms: s.next_attempt_at_ms },
        }
    }
}

impl StoredMessageFfi {
    pub fn into_app_core(self) -> AppStoredMessageFfi {
        AppStoredMessageFfi {
            id: self.id,
            conversation_id: self.conversation_id,
            sender_did: self.sender_did,
            body: self.body,
            sent_at_ms: self.sent_at_ms,
            edited_at_ms: self.edited_at_ms,
            read_at_ms: self.read_at_ms,
            delivery_status: self.delivery_status,
            edit_count: self.edit_count,
            deleted: self.deleted,
            kind: self.kind,
            metadata: self.metadata,
            expire_timer_secs: self.expire_timer_secs,
            expire_at_ms: self.expire_at_ms,
        }
    }
}

// ── Structs ──────────────────────────────────────────────────────────────────

/// Result returned by account-factory commands (`create_account`, `login`,
/// `recover_from_blob`). Desktop-specific convenience type — app-core returns
/// `Arc<AppCore>` from these constructors; we extract the fields the UI needs.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountResult {
    pub did: String,
    pub display_name: String,
}

/// A decrypted inbound message (mirrors app-core `DecryptedMessage`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DecryptedMessage {
    pub server_id: i64,
    pub sender_did: String,
    pub sender_device_id: u32,
    pub plaintext: Vec<u8>,
    pub sent_at_ms: Option<i64>,
    pub group_id: Option<String>,
    pub expire_timer_secs: u32,
    pub profile_key: Option<Vec<u8>>,
    pub is_request: bool,
}

/// A message from local history (persisted in SQLCipher).
/// Mirrors app-core `StoredMessageFfi`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StoredMessageFfi {
    pub id: String,
    pub conversation_id: String,
    pub sender_did: String,
    pub body: String,
    pub sent_at_ms: i64,
    pub edited_at_ms: Option<i64>,
    pub read_at_ms: Option<i64>,
    pub delivery_status: u8,
    pub edit_count: u32,
    pub deleted: bool,
    pub kind: i64,
    pub metadata: Option<String>,
    pub expire_timer_secs: u32,
    pub expire_at_ms: Option<i64>,
}

/// A conversation summary for the chat list. Mirrors app-core
/// `ConversationSummaryFfi`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummaryFfi {
    pub conversation_id: String,
    pub group_title: Option<String>,
    pub last_message: Option<StoredMessageFfi>,
    pub is_request: bool,
    pub is_blocked: bool,
}

/// Public metadata for an account. Mirrors app-core `AccountInfoFfi`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfoFfi {
    pub did: String,
    pub display_name: Option<String>,
    pub is_bot: bool,
}

/// A first-party Project. Mirrors app-core `ProjectInfoFfi`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfoFfi {
    pub name: String,
    pub url: String,
    pub description: String,
}

/// Minimal contact-list row. Mirrors app-core `ContactRowFfi`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContactRowFfi {
    pub did: String,
    pub display_name: String,
    pub is_curated: bool,
    pub last_interaction_at_ms: i64,
}

/// A reaction on a message. Mirrors app-core `ReactionFfi`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReactionFfi {
    pub conversation_id: String,
    pub target_author: String,
    pub target_sent_at_ms: i64,
    pub reactor_did: String,
    pub emoji: String,
    pub reacted_at_ms: i64,
}

/// A prior body of an edited message. Mirrors app-core `MessageRevisionFfi`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageRevisionFfi {
    pub body: String,
    pub replaced_at_ms: i64,
}

/// A delivery-status update for an outgoing message (e.g. read receipt).
/// Mirrors app-core `DeliveryStatusUpdate`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryStatusUpdate {
    pub conversation_id: String,
    pub sent_at_ms: i64,
    pub delivery_status: u8,
}

/// Result of `create_group`. Mirrors app-core `CreatedGroupFfi`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreatedGroupFfi {
    pub group_id: String,
    pub master_key: Vec<u8>,
}

/// A member's row in a group. Mirrors app-core `GroupMemberFfi`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupMemberFfi {
    pub did: String,
    pub encrypted_member_id: String,
    pub role: i16,
    pub joined_at_ms: i64,
}

/// A pending invite or approval entry. Mirrors app-core `GroupPendingFfi`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupPendingFfi {
    pub encrypted_member_id: String,
    pub timestamp_ms: i64,
}

/// Snapshot of a group's decrypted state. Mirrors app-core `GroupSummaryFfi`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupSummaryFfi {
    pub group_id: String,
    pub master_key: Vec<u8>,
    pub revision: i64,
    pub title: String,
    pub description: String,
    pub expiry_seconds: u32,
    pub members: Vec<GroupMemberFfi>,
    pub pending_invites: Vec<GroupPendingFfi>,
    pub pending_approvals: Vec<GroupPendingFfi>,
}

/// Decoded invite token info. Mirrors app-core `InviteInfo`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InviteInfo {
    pub server_url: String,
    pub server_name: String,
    pub inviter_did: Option<String>,
    pub post_onboarding_redirect: Option<String>,
    pub inviter_display_name: Option<String>,
    pub inviter_profile_key: Option<Vec<u8>>,
}

/// One derived chat-timeline entry describing a membership/metadata change.
/// Mirrors app-core `GroupMetadataEvent`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupMetadataEvent {
    pub group_id: String,
    pub revision: i64,
    pub kind: GroupEventKind,
    pub actor_did: String,
    pub target_did: String,
    pub target_emi: String,
    pub occurred_at_ms: i64,
    pub expiry_seconds: u32,
    pub new_title: String,
    pub summary: String,
}

// ── Enums ────────────────────────────────────────────────────────────────────

/// Liveness of the connection to the homeserver. Mirrors app-core
/// `ConnectionState`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting(ReconnectingState),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReconnectingState {
    pub next_attempt_at_ms: i64,
}

/// Where a content operation is directed. Mirrors app-core `MessageTarget`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MessageTarget {
    Dm(DmTarget),
    Group(GroupTarget),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DmTarget {
    pub recipient_did: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupTarget {
    pub group_id: String,
}

/// Outcome of `join_via_link`. Mirrors app-core `JoinResultFfi`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
#[allow(dead_code)] // used once join_via_link is added as a command
pub enum JoinResultFfi {
    Member,
    Pending,
}

/// Membership/metadata change kind for group timeline entries.
/// Mirrors app-core `GroupEventKind`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GroupEventKind {
    MemberJoined,
    MemberJoinedViaLink,
    MemberRequestedToJoin,
    MemberInvited,
    MemberLeft,
    MemberRemoved,
    JoinRequestApproved,
    JoinRequestDenied,
    InviteDeclined,
    JoinRequestCancelled,
    RoleChangedToAdmin,
    RoleChangedToMember,
    TitleChanged,
    DescriptionChanged,
    ExpiryChanged,
    PolicyChanged,
}

/// A single event surfaced to the FFI from the background reconnect task.
/// Mirrors app-core `IncomingEvent`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum IncomingEvent {
    Message(MessageEvent),
    ReceiptUpdate(ReceiptUpdateEvent),
    GroupInvite(GroupInviteEvent),
    MessageEdited(MessageEditedEvent),
    MessageDeleted(MessageDeletedEvent),
    ReactionUpdated(ReactionUpdatedEvent),
    StorageSynced,
    GroupMetadataChanged(GroupMetadataChangedEvent),
    MessagesExpired(MessagesExpiredEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageEvent {
    pub msg: DecryptedMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptUpdateEvent {
    pub update: DeliveryStatusUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupInviteEvent {
    pub group_id: String,
    pub hosting_server_url: String,
    pub inviter_did: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageEditedEvent {
    pub conversation_id: String,
    pub author_did: String,
    pub sent_at_ms: i64,
    pub new_body: String,
    pub edited_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageDeletedEvent {
    pub conversation_id: String,
    pub author_did: String,
    pub sent_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReactionUpdatedEvent {
    pub conversation_id: String,
    pub target_author: String,
    pub target_sent_at_ms: i64,
    pub reactor_did: String,
    pub emoji: String,
    pub removed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupMetadataChangedEvent {
    pub event: GroupMetadataEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MessagesExpiredEvent {
    pub conversation_ids: Vec<String>,
}
