import Foundation

enum DeliveryStatus: Int {
    case sending = 0
    case sent = 1
    case delivered = 2
    case read = 3
}

struct Message: Identifiable {
    let id: String
    let conversationId: String
    let senderAccountId: String
    let body: String
    /// Sender's timestamp in unix millis. Single source of truth — never round-trip through Date.
    let sentAtMs: Int64
    var editedAtMs: Int64?
    /// nil = unread, non-nil = unix millis when marked read. Outgoing messages set this to sentAtMs.
    var readAtMs: Int64?
    /// Delivery status for outgoing messages (sent/delivered/read).
    var deliveryStatus: DeliveryStatus

    var sentAt: Date { Date(timeIntervalSince1970: Double(sentAtMs) / 1000.0) }
    var isEdited: Bool { editedAtMs != nil }
    var isRead: Bool { readAtMs != nil }
}
