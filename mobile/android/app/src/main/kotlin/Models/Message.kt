package net.theavalanche.app

import org.json.JSONObject
import java.util.Date
import uniffi.app_core.GroupEventKind

// `GroupEventKind` is the UniFFI-generated enum (from Generated/uniffi/app_core/app_core.kt).
// This companion maps the persisted `kind_code` back to it for rows loaded from the store.
// Keep the codes in sync with the Rust side (core/crates/app-core/src/groups.rs).
fun GroupEventKind.Companion.fromCode(code: Int): GroupEventKind? = when (code) {
    1 -> GroupEventKind.MEMBER_JOINED
    2 -> GroupEventKind.MEMBER_JOINED_VIA_LINK
    3 -> GroupEventKind.MEMBER_REQUESTED_TO_JOIN
    4 -> GroupEventKind.MEMBER_INVITED
    5 -> GroupEventKind.MEMBER_LEFT
    6 -> GroupEventKind.MEMBER_REMOVED
    7 -> GroupEventKind.JOIN_REQUEST_APPROVED
    8 -> GroupEventKind.JOIN_REQUEST_DENIED
    9 -> GroupEventKind.INVITE_DECLINED
    10 -> GroupEventKind.JOIN_REQUEST_CANCELLED
    11 -> GroupEventKind.ROLE_CHANGED_TO_ADMIN
    12 -> GroupEventKind.ROLE_CHANGED_TO_MEMBER
    13 -> GroupEventKind.TITLE_CHANGED
    14 -> GroupEventKind.DESCRIPTION_CHANGED
    15 -> GroupEventKind.EXPIRY_CHANGED
    16 -> GroupEventKind.POLICY_CHANGED
    else -> null
}

/// Structured payload carried in a system row's `metadata` JSON.
data class GroupEventMeta(
    val event: GroupEventKind,
    val actorDid: String,
    val targetDid: String,
    val targetEmi: String,
    /// For `.expiryChanged`, the new disappearing-message timer in seconds
    /// (`0` = off). `0` for other kinds.
    val expirySeconds: UInt,
    /// For `.titleChanged`, the new group name. Empty for other kinds.
    val newTitle: String,
)

enum class DeliveryStatus(val code: Int) {
    SENDING(0),
    SENT(1),
    DELIVERED(2),
    READ(3),
    /// Send failed (network down, server rejected, etc.). UI shows a red indicator.
    FAILED(4);

    companion object {
        fun fromCode(code: Int): DeliveryStatus =
            values().firstOrNull { it.code == code } ?: SENDING
    }
}

data class Message(
    val id: String,
    val conversationId: String,
    val senderAccountId: String,
    var body: String,
    /// Sender's timestamp in unix millis. Single source of truth — never round-trip through Date.
    val sentAtMs: Long,
    var editedAtMs: Long? = null,
    /// null = unread, non-null = unix millis when marked read. Outgoing messages set this to sentAtMs.
    var readAtMs: Long? = null,
    /// Delivery status for outgoing messages (sent/delivered/read).
    var deliveryStatus: DeliveryStatus = DeliveryStatus.SENDING,
    /// Number of times this message has been edited (docs/36).
    var editCount: Int = 0,
    /// FOR_EVERYONE tombstone: render the "This message was deleted" placeholder
    /// instead of the body (docs/36).
    var isDeleted: Boolean = false,
    /// 0 = normal chat message; >0 = a group system/metadata timeline entry
    /// (docs/03 §3.6). Rendered as a centered grey line, not a chat bubble.
    var kind: Int = 0,
    /// JSON payload for system rows (`metadata` from the store); null otherwise.
    var metadata: String? = null,
    /// Disappearing-messages timer in seconds (docs/03 §5); 0 = no expiry.
    var expireTimerSecs: UInt = 0u,
    /// Unix-millis deletion deadline once the countdown started (on read), or
    /// null. The UI schedules the live disappear from this.
    var expireAtMs: Long? = null,
) {
    val sentAt: Date get() = Date(sentAtMs)

    val isEdited: Boolean get() = editedAtMs != null

    val isRead: Boolean get() = readAtMs != null

    /// True for a group system/metadata entry ("Alice added Bob", "Bob left", …).
    val isSystemEvent: Boolean get() = kind > 0

    /// Decoded structured payload for a system event, if this is one and the
    /// metadata parses. UIs prefer this (resolving DIDs to display names) over
    /// the pre-rendered English `body`.
    val groupEvent: GroupEventMeta?
        get() {
            if (kind <= 0) return null
            val meta = metadata ?: return null
            return try {
                val obj = JSONObject(meta)
                val code = obj.optInt("event", -1)
                val event = GroupEventKind.fromCode(code) ?: return null
                GroupEventMeta(
                    event = event,
                    actorDid = obj.optString("actor_did", ""),
                    targetDid = obj.optString("target_did", ""),
                    targetEmi = obj.optString("target_emi", ""),
                    expirySeconds = obj.optInt("expiry_seconds", 0).toUInt(),
                    newTitle = obj.optString("new_title", ""),
                )
            } catch (e: Exception) {
                null
            }
        }
}
