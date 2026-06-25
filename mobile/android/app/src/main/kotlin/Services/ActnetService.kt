package net.theavalanche.app

import uniffi.app_core.AccountInfoFfi
import uniffi.app_core.AppCore
import uniffi.app_core.AppErrorFfi
import uniffi.app_core.ConnectionState
import uniffi.app_core.ContactRowFfi
import uniffi.app_core.ConversationSummaryFfi
import uniffi.app_core.CreatedGroupFfi
import uniffi.app_core.DecryptedMessage
import uniffi.app_core.GroupSummaryFfi
import uniffi.app_core.IncomingEvent
import uniffi.app_core.JoinResultFfi
import uniffi.app_core.MessageRevisionFfi
import uniffi.app_core.MessageTarget
import uniffi.app_core.PreparedAccount
import uniffi.app_core.ProjectInfoFfi
import uniffi.app_core.ReactionFfi
import uniffi.app_core.StoredMessageFfi

// ---------------------------------------------------------------------------
// ActnetService — top-level seam between UI and the Rust core.
//
// Mirrors iOS Sources/Services/ActnetService.swift (the `ActnetService`
// protocol + the `AppCoreProtocol` definition that lives alongside it in
// the same Swift file hierarchy).
//
// Responsibilities:
//  • createAccount / login / prepareAccount / finalizeAccount / recoverFromBlob
//    — these are account-lifecycle operations. The live implementation calls
//    AppCore static constructors on Dispatchers.IO.
//  • Every other method is on AppCoreProtocol, which wraps an open AppCore
//    instance returned by one of the constructors above.
//
// Default method bodies below mirror AppCoreProtocol+Defaults.swift so that
// MockActnetService only needs to override what it cares about.
// ---------------------------------------------------------------------------

/**
 * Abstraction over the Rust AppCore for account creation/login.
 *
 * The [AppCoreProtocol] instance returned by [createAccount]/[login]/[finalizeAccount]/
 * [recoverFromBlob] handles all subsequent operations (send, receive, etc.)
 * matching the UniFFI-generated interface.
 *
 * Mirrors iOS `ActnetService` protocol.
 */
interface ActnetService {
    /**
     * Create a new account. [prfOutput] is the raw 32-byte WebAuthn PRF output
     * from a passkey ceremony (or the hash of a recovery phrase). Pass an empty
     * ByteArray to skip recovery setup (random rotation key, no blob — identity
     * is unrecoverable on device loss).
     *
     * [displayName] is the user's chosen display name; encrypted under a freshly
     * generated profile key and uploaded alongside registration.
     */
    @Throws(AppErrorFfi::class)
    fun createAccount(
        serverUrl: String,
        dbPath: String,
        dbKey: String,
        prfOutput: ByteArray,
        displayName: String,
        inviteToken: String?,
    ): AppCoreProtocol

    @Throws(AppErrorFfi::class)
    fun login(dbPath: String, dbKey: String): AppCoreProtocol

    /**
     * Two-stage account creation — stage 1. Derives rotation key + DID from
     * [prfOutput] without contacting the homeserver. Returns a [PreparedAccount]
     * handle whose [PreparedAccount.did] is already settled.
     */
    @Throws(AppErrorFfi::class)
    fun prepareAccount(serverUrl: String, prfOutput: ByteArray): PreparedAccount

    /**
     * Two-stage account creation — stage 2. Submits PLC ops, uploads the
     * recovery blob, and registers the account. Consumes [prepared].
     */
    @Throws(AppErrorFfi::class)
    fun finalizeAccount(
        prepared: PreparedAccount,
        dbPath: String,
        dbKey: String,
        displayName: String,
        inviteToken: String?,
    ): AppCoreProtocol

    /**
     * Recover an account from a passkey-protected recovery blob. Downloads the
     * blob, decrypts with the PRF-derived key, replaces the old device on the
     * home server, and returns an [AppCoreProtocol] bound to a fresh local store.
     */
    @Throws(AppErrorFfi::class)
    fun recoverFromBlob(
        serverUrl: String,
        did: String,
        prfOutput: ByteArray,
        dbPath: String,
        dbKey: String,
        displayName: String,
    ): AppCoreProtocol
}

