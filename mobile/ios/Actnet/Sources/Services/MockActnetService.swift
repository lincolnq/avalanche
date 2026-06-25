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
    private var ownDisplayName_: String
    private var contactDisplayNames: [String: String] = [:]

    init(did: String? = nil, displayName: String = "") {
        self.mockDid = did ?? "did:plc:mock\(UUID().uuidString.prefix(8).lowercased())"
        self.ownDisplayName_ = displayName
    }

    func did() -> String { mockDid }
    func deviceId() -> UInt32 { mockDeviceId }

    func sendDm(recipientDid: String, plaintext: Data, sentAtMs: Int64) throws {
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

    // MARK: - Reactions / editing / deletion (mock; docs/33, docs/36)

    /// Reactions keyed by conversation id (`dm-<mockDid>-<peer>` or `group-<id>`).
    private var reactionsByConv: [String: [ReactionFfi]] = [:]
    /// Prior bodies keyed by "<convId>|<author>|<sentAt>".
    private var revisionsByTarget: [String: [MessageRevisionFfi]] = [:]

    /// Local conversation id for a target — mirrors the core's `conv_id_for`.
    private func convId(for target: MessageTarget) -> String {
        switch target {
        case .dm(let recipientDid): return "dm-\(mockDid)-\(recipientDid)"
        case .group(let groupId): return "group-\(groupId)"
        }
    }

    func sendReaction(target: MessageTarget, targetAuthor: String, targetSentAtMs: Int64, emoji: String, remove: Bool, sentAtMs: Int64) throws {
        let cid = convId(for: target)
        lock.lock(); defer { lock.unlock() }
        var list = reactionsByConv[cid] ?? []
        // One reaction per (target, reactor=self): drop any prior, then re-add.
        list.removeAll { $0.targetAuthor == targetAuthor && $0.targetSentAtMs == targetSentAtMs && $0.reactorDid == mockDid }
        if !remove {
            list.append(ReactionFfi(conversationId: cid, targetAuthor: targetAuthor, targetSentAtMs: targetSentAtMs, reactorDid: mockDid, emoji: emoji, reactedAtMs: sentAtMs))
        }
        reactionsByConv[cid] = list
    }

    func loadReactions(conversationId: String) throws -> [ReactionFfi] {
        lock.lock(); defer { lock.unlock() }
        return reactionsByConv[conversationId] ?? []
    }

    func sendEdit(target: MessageTarget, targetSentAtMs: Int64, newBody: String, sentAtMs: Int64) throws {
        let cid = convId(for: target)
        lock.lock(); defer { lock.unlock() }
        guard var msgs = storedMessages[cid] else { return }
        for i in msgs.indices where msgs[i].senderDid == mockDid && msgs[i].sentAtMs == targetSentAtMs && !msgs[i].deleted {
            revisionsByTarget["\(cid)|\(mockDid)|\(targetSentAtMs)", default: []].append(MessageRevisionFfi(body: msgs[i].body, replacedAtMs: sentAtMs))
            msgs[i].body = newBody
            msgs[i].editedAtMs = sentAtMs
            msgs[i].editCount += 1
        }
        storedMessages[cid] = msgs
    }

    func loadMessageRevisions(conversationId: String, author: String, sentAtMs: Int64) throws -> [MessageRevisionFfi] {
        lock.lock(); defer { lock.unlock() }
        return revisionsByTarget["\(conversationId)|\(author)|\(sentAtMs)"] ?? []
    }

    func sendDelete(target: MessageTarget, targetAuthor: String, targetSentAtMs: Int64, forEveryone: Bool, sentAtMs: Int64) throws {
        let cid = convId(for: target)
        lock.lock(); defer { lock.unlock() }
        guard var msgs = storedMessages[cid] else { return }
        if forEveryone {
            for i in msgs.indices where msgs[i].senderDid == targetAuthor && msgs[i].sentAtMs == targetSentAtMs {
                msgs[i].body = ""
                msgs[i].editedAtMs = nil
                msgs[i].deleted = true
            }
        } else {
            msgs.removeAll { $0.senderDid == targetAuthor && $0.sentAtMs == targetSentAtMs }
        }
        storedMessages[cid] = msgs
        reactionsByConv[cid]?.removeAll { $0.targetAuthor == targetAuthor && $0.targetSentAtMs == targetSentAtMs }
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

    func loadLastMessage(conversationId: String) throws -> StoredMessageFfi? {
        lock.lock()
        let msgs = storedMessages[conversationId] ?? []
        lock.unlock()
        return msgs.max(by: { $0.sentAtMs < $1.sentAtMs })
    }

    func loadConversations() throws -> [ConversationSummaryFfi] {
        lock.lock()
        let snapshot = storedMessages
        lock.unlock()
        return snapshot.compactMap { (convId, msgs) -> ConversationSummaryFfi? in
            guard let last = msgs.max(by: { $0.sentAtMs < $1.sentAtMs }) else { return nil }
            return ConversationSummaryFfi(conversationId: convId, groupTitle: nil, lastMessage: last, isRequest: false, isBlocked: false)
        }
        .sorted { ($0.lastMessage?.sentAtMs ?? 0) > ($1.lastMessage?.sentAtMs ?? 0) }
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
                deliveryStatus: msgs[i].deliveryStatus,
                editCount: msgs[i].editCount,
                deleted: msgs[i].deleted,
                kind: msgs[i].kind,
                metadata: msgs[i].metadata,
                expireTimerSecs: msgs[i].expireTimerSecs,
                expireAtMs: msgs[i].expireAtMs
            )
            count += 1
        }
        storedMessages[conversationId] = msgs
        return count
    }

    func sendReadReceipt(recipientDid: String, timestamps: [Int64]) throws {
        // Mock: no-op
    }

    func registerPushToken(deviceToken: String, platform: String, relayUrl: String, environment: String) throws {
        // Mock: no-op
    }

    func unregisterPushToken(relayUrl: String) throws {
        // Mock: no-op
    }


    // MARK: - Connection state

    func connectionState() -> ConnectionState {
        // Mock is always "online".
        .connected
    }

    func waitForConnectionStateChange(last: ConnectionState) throws -> ConnectionState {
        // Mock never changes — block forever (the listener task is fine
        // sitting on a never-resolving call).
        Thread.sleep(forTimeInterval: 60 * 60)
        return .connected
    }

    func nextEvents() throws -> [IncomingEvent] {
        // Drain any pending incoming messages as Message events.
        for _ in 0..<20 {
            Thread.sleep(forTimeInterval: 0.1)
            lock.lock()
            if !pendingMessages.isEmpty {
                let msgs = pendingMessages
                pendingMessages.removeAll()
                lock.unlock()
                return msgs.map { IncomingEvent.message(msg: $0) }
            }
            lock.unlock()
        }
        // No events arrived in the polling window — return empty so caller loops.
        return []
    }

    func hasRecovery() -> Bool {
        false
    }

    func updateRecoveryBlob(prfOutput: Data, servers: [String]) throws {
        // Mock: no-op
    }

    func ownDisplayName() throws -> String {
        lock.lock(); defer { lock.unlock() }
        return ownDisplayName_
    }

    func setDisplayName(displayName: String) throws {
        lock.lock(); defer { lock.unlock() }
        ownDisplayName_ = displayName
    }

    func contactDisplayName(did: String) throws -> String {
        lock.lock(); defer { lock.unlock() }
        return contactDisplayNames[did] ?? ""
    }

    func refreshContactProfile(did: String) throws -> Bool {
        // Mock: nothing to refresh from.
        false
    }

    func primeContactProfile(did: String, displayName: String, profileKey: Data) throws {
        lock.lock(); defer { lock.unlock() }
        contactDisplayNames[did] = displayName
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

    // Group methods inherit the no-op defaults from
    // `AppCoreProtocol+Defaults.swift`; override here if a preview needs
    // a more interesting behavior.

    func enqueueMessage(from senderDid: String, text: String) {
        lock.lock()
        let now = Int64(Date().timeIntervalSince1970 * 1000)
        let msg = DecryptedMessage(
            serverId: nextMessageId,
            senderDid: senderDid,
            senderDeviceId: 1,
            plaintext: Data(text.utf8),
            sentAtMs: now,
            groupId: nil,
            expireTimerSecs: 0,
            profileKey: nil,
            isRequest: false
        )
        nextMessageId += 1
        pendingMessages.append(msg)
        lock.unlock()
    }
}

