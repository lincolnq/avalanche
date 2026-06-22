package net.theavalanche.app

import java.util.Date

/**
 * In-memory view of a conversation as the chat list sees it.
 *
 * The persistent source of truth is `message_history` in SQLCipher; this
 * class is rebuilt from `AppCore.loadConversations()` on startup and kept
 * in sync from in-session message activity. Nothing in this class is
 * persisted to SharedPreferences — plaintext bodies and DIDs would leak there.
 */
data class Conversation(
    val id: String,
    var title: String,
    val accountId: String,           // which DID this conversation belongs to
    val serverUrl: String,
    var recipientDid: String? = null, // for DMs: the other party's DID
    /** URL-safe-no-pad base64 group id when this is a group conversation. */
    var groupId: String? = null,
    var lastMessage: String? = null,
    var lastMessageDate: Date? = null,
    /**
     * When the last message is a group system/metadata event (docs/03 §3.6),
     * these let the row render the resolved preview ("You made Bob an admin")
     * reactively — resolving DIDs to names at display time rather than freezing
     * a string at load time (when names may not be cached yet). `0`/`null` for a
     * normal chat message, in which case `lastMessage` (the body) is shown.
     */
    var lastMessageKind: Int = 0,
    var lastMessageMetadata: String? = null,
    var lastMessageSenderDid: String? = null,
    var isGroup: Boolean = false,
    /**
     * True for a DM from an un-curated, un-blocked sender — an unaccepted
     * message request (docs/12 §1). Drives the "Message request" label and
     * the Accept/Delete/Report gate in ConversationView.
     */
    var isRequest: Boolean = false,
    /** True for a DM with a blocked contact (docs/12 §2). */
    var isBlocked: Boolean = false,
) {
    /**
     * Clear the system-event overlay when the latest message becomes a normal
     * chat message, so the preview stops rendering a stale event line. The
     * sender is set separately (it's needed for the "You:"/name preview prefix).
     */
    fun clearLastMessageEvent(): Conversation = copy(
        lastMessageKind = 0,
        lastMessageMetadata = null,
    )
}

/**
 * Build a stable conversation id from a server-visible group id (URL-safe
 * base64). Used for both the in-memory list and the persisted
 * `message_history.conversation_id` column.
 */
fun groupConversationId(groupId: String): String = "group-$groupId"
