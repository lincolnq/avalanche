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

    /// Active AppCore instances, keyed by DID.
    private var cores: [String: any AppCoreProtocol] = [:]
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

    /// Attempt to restore persisted accounts on launch.
    ///
    /// The persisted list is the source of truth for "which accounts exist".
    /// We always show persisted accounts in the UI. If the server round-trip
    /// in `login()` fails (network down, server unreachable), the account
    /// appears but without an active core — network operations will fail
    /// gracefully until the user retries or the app is relaunched.
    func restoreAccounts() async {
        let persisted = Self.loadPersistedAccounts()
        guard !persisted.isEmpty else { return }

        let svc = _service
        let dbKey = Self.dbKey
        let dir = dbDir

        for p in persisted {
            let dbPath = dir.appendingPathComponent(p.dbFilename).path

            // Check the DB file actually exists — if not, the account is
            // truly gone (e.g. app was reinstalled). Remove it from persistence.
            guard FileManager.default.fileExists(atPath: dbPath) else {
                print("DB file missing for \(p.did), removing from persisted accounts")
                Self.removePersistedAccount(did: p.did)
                continue
            }

            // Always add the account to the UI so it's visible.
            let account = Account(
                id: p.did,
                displayName: p.displayName,
                avatarData: nil,
                servers: p.servers.map { s in
                    ServerInfo(id: s.id, name: s.name, url: URL(string: s.url)!)
                }
            )
            accounts.append(account)

            // Try to login (opens DB + authenticates with server).
            // If it fails, the account is still shown — just without a live core.
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
            }

            startMessagePolling()
        }
    }

    func switchMode(_ mode: ServiceMode) {
        serviceMode = mode
        UserDefaults.standard.set(mode.rawValue, forKey: Self.serviceModeKey)
        _service = Self.makeService(mode: mode)
        // Reset state on mode switch
        accounts.removeAll()
        conversations.removeAll()
        messagesByConversation.removeAll()
        cores.removeAll()
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
    }

    func joinServer(serverUrl: String, serverName: String, existingAccountId: String) async throws {
        if let idx = accounts.firstIndex(where: { $0.id == existingAccountId }) {
            accounts[idx].servers.append(
                ServerInfo(id: serverUrl, name: serverName, url: URL(string: serverUrl)!)
            )
        }
        isOnboarding = false
    }

    func sendMessage(conversationId: String, text: String, recipientDid: String, senderAccountId: String) async throws {
        guard let core = cores[senderAccountId] else { return }
        let plaintext = Data(text.utf8)
        try await Task.detached {
            try core.sendDm(recipientDid: recipientDid, recipientDeviceId: 1, plaintext: plaintext)
        }.value
    }

    func pollMessages(for accountId: String) async throws -> [DecryptedMessage] {
        guard let core = cores[accountId] else { return [] }
        return try await Task.detached {
            try core.receiveMessages()
        }.value
    }

    /// Fetch the list of Projects from a server.
    func fetchProjects(serverUrl: String) async -> [ProjectInfo] {
        // Find an account on this server to use for the API call.
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
    /// Returns the token string.
    func requestProjectToken(accountId: String, projectUrl: String) async throws -> String {
        guard let core = cores[accountId] else {
            throw NSError(domain: "AppState", code: 1, userInfo: [NSLocalizedDescriptionKey: "No account"])
        }
        return try await Task.detached {
            try core.requestProjectToken(projectUrl: projectUrl)
        }.value
    }

    // MARK: - Message receiving (WebSocket)

    /// Start a background WebSocket listener for each account.
    func startMessagePolling() {
        for account in accounts {
            let accountId = account.id
            Task {
                await messageWsLoop(accountId: accountId)
            }
        }
    }

    /// Connects via WebSocket and waits for messages. Reconnects on error.
    private func messageWsLoop(accountId: String) async {
        print("[ws] starting message loop for \(accountId)")
        while !Task.isCancelled {
            guard let core = cores[accountId] else {
                print("[ws] no core for \(accountId), waiting...")
                try? await Task.sleep(nanoseconds: 2_000_000_000)
                continue
            }
            do {
                let messages = try await Task.detached {
                    try core.receiveMessagesWs()
                }.value
                print("[ws] received \(messages.count) message(s) for \(accountId)")
                for msg in messages {
                    handleIncomingMessage(msg, accountId: accountId)
                }
                if messages.isEmpty {
                    // Connection closed — brief delay before reconnect.
                    try? await Task.sleep(nanoseconds: 1_000_000_000)
                }
            } catch {
                print("[ws] error for \(accountId): \(error), reconnecting in 2s")
                try? await Task.sleep(nanoseconds: 2_000_000_000)
            }
        }
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
            conversations[idx].unreadCount += 1
        } else {
            // Auto-create a new conversation for this DID.
            let serverUrl = accounts.first(where: { $0.id == accountId })?.servers.first?.id ?? ""
            convId = "dm-\(accountId)-\(senderDid)"
            let conv = Conversation(
                id: convId,
                title: senderDid,
                accountId: accountId,
                serverUrl: serverUrl,
                recipientDid: senderDid,
                lastMessage: text,
                lastMessageDate: Date(),
                unreadCount: 1,
                isGroup: false
            )
            conversations.append(conv)
        }

        let message = Message(
            id: UUID().uuidString,
            conversationId: convId,
            senderAccountId: senderDid,
            body: text,
            sentAt: Date()
        )
        messagesByConversation[convId, default: []].append(message)
        persistConversations()
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
