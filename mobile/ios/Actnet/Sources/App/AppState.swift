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
    /// Reactions per conversation (docs/33), keyed by conversation id. Each
    /// reaction carries its target message's wire identity
    /// `(targetAuthor, targetSentAtMs)`; the UI clusters by target + emoji.
    @Published var reactionsByConversation: [String: [ReactionFfi]] = [:]
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
        case chats, network
    }

    /// Active AppCore instances, keyed by DID.
    private var cores: [String: any AppCoreProtocol] = [:]
    /// DIDs with an in-flight `recoverAccount`. Single-flight guard: the
    /// recovery console's SwiftUI `.task` can fire more than once, and recovery
    /// is heavy and side-effectful (device replace, group rejoin, welcome
    /// sends), so it must run exactly once per DID.
    private var recoveriesInFlight: Set<String> = []
    /// Per-account connection-state listener tasks. Cancelled on logout/mode switch.
    private var stateTasks: [String: Task<Void, Never>] = [:]
    /// Per-account event-channel listener tasks. Cancelled on logout/mode switch.
    private var eventTasks: [String: Task<Void, Never>] = [:]
    /// Cached display names for remote DIDs, keyed by DID.
    private var displayNameCache: [String: String] = [:]
    /// DIDs currently being fetched (to avoid duplicate requests).
    private var displayNameInFlight: Set<String> = []
    /// DIDs that resolved to no name this session. Suppresses re-spawning a
    /// resolve task on every re-render of an unnameable row. The persistent
    /// per-outcome throttle in core (docs/52) already makes the *server* side
    /// cheap; this just avoids the per-render Task churn on the client.
    /// Cleared on reconnect so coming back online retries.
    private var unresolvedDids: Set<String> = []
    /// Cached bot status for remote DIDs, keyed by DID. Populated as a
    /// side-effect of name resolution (same server account record), and read
    /// by avatar rendering to pick the bot frame + badge
    /// (docs/54-bot-presentation.md). A missing entry renders as a person.
    private var isBotCache: [String: Bool] = [:]
    /// Cached group titles, keyed by URL-safe-no-pad base64 group_id.
    /// Populated by `fetchGroupTitle` and consumed by the conversation
    /// list / `Conversation.title`.
    private var groupTitleCache: [String: String] = [:]
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
            return .devServer
        }()
        self.serviceMode = resolved
        self._service = Self.makeService(mode: resolved)
        self.isOnboarding = true
    }

    // MARK: - Deep linking

    /// Handle a deep link URL.
    /// Supported:
    /// - `https://go.theavalanche.net/conversation/<recipient_did>`
    /// - `https://go.theavalanche.net/i/<token>` (or legacy `/invite/<token>`)
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

        case "i", "invite":
            let token = pathComponents[1]
            print("[DeepLink] handling invite token")
            // Try to decode the token locally to check if we're already on the
            // server. Single-char wire keys: s=server_url, d=inviter_did.
            if let data = Data(base64URLEncoded: token),
               let payload = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let serverUrl = payload["s"] as? String,
               let inviterDid = payload["d"] as? String,
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
        unresolvedDids.removeAll()
        isBotCache.removeAll()
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
        unresolvedDids.removeAll()
        isBotCache.removeAll()
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

    /// Create a new account. `prfOutput` is the raw 32-byte WebAuthn PRF
    /// output from the just-completed passkey ceremony (or the hash of a
    /// recovery phrase). Pass empty Data to skip recovery setup.
    func createAccount(serverUrl: String, serverName: String, displayName: String, inviteToken: String?, prfOutput: Data = Data()) async throws {
        let dir = dbDir
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

        let dbFilename = "account-\(UUID().uuidString.prefix(8)).db"
        let dbPath = dir.appendingPathComponent(dbFilename).path
        let dbKey = try SecureEnclaveKeyManager.dbPassphrase()

        let svc = _service
        let prf = prfOutput
        let dn = displayName
        // Forward the raw invite token the onboarding flow validated (the server
        // re-evaluates it; docs/24). Threaded explicitly from the InviteToken so
        // every entry path works — pasted link, QR scan, and deep link all reach
        // here with the token, not just the deep-link path that set
        // `pendingInviteToken`. The dev server runs closed registration, so this
        // token carries the bootstrap secret.
        let token = inviteToken
        let core = try await Task.detached {
            try svc.createAccount(serverUrl: serverUrl, dbPath: dbPath, dbKey: dbKey, prfOutput: prf, displayName: dn, inviteToken: token)
        }.value

        try await finishAccountRegistration(core: core, serverUrl: serverUrl, serverName: serverName, displayName: displayName, dbFilename: dbFilename)
    }

    /// Prepare a fresh identity (Stage 1 of the passkey signup flow).
    ///
    /// Call this *after* the WebAuthn passkey ceremony, with the raw 32-byte
    /// PRF output. Rust derives the rotation key from the PRF, builds the
    /// genesis + identity-update PLC ops, and produces the final DID — which
    /// is then exposed via `PreparedAccountProtocol.did()`.
    func prepareAccount(serverUrl: String, prfOutput: Data) async throws -> any PreparedAccountProtocol {
        let svc = _service
        let prf = prfOutput
        return try await Task.detached {
            try svc.prepareAccount(serverUrl: serverUrl, prfOutput: prf)
        }.value
    }

    /// Finalize an account previously created by `prepareAccount` (Stage 2 of
    /// the passkey flow). Submits the PLC ops, encrypts the recovery blob
    /// using the same passkey-derived key carried inside `prepared`, and
    /// registers with the server.
    func finalizePreparedAccount(
        prepared: any PreparedAccountProtocol,
        serverUrl: String,
        serverName: String,
        displayName: String,
        inviteToken: String?
    ) async throws {
        let dir = dbDir
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

        let dbFilename = "account-\(UUID().uuidString.prefix(8)).db"
        let dbPath = dir.appendingPathComponent(dbFilename).path
        let dbKey = try SecureEnclaveKeyManager.dbPassphrase()

        let svc = _service
        let dn = displayName
        // Explicitly threaded from the validated InviteToken — see createAccount.
        let token = inviteToken
        let core = try await Task.detached {
            try svc.finalizeAccount(prepared: prepared, dbPath: dbPath, dbKey: dbKey, displayName: dn, inviteToken: token)
        }.value

        try await finishAccountRegistration(core: core, serverUrl: serverUrl, serverName: serverName, displayName: displayName, dbFilename: dbFilename)
    }

    /// Recover an account from a passkey-protected recovery blob. Downloads
    /// the blob from `serverUrl` keyed by `did`, decrypts with the
    /// PRF-derived blob key (Rust handles the derivation), replaces the
    /// device on the home server, and signs the user in.
    ///
    /// `displayName` may be empty — the recovered account will appear in the
    /// account list with a placeholder until the user updates it from Settings.
    func recoverAccount(
        serverUrl: String,
        serverName: String,
        did: String,
        prfOutput: Data,
        displayName: String
    ) async throws {
        // Single-flight: run recovery once per DID. Atomic because AppState is
        // @MainActor and these checks complete before the first `await`, so a
        // second invocation (e.g. the console's `.task` re-firing) is dropped
        // rather than spawning a duplicate AppCore + duplicate account entry and
        // re-doing the device replace / group rejoin / welcome sends.
        guard !accounts.contains(where: { $0.id == did }) else { return }
        guard recoveriesInFlight.insert(did).inserted else { return }
        defer { recoveriesInFlight.remove(did) }

        let dir = dbDir
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

        let dbFilename = "account-\(UUID().uuidString.prefix(8)).db"
        let dbPath = dir.appendingPathComponent(dbFilename).path
        let dbKey = try SecureEnclaveKeyManager.dbPassphrase()

        let svc = _service
        let prf = prfOutput
        let recoveryDid = did
        let core = try await Task.detached {
            try svc.recoverFromBlob(
                serverUrl: serverUrl,
                did: recoveryDid,
                prfOutput: prf,
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

        // Rebuild the conversation list from the freshly-registered core's
        // store. For recovery this is what surfaces groups restored from the
        // recovery blob (their master keys are already in the `groups` table,
        // which `loadConversations` lists even before any message arrives) —
        // otherwise recovered groups don't appear until the next launch. For a
        // brand-new account it's a harmless empty load. Done before the mock
        // seed below so mock previews still append their fixtures.
        await loadConversationsFromStore()

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

    /// Look up the AppCore bound to a given account DID. Used by per-account
    /// views (group detail, etc.) that need to call FFI directly.
    func core(accountId: String) -> (any AppCoreProtocol)? {
        cores[accountId]
    }

    func joinServer(serverUrl: String, serverName: String, existingAccountId: String) async throws {
        if let idx = accounts.firstIndex(where: { $0.id == existingAccountId }) {
            accounts[idx].servers.append(
                ServerInfo(id: serverUrl, name: serverName, url: URL(string: serverUrl)!)
            )
        }
        isOnboarding = false
    }

    // MARK: - Account teardown (docs/53-multi-account-ux.md)

    /// Leave a server (docs/53 §Leave): the core leaves every group hosted
    /// there and deletes the account on the server. Today each account is bound
    /// to a single server (one core, docs/06 §9 N=1), so the membership and the
    /// account coincide — leaving removes the account from this device.
    func leaveServer(account: Account, server: ServerInfo) async throws {
        guard let core = cores[account.id] else {
            throw NSError(domain: "AppState", code: 1, userInfo: [NSLocalizedDescriptionKey: "No active connection for this account"])
        }
        try await Task.detached { try core.leaveServer() }.value
        removeAccountLocally(accountId: account.id)
    }

    /// Delete an identity (docs/53 §Delete identity): the core leaves every
    /// server, submits a rotation-key-signed PLC tombstone for the DID, and
    /// wipes its own SQLCipher rows. This then removes the now-empty account
    /// from the device. Throws (leaving the account in place) if the tombstone
    /// could not be submitted, so the user can retry.
    func deleteIdentity(account: Account) async throws {
        guard let core = cores[account.id] else {
            throw NSError(domain: "AppState", code: 1, userInfo: [NSLocalizedDescriptionKey: "No active connection for this account"])
        }
        try await Task.detached { try core.deleteIdentity() }.value
        removeAccountLocally(accountId: account.id)
    }

    /// Tear down all local state for one account: listener tasks, the live
    /// core, its in-memory conversations/messages, the persisted entry, and the
    /// SQLCipher database files (identity.db + device.db and their WAL/SHM
    /// siblings). Returns to onboarding if it was the last account.
    private func removeAccountLocally(accountId: String) {
        stateTasks[accountId]?.cancel()
        stateTasks.removeValue(forKey: accountId)
        eventTasks[accountId]?.cancel()
        eventTasks.removeValue(forKey: accountId)
        cores.removeValue(forKey: accountId)
        connectionStates.removeValue(forKey: accountId)
        accounts.removeAll { $0.id == accountId }

        let convIds = conversations.filter { $0.accountId == accountId }.map(\.id)
        conversations.removeAll { $0.accountId == accountId }
        for id in convIds {
            messagesByConversation.removeValue(forKey: id)
            reactionsByConversation.removeValue(forKey: id)
        }

        // Delete the on-disk SQLCipher files. The identity DB is the base path;
        // the device DB is `<base>.device` (store::open_split). Each may have
        // `-wal` / `-shm` siblings.
        if let filename = Self.persistedDbFilename(did: accountId) {
            let base = dbDir.appendingPathComponent(filename).path
            for path in [base, base + ".device"] {
                for suffix in ["", "-wal", "-shm"] {
                    try? FileManager.default.removeItem(atPath: path + suffix)
                }
            }
        }

        Self.removePersistedAccount(did: accountId)
        if accounts.isEmpty {
            isOnboarding = true
        }
    }

    // MARK: - Display name resolution

    /// Returns the cached display name for a DID, or the DID itself if unknown.
    /// Kicks off a background fetch if not cached yet.
    ///
    /// This is the single client-side name resolver: it unifies the two
    /// server/client name sources (encrypted profile for humans, server record
    /// for bots — see `resolveDisplayName`) behind one cache. The DID return is
    /// an *unresolved* sentinel used by the conversation-title flow; anything
    /// rendering a name to the user should call `resolvedName(for:)`, which
    /// never yields a DID.
    func displayName(for did: String, accountId: String) -> String {
        // Own accounts: the name lives in the `Account` model, not the contact
        // cache or the server record (humans publish no plaintext name
        // server-side), so the async resolver would never find it. This is why
        // self showed as "Unknown" in groups you create.
        if let account = accounts.first(where: { $0.id == did }) {
            return account.displayName
        }
        if let name = displayNameCache[did] { return name }
        resolveDisplayName(did: did, accountId: accountId)
        return did
    }

    /// UI-facing display name for a DID: the resolved name, or `"Unknown"`
    /// while it resolves (or if it never does). Never returns a DID — DIDs are
    /// not a user-visible concept. This is the only accessor UI should use to
    /// show someone's name, so human and bot names always flow through the
    /// same path.
    func resolvedName(for did: String, accountId: String) -> String {
        let name = displayName(for: did, accountId: accountId)
        return name == did ? "Unknown" : name
    }

    /// Human-readable line for a group system/metadata event (docs/03 §3.6),
    /// resolving actor/target DIDs to display names ("You" for self). Falls
    /// back to the stored English summary if the structured metadata is
    /// missing or unparseable.
    func groupEventText(_ message: Message, accountId: String) -> String {
        guard let ev = message.groupEvent else { return message.body }
        let actor = eventName(ev.actorDid, accountId: accountId, capitalized: true)
        let target = eventName(ev.targetDid, accountId: accountId, capitalized: false)
        switch ev.event {
        case .memberJoined: return "\(actor) joined"
        case .memberJoinedViaLink: return "\(actor) joined via invite link"
        case .memberRequestedToJoin: return "\(actor) requested to join"
        case .memberInvited: return "\(actor) invited \(target)"
        case .memberLeft: return "\(actor) left the group"
        case .memberRemoved: return "\(actor) removed \(target)"
        case .joinRequestApproved: return "\(actor) approved \(target)'s request to join"
        case .joinRequestDenied: return "\(actor) declined a join request"
        case .inviteDeclined: return "\(actor) declined the invitation"
        case .joinRequestCancelled: return "\(actor) cancelled their request to join"
        case .roleChangedToAdmin: return "\(actor) made \(target) an admin"
        case .roleChangedToMember: return "\(actor) removed \(target) as an admin"
        case .titleChanged:
            // Group names are never empty; the empty arm is a defensive fallback.
            return ev.newTitle.isEmpty
                ? "\(actor) changed the group name"
                : "\(actor) changed the group name to \u{201C}\(ev.newTitle)\u{201D}"
        case .descriptionChanged: return "\(actor) changed the group description"
        case .expiryChanged:
            if ev.expirySeconds == 0 {
                return "\(actor) turned off disappearing messages"
            }
            return "\(actor) set disappearing messages to \(DisappearingMessagesPicker.label(for: ev.expirySeconds))"
        case .policyChanged: return "\(actor) changed the group settings"
        }
    }

    private func eventName(_ did: String, accountId: String, capitalized: Bool) -> String {
        if did.isEmpty { return capitalized ? "Someone" : "someone" }
        if did == accountId { return capitalized ? "You" : "you" }
        return resolvedName(for: did, accountId: accountId)
    }

    /// Whether a DID is a bot account, for avatar/badge presentation
    /// (docs/54-bot-presentation.md). Sourced from the same server account
    /// record `resolveDisplayName` consults, cached alongside the name, so it
    /// adds no extra round-trips for any DID already being named. Returns
    /// `false` while unresolved or for humans/own accounts — the absence of a
    /// positive bot signal renders as a person, per the spec.
    func isBot(_ did: String, accountId: String) -> Bool {
        if accounts.contains(where: { $0.id == did }) { return false }
        if let known = isBotCache[did] { return known }
        resolveDisplayName(did: did, accountId: accountId)
        return false
    }

    /// Seed the name cache with a name a caller already holds (e.g. the
    /// contact-list FFI rows carry the cached profile name), so the async
    /// resolver doesn't re-fetch it. No-op if empty or already cached.
    func cacheDisplayName(_ name: String, for did: String) {
        guard !name.isEmpty, displayNameCache[did] == nil else { return }
        applyResolvedDisplayName(did: did, name: name)
    }

    /// Resolve a display name for a DID. Two sources, in order:
    /// 1. Local SQLCipher `contact_profiles` cache — populated automatically by
    ///    app-core when an inbound message carries the sender's profile_key.
    ///    This is the path for human contacts.
    /// 2. Server-side `/v1/accounts/{did}` lookup — only returns a name for
    ///    bot accounts (humans never put a plaintext name on the server).
    private func resolveDisplayName(did: String, accountId: String) {
        guard !displayNameInFlight.contains(did) else { return }
        // Already resolved-to-empty this session — don't re-spawn until reconnect.
        guard !unresolvedDids.contains(did) else { return }
        guard let core = cores[accountId] else { return }
        displayNameInFlight.insert(did)
        let targetDid = did
        Task.detached { [weak self] in
            // Local contact_profiles first — fast, no network.
            let localName = (try? core.contactDisplayName(did: targetDid)) ?? ""
            // Fall back to server lookup (bots) only if the local cache is empty.
            // The same record carries `isBot`, so we learn bot status here too
            // (docs/54-bot-presentation.md) without a separate round-trip. Core
            // throttles this call (docs/52) — offline/throttled lookups return
            // a cached record or throw, never a fresh server hit per render.
            let serverInfo = localName.isEmpty ? try? core.getAccountInfo(did: targetDid) : nil
            let resolved = !localName.isEmpty ? localName : (serverInfo?.displayName ?? "")
            // A resolved local name means a human contact; only the server
            // record marks bots. Absent info → not a bot.
            let isBot = serverInfo?.isBot ?? false

            await MainActor.run {
                guard let self else { return }
                self.displayNameInFlight.remove(targetDid)
                self.isBotCache[targetDid] = isBot
                guard !resolved.isEmpty else {
                    // Negative-cache so we don't re-spawn this task every render.
                    self.unresolvedDids.insert(targetDid)
                    return
                }
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

    // MARK: - Abuse handling (docs/12-abuse-handling.md)

    /// Accept a message request: curate the sender (which clears the gate and
    /// opens read receipts) and refresh the list so the banner disappears.
    func acceptRequest(did: String, accountId: String) async {
        guard let core = cores[accountId] else { return }
        try? await Task.detached { try core.acceptRequest(did: did) }.value
        await loadConversationsFromStore()
    }

    /// Delete a message request: clear the pending flag and drop the local
    /// conversation. A later inbound message starts a fresh request.
    func deleteRequest(did: String, accountId: String) async {
        guard let core = cores[accountId] else { return }
        try? await Task.detached { try core.deleteRequest(did: did) }.value
        await loadConversationsFromStore()
    }

    /// Report Spam and Block: file a content-free report with the homeserver,
    /// then block locally. Surfaced only in the message-request UI (docs/12 §3).
    func reportAndBlock(did: String, accountId: String, reason: String = "spam") async {
        guard let core = cores[accountId] else { return }
        try? await Task.detached { try core.reportAndBlock(did: did, reason: reason) }.value
        await loadConversationsFromStore()
    }

    /// Block a contact (docs/12 §2). Multi-device synced; outbound messages to
    /// the DID are then refused and inbound ones dropped.
    func blockContact(did: String, accountId: String) async {
        guard let core = cores[accountId] else { return }
        try? await Task.detached { try core.blockContact(did: did) }.value
        await loadConversationsFromStore()
    }

    /// Unblock a contact (docs/12 §2).
    func unblockContact(did: String, accountId: String) async {
        guard let core = cores[accountId] else { return }
        try? await Task.detached { try core.unblockContact(did: did) }.value
        await loadConversationsFromStore()
    }

    /// The block list for an account — backs Settings → Privacy → Blocked.
    func listBlocked(accountId: String) async -> [ContactRowFfi] {
        guard let core = cores[accountId] else { return [] }
        return (try? await Task.detached { try core.listBlocked() }.value) ?? []
    }

    // MARK: - Messaging

    func sendMessage(conversationId: String, text: String, recipientDid: String, senderAccountId: String, messageId: String, sentAtMs: Int64) async throws {
        guard let core = cores[senderAccountId] else { return }
        let plaintext = Data(text.utf8)
        let nowMs = sentAtMs
        // Stamp the local copy with the same disappearing-messages timer the
        // send path puts on the wire (docs/03 §5), so the sender's copy expires
        // too. The DM timer is keyed by peer DID.
        let timer = ((try? await Task.detached {
            try core.getConversationTimer(conversationId: recipientDid)
        }.value) ?? nil) ?? 0

        // Persist as "sending" up front so failures are recoverable across launches.
        let pending = StoredMessageFfi(
            id: messageId,
            conversationId: conversationId,
            senderDid: senderAccountId,
            body: text,
            sentAtMs: nowMs,
            editedAtMs: nil,
            readAtMs: nowMs,
            deliveryStatus: UInt8(DeliveryStatus.sending.rawValue),
            editCount: 0,
            deleted: false,
            kind: 0,
            metadata: nil,
            expireTimerSecs: timer,
            expireAtMs: nil
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
                deliveryStatus: UInt8(DeliveryStatus.sent.rawValue), editCount: 0, deleted: false, kind: 0, metadata: nil,
                expireTimerSecs: timer, expireAtMs: nil
            )
            Task.detached { try? core.saveMessage(msg: sent) }
        } catch {
            AppLog.error("send", "DM to \(recipientDid) failed: \(error.localizedDescription)")
            updateMessageStatus(messageId: messageId, conversationId: conversationId, newStatus: .failed)
            let failed = StoredMessageFfi(
                id: messageId, conversationId: conversationId, senderDid: senderAccountId,
                body: text, sentAtMs: nowMs, editedAtMs: nil, readAtMs: nowMs,
                deliveryStatus: UInt8(DeliveryStatus.failed.rawValue), editCount: 0, deleted: false, kind: 0, metadata: nil,
                expireTimerSecs: timer, expireAtMs: nil
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
        // Expired messages (docs/03 §5) are excluded by the store reads
        // themselves, so the preview can't reflect a disappeared message; the
        // app-core reaper handles physical deletion + refresh events.
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
        var groupsNeedingRefresh: [(groupId: String, accountId: String)] = []
        for (accountId, summaries) in summariesPerAccount {
            let serverUrl = accounts.first(where: { $0.id == accountId })?.servers.first?.id ?? ""
            for s in summaries {
                let date = s.lastMessage.map {
                    Date(timeIntervalSince1970: TimeInterval($0.sentAtMs) / 1000.0)
                }
                // Carry the body + (for system events) the structured kind /
                // metadata so the row renders the preview reactively, resolving
                // DIDs to names at display time. Freezing groupEventText here
                // would bake in "Unknown" before names are cached.
                let preview = s.lastMessage?.body
                let lastKind = Int(s.lastMessage?.kind ?? 0)
                let lastMeta = s.lastMessage?.metadata
                let lastSender = s.lastMessage?.senderDid
                if let groupId = Self.groupId(from: s.conversationId) {
                    // `group_title` comes resolved from local state in
                    // `loadConversations`; cache it so later rebuilds and
                    // `findOrCreateGroupConversation` reuse it.
                    if let t = s.groupTitle, !t.isEmpty {
                        groupTitleCache[groupId] = t
                    }
                    let title = groupTitleCache[groupId] ?? "Group"
                    if groupTitleCache[groupId] == nil {
                        // No local state yet (e.g. a freshly received invite) —
                        // pull it from the server in the background.
                        groupsNeedingRefresh.append((groupId, accountId))
                    }
                    newConvs.append(Conversation(
                        id: s.conversationId,
                        title: title,
                        accountId: accountId,
                        serverUrl: serverUrl,
                        recipientDid: nil,
                        groupId: groupId,
                        lastMessage: preview,
                        lastMessageDate: date,
                        lastMessageKind: lastKind,
                        lastMessageMetadata: lastMeta,
                        lastMessageSenderDid: lastSender,
                        isGroup: true
                    ))
                    continue
                }
                let recipientDid = Self.recipientDid(from: s.conversationId, accountId: accountId)
                let title = recipientDid.flatMap { displayNameCache[$0] } ?? recipientDid ?? s.conversationId
                newConvs.append(Conversation(
                    id: s.conversationId,
                    title: title,
                    accountId: accountId,
                    serverUrl: serverUrl,
                    recipientDid: recipientDid,
                    lastMessage: preview,
                    lastMessageDate: date,
                    isGroup: false,
                    isRequest: s.isRequest,
                    isBlocked: s.isBlocked
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

        // For groups with no locally-cached title (e.g. just-received invites),
        // fetch state from the server so "Group" gets replaced with the real name.
        for g in groupsNeedingRefresh {
            refreshGroupTitle(groupId: g.groupId, accountId: g.accountId)
        }
    }

    /// Parse the recipient DID out of a conversation ID of the form
    /// `dm-<accountDid>-<recipientDid>`. Returns nil for non-DM IDs.
    private static func recipientDid(from conversationId: String, accountId: String) -> String? {
        let prefix = "dm-\(accountId)-"
        guard conversationId.hasPrefix(prefix) else { return nil }
        return String(conversationId.dropFirst(prefix.count))
    }

    /// Parse the group_id out of a conversation ID of the form
    /// `group-<groupIdB64>`. Returns nil for non-group IDs.
    private static func groupId(from conversationId: String) -> String? {
        let prefix = "group-"
        guard conversationId.hasPrefix(prefix) else { return nil }
        return String(conversationId.dropFirst(prefix.count))
    }

    /// Re-read a group's timeline from the store, but only if it's already
    /// loaded in memory — used after `groupMetadataChanged` so a freshly
    /// persisted system row shows in the open conversation. (If the
    /// conversation isn't loaded yet, the next `loadMessagesFromStore` picks up
    /// the row anyway; we must not seed a partial array here, or the load-once
    /// guard would then hide the rest of the history.)
    func reloadGroupTimelineIfLoaded(groupId: String, accountId: String) {
        reloadMessagesIfLoaded(conversationId: groupConversationId(groupId), accountId: accountId)
    }

    /// Re-read a conversation's timeline from the store, but only if it's
    /// already loaded in memory. (If not loaded, the next `loadMessagesFromStore`
    /// reads it fresh; seeding a partial array here would trip the load-once
    /// guard and hide the rest of the history.)
    func reloadMessagesIfLoaded(conversationId convId: String, accountId: String) {
        guard let core = cores[accountId] else { return }
        guard messagesByConversation[convId] != nil else { return }
        Task.detached { [weak self] in
            guard let msgs = try? core.loadMessages(conversationId: convId) else { return }
            let messages = msgs.map(Self.message(from:))
            await MainActor.run { self?.messagesByConversation[convId] = messages }
        }
    }


    /// Map a stored FFI message row to the view `Message` model. `nonisolated`
    /// so it can run inside the detached store-read tasks.
    nonisolated static func message(from m: StoredMessageFfi) -> Message {
        Message(
            id: m.id,
            conversationId: m.conversationId,
            senderAccountId: m.senderDid,
            body: m.body,
            sentAtMs: m.sentAtMs,
            editedAtMs: m.editedAtMs,
            readAtMs: m.readAtMs,
            deliveryStatus: DeliveryStatus(rawValue: Int(m.deliveryStatus)) ?? .sent,
            editCount: Int(m.editCount),
            isDeleted: m.deleted,
            kind: Int(m.kind),
            metadata: m.metadata,
            expireTimerSecs: m.expireTimerSecs,
            expireAtMs: m.expireAtMs
        )
    }

    func loadMessagesFromStore(conversationId: String, accountId: String) {
        guard let core = cores[accountId] else { return }
        // Only load if we haven't already loaded for this conversation.
        guard messagesByConversation[conversationId] == nil else { return }
        let convId = conversationId
        Task.detached { [weak self] in
            guard let msgs = try? core.loadMessages(conversationId: convId) else { return }
            let messages = msgs.map(Self.message(from:))
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

    /// Find or create a group conversation. `title` is used if the
    /// conversation row is being created for the first time; existing rows
    /// keep their cached title until `refreshGroupTitle` overwrites it.
    func findOrCreateGroupConversation(
        groupId: String,
        title: String,
        accountId: String,
        serverUrl: String
    ) -> Conversation {
        let convId = groupConversationId(groupId)
        if let existing = conversations.first(where: { $0.id == convId }) {
            return existing
        }
        let conv = Conversation(
            id: convId,
            title: title,
            accountId: accountId,
            serverUrl: serverUrl,
            recipientDid: nil,
            groupId: groupId,
            isGroup: true
        )
        conversations.append(conv)
        groupTitleCache[groupId] = title
        return conv
    }

    /// Whether the current identity is still a member of a group (docs/53 §Leave).
    /// Reads the locally-cached state via the core — `false` after leaving, which
    /// the conversation view uses to hide the composer. Defaults to `true` on
    /// error so a transient read failure doesn't lock a real member out.
    func isGroupMember(groupId: String, accountId: String) async -> Bool {
        guard let core = cores[accountId] else { return true }
        return (try? await Task.detached { try core.isGroupMember(groupId: groupId) }.value) ?? true
    }

    /// Refresh the cached title for a group from `fetchGroupState`. Updates
    /// any in-memory `Conversation` row with the new title.
    func refreshGroupTitle(groupId: String, accountId: String) {
        guard let core = cores[accountId] else { return }
        Task.detached { [weak self] in
            // Pull any pending membership/metadata changes first (docs/03 §3.6):
            // this derives + persists the group system-event timeline rows and
            // emits `groupMetadataChanged` for each (handled by the event loop),
            // and fast-forwards the cached state so the title below is current.
            _ = try? core.applyPendingGroupChanges(groupId: groupId)
            guard let summary = try? core.fetchGroupState(groupId: groupId) else { return }
            let title = summary.title.isEmpty ? "Group" : summary.title
            await MainActor.run {
                guard let self else { return }
                self.groupTitleCache[groupId] = title
                let convId = groupConversationId(groupId)
                if let idx = self.conversations.firstIndex(where: { $0.id == convId }) {
                    self.conversations[idx].title = title
                }
            }
        }
    }

    /// Compose entry point: create a new group with the given recipients,
    /// invite each member, and (optionally) send the first message. Returns
    /// the new conversation once `create_group` succeeds. Invites and the
    /// first send happen asynchronously; failures surface via banners on
    /// the returned thread (TODO: wire partial-failure banner per docs/30).
    func createGroupAndOpen(
        accountId: String,
        serverUrl: String,
        title: String,
        recipientDids: [String],
        expirySeconds: UInt32,
        firstMessage: String?
    ) async throws -> Conversation {
        guard let core = cores[accountId] else {
            throw NSError(domain: "AppState", code: 1, userInfo: [NSLocalizedDescriptionKey: "No core for account"])
        }
        let titleForCreate = title
        let expiry = expirySeconds
        let created = try await Task.detached {
            try core.createGroup(title: titleForCreate, description: "", expirySeconds: expiry)
        }.value
        let groupId = created.groupId

        // Fan out invites. Best-effort — one failure doesn't abort the rest.
        let invitees = recipientDids
        Task.detached {
            for did in invitees {
                do {
                    try core.inviteMember(groupId: groupId, recipientDid: did, role: 0)
                } catch {
                    AppLog.warn("compose", "invite \(did) to \(groupId) failed: \(error.localizedDescription)")
                }
            }
        }

        let conv = findOrCreateGroupConversation(
            groupId: groupId,
            title: titleForCreate.isEmpty ? "Group" : titleForCreate,
            accountId: accountId,
            serverUrl: serverUrl
        )

        if let body = firstMessage, !body.isEmpty {
            let messageId = UUID().uuidString
            let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
            let optimistic = Message(
                id: messageId,
                conversationId: conv.id,
                senderAccountId: accountId,
                body: body,
                sentAtMs: nowMs,
                readAtMs: nowMs,
                deliveryStatus: .sending
            )
            messagesByConversation[conv.id, default: []].append(optimistic)
            try await sendGroupMessage(
                conversation: conv,
                text: body,
                messageId: messageId,
                sentAtMs: nowMs
            )
        }
        return conv
    }

    /// Send a message into a group conversation. Mirrors `sendMessage` for
    /// DMs: persist as `sending`, dispatch over FFI, update status on
    /// success / failure.
    func sendGroupMessage(
        conversation: Conversation,
        text: String,
        messageId: String,
        sentAtMs: Int64
    ) async throws {
        guard let groupId = conversation.groupId else { return }
        guard let core = cores[conversation.accountId] else { return }
        let plaintext = Data(text.utf8)
        // Stamp the local copy with the group's current disappearing-messages
        // timer (docs/03 §5) so the sender's copy expires like everyone else's.
        let timer = (try? await Task.detached {
            try core.groupExpirySeconds(groupId: groupId)
        }.value) ?? 0

        let pending = StoredMessageFfi(
            id: messageId,
            conversationId: conversation.id,
            senderDid: conversation.accountId,
            body: text,
            sentAtMs: sentAtMs,
            editedAtMs: nil,
            readAtMs: sentAtMs,
            deliveryStatus: UInt8(DeliveryStatus.sending.rawValue),
            editCount: 0,
            deleted: false,
            kind: 0,
            metadata: nil,
            expireTimerSecs: timer,
            expireAtMs: nil
        )
        try await Task.detached { try core.saveMessage(msg: pending) }.value

        do {
            try await Task.detached {
                try core.sendGroupMessage(groupId: groupId, plaintext: plaintext, sentAtMs: sentAtMs)
            }.value
            updateMessageStatus(messageId: messageId, conversationId: conversation.id, newStatus: .sent)
            let sent = StoredMessageFfi(
                id: messageId, conversationId: conversation.id, senderDid: conversation.accountId,
                body: text, sentAtMs: sentAtMs, editedAtMs: nil, readAtMs: sentAtMs,
                deliveryStatus: UInt8(DeliveryStatus.sent.rawValue), editCount: 0, deleted: false, kind: 0, metadata: nil,
                expireTimerSecs: timer, expireAtMs: nil
            )
            Task.detached { try? core.saveMessage(msg: sent) }
        } catch {
            AppLog.error("send", "group send to \(groupId) failed: \(error.localizedDescription)")
            updateMessageStatus(messageId: messageId, conversationId: conversation.id, newStatus: .failed)
            let failed = StoredMessageFfi(
                id: messageId, conversationId: conversation.id, senderDid: conversation.accountId,
                body: text, sentAtMs: sentAtMs, editedAtMs: nil, readAtMs: sentAtMs,
                deliveryStatus: UInt8(DeliveryStatus.failed.rawValue), editCount: 0, deleted: false, kind: 0, metadata: nil,
                expireTimerSecs: timer, expireAtMs: nil
            )
            Task.detached { try? core.saveMessage(msg: failed) }
            throw error
        }
    }

    // MARK: - Contacts (docs/52-contacts-and-profiles.md)

    /// Snapshot of the contact list for the given account, joined with
    /// cached display names. The compose autocomplete is built directly
    /// from this.
    func listContacts(accountId: String) async -> [ContactRowFfi] {
        guard let core = cores[accountId] else { return [] }
        return await Task.detached {
            (try? core.listContacts()) ?? []
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
            // Coming back online is a chance to resolve names we gave up on
            // while offline/throttled — drop the session negative cache so the
            // next render re-attempts (core still applies its own throttle).
            if case .connected = next {
                unresolvedDids.removeAll()
            }
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
            var needsConversationReload = false
            var groupsWithNewEvents: Set<String> = []
            for ev in events {
                switch ev {
                case .message(let msg): messages.append(msg)
                case .receiptUpdate(let upd): receiptUpdates.append(upd)
                case .groupInvite:
                    // Master key already persisted by app-core; just refresh
                    // the chat list so the new group becomes visible.
                    needsConversationReload = true
                case .groupMetadataChanged(let event):
                    // A membership/metadata change was derived from the change
                    // log (docs/03 §3.6). app-core has already persisted the
                    // matching system row; refresh the group's open timeline and
                    // the chat list so the line appears.
                    groupsWithNewEvents.insert(event.groupId)
                    needsConversationReload = true
                case .storageSynced:
                    // A background storage sync applied remote durable state
                    // (e.g. a group key synced from another device). The store
                    // is updated; rebuild the chat list so it shows without a
                    // restart.
                    needsConversationReload = true
                case let .messageEdited(conversationId, authorDid, sentAtMs, newBody, editedAtMs):
                    applyInboundEdit(conversationId: conversationId, authorDid: authorDid, sentAtMs: sentAtMs, newBody: newBody, editedAtMs: editedAtMs)
                case let .messageDeleted(conversationId, authorDid, sentAtMs):
                    applyInboundDelete(conversationId: conversationId, authorDid: authorDid, sentAtMs: sentAtMs)
                case let .reactionUpdated(conversationId, targetAuthor, targetSentAtMs, reactorDid, emoji, removed):
                    applyInboundReaction(conversationId: conversationId, targetAuthor: targetAuthor, targetSentAtMs: targetSentAtMs, reactorDid: reactorDid, emoji: emoji, removed: removed)
                case let .messagesExpired(conversationIds):
                    // The app-core reaper hard-deleted disappearing messages
                    // (docs/03 §5). Refresh each affected conversation's open
                    // timeline; the chat-list preview rebuilds below.
                    for convId in conversationIds {
                        reloadMessagesIfLoaded(conversationId: convId, accountId: accountId)
                    }
                    needsConversationReload = true
                }
            }
            for msg in messages {
                handleIncomingMessage(msg, accountId: accountId)
            }
            if !receiptUpdates.isEmpty {
                applyDeliveryStatusUpdates(receiptUpdates)
            }
            for groupId in groupsWithNewEvents {
                reloadGroupTimelineIfLoaded(groupId: groupId, accountId: accountId)
            }
            if needsConversationReload {
                await loadConversationsFromStore()
            }
        }
        eventTasks.removeValue(forKey: accountId)
        AppLog.info("evt", "event listener ended for \(accountId)")
    }

    // MARK: - Reactions, editing, deletion (docs/33, docs/36)

    /// Reactions currently on a specific message (for the on-bubble cluster).
    func reactions(for message: Message) -> [ReactionFfi] {
        (reactionsByConversation[message.conversationId] ?? []).filter {
            $0.targetAuthor == message.senderAccountId && $0.targetSentAtMs == message.sentAtMs
        }
    }

    /// Load a conversation's reactions from the store into memory.
    func loadReactions(conversationId: String, accountId: String) {
        guard let core = cores[accountId] else { return }
        let convId = conversationId
        Task.detached { [weak self] in
            guard let rows = try? core.loadReactions(conversationId: convId) else { return }
            await MainActor.run { self?.reactionsByConversation[convId] = rows }
        }
    }

    /// Where a content op for `conversation` is directed — the unified
    /// `MessageTarget` the core uses for DMs and groups alike.
    private func messageTarget(for conversation: Conversation) -> MessageTarget? {
        if let groupId = conversation.groupId { return .group(groupId: groupId) }
        if let recipientDid = conversation.recipientDid { return .dm(recipientDid: recipientDid) }
        return nil
    }

    /// Toggle this account's reaction on a message: tapping the emoji we already
    /// have removes it; otherwise it replaces any prior one (one per person per
    /// message, docs/33).
    func toggleReaction(message: Message, emoji: String, conversation: Conversation) {
        guard let core = cores[conversation.accountId],
              let target = messageTarget(for: conversation) else { return }
        let myDid = conversation.accountId
        let convId = conversation.id
        let targetAuthor = message.senderAccountId
        let targetSentAt = message.sentAtMs
        let existingMine = (reactionsByConversation[convId] ?? []).first {
            $0.targetAuthor == targetAuthor && $0.targetSentAtMs == targetSentAt && $0.reactorDid == myDid
        }
        let remove = existingMine?.emoji == emoji
        let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
        // Optimistic in-memory update.
        var list = reactionsByConversation[convId] ?? []
        list.removeAll { $0.targetAuthor == targetAuthor && $0.targetSentAtMs == targetSentAt && $0.reactorDid == myDid }
        if !remove {
            list.append(ReactionFfi(conversationId: convId, targetAuthor: targetAuthor, targetSentAtMs: targetSentAt, reactorDid: myDid, emoji: emoji, reactedAtMs: nowMs))
        }
        reactionsByConversation[convId] = list
        Task.detached {
            try? core.sendReaction(target: target, targetAuthor: targetAuthor, targetSentAtMs: targetSentAt, emoji: emoji, remove: remove, sentAtMs: nowMs)
        }
    }

    /// Edit one of my own messages in place (docs/36). DM only.
    func editMessage(message: Message, newBody: String, conversation: Conversation) {
        guard let core = cores[conversation.accountId],
              let target = messageTarget(for: conversation) else { return }
        let trimmed = newBody.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, trimmed != message.body else { return }
        let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
        let convId = conversation.id
        if var msgs = messagesByConversation[convId], let i = msgs.firstIndex(where: { $0.id == message.id }) {
            msgs[i].body = trimmed
            msgs[i].editedAtMs = nowMs
            msgs[i].editCount += 1
            messagesByConversation[convId] = msgs
        }
        let targetSentAt = message.sentAtMs
        Task.detached {
            try? core.sendEdit(target: target, targetSentAtMs: targetSentAt, newBody: trimmed, sentAtMs: nowMs)
        }
    }

    /// Delete a message (docs/36). `forEveryone` tombstones for both sides (own
    /// messages only); otherwise removes it from this device. DM only.
    func deleteMessage(message: Message, forEveryone: Bool, conversation: Conversation) {
        guard let core = cores[conversation.accountId],
              let target = messageTarget(for: conversation) else { return }
        let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
        let convId = conversation.id
        if var msgs = messagesByConversation[convId] {
            if forEveryone {
                if let i = msgs.firstIndex(where: { $0.id == message.id }) {
                    msgs[i].body = ""
                    msgs[i].isDeleted = true
                    msgs[i].editedAtMs = nil
                }
            } else {
                msgs.removeAll { $0.id == message.id }
            }
            messagesByConversation[convId] = msgs
        }
        reactionsByConversation[convId]?.removeAll { $0.targetAuthor == message.senderAccountId && $0.targetSentAtMs == message.sentAtMs }
        let targetAuthor = message.senderAccountId
        let targetSentAt = message.sentAtMs
        Task.detached {
            try? core.sendDelete(target: target, targetAuthor: targetAuthor, targetSentAtMs: targetSentAt, forEveryone: forEveryone, sentAtMs: nowMs)
        }
    }

    /// Load the prior bodies of an edited message for the history sheet (docs/36).
    func loadMessageRevisions(message: Message, conversation: Conversation) async -> [MessageRevisionFfi] {
        guard let core = cores[conversation.accountId] else { return [] }
        let convId = conversation.id
        let author = message.senderAccountId
        let sentAt = message.sentAtMs
        return (try? await Task.detached {
            try core.loadMessageRevisions(conversationId: convId, author: author, sentAtMs: sentAt)
        }.value) ?? []
    }

    // Inbound op handlers — the store is already updated by app-core; these
    // patch the in-memory model so the open conversation refreshes live.

    private func applyInboundEdit(conversationId: String, authorDid: String, sentAtMs: Int64, newBody: String, editedAtMs: Int64) {
        guard var msgs = messagesByConversation[conversationId],
              let i = msgs.firstIndex(where: { $0.senderAccountId == authorDid && $0.sentAtMs == sentAtMs }),
              !msgs[i].isDeleted else { return }
        msgs[i].body = newBody
        msgs[i].editedAtMs = editedAtMs
        msgs[i].editCount += 1
        messagesByConversation[conversationId] = msgs
    }

    private func applyInboundDelete(conversationId: String, authorDid: String, sentAtMs: Int64) {
        if var msgs = messagesByConversation[conversationId],
           let i = msgs.firstIndex(where: { $0.senderAccountId == authorDid && $0.sentAtMs == sentAtMs }) {
            msgs[i].body = ""
            msgs[i].isDeleted = true
            msgs[i].editedAtMs = nil
            messagesByConversation[conversationId] = msgs
        }
        reactionsByConversation[conversationId]?.removeAll { $0.targetAuthor == authorDid && $0.targetSentAtMs == sentAtMs }
    }

    private func applyInboundReaction(conversationId: String, targetAuthor: String, targetSentAtMs: Int64, reactorDid: String, emoji: String, removed: Bool) {
        var list = reactionsByConversation[conversationId] ?? []
        list.removeAll { $0.targetAuthor == targetAuthor && $0.targetSentAtMs == targetSentAtMs && $0.reactorDid == reactorDid }
        if !removed {
            let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
            list.append(ReactionFfi(conversationId: conversationId, targetAuthor: targetAuthor, targetSentAtMs: targetSentAtMs, reactorDid: reactorDid, emoji: emoji, reactedAtMs: nowMs))
        }
        reactionsByConversation[conversationId] = list
    }

    private func handleIncomingMessage(_ msg: DecryptedMessage, accountId: String) {
        let senderDid = msg.senderDid
        let text = String(data: msg.plaintext, encoding: .utf8) ?? "(binary)"

        var convId: String

        if let groupId = msg.groupId {
            // Group message: route to the group thread, creating one on the
            // fly if a GroupContext DM hasn't surfaced yet. The Conversation
            // row's title is filled in lazily by `refreshGroupTitle`.
            let serverUrl = accounts.first(where: { $0.id == accountId })?.servers.first?.id ?? ""
            let title = groupTitleCache[groupId] ?? "Group"
            let conv = findOrCreateGroupConversation(
                groupId: groupId,
                title: title,
                accountId: accountId,
                serverUrl: serverUrl
            )
            convId = conv.id
            if let idx = conversations.firstIndex(where: { $0.id == convId }) {
                conversations[idx].lastMessage = text
                conversations[idx].lastMessageDate = Date()
                conversations[idx].lastMessageSenderDid = senderDid
                conversations[idx].clearLastMessageEvent()
            }
            // Ensure we render the right title once state has been fetched.
            refreshGroupTitle(groupId: groupId, accountId: accountId)
        } else if let idx = conversations.firstIndex(where: {
            $0.accountId == accountId && $0.recipientDid == senderDid
        }) {
            convId = conversations[idx].id
            conversations[idx].lastMessage = text
            conversations[idx].lastMessageDate = Date()
            conversations[idx].lastMessageSenderDid = senderDid
            conversations[idx].clearLastMessageEvent()
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
                lastMessageSenderDid: senderDid,
                isGroup: false
            )
            conversations.append(conv)
        }

        // Use sender's timestamp if available, otherwise fall back to local time.
        let sentAtMs: Int64 = msg.sentAtMs ?? Int64(Date().timeIntervalSince1970 * 1000)
        let messageId = UUID().uuidString
        // Incoming messages are unread (readAtMs = nil). Carry the sender's
        // disappearing-messages timer (docs/03 §5) so the live-expiry scheduler
        // sees it once the message is read.
        let message = Message(
            id: messageId,
            conversationId: convId,
            senderAccountId: senderDid,
            body: text,
            sentAtMs: sentAtMs,
            readAtMs: nil,
            deliveryStatus: .sent,
            expireTimerSecs: msg.expireTimerSecs
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
                deliveryStatus: 1,  // sent
                editCount: 0,
                deleted: false,
                kind: 0,
                metadata: nil,
                // Carry the sender-stamped timer (docs/03 §5); the store starts
                // the countdown when this message is marked read.
                expireTimerSecs: msg.expireTimerSecs,
                expireAtMs: nil
            )
            Task.detached { try? core.saveMessage(msg: stored) }

            // Contact/profile metadata is now an explicit client opt-in:
            // app-core decrypts and surfaces the message but no longer writes
            // these on receive (keeps non-UI clients like bots from persisting
            // metadata they don't need). Mirror the old behavior here.
            //  - recency bump (non-curating) so the People list / sort updates,
            //  - pending-request flag when the gate flagged this as a request,
            //  - fetch + cache the sender's display name if a profile_key rode
            //    along (blocks on the network, hence detached).
            let profileKey = msg.profileKey
            let isRequest = msg.isRequest
            Task.detached {
                try? core.touchContact(did: senderDid, curated: false)
                if isRequest {
                    try? core.setPendingRequest(did: senderDid, pending: true)
                }
                if let pk = profileKey {
                    try? core.fetchAndCacheProfile(did: senderDid, profileKey: pk)
                }
            }
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

    private static func persistedDbFilename(did: String) -> String? {
        loadPersistedAccounts().first(where: { $0.did == did })?.dbFilename
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

#if DEBUG
extension AppState {
    /// Build an `AppState` wired for SwiftUI previews: mock service, the given
    /// accounts, and an in-memory core per account that serves the supplied
    /// contact rows. No network, no DB. `botNames` maps a DID to a server-side
    /// name so previews can exercise the bot resolution path (`resolvedName`),
    /// just like the real app.
    static func preview(
        accounts: [Account],
        contacts: [ContactRowFfi] = [],
        botNames: [String: String] = [:],
        groups: [String: GroupSummaryFfi] = [:]
    ) -> AppState {
        let state = AppState(mode: .mock)
        state.accounts = accounts
        for account in accounts {
            state.cores[account.id] = PreviewAppCore(
                did: account.id, contacts: contacts, botNames: botNames, groups: groups
            )
        }
        return state
    }
}

/// Minimal `AppCoreProtocol` for previews: serves canned contacts and resolves
/// bot names server-side. Everything else falls through to the protocol
/// defaults in `AppCoreProtocol+Defaults.swift`.
final class PreviewAppCore: AppCoreProtocol, @unchecked Sendable {
    private let mockDid: String
    private let contacts: [ContactRowFfi]
    private let botNames: [String: String]
    private let groups: [String: GroupSummaryFfi]

    init(
        did: String,
        contacts: [ContactRowFfi],
        botNames: [String: String],
        groups: [String: GroupSummaryFfi] = [:]
    ) {
        self.mockDid = did
        self.contacts = contacts
        self.botNames = botNames
        self.groups = groups
    }

    func did() -> String { mockDid }
    func listContacts() throws -> [ContactRowFfi] { contacts }

    func getAccountInfo(did: String) throws -> AccountInfoFfi {
        if let name = botNames[did] {
            return AccountInfoFfi(did: did, displayName: name, isBot: true)
        }
        return AccountInfoFfi(did: did, displayName: nil, isBot: false)
    }

    /// Resolve human member names in previews from the canned contact rows
    /// (the real core reads its local `contact_profiles` cache here).
    func contactDisplayName(did: String) throws -> String {
        contacts.first(where: { $0.did == did })?.displayName ?? ""
    }

    func fetchGroupState(groupId: String) throws -> GroupSummaryFfi {
        groups[groupId] ?? GroupSummaryFfi(
            groupId: groupId, masterKey: Data(count: 32), revision: 0,
            title: "Group", description: "", expirySeconds: 0,
            members: [], pendingInvites: [], pendingApprovals: []
        )
    }
}
#endif
