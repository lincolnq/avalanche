import SwiftUI

enum ServiceMode: String, CaseIterable {
    case mock = "Mock (no server)"
    case devServer = "Dev Server"
}

/// Minimal account info persisted to UserDefaults so we can restore on launch.
/// `dbFilename` is just the filename (e.g. "account-34B35698.db"), resolved
/// against the current app container's dbDir at runtime. This avoids breakage
/// when the simulator reassigns container UUIDs between launches.
private struct PersistedAccount: Codable {
    let did: String
    let displayName: String
    let dbFilename: String
    let servers: [PersistedServer]
}

private struct PersistedServer: Codable {
    let id: String
    let name: String
    let url: String
}

/// Top-level app state. Tracks accounts (each backed by an AppCore instance)
/// and routes between onboarding and the main UI.
@MainActor
final class AppState: ObservableObject {
    @Published var accounts: [Account] = []
    @Published var isOnboarding: Bool = true
    @Published var conversations: [Conversation] = []
    @Published var messagesByConversation: [String: [Message]] = [:]
    @Published var serviceMode: ServiceMode
    @Published var selectedTab: Tab = .chats
    @Published var navigateToConversation: Conversation?

    enum Tab {
        case calls, chats, network
    }

    /// Active AppCore instances, keyed by DID.
    private var cores: [String: any AppCoreProtocol] = [:]
    /// Running WebSocket loop tasks, keyed by DID. Cancelled when account is removed.
    private var wsLoopTasks: [String: Task<Void, Never>] = [:]
    /// Cached display names for remote DIDs, keyed by DID.
    private var displayNameCache: [String: String] = [:]
    /// DIDs currently being fetched (to avoid duplicate requests).
    private var displayNameInFlight: Set<String> = []
    private var _service: any ActnetService

    var service: any ActnetService { _service }

    private static let serviceModeKey = "serviceMode"
    private static let accountsKey = "persistedAccounts"
    private static let conversationsKey = "persistedConversations"
    // TODO: Derive key from iOS Secure Enclave instead of hardcoded passphrase
    private static let dbKey = "dev-placeholder-key"

    init(mode: ServiceMode? = nil) {
        let resolved = mode ?? {
            if let saved = UserDefaults.standard.string(forKey: AppState.serviceModeKey),
               let m = ServiceMode(rawValue: saved) {
                return m
            }
            return .mock
        }()
        self.serviceMode = resolved
        self._service = Self.makeService(mode: resolved)
        self.isOnboarding = true
    }

    // MARK: - Deep linking

    /// Handle an `actnet://` deep link URL.
    /// Supported: `actnet://conversation/<recipient_did>`
    func handleDeepLink(_ url: URL) {
        print("[DeepLink] handleDeepLink: \(url), scheme=\(url.scheme ?? "nil"), host=\(url.host ?? "nil"), path=\(url.path)")
        guard url.scheme == "actnet" else { return }
        guard url.host == "conversation" else { return }

        // Path is "/<recipient_did>"
        let did = url.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        guard !did.isEmpty, let accountId = accounts.first?.id else {
            print("[DeepLink] failed: did='\(did)', accounts=\(accounts.count)")
            return
        }

        print("[DeepLink] navigating to conversation with \(did)")
        let conv = findOrCreateDMConversation(recipientDid: did, accountId: accountId)
        selectedTab = .chats
        navigateToConversation = conv
    }

    // MARK: - Unread count (derived from in-memory messages)

    /// Compute unread count for a conversation from in-memory messages.
    func unreadCount(for conversation: Conversation) -> Int {
        let messages = messagesByConversation[conversation.id] ?? []
        return messages.filter { $0.readAtMs == nil && $0.senderAccountId != conversation.accountId }.count
    }

    // MARK: - Account lifecycle

