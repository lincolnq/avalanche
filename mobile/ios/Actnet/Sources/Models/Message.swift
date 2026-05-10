import Foundation

struct Message: Identifiable {
    let id: String
    let conversationId: String
    let senderAccountId: String
    let body: String
    let sentAt: Date
    var editedAt: Date?
    /// nil = unread, non-nil = when it was read. Outgoing messages set this to sentAt.
    var readAt: Date?
    var isEdited: Bool { editedAt != nil }
    var isRead: Bool { readAt != nil }
}
