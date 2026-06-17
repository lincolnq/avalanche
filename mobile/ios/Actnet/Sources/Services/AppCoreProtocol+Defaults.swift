import Foundation

// Default implementations for every method on `AppCoreProtocol`.
//
// Purpose: keep mocks cheap. The real `AppCore` (UniFFI-generated) provides
// its own concrete implementations, which always take precedence over these
// defaults. Mocks (e.g. `MockAppCore`) only override the methods that drive
// the specific UI/preview behavior they care about — everything else falls
// through to a safe no-op here.
//
// When a new FFI method is added, drop a default below; existing mocks
// keep building untouched.

extension AppCoreProtocol {
    // MARK: - Identity / account

    func did() -> String { "did:plc:mock" }
    func deviceId() -> UInt32 { 1 }

    func getAccountInfo(did: String) throws -> AccountInfoFfi {
        AccountInfoFfi(did: did, displayName: nil, isBot: false)
    }

    func ownDisplayName() throws -> String { "" }
    func setDisplayName(displayName: String) throws {}
    func contactDisplayName(did: String) throws -> String { "" }
    func refreshContactProfile(did: String) throws -> Bool { false }
    func primeContactProfile(did: String, displayName: String, profileKey: Data) throws {}
    func listContacts() throws -> [ContactRowFfi] { [] }
    func touchContact(did: String, curated: Bool) throws {}

    // MARK: - Abuse handling (docs/12-abuse-handling.md)

    func blockContact(did: String) throws {}
    func unblockContact(did: String) throws {}
    func listBlocked() throws -> [ContactRowFfi] { [] }
    func acceptRequest(did: String) throws {}
    func deleteRequest(did: String) throws {}
    func reportAndBlock(did: String, reason: String) throws {}

    func hasRecovery() -> Bool { false }
    func updateRecoveryBlob(prfOutput: Data, servers: [String]) throws {}

    // MARK: - Storage sync (docs/05-device-data-sync.md)

    func syncStorage() throws {}

    // MARK: - Messaging

    func sendDm(recipientDid: String, plaintext: Data, sentAtMs: Int64) throws {}
    func sendMessage(target: MessageTarget, plaintext: Data, sentAtMs: Int64) throws {}
    func sendReadReceipt(recipientDid: String, timestamps: [Int64]) throws {}
    func receiveMessages() throws -> [DecryptedMessage] { [] }

    func saveMessage(msg: StoredMessageFfi) throws {}
    func loadMessages(conversationId: String) throws -> [StoredMessageFfi] { [] }
    func loadLastMessage(conversationId: String) throws -> StoredMessageFfi? { nil }
    func loadConversations() throws -> [ConversationSummaryFfi] { [] }
    func markMessagesRead(conversationId: String, upToSentAtMs: Int64) throws -> UInt64 { 0 }
    func unreadCount(conversationId: String) throws -> UInt64 { 0 }
    func getConversationTimer(conversationId: String) throws -> UInt32? { nil }
    func setConversationTimer(recipientDid: String, expirySecs: UInt32?) throws {}

    // MARK: - Reactions / editing / deletion (docs/33, docs/36)

    func sendReaction(target: MessageTarget, targetAuthor: String, targetSentAtMs: Int64, emoji: String, remove: Bool, sentAtMs: Int64) throws {}
    func sendEdit(target: MessageTarget, targetSentAtMs: Int64, newBody: String, sentAtMs: Int64) throws {}
    func sendDelete(target: MessageTarget, targetAuthor: String, targetSentAtMs: Int64, forEveryone: Bool, sentAtMs: Int64) throws {}
    func loadReactions(conversationId: String) throws -> [ReactionFfi] { [] }
    func loadMessageRevisions(conversationId: String, author: String, sentAtMs: Int64) throws -> [MessageRevisionFfi] { [] }

    // MARK: - Projects / push

    func fetchProjects() throws -> [ProjectInfoFfi] { [] }
    func requestProjectToken(projectUrl: String) throws -> String { "" }
    func registerPushToken(deviceToken: String, platform: String, relayUrl: String, environment: String) throws {}

    // MARK: - Connection state / events

    func connectionState() -> ConnectionState { .connected }
    func waitForConnectionStateChange(last: ConnectionState) throws -> ConnectionState {
        // Block "forever"; the real listener task is happy to sit on an
        // unresolved call.
        Thread.sleep(forTimeInterval: 60 * 60)
        return .connected
    }
    func nextEvents() throws -> [IncomingEvent] {
        Thread.sleep(forTimeInterval: 0.1)
        return []
    }

    // MARK: - Groups (docs/03-groups.md §5)

    func createGroup(title: String, description: String, expirySeconds: UInt32) throws -> CreatedGroupFfi {
        CreatedGroupFfi(groupId: "mock-group-\(UUID().uuidString.prefix(8))", masterKey: Data(count: 32))
    }

    func fetchGroupState(groupId: String) throws -> GroupSummaryFfi {
        GroupSummaryFfi(
            groupId: groupId,
            masterKey: Data(count: 32),
            revision: 0,
            title: "Mock Group",
            description: "",
            expirySeconds: 0,
            members: [],
            pendingInvites: [],
            pendingApprovals: []
        )
    }

    func inviteMember(groupId: String, recipientDid: String, role: Int16) throws {}
    func acceptInvite(groupId: String) throws {}
    func declineInvite(groupId: String) throws {}
    func joinViaLink(masterKey: Data, hostingServerUrl: String, password: Data) throws -> JoinResultFfi { .member }
    func cancelJoinRequest(groupId: String) throws {}
    func approveJoinRequest(groupId: String, encryptedMemberId: String) throws {}
    func denyJoinRequest(groupId: String, encryptedMemberId: String) throws {}
    func removeMember(groupId: String, encryptedMemberId: String) throws {}
    func changeMemberRole(groupId: String, encryptedMemberId: String, newRole: Int16) throws {}
    func applyPendingGroupChanges(groupId: String) throws -> Int64 { 0 }
    func rotateGroupPseudonym(groupId: String) throws -> Data { Data(count: 24) }
    func sendGroupMessage(groupId: String, plaintext: Data, sentAtMs: Int64) throws {}
}
