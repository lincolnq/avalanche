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
    /// ID of the conversation currently visible on screen, or nil. Set by
    /// `ConversationView.onAppear`/`onDisappear`. Used to suppress
    /// notifications for the chat the user is actively reading.
    @Published var currentConversationId: String?
    /// Whether the app's scene is in the `.active` phase. Driven by
    /// `ActnetApp`'s `onChange(of: scenePhase)`. Used to decide whether to
    /// fire a banner (background/inactive → always show; active → suppress
    /// only when viewing the relevant conversation).
    @Published var isAppActive: Bool = true

    /// Per-account connection state, keyed by DID. Sourced from the Rust
    /// reconnect task via `waitForConnectionStateChange`.
    @Published var connectionStates: [String: ConnectionState] = [:]

    enum Tab {
        case calls, chats, network
    }

    /// Active AppCore instances, keyed by DID.
    private var cores: [String: any AppCoreProtocol] = [:]
    /// Per-account connection-state listener tasks. Cancelled on logout/mode switch.
    private var stateTasks: [String: Task<Void, Never>] = [:]
    /// Per-account event-channel listener tasks. Cancelled on logout/mode switch.
    private var eventTasks: [String: Task<Void, Never>] = [:]
    /// Cached display names for remote DIDs, keyed by DID.
    private var displayNameCache: [String: String] = [:]
    /// DIDs currently being fetched (to avoid duplicate requests).
    private var displayNameInFlight: Set<String> = []
    private var _service: any ActnetService

    var service: any ActnetService { _service }

    private static let serviceModeKey = "serviceMode"
    private static let accountsKey = "persistedAccounts"


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

    /// Handle a deep link URL.
    /// Supported:
    /// - `https://go.theavalanche.net/conversation/<recipient_did>`
    /// - `https://go.theavalanche.net/invite/<token>`
    func handleDeepLink(_ url: URL) {
        print("[DeepLink] handleDeepLink: \(url), scheme=\(url.scheme ?? "nil"), host=\(url.host ?? "nil"), path=\(url.path)")
        guard Self.isDeepLink(url) else { return }

        let pathComponents = url.pathComponents.filter { $0 != "/" }
        guard let action = pathComponents.first, pathComponents.count >= 2 else { return }

        switch action {
        case "conversation":
            let did = pathComponents[1]
            guard !did.isEmpty, let accountId = accounts.first?.id else {
                print("[DeepLink] failed: did='\(did)', accounts=\(accounts.count)")
                return
            }
            print("[DeepLink] navigating to conversation with \(did)")
            let conv = findOrCreateDMConversation(recipientDid: did, accountId: accountId)
            selectedTab = .chats
            navigateToConversation = conv

        case "invite":
            let token = pathComponents[1]
            print("[DeepLink] handling invite token")
            // Try to decode the token locally to check if we're already on the server.
            if let data = Data(base64URLEncoded: token),
               let payload = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let serverUrl = payload["server_url"] as? String,
               let inviterDid = payload["inviter_did"] as? String,
               let account = accounts.first(where: { $0.servers.contains(where: { $0.url.absoluteString.trimmingCharacters(in: CharacterSet(charactersIn: "/")) == serverUrl.trimmingCharacters(in: CharacterSet(charactersIn: "/")) }) }) {
                // Already registered on this server — skip to DM.
                print("[DeepLink] already on server, opening DM with \(inviterDid)")
                let conv = findOrCreateDMConversation(recipientDid: inviterDid, accountId: account.id)
                selectedTab = .chats
                navigateToConversation = conv
            } else {
                // Not on this server — start onboarding flow.
                pendingInviteToken = token
            }

        default:
            print("[DeepLink] unknown action: \(action)")
        }
    }

    /// Pending invite token from a deep link, picked up by the onboarding flow.
    @Published var pendingInviteToken: String?

    /// Check if a URL is a deep link for this app.
    static func isDeepLink(_ url: URL) -> Bool {
        url.host == "go.theavalanche.net"
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
        let dir = dbDir

        guard let dbKey = try? SecureEnclaveKeyManager.dbPassphrase() else {
            print("Failed to retrieve DB encryption key, cannot restore accounts")
            return
        }

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

                // Refresh display name from the local profile store. The
                // persisted name in UserDefaults can be stale (e.g. recovered
                // accounts started with an empty placeholder).
                if let coreName = try? await Task.detached(operation: { try core.ownDisplayName() }).value,
                   !coreName.isEmpty,
                   coreName != p.displayName,
                   let idx = accounts.firstIndex(where: { $0.id == p.did }) {
                    accounts[idx].displayName = coreName
                    Self.persistAccount(PersistedAccount(
                        did: p.did,
                        displayName: coreName,
                        dbFilename: p.dbFilename,
                        servers: p.servers
                    ))
                }
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
                // Conversation list is derived from message_history in
                // SQLCipher — no parallel UserDefaults state. One indexed
                // query per account returns every conversation with at least
                // one persisted message, already sorted newest-first.
                await loadConversationsFromStore()
            }

            startMessagePolling()
            Task { await PushManager.requestPermissionAndRegister(appState: self) }
        }
    }

    func logout() {
        cancelAllListenerTasks()
        accounts.removeAll()
        conversations.removeAll()
        messagesByConversation.removeAll()
        cores.removeAll()
        connectionStates.removeAll()
        displayNameCache.removeAll()
        displayNameInFlight.removeAll()
        Self.clearPersistedAccounts()
        isOnboarding = true
    }

    func switchMode(_ mode: ServiceMode) {
        serviceMode = mode
        UserDefaults.standard.set(mode.rawValue, forKey: Self.serviceModeKey)
        _service = Self.makeService(mode: mode)
        // Cancel all listener tasks before clearing state
        cancelAllListenerTasks()
        // Reset state on mode switch
        accounts.removeAll()
        conversations.removeAll()
        messagesByConversation.removeAll()
        cores.removeAll()
        connectionStates.removeAll()
        displayNameCache.removeAll()
        displayNameInFlight.removeAll()
        Self.clearPersistedAccounts()
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

    /// Create a new account. `recoveryKey` is a 32-byte symmetric key from
    /// passkey PRF or recovery phrase. Pass empty Data to skip recovery setup.
    func createAccount(serverUrl: String, serverName: String, displayName: String, recoveryKey: Data = Data()) async throws {
        let dir = dbDir
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

        let dbFilename = "account-\(UUID().uuidString.prefix(8)).db"
        let dbPath = dir.appendingPathComponent(dbFilename).path
        let dbKey = try SecureEnclaveKeyManager.dbPassphrase()

        let svc = _service
        let rk = recoveryKey
        let dn = displayName
        let core = try await Task.detached {
            try svc.createAccount(serverUrl: serverUrl, dbPath: dbPath, dbKey: dbKey, recoveryKey: rk, displayName: dn)
        }.value

        try await finishAccountRegistration(core: core, serverUrl: serverUrl, serverName: serverName, displayName: displayName, dbFilename: dbFilename)
    }

    /// Prepare a fresh identity (Stage 1 of the passkey flow). The returned
    /// `PreparedAccountProtocol` exposes the DID derived from the new keys,
    /// which the caller writes into the passkey's user handle before
    /// completing registration via `finalizePreparedAccount`.
    func prepareAccount(serverUrl: String) async throws -> any PreparedAccountProtocol {
        let svc = _service
        return try await Task.detached {
            try svc.prepareAccount(serverUrl: serverUrl)
        }.value
    }

    /// Finalize an account previously created by `prepareAccount` (Stage 2 of
    /// the passkey flow). Submits the PLC genesis op and registers with the
    /// server using the prepared keys.
    func finalizePreparedAccount(
        prepared: any PreparedAccountProtocol,
        serverUrl: String,
        serverName: String,
        displayName: String,
        recoveryKey: Data
    ) async throws {
        let dir = dbDir
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

        let dbFilename = "account-\(UUID().uuidString.prefix(8)).db"
        let dbPath = dir.appendingPathComponent(dbFilename).path
        let dbKey = try SecureEnclaveKeyManager.dbPassphrase()

        let svc = _service
        let rk = recoveryKey
        let dn = displayName
        let core = try await Task.detached {
            try svc.finalizeAccount(prepared: prepared, dbPath: dbPath, dbKey: dbKey, recoveryKey: rk, displayName: dn)
        }.value

        try await finishAccountRegistration(core: core, serverUrl: serverUrl, serverName: serverName, displayName: displayName, dbFilename: dbFilename)
    }

    /// Recover an account from a passkey-protected recovery blob. Downloads
    /// the blob from `serverUrl` keyed by `did`, decrypts with `recoveryKey`,
    /// replaces the device on the home server, and signs the user in.
    ///
    /// `displayName` may be empty — the recovered account will appear in the
    /// account list with a placeholder until the user updates it from Settings.
    func recoverAccount(
        serverUrl: String,
        serverName: String,
        did: String,
        recoveryKey: Data,
        displayName: String
    ) async throws {
        let dir = dbDir
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

        let dbFilename = "account-\(UUID().uuidString.prefix(8)).db"
        let dbPath = dir.appendingPathComponent(dbFilename).path
        let dbKey = try SecureEnclaveKeyManager.dbPassphrase()

        let svc = _service
        let rk = recoveryKey
        let recoveryDid = did
        let core = try await Task.detached {
            try svc.recoverFromBlob(
                serverUrl: serverUrl,
                did: recoveryDid,
                recoveryKey: rk,
                dbPath: dbPath,
                dbKey: dbKey,
                displayName: displayName
            )
        }.value

        // Prefer the display name that the recovery blob restored into the
        // local profile store. Falls back to the user-supplied name, then to
        // a placeholder.
        let restoredName = (try? await Task.detached(operation: { try core.ownDisplayName() }).value) ?? ""
        let resolvedDisplayName: String
        if !restoredName.isEmpty {
            resolvedDisplayName = restoredName
        } else if !displayName.isEmpty {
            resolvedDisplayName = displayName
        } else {
            resolvedDisplayName = "Account \(String(did.suffix(6)))"
        }
        try await finishAccountRegistration(
            core: core,
            serverUrl: serverUrl,
            serverName: serverName,
            displayName: resolvedDisplayName,
            dbFilename: dbFilename
        )
    }

    private func finishAccountRegistration(
        core: any AppCoreProtocol,
        serverUrl: String,
        serverName: String,
        displayName: String,
        dbFilename: String
    ) async throws {
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

    /// Resolve a display name for a DID. Two sources, in order:
    /// 1. Local SQLCipher `contact_profiles` cache — populated automatically by
    ///    app-core when an inbound message carries the sender's profile_key.
    ///    This is the path for human contacts.
    /// 2. Server-side `/v1/accounts/{did}` lookup — only returns a name for
    ///    bot accounts (humans never put a plaintext name on the server).
    private func resolveDisplayName(did: String, accountId: String) {
        guard !displayNameInFlight.contains(did) else { return }
        guard let core = cores[accountId] else { return }
        displayNameInFlight.insert(did)
        let targetDid = did
        Task.detached { [weak self] in
            // Local contact_profiles first — fast, no network.
            let localName = (try? core.contactDisplayName(did: targetDid)) ?? ""
            // Fall back to server lookup (bots) only if the local cache is empty.
            let serverName: String? = localName.isEmpty
                ? (try? core.getAccountInfo(did: targetDid))?.displayName
                : nil
            let resolved = !localName.isEmpty ? localName : (serverName ?? "")

            await MainActor.run {
                guard let self else { return }
                self.displayNameInFlight.remove(targetDid)
                guard !resolved.isEmpty else { return }
                self.applyResolvedDisplayName(did: targetDid, name: resolved)
            }
        }
    }

    /// Cache a resolved name and update any conversation title that doesn't
    /// already match. Handles both first-time resolution (title was the raw
    /// DID) and refresh-after-rename (title was the old name).
    private func applyResolvedDisplayName(did: String, name: String) {
        displayNameCache[did] = name
        var changed = false
        for i in conversations.indices {
            if conversations[i].recipientDid == did && conversations[i].title != name {
                conversations[i].title = name
                changed = true
            }
        }
        _ = changed
    }

    /// Re-fetch a contact's encrypted profile from the homeserver and refresh
    /// the cached display name if it changed. Called when a conversation opens
    /// — the primary change-detection path, per `docs/35-profiles.md`.
    func refreshContactProfile(did: String, accountId: String) {
        guard let core = cores[accountId] else { return }
        Task.detached { [weak self] in
            let changed = (try? core.refreshContactProfile(did: did)) ?? false
            guard changed else { return }
            let newName = (try? core.contactDisplayName(did: did)) ?? ""
            guard !newName.isEmpty else { return }
            await MainActor.run {
                self?.applyResolvedDisplayName(did: did, name: newName)
            }
        }
    }

    // MARK: - Messaging

    func sendMessage(conversationId: String, text: String, recipientDid: String, senderAccountId: String, messageId: String, sentAtMs: Int64) async throws {
        guard let core = cores[senderAccountId] else { return }
        let plaintext = Data(text.utf8)
        let nowMs = sentAtMs

        // Persist as "sending" up front so failures are recoverable across launches.
        let pending = StoredMessageFfi(
            id: messageId,
            conversationId: conversationId,
            senderDid: senderAccountId,
            body: text,
            sentAtMs: nowMs,
            editedAtMs: nil,
            readAtMs: nowMs,
            deliveryStatus: UInt8(DeliveryStatus.sending.rawValue)
        )
        try await Task.detached { try core.saveMessage(msg: pending) }.value

        do {
            try await Task.detached {
                try core.sendDm(recipientDid: recipientDid, plaintext: plaintext, sentAtMs: nowMs)
            }.value
            updateMessageStatus(messageId: messageId, conversationId: conversationId, newStatus: .sent)
            let sent = StoredMessageFfi(
                id: messageId, conversationId: conversationId, senderDid: senderAccountId,
                body: text, sentAtMs: nowMs, editedAtMs: nil, readAtMs: nowMs,
                deliveryStatus: UInt8(DeliveryStatus.sent.rawValue)
            )
            Task.detached { try? core.saveMessage(msg: sent) }
        } catch {
            AppLog.error("send", "DM to \(recipientDid) failed: \(error.localizedDescription)")
            updateMessageStatus(messageId: messageId, conversationId: conversationId, newStatus: .failed)
            let failed = StoredMessageFfi(
                id: messageId, conversationId: conversationId, senderDid: senderAccountId,
                body: text, sentAtMs: nowMs, editedAtMs: nil, readAtMs: nowMs,
                deliveryStatus: UInt8(DeliveryStatus.failed.rawValue)
            )
            Task.detached { try? core.saveMessage(msg: failed) }
            throw error
        }
    }

    /// Update an in-memory message's delivery status by id.
    private func updateMessageStatus(messageId: String, conversationId: String, newStatus: DeliveryStatus) {
        guard var msgs = messagesByConversation[conversationId] else { return }
        guard let idx = msgs.firstIndex(where: { $0.id == messageId }) else { return }
        msgs[idx].deliveryStatus = newStatus
        messagesByConversation[conversationId] = msgs
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
        NotificationPresenter.updateBadge(appState: self)

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
                        timestamps: timestamps
                    )
                }
            }
        }
    }

    /// Mark messages as read up to (and including) the given sentAtMs timestamp.
    /// Only marks received messages (not own outgoing). Sends read receipts for newly-read messages.
    func markMessagesReadUpTo(sentAtMs threshold: Int64, conversationId: String, accountId: String) {
        guard var messages = messagesByConversation[conversationId] else { return }
        let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
        var readTimestampsBySender: [String: [Int64]] = [:]
        var changed = false
        for i in messages.indices {
            let msg = messages[i]
            guard msg.readAtMs == nil && msg.senderAccountId != accountId && msg.sentAtMs <= threshold else { continue }
            messages[i].readAtMs = nowMs
            changed = true
            readTimestampsBySender[msg.senderAccountId, default: []].append(msg.sentAtMs)
        }
        guard changed else { return }
        messagesByConversation[conversationId] = messages
        NotificationPresenter.updateBadge(appState: self)

        if let core = cores[accountId] {
            let convId = conversationId
            let timestampsBySender = readTimestampsBySender
            Task.detached {
                try? core.markMessagesRead(conversationId: convId, upToSentAtMs: threshold)
                for (senderDid, timestamps) in timestampsBySender {
                    try? core.sendReadReceipt(recipientDid: senderDid, timestamps: timestamps)
                }
            }
        }
    }

    /// Load persisted messages from SQLCipher for a conversation.
    /// Derive the conversation list from each account's `message_history`
    /// via a single indexed query. Sorted newest-first; titles are resolved
    /// asynchronously through `displayName(for:accountId:)`.
    ///
    /// Conversation IDs follow the format `dm-<accountId>-<recipientDid>`.
    private func loadConversationsFromStore() async {
        let pairs: [(String, any AppCoreProtocol)] = accounts.compactMap { acct in
            cores[acct.id].map { (acct.id, $0) }
        }
        let summariesPerAccount = await withTaskGroup(of: (String, [ConversationSummaryFfi]).self) { group in
            for (accountId, core) in pairs {
                group.addTask {
                    let rows = (try? core.loadConversations()) ?? []
                    return (accountId, rows)
                }
            }
            var out: [(String, [ConversationSummaryFfi])] = []
            for await result in group { out.append(result) }
            return out
        }

        var newConvs: [Conversation] = []
        for (accountId, summaries) in summariesPerAccount {
            let serverUrl = accounts.first(where: { $0.id == accountId })?.servers.first?.id ?? ""
            for s in summaries {
                let recipientDid = Self.recipientDid(from: s.conversationId, accountId: accountId)
                let title = recipientDid.flatMap { displayNameCache[$0] } ?? recipientDid ?? s.conversationId
                newConvs.append(Conversation(
                    id: s.conversationId,
                    title: title,
                    accountId: accountId,
                    serverUrl: serverUrl,
                    recipientDid: recipientDid,
                    lastMessage: s.lastMessage.body,
                    lastMessageDate: Date(timeIntervalSince1970: TimeInterval(s.lastMessage.sentAtMs) / 1000.0),
                    isGroup: false
                ))
            }
        }
        conversations = newConvs.sorted {
            ($0.lastMessageDate ?? .distantPast) > ($1.lastMessageDate ?? .distantPast)
        }

        // Kick off async name resolution for any conversation still showing the raw DID.
        for conv in conversations {
            if let did = conv.recipientDid, conv.title == did {
                _ = displayName(for: did, accountId: conv.accountId)
            }
        }
    }

    /// Parse the recipient DID out of a conversation ID of the form
    /// `dm-<accountDid>-<recipientDid>`. Returns nil for non-DM IDs.
    private static func recipientDid(from conversationId: String, accountId: String) -> String? {
        let prefix = "dm-\(accountId)-"
        guard conversationId.hasPrefix(prefix) else { return nil }
        return String(conversationId.dropFirst(prefix.count))
    }

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

    // MARK: - Connection state + incoming events

    /// Aggregate connection state across all accounts. The banner shows
    /// whenever any account is not `.connected`. Earliest backoff timestamp
    /// wins the countdown.
    var aggregateConnectionState: ConnectionState {
        let states = connectionStates.values
        if states.isEmpty { return .connected }
        if states.allSatisfy({ if case .connected = $0 { return true }; return false }) {
            return .connected
        }
        // Pick the "worst" — prefer Reconnecting (earliest next attempt), then
        // Connecting, then Disconnected, then Connected.
        var bestReconnect: Int64?
        var sawConnecting = false
        var sawDisconnected = false
        for state in states {
            switch state {
            case .reconnecting(let nextAttemptAtMs):
                if let cur = bestReconnect {
                    bestReconnect = min(cur, nextAttemptAtMs)
                } else {
                    bestReconnect = nextAttemptAtMs
                }
            case .connecting: sawConnecting = true
            case .disconnected: sawDisconnected = true
            case .connected: break
            }
        }
        if let next = bestReconnect {
            return .reconnecting(nextAttemptAtMs: next)
        }
        if sawConnecting { return .connecting }
        if sawDisconnected { return .disconnected }
        return .connected
    }

    /// Start the per-account state + event listener tasks for any account
    /// that has a live core. Idempotent — restarts only missing tasks.
    func startMessagePolling() {
        for account in accounts {
            let accountId = account.id
            guard cores[accountId] != nil else { continue }
            if stateTasks[accountId] == nil {
                stateTasks[accountId] = Task { [accountId] in
                    await connectionStateLoop(accountId: accountId)
                }
            }
            if eventTasks[accountId] == nil {
                eventTasks[accountId] = Task { [accountId] in
                    await eventLoop(accountId: accountId)
                }
            }
        }
    }

    /// Cancel all per-account listener tasks. Called on logout/mode switch.
    private func cancelAllListenerTasks() {
        for (_, task) in stateTasks { task.cancel() }
        stateTasks.removeAll()
        for (_, task) in eventTasks { task.cancel() }
        eventTasks.removeAll()
    }

    /// Block on `waitForConnectionStateChange` in a loop and mirror updates
    /// into `connectionStates[accountId]`.
    private func connectionStateLoop(accountId: String) async {
        AppLog.info("conn", "starting connection-state listener for \(accountId)")
        // Seed from the current snapshot so we don't miss the initial transition.
        guard let core = cores[accountId] else { return }
        var last: ConnectionState = await Task.detached { core.connectionState() }.value
        connectionStates[accountId] = last
        while !Task.isCancelled {
            guard let core = cores[accountId] else { break }
            let lastSnapshot = last
            let next: ConnectionState
            do {
                next = try await Task.detached { try core.waitForConnectionStateChange(last: lastSnapshot) }.value
            } catch {
                AppLog.warn("conn", "state listener for \(accountId) ended: \(error.localizedDescription)")
                break
            }
            guard !Task.isCancelled else { break }
            last = next
            connectionStates[accountId] = next
        }
        stateTasks.removeValue(forKey: accountId)
        AppLog.info("conn", "connection-state listener ended for \(accountId)")
    }

    /// Block on `nextEvents` in a loop and dispatch each event.
    private func eventLoop(accountId: String) async {
        AppLog.info("evt", "starting event listener for \(accountId)")
        while !Task.isCancelled {
            guard let core = cores[accountId] else { break }
            let events: [IncomingEvent]
            do {
                events = try await Task.detached { try core.nextEvents() }.value
            } catch {
                AppLog.warn("evt", "event listener for \(accountId) ended: \(error.localizedDescription)")
                break
            }
            guard !Task.isCancelled else { break }
            var messages: [DecryptedMessage] = []
            var receiptUpdates: [DeliveryStatusUpdate] = []
            for ev in events {
                switch ev {
                case .message(let msg): messages.append(msg)
                case .receiptUpdate(let upd): receiptUpdates.append(upd)
                }
            }
            for msg in messages {
                handleIncomingMessage(msg, accountId: accountId)
            }
            if !receiptUpdates.isEmpty {
                applyDeliveryStatusUpdates(receiptUpdates)
            }
        }
        eventTasks.removeValue(forKey: accountId)
        AppLog.info("evt", "event listener ended for \(accountId)")
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

        // Fire a local notification (respects scene phase + currently-viewed
        // conversation; updates the app badge regardless).
        if let conv = conversations.first(where: { $0.id == convId }) {
            NotificationPresenter.present(
                message: message,
                conversation: conv,
                senderDisplayName: displayName(for: senderDid, accountId: accountId),
                appState: self
            )
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

}