// ---------------------------------------------------------------------------
// AppCoreProtocol — the per-account interface that all UI code drives.
//
// Mirrors the Swift AppCoreProtocol (which is the UniFFI-generated AppCore
// class in the real implementation). All methods are synchronous; callers
// MUST dispatch to Dispatchers.IO before calling.
//
// Default method bodies keep MockActnetService cheap — override only what the
// test/preview cares about. Real implementation delegates straight to AppCore.
// ---------------------------------------------------------------------------

/**
 * Per-session handle to the Rust core after login/registration.
 *
 * All methods are synchronous and BLOCKING — always call from Dispatchers.IO.
 * The live implementation ([LiveAppCoreProtocol]) wraps an [AppCore] and
 * delegates every call to it.
 */
interface AppCoreProtocol {

    // -----------------------------------------------------------------------
    // Identity / account
    // -----------------------------------------------------------------------

    @Throws(AppErrorFfi::class) fun did(): String
    @Throws(AppErrorFfi::class) fun deviceId(): UInt

    @Throws(AppErrorFfi::class) fun getAccountInfo(did: String): AccountInfoFfi
    @Throws(AppErrorFfi::class) fun ownDisplayName(): String
    @Throws(AppErrorFfi::class) fun setDisplayName(displayName: String)
    @Throws(AppErrorFfi::class) fun contactDisplayName(did: String): String
    @Throws(AppErrorFfi::class) fun refreshContactProfile(did: String): Boolean
    @Throws(AppErrorFfi::class) fun primeContactProfile(did: String, displayName: String, profileKey: ByteArray)
    @Throws(AppErrorFfi::class) fun listContacts(): List<ContactRowFfi>
    @Throws(AppErrorFfi::class) fun touchContact(did: String, curated: Boolean)

    // -----------------------------------------------------------------------
    // Abuse handling (docs/12-abuse-handling.md)
    // -----------------------------------------------------------------------

    @Throws(AppErrorFfi::class) fun blockContact(did: String)
    @Throws(AppErrorFfi::class) fun unblockContact(did: String)
    @Throws(AppErrorFfi::class) fun listBlocked(): List<ContactRowFfi>
    @Throws(AppErrorFfi::class) fun acceptRequest(did: String)
    @Throws(AppErrorFfi::class) fun deleteRequest(did: String)
    @Throws(AppErrorFfi::class) fun setPendingRequest(did: String, pending: Boolean)
    @Throws(AppErrorFfi::class) fun fetchAndCacheProfile(did: String, profileKey: ByteArray)
    @Throws(AppErrorFfi::class) fun reportAndBlock(did: String, reason: String)

    fun hasRecovery(): Boolean
    @Throws(AppErrorFfi::class) fun updateRecoveryBlob(prfOutput: ByteArray, servers: List<String>)

    // -----------------------------------------------------------------------
    // Account lifecycle (docs/53-multi-account-ux.md)
    // -----------------------------------------------------------------------

    @Throws(AppErrorFfi::class) fun leaveServer()
    @Throws(AppErrorFfi::class) fun deleteIdentity()

    // -----------------------------------------------------------------------
    // Storage sync (docs/05-device-data-sync.md)
    // -----------------------------------------------------------------------

    @Throws(AppErrorFfi::class) fun syncStorage()

    // -----------------------------------------------------------------------
    // Messaging
    // -----------------------------------------------------------------------

    @Throws(AppErrorFfi::class) fun sendDm(recipientDid: String, plaintext: ByteArray, sentAtMs: Long)
    @Throws(AppErrorFfi::class) fun sendMessage(target: MessageTarget, plaintext: ByteArray, sentAtMs: Long)
    @Throws(AppErrorFfi::class) fun sendReadReceipt(recipientDid: String, timestamps: List<Long>)
    @Throws(AppErrorFfi::class) fun receiveMessages(): List<DecryptedMessage>