    /// Attempt to restore persisted accounts on launch.
    func restoreAccounts() async {
        let persisted = Self.loadPersistedAccounts()
        guard !persisted.isEmpty else { return }

        let svc = _service
        let dbKey = Self.dbKey
        let dir = dbDir

        for p in persisted {
            let dbPath = dir.appendingPathComponent(p.dbFilename).path

            guard FileManager.default.fileExists(atPath: dbPath) else {
                print("DB file missing for \(p.did), removing from persisted accounts")
                Self.removePersistedAccount(did: p.did)
                continue
            }

            let account = Account(
                id: p.did,
                displayName: p.displayName,
                avatarData: nil,
                servers: p.servers.map { s in
                    ServerInfo(id: s.id, name: s.name, url: URL(string: s.url)!)
                }
            )
            accounts.append(account)

            do {
                let core = try await Task.detached {
                    try svc.login(dbPath: dbPath, dbKey: dbKey)
                }.value
                self.cores[core.did()] = core
            } catch {
                print("Failed to authenticate account \(p.did) (will show offline): \(error)")
            }
        }

        if !accounts.isEmpty {
            isOnboarding = false

            if serviceMode == .mock {
                for account in accounts {
                    conversations.append(contentsOf: MockData.seedConversations(
                        accountId: account.id,
                        serverUrl: account.servers.first?.id ?? ""
                    ))
                }
            } else {
                conversations = Self.loadPersistedConversations()
                // Resolve display names for conversations that still show raw DIDs.
                for conv in conversations {
                    if let did = conv.recipientDid, conv.title == did {
                        _ = displayName(for: did, accountId: conv.accountId)
                    }
                }
            }

            startMessagePolling()
            Task { await PushManager.requestPermissionAndRegister(appState: self) }
        }
    }

    func switchMode(_ mode: ServiceMode) {
        serviceMode = mode
        UserDefaults.standard.set(mode.rawValue, forKey: Self.serviceModeKey)
        _service = Self.makeService(mode: mode)
        // Cancel all WS loops before clearing state
        for (_, task) in wsLoopTasks { task.cancel() }
        wsLoopTasks.removeAll()
        // Reset state on mode switch
        accounts.removeAll()
        conversations.removeAll()
        messagesByConversation.removeAll()
        cores.removeAll()
        displayNameCache.removeAll()
        displayNameInFlight.removeAll()
        Self.clearPersistedAccounts()
        Self.clearPersistedConversations()
        isOnboarding = true
    }

    private static func makeService(mode: ServiceMode) -> any ActnetService {
        switch mode {
        case .mock:
            return MockActnetService()
        case .devServer:
            return DevServerActnetService()
        }
    }

