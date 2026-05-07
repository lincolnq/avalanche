import Foundation

/// Mock AppCore for UI development without the Rust library.
/// Simulates registration, conversations appearing, and echo replies.
final class MockAppCore: AppCoreProtocol, @unchecked Sendable {
    private let mockDid: String
    private let mockDeviceId: UInt32 = 1
    private var pendingMessages: [DecryptedMessage] = []
    private var nextMessageId: Int64 = 1
    private let lock = NSLock()

    init(did: String? = nil) {
        self.mockDid = did ?? "did:plc:mock\(UUID().uuidString.prefix(8).lowercased())"
    }

    func did() -> String { mockDid }
    func deviceId() -> UInt32 { mockDeviceId }

    func sendDm(recipientDid: String, recipientDeviceId: UInt32, plaintext: Data) throws {
        // Simulate a slight network delay
        Thread.sleep(forTimeInterval: 0.1)

        // Schedule an echo reply after a short delay
        let text = String(data: plaintext, encoding: .utf8) ?? ""
        DispatchQueue.global().asyncAfter(deadline: .now() + 1.5) { [weak self] in
            self?.enqueueMessage(from: recipientDid, text: "Echo: \(text)")
        }
    }

    func fetchProjects() throws -> [ProjectInfoFfi] {
        [ProjectInfoFfi(name: "Testbot", url: "http://localhost:3001", description: "Chat with an AI bot")]
    }

    func requestProjectToken(projectUrl: String) throws -> String {
        "mock-project-token-\(UUID().uuidString.prefix(8))"
    }

    func receiveMessages() throws -> [DecryptedMessage] {
        Thread.sleep(forTimeInterval: 0.1)
        lock.lock()
        let msgs = pendingMessages
        pendingMessages.removeAll()
        lock.unlock()
        return msgs
    }

    func receiveMessagesWs() throws -> [DecryptedMessage] {
        // Simulate WebSocket blocking: sleep until messages arrive.
        for _ in 0..<20 {
            Thread.sleep(forTimeInterval: 0.1)
            lock.lock()
            if !pendingMessages.isEmpty {
                let msgs = pendingMessages
                pendingMessages.removeAll()
                lock.unlock()
                return msgs
            }
            lock.unlock()
        }
        return []
    }

    func enqueueMessage(from senderDid: String, text: String) {
        lock.lock()
        let msg = DecryptedMessage(
            serverId: nextMessageId,
            senderDid: senderDid,
            senderDeviceId: 1,
            plaintext: Data(text.utf8)
        )
        nextMessageId += 1
        pendingMessages.append(msg)
        lock.unlock()
    }
}

/// Mock service that creates fake accounts and seeds initial conversations.
struct MockActnetService: ActnetService {
    func createAccount(serverUrl: String, dbPath: String, dbKey: String) throws -> any AppCoreProtocol {
        Thread.sleep(forTimeInterval: 0.5) // simulate network
        return MockAppCore()
    }

    func login(dbPath: String, dbKey: String) throws -> any AppCoreProtocol {
        MockAppCore()
    }
}

/// Seed data for mock mode — conversations that appear after registration.
enum MockData {
    static func seedConversations(accountId: String, serverUrl: String) -> [Conversation] {
        let now = Date()
        return [
            Conversation(
                id: "conv-general",
                title: "General",
                accountId: accountId,
                serverUrl: serverUrl,
                lastMessage: "Welcome to the server!",
                lastMessageDate: now.addingTimeInterval(-60),
                unreadCount: 1,
                isGroup: true
            ),
            Conversation(
                id: "conv-announcements",
                title: "Announcements",
                accountId: accountId,
                serverUrl: serverUrl,
                lastMessage: "Rally this Saturday at 10am",
                lastMessageDate: now.addingTimeInterval(-3600),
                unreadCount: 0,
                isGroup: true
            ),
            Conversation(
                id: "conv-dm-organizer",
                title: "Jamie (Organizer)",
                accountId: accountId,
                serverUrl: serverUrl,
                lastMessage: "Hey, welcome aboard!",
                lastMessageDate: now.addingTimeInterval(-120),
                unreadCount: 1,
                isGroup: false
            ),
        ]
    }
}