    @Throws(AppErrorFfi::class) fun saveMessage(msg: StoredMessageFfi)
    @Throws(AppErrorFfi::class) fun loadMessages(conversationId: String): List<StoredMessageFfi>
    @Throws(AppErrorFfi::class) fun loadLastMessage(conversationId: String): StoredMessageFfi?
    @Throws(AppErrorFfi::class) fun loadConversations(): List<ConversationSummaryFfi>
    @Throws(AppErrorFfi::class) fun markMessagesRead(conversationId: String, upToSentAtMs: Long): ULong
    @Throws(AppErrorFfi::class) fun unreadCount(conversationId: String): ULong
    @Throws(AppErrorFfi::class) fun getConversationTimer(conversationId: String): UInt?
    @Throws(AppErrorFfi::class) fun setConversationTimer(recipientDid: String, expirySecs: UInt?)

    // -----------------------------------------------------------------------
    // Reactions / editing / deletion (docs/33, docs/36)
    // -----------------------------------------------------------------------

    @Throws(AppErrorFfi::class)
    fun sendReaction(
        target: MessageTarget,
        targetAuthor: String,
        targetSentAtMs: Long,
        emoji: String,
        remove: Boolean,
        sentAtMs: Long,
    )

    @Throws(AppErrorFfi::class)
    fun sendEdit(target: MessageTarget, targetSentAtMs: Long, newBody: String, sentAtMs: Long)

    @Throws(AppErrorFfi::class)
    fun sendDelete(
        target: MessageTarget,
        targetAuthor: String,
        targetSentAtMs: Long,
        forEveryone: Boolean,
        sentAtMs: Long,
    )

    @Throws(AppErrorFfi::class) fun loadReactions(conversationId: String): List<ReactionFfi>

    @Throws(AppErrorFfi::class)
    fun loadMessageRevisions(
        conversationId: String,
        author: String,
        sentAtMs: Long,
    ): List<MessageRevisionFfi>

    // -----------------------------------------------------------------------
    // Projects / push
    // -----------------------------------------------------------------------

    @Throws(AppErrorFfi::class) fun fetchProjects(): List<ProjectInfoFfi>
    @Throws(AppErrorFfi::class) fun requestProjectToken(projectUrl: String): String

    @Throws(AppErrorFfi::class)
    fun registerPushToken(
        deviceToken: String,
        platform: String,
        relayUrl: String,
        environment: String,
    )

    /** Deregister this device's push token (e.g. on logout). Best-effort. */
    @Throws(AppErrorFfi::class)
    fun unregisterPushToken(relayUrl: String)

    // -----------------------------------------------------------------------
    // Connection state / events
    // -----------------------------------------------------------------------

    fun connectionState(): ConnectionState

    /**
     * Blocks until the connection state changes from [last]. Suitable for a
     * background thread poll loop.
     */
    @Throws(AppErrorFfi::class) fun waitForConnectionStateChange(last: ConnectionState): ConnectionState

    /**
     * Returns any newly arrived events. Blocks briefly if the queue is empty.
     */
    @Throws(AppErrorFfi::class) fun nextEvents(): List<IncomingEvent>

    // -----------------------------------------------------------------------
    // Groups (docs/03-groups.md §5)
    // -----------------------------------------------------------------------