    private var dbDir: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("actnet", isDirectory: true)
    }

    func createAccount(serverUrl: String, serverName: String, displayName: String) async throws {
        let dir = dbDir
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

        let dbFilename = "account-\(UUID().uuidString.prefix(8)).db"
        let dbPath = dir.appendingPathComponent(dbFilename).path
        let dbKey = Self.dbKey

        let svc = _service
        let core = try await Task.detached {
            try svc.createAccount(serverUrl: serverUrl, dbPath: dbPath, dbKey: dbKey)
        }.value

        let did = core.did()
        cores[did] = core

        let account = Account(
            id: did,
            displayName: displayName,
            avatarData: nil,
            servers: [ServerInfo(id: serverUrl, name: serverName, url: URL(string: serverUrl)!)]
        )
        accounts.append(account)

        Self.persistAccount(PersistedAccount(
            did: did,
            displayName: displayName,
            dbFilename: dbFilename,
            servers: [PersistedServer(id: serverUrl, name: serverName, url: serverUrl)]
        ))

        // In mock mode, seed some fake conversations
        if serviceMode == .mock {
            conversations.append(contentsOf: MockData.seedConversations(
                accountId: did,
                serverUrl: serverUrl
            ))
        }

        isOnboarding = false
        startMessagePolling()
        Task { await PushManager.requestPermissionAndRegister(appState: self) }
    }

    /// Returns all active core instances. Used by PushManager and other utilities
    /// that need to iterate across accounts without direct access to `cores`.
    func activeCores() -> [any AppCoreProtocol] {
        Array(cores.values)
    }

    func joinServer(serverUrl: String, serverName: String, existingAccountId: String) async throws {
        if let idx = accounts.firstIndex(where: { $0.id == existingAccountId }) {
            accounts[idx].servers.append(
                ServerInfo(id: serverUrl, name: serverName, url: URL(string: serverUrl)!)
            )
        }
        isOnboarding = false
    }

    // MARK: - Display name resolution

    /// Returns the cached display name for a DID, or the DID itself if unknown.
    /// Kicks off a background fetch if not cached yet.
    func displayName(for did: String, accountId: String) -> String {
        if let name = displayNameCache[did] { return name }
        resolveDisplayName(did: did, accountId: accountId)
        return did
    }

    /// Fetch display name from the server and update conversation titles.
    private func resolveDisplayName(did: String, accountId: String) {
        guard !displayNameInFlight.contains(did) else { return }
        guard let core = cores[accountId] else { return }
        displayNameInFlight.insert(did)
        let targetDid = did
        Task.detached { [weak self] in
            let info = try? core.getAccountInfo(did: targetDid)
            await MainActor.run {
                guard let self else { return }
                self.displayNameInFlight.remove(targetDid)
                guard let name = info?.displayName, !name.isEmpty else { return }
                self.displayNameCache[targetDid] = name
                // Update titles of any conversations with this DID.
                for i in self.conversations.indices {
                    if self.conversations[i].recipientDid == targetDid && self.conversations[i].title == targetDid {
                        self.conversations[i].title = name
                    }
                }
                self.persistConversations()
            }
        }
    }

    // MARK: - Messaging

    func sendMessage(conversationId: String, text: String, recipientDid: String, senderAccountId: String, messageId: String, sentAtMs: Int64) async throws {
        guard let core = cores[senderAccountId] else { return }
        let plaintext = Data(text.utf8)

        // Persist outbound message to SQLCipher (outgoing messages are immediately "read").
        let nowMs = sentAtMs
        let stored = StoredMessageFfi(
            id: messageId,
            conversationId: conversationId,
            senderDid: senderAccountId,
            body: text,
            sentAtMs: nowMs,
            editedAtMs: nil,
            readAtMs: nowMs,  // outgoing = immediately read
            deliveryStatus: 1  // sent
        )
        try await Task.detached { try core.saveMessage(msg: stored) }.value

        try await Task.detached {
            try core.sendDm(recipientDid: recipientDid, recipientDeviceId: 1, plaintext: plaintext, sentAtMs: nowMs)
        }.value
    }

    /// Mark all messages in a conversation as read (sets read_at on unread messages).
    /// Sends read receipts to the sender.
    func markAllMessagesRead(conversationId: String, accountId: String) {
        guard var messages = messagesByConversation[conversationId] else { return }
        let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
        var changed = false
        // Collect timestamps of newly-read messages, grouped by sender DID.
        var readTimestampsBySender: [String: [Int64]] = [:]
        for i in messages.indices {
            if messages[i].readAtMs == nil && messages[i].senderAccountId != accountId {
                messages[i].readAtMs = nowMs
                changed = true
                readTimestampsBySender[messages[i].senderAccountId, default: []].append(messages[i].sentAtMs)
            }
        }
        guard changed else { return }
        messagesByConversation[conversationId] = messages

        // Persist to SQLCipher and send read receipts in the background.
        if let core = cores[accountId] {
            let convId = conversationId
            let timestampsBySender = readTimestampsBySender
            Task.detached {
                try? core.markMessagesRead(conversationId: convId, upToSentAtMs: nowMs)
                // Send read receipts to each sender.
                for (senderDid, timestamps) in timestampsBySender {
                    try? core.sendReadReceipt(
                        recipientDid: senderDid,
                        recipientDeviceId: 1,
                        timestamps: timestamps
                    )
                }
            }
        }
    }

    /// Load persisted messages from SQLCipher for a conversation.
    func loadMessagesFromStore(conversationId: String, accountId: String) {
        guard let core = cores[accountId] else { return }
        // Only load if we haven't already loaded for this conversation.
        guard messagesByConversation[conversationId] == nil else { return }
        let convId = conversationId
        Task.detached { [weak self] in
            guard let msgs = try? core.loadMessages(conversationId: convId) else { return }
            let messages = msgs.map { m in
                Message(
                    id: m.id,
                    conversationId: m.conversationId,
                    senderAccountId: m.senderDid,
                    body: m.body,
                    sentAtMs: m.sentAtMs,
                    editedAtMs: m.editedAtMs,
                    readAtMs: m.readAtMs,
                    deliveryStatus: DeliveryStatus(rawValue: Int(m.deliveryStatus)) ?? .sent
                )
            }
            await MainActor.run {
                if self?.messagesByConversation[convId] == nil {
                    self?.messagesByConversation[convId] = messages
                }
            }
        }
    }

    /// Find or create a DM conversation with a recipient DID.
    func findOrCreateDMConversation(recipientDid: String, accountId: String) -> Conversation {
        if let existing = conversations.first(where: {
            $0.accountId == accountId && $0.recipientDid == recipientDid
        }) {
            return existing
        }
        let serverUrl = accounts.first(where: { $0.id == accountId })?.servers.first?.id ?? ""
        let convId = "dm-\(accountId)-\(recipientDid)"
        let title = displayName(for: recipientDid, accountId: accountId)
        let conv = Conversation(
            id: convId,
            title: title,
            accountId: accountId,
            serverUrl: serverUrl,
            recipientDid: recipientDid,
            isGroup: false
        )
        conversations.append(conv)
        persistConversations()
        return conv
    }

    func pollMessages(for accountId: String) async throws -> [DecryptedMessage] {
        guard let core = cores[accountId] else { return [] }
        return try await Task.detached {
            try core.receiveMessages()
        }.value
    }

    /// Fetch the list of Projects from a server.
    func fetchProjects(serverUrl: String) async -> [ProjectInfo] {
        guard let account = accounts.first(where: {
            $0.servers.contains(where: { $0.id == serverUrl })
        }), let core = cores[account.id] else {
            return []
        }
        do {
            let projects = try await Task.detached {
                try core.fetchProjects()
            }.value
            return projects.map { ProjectInfo(name: $0.name, url: $0.url, description: $0.description) }
        } catch {
            print("Failed to fetch projects: \(error)")
            return []
        }
    }

    /// Request a Project token from the homeserver.
    func requestProjectToken(accountId: String, projectUrl: String) async throws -> String {
        guard let core = cores[accountId] else {
            throw NSError(domain: "AppState", code: 1, userInfo: [NSLocalizedDescriptionKey: "No account"])
        }
        return try await Task.detached {
            try core.requestProjectToken(projectUrl: projectUrl)
        }.value
    }

    // MARK: - Message receiving (WebSocket)

    /// Start a background WebSocket listener for each account that has a live core.
    func startMessagePolling() {
        for account in accounts {
            let accountId = account.id
            // Skip if there's no core (account is offline) or loop already running.
            guard cores[accountId] != nil, wsLoopTasks[accountId] == nil else { continue }
            wsLoopTasks[accountId] = Task {
                await messageWsLoop(accountId: accountId)
            }
        }
    }

    /// Connects via WebSocket and waits for messages. Reconnects on error.
    /// Exits cleanly when the task is cancelled or the core is removed.
    private func messageWsLoop(accountId: String) async {
        print("[ws] starting message loop for \(accountId)")
        while !Task.isCancelled {
            guard let core = cores[accountId] else {
                // Core was removed (account deleted/recreated). Stop the loop.
                print("[ws] no core for \(accountId), stopping loop")
                break
            }
            do {
                let messages = try await Task.detached {
                    try core.receiveMessagesWs()
                }.value
                guard !Task.isCancelled else { break }
                print("[ws] received \(messages.count) message(s) for \(accountId)")
                for msg in messages {
                    handleIncomingMessage(msg, accountId: accountId)
                }
                // Apply any delivery status updates from incoming read receipts.
                let updates = core.drainReceiptUpdates()
                if !updates.isEmpty {
                    applyDeliveryStatusUpdates(updates)
                }
                if messages.isEmpty && updates.isEmpty {
                    // Connection closed — brief delay before reconnect.
                    try? await Task.sleep(nanoseconds: 1_000_000_000)
                }
            } catch {
                guard !Task.isCancelled else { break }
                print("[ws] error for \(accountId): \(error), reconnecting in 2s")
                try? await Task.sleep(nanoseconds: 2_000_000_000)
            }
        }
        // Clean up task reference on exit.
        wsLoopTasks.removeValue(forKey: accountId)
        print("[ws] loop ended for \(accountId)")
    }

    private func handleIncomingMessage(_ msg: DecryptedMessage, accountId: String) {
        let senderDid = msg.senderDid
        let text = String(data: msg.plaintext, encoding: .utf8) ?? "(binary)"

        var convId: String

        // Find existing conversation with this sender.
        if let idx = conversations.firstIndex(where: {
            $0.accountId == accountId && $0.recipientDid == senderDid
        }) {
            convId = conversations[idx].id
            conversations[idx].lastMessage = text
            conversations[idx].lastMessageDate = Date()
        } else {
            // Auto-create a new conversation for this DID.
            let serverUrl = accounts.first(where: { $0.id == accountId })?.servers.first?.id ?? ""
            convId = "dm-\(accountId)-\(senderDid)"
            let title = displayName(for: senderDid, accountId: accountId)
            let conv = Conversation(
                id: convId,
                title: title,
                accountId: accountId,
                serverUrl: serverUrl,
                recipientDid: senderDid,
                lastMessage: text,
                lastMessageDate: Date(),
                isGroup: false
            )
            conversations.append(conv)
        }

        // Use sender's timestamp if available, otherwise fall back to local time.
        let sentAtMs: Int64 = msg.sentAtMs ?? Int64(Date().timeIntervalSince1970 * 1000)
        let messageId = UUID().uuidString
        // Incoming messages are unread (readAtMs = nil).
        let message = Message(
            id: messageId,
            conversationId: convId,
            senderAccountId: senderDid,
            body: text,
            sentAtMs: sentAtMs,
            readAtMs: nil,
            deliveryStatus: .sent
        )
        messagesByConversation[convId, default: []].append(message)
        persistConversations()

        // Persist to SQLCipher in the background.
        if let core = cores[accountId] {
            let stored = StoredMessageFfi(
                id: messageId,
                conversationId: convId,
                senderDid: senderDid,
                body: text,
                sentAtMs: sentAtMs,
                editedAtMs: nil,
                readAtMs: nil,  // unread
                deliveryStatus: 1  // sent
            )
            Task.detached { try? core.saveMessage(msg: stored) }
        }
    }

    // MARK: - Delivery status updates

    /// Apply delivery status updates (from read receipts) to in-memory messages.
    private func applyDeliveryStatusUpdates(_ updates: [DeliveryStatusUpdate]) {
        for update in updates {
            guard var messages = messagesByConversation[update.conversationId] else { continue }
            var changed = false
            for i in messages.indices {
                if messages[i].sentAtMs == update.sentAtMs,
                   let newStatus = DeliveryStatus(rawValue: Int(update.deliveryStatus)),
                   newStatus.rawValue > messages[i].deliveryStatus.rawValue {
                    messages[i].deliveryStatus = newStatus
                    changed = true
                }
            }
            if changed {
                messagesByConversation[update.conversationId] = messages
            }
        }
    }

    // MARK: - Persistence helpers

    private static func loadPersistedAccounts() -> [PersistedAccount] {
        guard let data = UserDefaults.standard.data(forKey: accountsKey) else { return [] }
        return (try? JSONDecoder().decode([PersistedAccount].self, from: data)) ?? []
    }

    private static func persistAccount(_ account: PersistedAccount) {
        var existing = loadPersistedAccounts()
        existing.removeAll { $0.did == account.did }
        existing.append(account)
        if let data = try? JSONEncoder().encode(existing) {
            UserDefaults.standard.set(data, forKey: accountsKey)
        }
    }

    private static func removePersistedAccount(did: String) {
        var existing = loadPersistedAccounts()
        existing.removeAll { $0.did == did }
        if let data = try? JSONEncoder().encode(existing) {
            UserDefaults.standard.set(data, forKey: accountsKey)
        }
    }

    private static func clearPersistedAccounts() {
        UserDefaults.standard.removeObject(forKey: accountsKey)
    }

    // MARK: - Conversation persistence

    private func persistConversations() {
        if let data = try? JSONEncoder().encode(conversations) {
            UserDefaults.standard.set(data, forKey: Self.conversationsKey)
        }
    }

    private static func loadPersistedConversations() -> [Conversation] {
        guard let data = UserDefaults.standard.data(forKey: conversationsKey) else { return [] }
        return (try? JSONDecoder().decode([Conversation].self, from: data)) ?? []
    }

    private static func clearPersistedConversations() {
        UserDefaults.standard.removeObject(forKey: conversationsKey)
    }
}
