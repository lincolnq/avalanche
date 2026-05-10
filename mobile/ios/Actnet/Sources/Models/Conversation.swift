import Foundation

struct Conversation: Identifiable, Codable {
    let id: String
    let title: String
    let accountId: String  // which DID this conversation belongs to
    let serverUrl: String
    var recipientDid: String?  // for DMs: the other party's DID
    var lastMessage: String?
    var lastMessageDate: Date?
    var isGroup: Bool = false

    // Exclude lastMessage (plaintext) from UserDefaults persistence.
    // Timestamps are non-sensitive metadata, safe to persist.
    private enum CodingKeys: String, CodingKey {
        case id, title, accountId, serverUrl, recipientDid, isGroup
        case lastMessageDate
    }
}