    @Throws(AppErrorFfi::class) fun createGroup(title: String, description: String, expirySeconds: UInt): CreatedGroupFfi
    @Throws(AppErrorFfi::class) fun fetchGroupState(groupId: String): GroupSummaryFfi
    @Throws(AppErrorFfi::class) fun cachedGroupState(groupId: String): GroupSummaryFfi?
    @Throws(AppErrorFfi::class) fun inviteMember(groupId: String, recipientDid: String, role: Short)
    @Throws(AppErrorFfi::class) fun acceptInvite(groupId: String)
    @Throws(AppErrorFfi::class) fun declineInvite(groupId: String)
    @Throws(AppErrorFfi::class) fun joinViaLink(masterKey: ByteArray, hostingServerUrl: String, password: ByteArray): JoinResultFfi
    @Throws(AppErrorFfi::class) fun cancelJoinRequest(groupId: String)
    @Throws(AppErrorFfi::class) fun approveJoinRequest(groupId: String, encryptedMemberId: String)
    @Throws(AppErrorFfi::class) fun denyJoinRequest(groupId: String, encryptedMemberId: String)
    @Throws(AppErrorFfi::class) fun removeMember(groupId: String, encryptedMemberId: String)
    @Throws(AppErrorFfi::class) fun leaveGroup(groupId: String)
    @Throws(AppErrorFfi::class) fun isGroupMember(groupId: String): Boolean
    @Throws(AppErrorFfi::class) fun changeMemberRole(groupId: String, encryptedMemberId: String, newRole: Short)
    @Throws(AppErrorFfi::class) fun setGroupExpiry(groupId: String, expirySeconds: UInt)
    @Throws(AppErrorFfi::class) fun setGroupTitle(groupId: String, newTitle: String)
    @Throws(AppErrorFfi::class) fun groupExpirySeconds(groupId: String): UInt
    @Throws(AppErrorFfi::class) fun deleteExpiredMessages(): List<String>
    @Throws(AppErrorFfi::class) fun applyPendingGroupChanges(groupId: String): Long
    @Throws(AppErrorFfi::class) fun listGroups(): List<String>
    @Throws(AppErrorFfi::class) fun rotateGroupPseudonym(groupId: String): ByteArray
    @Throws(AppErrorFfi::class) fun sendGroupMessage(groupId: String, plaintext: ByteArray, sentAtMs: Long)
}

// ---------------------------------------------------------------------------
// Live implementation — thin delegation to the UniFFI AppCore object.
// All FFI calls are synchronous and BLOCKING; callers use Dispatchers.IO.
// ---------------------------------------------------------------------------

/**
 * Live [AppCoreProtocol] backed by a real [AppCore] from the UniFFI bindings.
 */
class LiveAppCoreProtocol(private val core: AppCore) : AppCoreProtocol {

    override fun did(): String = core.did()
    override fun deviceId(): UInt = core.deviceId()

    override fun getAccountInfo(did: String): AccountInfoFfi = core.getAccountInfo(did)
    override fun ownDisplayName(): String = core.ownDisplayName()
    override fun setDisplayName(displayName: String) = core.setDisplayName(displayName)
    override fun contactDisplayName(did: String): String = core.contactDisplayName(did)
    override fun refreshContactProfile(did: String): Boolean = core.refreshContactProfile(did)
    override fun primeContactProfile(did: String, displayName: String, profileKey: ByteArray) =
        core.primeContactProfile(did, displayName, profileKey)
    override fun listContacts(): List<ContactRowFfi> = core.listContacts()
    override fun touchContact(did: String, curated: Boolean) = core.touchContact(did, curated)

    override fun blockContact(did: String) = core.blockContact(did)
    override fun unblockContact(did: String) = core.unblockContact(did)
    override fun listBlocked(): List<ContactRowFfi> = core.listBlocked()
    override fun acceptRequest(did: String) = core.acceptRequest(did)
    override fun deleteRequest(did: String) = core.deleteRequest(did)
    override fun setPendingRequest(did: String, pending: Boolean) = core.setPendingRequest(did, pending)
    override fun fetchAndCacheProfile(did: String, profileKey: ByteArray) = core.fetchAndCacheProfile(did, profileKey)
    override fun reportAndBlock(did: String, reason: String) = core.reportAndBlock(did, reason)

    override fun hasRecovery(): Boolean = core.hasRecovery()
    override fun updateRecoveryBlob(prfOutput: ByteArray, servers: List<String>) =
        core.updateRecoveryBlob(prfOutput, servers)

    override fun leaveServer() = core.leaveServer()
    override fun deleteIdentity() = core.deleteIdentity()

