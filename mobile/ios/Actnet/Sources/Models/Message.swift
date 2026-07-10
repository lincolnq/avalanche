import Foundation

/// `GroupEventKind` itself is the UniFFI-generated enum (carried live on
/// `IncomingEvent.groupMetadataChanged`). This maps the persisted `kind` code
/// (`kind_code` in `core/crates/app-core/src/groups.rs`) back to it for rows
/// loaded from the store. Keep the codes in sync with the Rust side.
extension GroupEventKind {
    init?(code: Int) {
        switch code {
        case 1: self = .memberJoined
        case 2: self = .memberJoinedViaLink
        case 3: self = .memberRequestedToJoin
        case 4: self = .memberInvited
        case 5: self = .memberLeft
        case 6: self = .memberRemoved
        case 7: self = .joinRequestApproved
        case 8: self = .joinRequestDenied
        case 9: self = .inviteDeclined
        case 10: self = .joinRequestCancelled
        case 11: self = .roleChangedToAdmin
        case 12: self = .roleChangedToMember
        case 13: self = .titleChanged
        case 14: self = .descriptionChanged
        case 15: self = .expiryChanged
        case 16: self = .policyChanged
        default: return nil
        }
    }
}

/// Structured payload carried in a system row's `metadata` JSON.
struct GroupEventMeta {
    let event: GroupEventKind
    let actorDid: String
    let targetDid: String
    let targetEmi: String
    /// For `.expiryChanged`, the new disappearing-message timer in seconds
    /// (`0` = off). `0` for other kinds.
    let expirySeconds: UInt32
    /// For `.titleChanged`, the new group name. Empty for other kinds.
    let newTitle: String
}

enum DeliveryStatus: Int {
    case sending = 0
    case sent = 1
    case delivered = 2
    case read = 3
    /// Send failed (network down, server rejected, etc.). UI shows a red indicator.
    case failed = 4
}

struct Message: Identifiable {
    let id: String
    let conversationId: String
    let senderAccountId: String
    var body: String
    /// Sender's timestamp in unix millis. Single source of truth — never round-trip through Date.
    let sentAtMs: Int64
    var editedAtMs: Int64?
    /// nil = unread, non-nil = unix millis when marked read. Outgoing messages set this to sentAtMs.
    var readAtMs: Int64?
    /// Delivery status for outgoing messages (sent/delivered/read).
    var deliveryStatus: DeliveryStatus
    /// Number of times this message has been edited (docs/36).
    var editCount: Int = 0
    /// FOR_EVERYONE tombstone: render the "This message was deleted" placeholder
    /// instead of the body (docs/36).
    var isDeleted: Bool = false
    /// 0 = normal chat message; >0 = a group system/metadata timeline entry
    /// (docs/03 §3.6). Rendered as a centered grey line, not a chat bubble.
    var kind: Int = 0
    /// JSON payload for system rows (`metadata` from the store); nil otherwise.
    var metadata: String? = nil
    /// Disappearing-messages timer in seconds (docs/03 §5); 0 = no expiry.
    var expireTimerSecs: UInt32 = 0
    /// Unix-millis deletion deadline once the countdown started (on read), or
    /// nil. The UI schedules the live disappear from this.
    var expireAtMs: Int64? = nil
    /// Attachments on this message (docs/35); empty for plain text.
    var attachments: [AttachmentFfi] = []
    /// Link-preview cards on this message (docs/35); empty for plain text.
    var previews: [LinkPreviewFfi] = []
    /// Shared contact cards on this message (docs/35); empty for plain text.
    /// Rendered as a tappable card with a "Save contact" action.
    var contacts: [SharedContactFfi] = []

    var sentAt: Date { Date(timeIntervalSince1970: Double(sentAtMs) / 1000.0) }
    var isEdited: Bool { editedAtMs != nil }
    var isRead: Bool { readAtMs != nil }

    /// True for a group system/metadata entry ("Alice added Bob", "Bob left", …).
    var isSystemEvent: Bool { kind > 0 }

    /// Decoded structured payload for a system event, if this is one and the
    /// metadata parses. UIs prefer this (resolving DIDs to display names) over
    /// the pre-rendered English `body`.
    var groupEvent: GroupEventMeta? {
        guard kind > 0, let data = metadata?.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let code = obj["event"] as? Int, let event = GroupEventKind(code: code)
        else { return nil }
        return GroupEventMeta(
            event: event,
            actorDid: obj["actor_did"] as? String ?? "",
            targetDid: obj["target_did"] as? String ?? "",
            targetEmi: obj["target_emi"] as? String ?? "",
            expirySeconds: (obj["expiry_seconds"] as? Int).map(UInt32.init) ?? 0,
            newTitle: obj["new_title"] as? String ?? ""
        )
    }
}