/// Mock `PreparedAccountProtocol` that just hands back a fabricated DID.
final class MockPreparedAccount: PreparedAccountProtocol, @unchecked Sendable {
    private let storedDid: String

    init() {
        self.storedDid = "did:plc:mock\(UUID().uuidString.prefix(8).lowercased())"
    }

    func did() -> String { storedDid }
}

/// Mock service that creates fake accounts and seeds initial conversations.
struct MockActnetService: ActnetService {
    func createAccount(serverUrl: String, dbPath: String, dbKey: String, prfOutput: Data, displayName: String, inviteToken: String?) throws -> any AppCoreProtocol {
        Thread.sleep(forTimeInterval: 0.5) // simulate network
        return MockAppCore(displayName: displayName)
    }

    func login(dbPath: String, dbKey: String) throws -> any AppCoreProtocol {
        MockAppCore()
    }

    func prepareAccount(serverUrl: String, prfOutput: Data) throws -> any PreparedAccountProtocol {
        MockPreparedAccount()
    }

    func finalizeAccount(prepared: any PreparedAccountProtocol, dbPath: String, dbKey: String, displayName: String, inviteToken: String?) throws -> any AppCoreProtocol {
        Thread.sleep(forTimeInterval: 0.5)
        return MockAppCore(did: prepared.did(), displayName: displayName)
    }

    func recoverFromBlob(serverUrl: String, did: String, prfOutput: Data, dbPath: String, dbKey: String, displayName: String) throws -> any AppCoreProtocol {
        Thread.sleep(forTimeInterval: 0.5)
        return MockAppCore(did: did, displayName: displayName)
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