    override fun syncStorage() = core.syncStorage()

    override fun sendDm(recipientDid: String, plaintext: ByteArray, sentAtMs: Long) =
        core.sendDm(recipientDid, plaintext, sentAtMs)
    override fun sendMessage(target: MessageTarget, plaintext: ByteArray, sentAtMs: Long) =
        core.sendMessage(target, plaintext, sentAtMs)
    override fun sendReadReceipt(recipientDid: String, timestamps: List<Long>) =
        core.sendReadReceipt(recipientDid, timestamps)
    override fun receiveMessages(): List<DecryptedMessage> = core.receiveMessages()

    override fun saveMessage(msg: StoredMessageFfi) = core.saveMessage(msg)
    override fun loadMessages(conversationId: String): List<StoredMessageFfi> = core.loadMessages(conversationId)
    override fun loadLastMessage(conversationId: String): StoredMessageFfi? = core.loadLastMessage(conversationId)
    override fun loadConversations(): List<ConversationSummaryFfi> = core.loadConversations()
    override fun markMessagesRead(conversationId: String, upToSentAtMs: Long): ULong =
        core.markMessagesRead(conversationId, upToSentAtMs)
    override fun unreadCount(conversationId: String): ULong = core.unreadCount(conversationId)
    override fun getConversationTimer(conversationId: String): UInt? = core.getConversationTimer(conversationId)
    override fun setConversationTimer(recipientDid: String, expirySecs: UInt?) =
        core.setConversationTimer(recipientDid, expirySecs)

    override fun sendReaction(
        target: MessageTarget,
        targetAuthor: String,
        targetSentAtMs: Long,
        emoji: String,
        remove: Boolean,
        sentAtMs: Long,
    ) = core.sendReaction(target, targetAuthor, targetSentAtMs, emoji, remove, sentAtMs)

    override fun sendEdit(target: MessageTarget, targetSentAtMs: Long, newBody: String, sentAtMs: Long) =
        core.sendEdit(target, targetSentAtMs, newBody, sentAtMs)

    override fun sendDelete(
        target: MessageTarget,
        targetAuthor: String,
        targetSentAtMs: Long,
        forEveryone: Boolean,
        sentAtMs: Long,
    ) = core.sendDelete(target, targetAuthor, targetSentAtMs, forEveryone, sentAtMs)

    override fun loadReactions(conversationId: String): List<ReactionFfi> = core.loadReactions(conversationId)
    override fun loadMessageRevisions(conversationId: String, author: String, sentAtMs: Long): List<MessageRevisionFfi> =
        core.loadMessageRevisions(conversationId, author, sentAtMs)

    override fun fetchProjects(): List<ProjectInfoFfi> = core.fetchProjects()
    override fun requestProjectToken(projectUrl: String): String = core.requestProjectToken(projectUrl)
    override fun registerPushToken(deviceToken: String, platform: String, relayUrl: String, environment: String) =
        core.registerPushToken(deviceToken, platform, relayUrl, environment)
    override fun unregisterPushToken(relayUrl: String) = core.unregisterPushToken(relayUrl)

    override fun connectionState(): ConnectionState = core.connectionState()
    override fun waitForConnectionStateChange(last: ConnectionState): ConnectionState =
        core.waitForConnectionStateChange(last)
    override fun nextEvents(): List<IncomingEvent> = core.nextEvents()

