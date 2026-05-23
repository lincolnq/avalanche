import Foundation

/// Mock AppCore for UI development without the Rust library.
/// Simulates registration, conversations appearing, and echo replies.
final class MockAppCore: AppCoreProtocol, @unchecked Sendable {
    private let mockDid: String
    private let mockDeviceId: UInt32 = 1
    private var pendingMessages: [DecryptedMessage] = []
    private var nextMessageId: Int64 = 1
    private let lock = NSLock()
    private var storedMessages: [String: [StoredMessageFfi]] = [:]  // keyed by conversation_id

    init(did: String? = nil) {
        self.mockDid = did ?? "did:plc:mock\(UUID().uuidString.prefix(8).lowercased())"
    }

    func did() -> String { mockDid }
    func deviceId() -> UInt32 { mockDeviceId }

    func sendDm(recipientDid: String, recipientDeviceId: UInt32, plaintext: Data, sentAtMs: Int64) throws {
        // Simulate a slight network delay
        Thread.sleep(forTimeInterval: 0.1)

        // Schedule an echo reply after a short delay
        let text = String(data: plaintext, encoding: .utf8) ?? ""
        DispatchQueue.global().asyncAfter(deadline: .now() + 1.5) { [weak self] in
            self?.enqueueMessage(from: recipientDid, text: "Echo: \(text)")
        }
    }

    func getAccountInfo(did: String) throws -> AccountInfoFfi {
        AccountInfoFfi(did: did, displayName: nil, isBot: false)
    }

    func fetchProjects() throws -> [ProjectInfoFfi] {
        [ProjectInfoFfi(name: "Testbot", url: "http://localhost:3001", description: "Chat with an AI bot")]
    }

    func requestProjectToken(projectUrl: String) throws -> String {
        "mock-project-token-\(UUID().uuidString.prefix(8))"
    }

    func saveMessage(msg: StoredMessageFfi) throws {
        lock.lock()
        storedMessages[msg.conversationId, default: []].append(msg)
        lock.unlock()
    }

    func loadMessages(conversationId: String) throws -> [StoredMessageFfi] {
        lock.lock()
        let msgs = storedMessages[conversationId] ?? []
        lock.unlock()
        return msgs.sorted { $0.sentAtMs < $1.sentAtMs }
    }

    func markMessagesRead(conversationId: String, upToSentAtMs: Int64) throws -> UInt64 {
        lock.lock()
        defer { lock.unlock() }
        var count: UInt64 = 0
        guard var msgs = storedMessages[conversationId] else { return 0 }
        let now = Int64(Date().timeIntervalSince1970 * 1000)
        for i in msgs.indices where msgs[i].sentAtMs <= upToSentAtMs && msgs[i].readAtMs == nil && msgs[i].senderDid != mockDid {
            msgs[i] = StoredMessageFfi(
                id: msgs[i].id,
                conversationId: msgs[i].conversationId,
                senderDid: msgs[i].senderDid,
                body: msgs[i].body,
                sentAtMs: msgs[i].sentAtMs,
                editedAtMs: msgs[i].editedAtMs,
                readAtMs: now,
                deliveryStatus: msgs[i].deliveryStatus
            )
            count += 1
        }
        storedMessages[conversationId] = msgs
        return count
    }

    func sendReadReceipt(recipientDid: String, recipientDeviceId: UInt32, timestamps: [Int64]) throws {
        // Mock: no-op
    }

    func registerPushToken(deviceToken: String, platform: String) throws {
        // Mock: no-op
    }

    func rotatePushPseudonym() throws {
        // Mock: no-op
    }

    func drainReceiptUpdates() -> [DeliveryStatusUpdate] {
        // Mock: no receipt updates
        []
    }

    func unreadCount(conversationId: String) throws -> UInt64 {
        lock.lock()
        defer { lock.unlock() }
        let msgs = storedMessages[conversationId] ?? []
        return UInt64(msgs.filter { $0.readAtMs == nil && $0.senderDid != mockDid }.count)
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
        let now = Int64(Date().timeIntervalSince1970 * 1000)
        let msg = DecryptedMessage(
            serverId: nextMessageId,
            senderDid: senderDid,
            senderDeviceId: 1,
            plaintext: Data(text.utf8),
            sentAtMs: now
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
                isGroup: true
            ),
            Conversation(
                id: "conv-announcements",
                title: "Announcements",
                accountId: accountId,
                serverUrl: serverUrl,
                lastMessage: "Rally this Saturday at 10am",
                lastMessageDate: now.addingTimeInterval(-3600),
                isGroup: true
            ),
            Conversation(
                id: "conv-dm-organizer",
                title: "Jamie (Organizer)",
                accountId: accountId,
                serverUrl: serverUrl,
                lastMessage: "Hey, welcome aboard!",
                lastMessageDate: now.addingTimeInterval(-120),
                isGroup: false
            ),
        ]
    }
}
