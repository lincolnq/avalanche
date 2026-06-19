import Foundation

/// In-memory view of a conversation as the chat list sees it.
///
/// The persistent source of truth is `message_history` in SQLCipher; this
/// struct is rebuilt from `AppCore.loadConversations()` on startup and kept
/// in sync from in-session message activity. Nothing in this struct is
/// persisted to UserDefaults — plaintext bodies and DIDs would leak there.
struct Conversation: Identifiable, Hashable {
    let id: String
    var title: String
    let accountId: String  // which DID this conversation belongs to
    let serverUrl: String
    var recipientDid: String?  // for DMs: the other party's DID
    /// URL-safe-no-pad base64 group id when this is a group conversation.
    var groupId: String?
    var lastMessage: String?
    var lastMessageDate: Date?
    /// When the last message is a group system/metadata event (docs/03 §3.6),
    /// these let the row render the resolved preview ("You made Bob an admin")
    /// reactively — resolving DIDs to names at display time rather than freezing
    /// a string at load time (when names may not be cached yet). `0`/`nil` for a
    /// normal chat message, in which case `lastMessage` (the body) is shown.
    var lastMessageKind: Int = 0
    var lastMessageMetadata: String?
    var lastMessageSenderDid: String?
    var isGroup: Bool = false
    /// True for a DM from an un-curated, un-blocked sender — an unaccepted
    /// message request (docs/12 §1). Drives the "Message request" label and
    /// the Accept/Delete/Report gate in `ConversationView`.
    var isRequest: Bool = false
    /// True for a DM with a blocked contact (docs/12 §2).
    var isBlocked: Bool = false

    /// Clear the system-event overlay when the latest message becomes a normal
    /// chat message, so the preview stops rendering a stale event line. The
    /// sender is set separately (it's needed for the "You:"/name preview prefix).
    mutating func clearLastMessageEvent() {
        lastMessageKind = 0
        lastMessageMetadata = nil
    }
}

/// Build a stable conversation id from a server-visible group id (URL-safe
/// base64). Used for both the in-memory list and the persisted
/// `message_history.conversation_id` column.
func groupConversationId(_ groupId: String) -> String {
    "group-\(groupId)"
}