    override fun createGroup(title: String, description: String, expirySeconds: UInt): CreatedGroupFfi =
        core.createGroup(title, description, expirySeconds)
    override fun fetchGroupState(groupId: String): GroupSummaryFfi = core.fetchGroupState(groupId)
    override fun cachedGroupState(groupId: String): GroupSummaryFfi? = core.cachedGroupState(groupId)
    override fun inviteMember(groupId: String, recipientDid: String, role: Short) =
        core.inviteMember(groupId, recipientDid, role)
    override fun acceptInvite(groupId: String) = core.acceptInvite(groupId)
    override fun declineInvite(groupId: String) = core.declineInvite(groupId)
    override fun joinViaLink(masterKey: ByteArray, hostingServerUrl: String, password: ByteArray): JoinResultFfi =
        core.joinViaLink(masterKey, hostingServerUrl, password)
    override fun cancelJoinRequest(groupId: String) = core.cancelJoinRequest(groupId)
    override fun approveJoinRequest(groupId: String, encryptedMemberId: String) =
        core.approveJoinRequest(groupId, encryptedMemberId)
    override fun denyJoinRequest(groupId: String, encryptedMemberId: String) =
        core.denyJoinRequest(groupId, encryptedMemberId)
    override fun removeMember(groupId: String, encryptedMemberId: String) =
        core.removeMember(groupId, encryptedMemberId)
    override fun leaveGroup(groupId: String) = core.leaveGroup(groupId)
    override fun isGroupMember(groupId: String): Boolean = core.isGroupMember(groupId)
    override fun changeMemberRole(groupId: String, encryptedMemberId: String, newRole: Short) =
        core.changeMemberRole(groupId, encryptedMemberId, newRole)
    override fun setGroupExpiry(groupId: String, expirySeconds: UInt) = core.setGroupExpiry(groupId, expirySeconds)
    override fun setGroupTitle(groupId: String, newTitle: String) = core.setGroupTitle(groupId, newTitle)
    override fun groupExpirySeconds(groupId: String): UInt = core.groupExpirySeconds(groupId)
    override fun deleteExpiredMessages(): List<String> = core.deleteExpiredMessages()
    override fun applyPendingGroupChanges(groupId: String): Long = core.applyPendingGroupChanges(groupId)
    override fun listGroups(): List<String> = core.listGroups()
    override fun rotateGroupPseudonym(groupId: String): ByteArray = core.rotateGroupPseudonym(groupId)
    override fun sendGroupMessage(groupId: String, plaintext: ByteArray, sentAtMs: Long) =
        core.sendGroupMessage(groupId, plaintext, sentAtMs)
}

// ---------------------------------------------------------------------------
// Live ActnetService implementation
// ---------------------------------------------------------------------------

/**
 * Live [ActnetService] that calls the UniFFI-generated [AppCore] static
 * constructors on a background thread.
 *
 * Callers (ViewModel) must already be on Dispatchers.IO — these are blocking.
 */
object LiveActnetService : ActnetService {

    @Throws(AppErrorFfi::class)
    override fun createAccount(
        serverUrl: String,
        dbPath: String,
        dbKey: String,
        prfOutput: ByteArray,
        displayName: String,
        inviteToken: String?,
    ): AppCoreProtocol =
        LiveAppCoreProtocol(
            AppCore.createAccount(serverUrl, dbPath, dbKey, prfOutput, displayName, inviteToken)
        )

    @Throws(AppErrorFfi::class)
    override fun login(dbPath: String, dbKey: String): AppCoreProtocol =
        LiveAppCoreProtocol(AppCore.login(dbPath, dbKey))

    @Throws(AppErrorFfi::class)
    override fun prepareAccount(serverUrl: String, prfOutput: ByteArray): PreparedAccount =
        PreparedAccount(serverUrl, prfOutput)

    @Throws(AppErrorFfi::class)
    override fun finalizeAccount(
        prepared: PreparedAccount,
        dbPath: String,
        dbKey: String,
        displayName: String,
        inviteToken: String?,
    ): AppCoreProtocol =
        LiveAppCoreProtocol(
            AppCore.finalizeAccount(prepared, dbPath, dbKey, displayName, inviteToken)
        )

    @Throws(AppErrorFfi::class)
    override fun recoverFromBlob(
        serverUrl: String,
        did: String,
        prfOutput: ByteArray,
        dbPath: String,
        dbKey: String,
        displayName: String,
    ): AppCoreProtocol =
        LiveAppCoreProtocol(
            AppCore.recoverFromBlob(serverUrl, did, prfOutput, dbPath, dbKey, displayName)
        )
}

