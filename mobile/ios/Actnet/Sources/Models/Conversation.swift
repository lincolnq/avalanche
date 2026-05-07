import Foundation

struct Conversation: Identifiable, Codable {
    let id: String
    let title: String
    let accountId: String  // which DID this conversation belongs to
    let serverUrl: String
    var recipientDid: String?  // for DMs: the other party's DID
    var lastMessage: String?
    var lastMessageDate: Date?
    var unreadCount: Int = 0
    var isGroup: Bool = false

    // Don't persist message content or ephemeral state to avoid
    // storing plaintext in unencrypted UserDefaults.
    private enum CodingKeys: String, CodingKey {
        case id, title, accountId, serverUrl, recipientDid, isGroup
    }
}