// ---------------------------------------------------------------------------
// MockActnetService — minimal stub for Compose previews and unit tests.
//
// Default method bodies mirror AppCoreProtocol+Defaults.swift. Override
// only what a specific preview/test needs.
// ---------------------------------------------------------------------------

/**
 * No-op [AppCoreProtocol] for previews and tests. Mirrors the default
 * implementations from iOS AppCoreProtocol+Defaults.swift — override only
 * the methods your preview/test cares about.
 */
open class MockAppCoreProtocol : AppCoreProtocol {

    override fun did(): String = "did:plc:mock"
    override fun deviceId(): UInt = 1u

    override fun getAccountInfo(did: String): AccountInfoFfi =
        AccountInfoFfi(did = did, displayName = null, isBot = false)

    override fun ownDisplayName(): String = ""
    override fun setDisplayName(displayName: String) {}
    override fun contactDisplayName(did: String): String = ""
    override fun refreshContactProfile(did: String): Boolean = false
    override fun primeContactProfile(did: String, displayName: String, profileKey: ByteArray) {}
    override fun listContacts(): List<ContactRowFfi> = emptyList()
    override fun touchContact(did: String, curated: Boolean) {}

    override fun blockContact(did: String) {}
    override fun unblockContact(did: String) {}
    override fun listBlocked(): List<ContactRowFfi> = emptyList()
    override fun acceptRequest(did: String) {}
    override fun deleteRequest(did: String) {}
    override fun setPendingRequest(did: String, pending: Boolean) {}
    override fun fetchAndCacheProfile(did: String, profileKey: ByteArray) {}
    override fun reportAndBlock(did: String, reason: String) {}

    override fun hasRecovery(): Boolean = false
    override fun updateRecoveryBlob(prfOutput: ByteArray, servers: List<String>) {}

    override fun leaveServer() {}
    override fun deleteIdentity() {}

    override fun syncStorage() {}

    override fun sendDm(recipientDid: String, plaintext: ByteArray, sentAtMs: Long) {}
    override fun sendMessage(target: MessageTarget, plaintext: ByteArray, sentAtMs: Long) {}
    override fun sendReadReceipt(recipientDid: String, timestamps: List<Long>) {}
    override fun receiveMessages(): List<DecryptedMessage> = emptyList()

    override fun saveMessage(msg: StoredMessageFfi) {}
    override fun loadMessages(conversationId: String): List<StoredMessageFfi> = emptyList()
    override fun loadLastMessage(conversationId: String): StoredMessageFfi? = null
    override fun loadConversations(): List<ConversationSummaryFfi> = emptyList()
    override fun markMessagesRead(conversationId: String, upToSentAtMs: Long): ULong = 0uL
    override fun unreadCount(conversationId: String): ULong = 0uL
    override fun getConversationTimer(conversationId: String): UInt? = null
    override fun setConversationTimer(recipientDid: String, expirySecs: UInt?) {}

    override fun sendReaction(
        target: MessageTarget,
        targetAuthor: String,
        targetSentAtMs: Long,
        emoji: String,
        remove: Boolean,
        sentAtMs: Long,
    ) {}

    override fun sendEdit(target: MessageTarget, targetSentAtMs: Long, newBody: String, sentAtMs: Long) {}

    override fun sendDelete(
        target: MessageTarget,
        targetAuthor: String,
        targetSentAtMs: Long,
        forEveryone: Boolean,
        sentAtMs: Long,
    ) {}

    override fun loadReactions(conversationId: String): List<ReactionFfi> = emptyList()
    override fun loadMessageRevisions(conversationId: String, author: String, sentAtMs: Long): List<MessageRevisionFfi> =
        emptyList()

    override fun fetchProjects(): List<ProjectInfoFfi> = emptyList()
    override fun requestProjectToken(projectUrl: String): String = ""
    override fun registerPushToken(deviceToken: String, platform: String, relayUrl: String, environment: String) {}
    override fun unregisterPushToken(relayUrl: String) {}

    override fun connectionState(): ConnectionState = ConnectionState.Connected

    // Blocks for a long time in the real impl; mock returns immediately.
    override fun waitForConnectionStateChange(last: ConnectionState): ConnectionState {
        // TODO(opus): in a real poll loop this blocks; mock just sleeps briefly.
        Thread.sleep(100)
        return ConnectionState.Connected
    }

    override fun nextEvents(): List<IncomingEvent> {
        Thread.sleep(100)
        return emptyList()
    }

    override fun createGroup(title: String, description: String, expirySeconds: UInt): CreatedGroupFfi =
        CreatedGroupFfi(
            groupId = "mock-group-${java.util.UUID.randomUUID().toString().take(8)}",
            masterKey = ByteArray(32),
        )

    override fun fetchGroupState(groupId: String): GroupSummaryFfi =
        GroupSummaryFfi(
            groupId = groupId,
            masterKey = ByteArray(32),
            revision = 0L,
            title = "Mock Group",
            description = "",
            expirySeconds = 0u,
            members = emptyList(),
            pendingInvites = emptyList(),
            pendingApprovals = emptyList(),
        )

    override fun cachedGroupState(groupId: String): GroupSummaryFfi? = null
    override fun inviteMember(groupId: String, recipientDid: String, role: Short) {}
    override fun acceptInvite(groupId: String) {}
    override fun declineInvite(groupId: String) {}
    override fun joinViaLink(masterKey: ByteArray, hostingServerUrl: String, password: ByteArray): JoinResultFfi =
        JoinResultFfi.MEMBER
    override fun cancelJoinRequest(groupId: String) {}
    override fun approveJoinRequest(groupId: String, encryptedMemberId: String) {}
    override fun denyJoinRequest(groupId: String, encryptedMemberId: String) {}
    override fun removeMember(groupId: String, encryptedMemberId: String) {}
    override fun leaveGroup(groupId: String) {}
    override fun isGroupMember(groupId: String): Boolean = true
    override fun changeMemberRole(groupId: String, encryptedMemberId: String, newRole: Short) {}
    override fun setGroupExpiry(groupId: String, expirySeconds: UInt) {}
    override fun setGroupTitle(groupId: String, newTitle: String) {}
    override fun groupExpirySeconds(groupId: String): UInt = 0u
    override fun deleteExpiredMessages(): List<String> = emptyList()
    override fun applyPendingGroupChanges(groupId: String): Long = 0L
    override fun listGroups(): List<String> = emptyList()
    override fun rotateGroupPseudonym(groupId: String): ByteArray = ByteArray(24)
    override fun sendGroupMessage(groupId: String, plaintext: ByteArray, sentAtMs: Long) {}
}

/**
 * No-op [ActnetService] that returns [MockAppCoreProtocol] instances.
 * Use as a DI provider for Compose previews.
 */
open class MockActnetService : ActnetService {
    override fun createAccount(
        serverUrl: String,
        dbPath: String,
        dbKey: String,
        prfOutput: ByteArray,
        displayName: String,
        inviteToken: String?,
    ): AppCoreProtocol = MockAppCoreProtocol()

    override fun login(dbPath: String, dbKey: String): AppCoreProtocol = MockAppCoreProtocol()

    override fun prepareAccount(serverUrl: String, prfOutput: ByteArray): PreparedAccount {
        // TODO(opus): PreparedAccount requires a real native pointer — cannot stub trivially.
        // Previews that trigger prepareAccount must use a real service or skip this path.
        throw UnsupportedOperationException("MockActnetService.prepareAccount is not supported in previews")
    }

    override fun finalizeAccount(
        prepared: PreparedAccount,
        dbPath: String,
        dbKey: String,
        displayName: String,
        inviteToken: String?,
    ): AppCoreProtocol = MockAppCoreProtocol()

    override fun recoverFromBlob(
        serverUrl: String,
        did: String,
        prfOutput: ByteArray,
        dbPath: String,
        dbKey: String,
        displayName: String,
    ): AppCoreProtocol = MockAppCoreProtocol()
}
