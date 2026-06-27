package net.theavalanche.app

import uniffi.app_core.AccountInfoFfi
import uniffi.app_core.ConversationSummaryFfi
import uniffi.app_core.DecryptedMessage
import uniffi.app_core.IncomingEvent
import uniffi.app_core.MessageRevisionFfi
import uniffi.app_core.MessageTarget
import uniffi.app_core.PreparedAccount
import uniffi.app_core.ProjectInfoFfi
import uniffi.app_core.ReactionFfi
import uniffi.app_core.StoredMessageFfi
import uniffi.app_core.ConnectionState
import java.util.Date
import java.util.UUID
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

// ---------------------------------------------------------------------------
// MockAppCore — rich in-memory AppCoreProtocol for UI development/tests.
//
// Mirrors iOS Sources/Services/MockActnetService.swift `MockAppCore`.
// Simulates registration, message echoes, reactions, edits, and deletions
// so Compose previews and instrumented tests can exercise real UI paths
// without a live Rust core.
// ---------------------------------------------------------------------------

/**
 * In-memory [AppCoreProtocol] for previews and tests.
 *
 * Simulates the full messaging surface (send/receive/react/edit/delete) in
 * process without touching the Rust library. Echoes every sent DM after a
 * short delay.
 */
class MockAppCore(
    did: String? = null,
    displayName: String = "",
) : MockAppCoreProtocol() {

    private val mockDid: String = did ?: "did:plc:mock${UUID.randomUUID().toString().take(8).lowercase()}"
    private val mockDeviceId: UInt = 1u
    private val lock = ReentrantLock()

    // Keyed by conversation_id
    private val storedMessages: MutableMap<String, MutableList<StoredMessageFfi>> = mutableMapOf()

    private val pendingMessages: MutableList<DecryptedMessage> = mutableListOf()
    private var nextMessageId: Long = 1L

    private var ownDisplayName_: String = displayName

    private val contactDisplayNames: MutableMap<String, String> = mutableMapOf()

    // Reactions keyed by conversation id
    private val reactionsByConv: MutableMap<String, MutableList<ReactionFfi>> = mutableMapOf()

    // Prior bodies keyed by "<convId>|<author>|<sentAt>"
    private val revisionsByTarget: MutableMap<String, MutableList<MessageRevisionFfi>> = mutableMapOf()

    // -----------------------------------------------------------------------
    // Identity
    // -----------------------------------------------------------------------

    override fun did(): String = mockDid
    override fun deviceId(): UInt = mockDeviceId
    override fun getAccountInfo(did: String): AccountInfoFfi =
        AccountInfoFfi(did = did, displayName = null, isBot = false)

    override fun ownDisplayName(): String = lock.withLock { ownDisplayName_ }
    override fun setDisplayName(displayName: String) { lock.withLock { ownDisplayName_ = displayName } }

    override fun contactDisplayName(did: String): String = lock.withLock {
        contactDisplayNames[did] ?: ""
    }

    override fun refreshContactProfile(did: String): Boolean = false

    override fun primeContactProfile(did: String, displayName: String, profileKey: ByteArray) {
        lock.withLock { contactDisplayNames[did] = displayName }
    }

    // -----------------------------------------------------------------------
    // Recovery (no-op in mock)
    // -----------------------------------------------------------------------

    override fun hasRecovery(): Boolean = false
    override fun updateRecoveryBlob(prfOutput: ByteArray, servers: List<String>) {}

    // -----------------------------------------------------------------------
    // Projects
    // -----------------------------------------------------------------------

    override fun fetchProjects(): List<ProjectInfoFfi> = listOf(
        ProjectInfoFfi(
            name = "Testbot",
            url = "http://localhost:3001",
            description = "Chat with an AI bot",
        )
    )

    override fun requestProjectToken(projectUrl: String): String =
        "mock-project-token-${UUID.randomUUID().toString().take(8)}"

    // -----------------------------------------------------------------------
    // Push (no-op)
    // -----------------------------------------------------------------------

    override fun registerPushToken(deviceToken: String, platform: String, relayUrl: String, environment: String) {}
    override fun unregisterPushToken(relayUrl: String) {}

    // -----------------------------------------------------------------------
    // Message storage
    // -----------------------------------------------------------------------

    override fun saveMessage(msg: StoredMessageFfi) {
        lock.withLock {
            storedMessages.getOrPut(msg.conversationId) { mutableListOf() }.add(msg)
        }
    }

    override fun loadMessages(conversationId: String): List<StoredMessageFfi> {
        return lock.withLock {
            (storedMessages[conversationId] ?: emptyList<StoredMessageFfi>()).toList()
        }.sortedBy { it.sentAtMs }
    }

    override fun loadLastMessage(conversationId: String): StoredMessageFfi? {
        return lock.withLock {
            storedMessages[conversationId]?.maxByOrNull { it.sentAtMs }
        }
    }

    override fun loadConversations(): List<ConversationSummaryFfi> {
        val snapshot = lock.withLock { storedMessages.toMap() }
        return snapshot.mapNotNull { (convId, msgs) ->
            val last = msgs.maxByOrNull { it.sentAtMs } ?: return@mapNotNull null
            ConversationSummaryFfi(
                conversationId = convId,
                groupTitle = null,
                lastMessage = last,
                isRequest = false,
                isBlocked = false,
            )
        }.sortedByDescending { it.lastMessage?.sentAtMs ?: 0L }
    }

    override fun markMessagesRead(conversationId: String, upToSentAtMs: Long): ULong {
        return lock.withLock {
            val msgs = storedMessages[conversationId] ?: return@withLock 0uL
            val now = System.currentTimeMillis()
            var count = 0uL
            msgs.replaceAll { msg ->
                if (msg.sentAtMs <= upToSentAtMs && msg.readAtMs == null && msg.senderDid != mockDid) {
                    count++
                    StoredMessageFfi(
                        id = msg.id,
                        conversationId = msg.conversationId,
                        senderDid = msg.senderDid,
                        body = msg.body,
                        sentAtMs = msg.sentAtMs,
                        editedAtMs = msg.editedAtMs,
                        readAtMs = now,
                        deliveryStatus = msg.deliveryStatus,
                        editCount = msg.editCount,
                        deleted = msg.deleted,
                        kind = msg.kind,
                        metadata = msg.metadata,
                        expireTimerSecs = msg.expireTimerSecs,
                        expireAtMs = msg.expireAtMs,
                    )
                } else {
                    msg
                }
            }
            count
        }
    }

    override fun sendReadReceipt(recipientDid: String, timestamps: List<Long>) {}

    override fun unreadCount(conversationId: String): ULong {
        return lock.withLock {
            val msgs = storedMessages[conversationId] ?: emptyList()
            msgs.count { it.readAtMs == null && it.senderDid != mockDid }.toULong()
        }
    }

    // -----------------------------------------------------------------------
    // Send DM — schedules an echo reply
    // -----------------------------------------------------------------------

    override fun sendDm(recipientDid: String, plaintext: ByteArray, sentAtMs: Long) {
        Thread.sleep(100) // simulate slight network delay
        val text = plaintext.toString(Charsets.UTF_8)
        Thread {
            Thread.sleep(1500)
            enqueueMessage(from = recipientDid, text = "Echo: $text")
        }.start()
    }

    // -----------------------------------------------------------------------
    // Reactions / editing / deletion (docs/33, docs/36)
    // -----------------------------------------------------------------------

    /** Local conversation id for a target — mirrors the core's `conv_id_for`. */
    private fun convId(target: MessageTarget): String = when (target) {
        is MessageTarget.Dm -> "dm-$mockDid-${target.recipientDid}"
        is MessageTarget.Group -> "group-${target.groupId}"
    }

    override fun sendReaction(
        target: MessageTarget,
        targetAuthor: String,
        targetSentAtMs: Long,
        emoji: String,
        remove: Boolean,
        sentAtMs: Long,
    ) {
        val cid = convId(target)
        lock.withLock {
            val list = reactionsByConv.getOrPut(cid) { mutableListOf() }
            // One reaction per (target, reactor=self): remove any prior, then re-add.
            list.removeAll { it.targetAuthor == targetAuthor && it.targetSentAtMs == targetSentAtMs && it.reactorDid == mockDid }
            if (!remove) {
                list.add(
                    ReactionFfi(
                        conversationId = cid,
                        targetAuthor = targetAuthor,
                        targetSentAtMs = targetSentAtMs,
                        reactorDid = mockDid,
                        emoji = emoji,
                        reactedAtMs = sentAtMs,
                    )
                )
            }
        }
    }

    override fun loadReactions(conversationId: String): List<ReactionFfi> {
        return lock.withLock { reactionsByConv[conversationId]?.toList() ?: emptyList() }
    }

    override fun sendEdit(target: MessageTarget, targetSentAtMs: Long, newBody: String, sentAtMs: Long) {
        val cid = convId(target)
        lock.withLock {
            val msgs = storedMessages[cid] ?: return@withLock
            msgs.replaceAll { msg ->
                if (msg.senderDid == mockDid && msg.sentAtMs == targetSentAtMs && !msg.deleted) {
                    revisionsByTarget
                        .getOrPut("$cid|$mockDid|$targetSentAtMs") { mutableListOf() }
                        .add(MessageRevisionFfi(body = msg.body, replacedAtMs = sentAtMs))
                    msg.copy(body = newBody, editedAtMs = sentAtMs, editCount = msg.editCount + 1u)
                } else {
                    msg
                }
            }
        }
    }

    override fun loadMessageRevisions(
        conversationId: String,
        author: String,
        sentAtMs: Long,
    ): List<MessageRevisionFfi> {
        return lock.withLock {
            revisionsByTarget["$conversationId|$author|$sentAtMs"]?.toList() ?: emptyList()
        }
    }

    override fun sendDelete(
        target: MessageTarget,
        targetAuthor: String,
        targetSentAtMs: Long,
        forEveryone: Boolean,
        sentAtMs: Long,
    ) {
        val cid = convId(target)
        lock.withLock {
            val msgs = storedMessages[cid] ?: return@withLock
            if (forEveryone) {
                msgs.replaceAll { msg ->
                    if (msg.senderDid == targetAuthor && msg.sentAtMs == targetSentAtMs) {
                        msg.copy(body = "", editedAtMs = null, deleted = true)
                    } else {
                        msg
                    }
                }
            } else {
                msgs.removeAll { it.senderDid == targetAuthor && it.sentAtMs == targetSentAtMs }
            }
            reactionsByConv[cid]?.removeAll { it.targetAuthor == targetAuthor && it.targetSentAtMs == targetSentAtMs }
        }
    }

    // -----------------------------------------------------------------------
    // Connection state / events
    // -----------------------------------------------------------------------

    override fun connectionState(): ConnectionState = ConnectionState.Connected

    override fun waitForConnectionStateChange(last: ConnectionState): ConnectionState {
        // Mock never changes — block for a long time so the listener loop idles.
        Thread.sleep(60 * 60 * 1000L)
        return ConnectionState.Connected
    }

    override fun nextEvents(): List<IncomingEvent> {
        // Poll for up to ~2 s, draining any pending messages.
        repeat(20) {
            Thread.sleep(100)
            val msgs = lock.withLock {
                if (pendingMessages.isEmpty()) null
                else {
                    val copy = pendingMessages.toList()
                    pendingMessages.clear()
                    copy
                }
            }
            if (msgs != null) {
                return msgs.map { IncomingEvent.Message(msg = it) }
            }
        }
        return emptyList()
    }

    override fun reconnectNow() {}
    override fun setAppActive(active: Boolean) {}

    override fun receiveMessages(): List<DecryptedMessage> {
        Thread.sleep(100)
        return lock.withLock {
            val copy = pendingMessages.toList()
            pendingMessages.clear()
            copy
        }
    }

    // -----------------------------------------------------------------------
    // Helper: enqueue a fake inbound message (used for echo replies)
    // -----------------------------------------------------------------------

    fun enqueueMessage(from: String, text: String) {
        lock.withLock {
            val now = System.currentTimeMillis()
            val msg = DecryptedMessage(
                serverId = nextMessageId++,
                senderDid = from,
                senderDeviceId = 1u,
                plaintext = text.toByteArray(Charsets.UTF_8),
                sentAtMs = now,
                groupId = null,
                expireTimerSecs = 0u,
                profileKey = null,
                isRequest = false,
            )
            pendingMessages.add(msg)
        }
    }
}

// ---------------------------------------------------------------------------
// MockPreparedAccount — fabricated DID for two-stage account creation.
//
// NOTE: [PreparedAccount] is a UniFFI-generated class backed by a native
// pointer and cannot be trivially instantiated in Kotlin without a live Rust
// library. This class serves as a stand-in for preview logic only; any code
// path that calls [LiveActnetService.prepareAccount] will use the real type.
// ---------------------------------------------------------------------------

// TODO(opus): PreparedAccount is a UniFFI object with a native pointer.
// We cannot construct a fake PreparedAccount without the library loaded.
// The MockActnetService.prepareAccount stub below throws UnsupportedOperationException
// to surface this clearly in tests rather than silently misbehaving.

// ---------------------------------------------------------------------------
// MockActnetService — rich ActnetService for previews and tests.
//
// Overrides the simple no-op MockActnetService defined in ActnetService.kt
// with one that returns a stateful MockAppCore so that multiple previews
// sharing the same service instance see consistent data.
// ---------------------------------------------------------------------------

/**
 * Stateful [ActnetService] for Compose previews and instrumented tests.
 *
 * Returns [MockAppCore] instances that carry in-memory state and simulate
 * DM echo replies. Mirrors iOS `MockActnetService` (Sources/Services/MockActnetService.swift).
 */
class RichMockActnetService : ActnetService {

    override fun createAccount(
        serverUrl: String,
        dbPath: String,
        dbKey: String,
        prfOutput: ByteArray,
        displayName: String,
        inviteToken: String?,
    ): AppCoreProtocol {
        Thread.sleep(500) // simulate network
        return MockAppCore(displayName = displayName)
    }

    override fun login(dbPath: String, dbKey: String): AppCoreProtocol = MockAppCore()

    /**
     * Not supported — [PreparedAccount] requires a live Rust native pointer
     * and cannot be fabricated in-process. Previews that exercise the
     * two-stage flow should use [LiveActnetService] with a dev server.
     */
    override fun prepareAccount(serverUrl: String, prfOutput: ByteArray): PreparedAccount {
        throw UnsupportedOperationException(
            "RichMockActnetService.prepareAccount is not supported — PreparedAccount requires a live Rust pointer."
        )
    }

    override fun finalizeAccount(
        prepared: PreparedAccount,
        dbPath: String,
        dbKey: String,
        displayName: String,
        inviteToken: String?,
    ): AppCoreProtocol {
        Thread.sleep(500)
        // TODO(opus): cannot read prepared.did() without the native library.
        return MockAppCore(displayName = displayName)
    }

    override fun recoverFromBlob(
        serverUrl: String,
        did: String,
        prfOutput: ByteArray,
        dbPath: String,
        dbKey: String,
        displayName: String,
    ): AppCoreProtocol {
        Thread.sleep(500)
        return MockAppCore(did = did, displayName = displayName)
    }

    override fun makeDeviceLink(): DeviceLink = MockDeviceLink()
}

// ---------------------------------------------------------------------------
// MockData — seed conversations for mock mode.
//
// Mirrors iOS `MockData.seedConversations(accountId:serverUrl:)`.
// ---------------------------------------------------------------------------

/**
 * Canned seed data for mock mode — conversations that appear after
 * registration in a preview or demo flow.
 */
object MockData {
    fun seedConversations(accountId: String, serverUrl: String): List<Conversation> {
        val now = Date()
        return listOf(
            Conversation(
                id = "conv-general",
                title = "General",
                accountId = accountId,
                serverUrl = serverUrl,
                lastMessage = "Welcome to the server!",
                lastMessageDate = Date(now.time - 60_000),
                isGroup = true,
            ),
            Conversation(
                id = "conv-announcements",
                title = "Announcements",
                accountId = accountId,
                serverUrl = serverUrl,
                lastMessage = "Rally this Saturday at 10am",
                lastMessageDate = Date(now.time - 3_600_000),
                isGroup = true,
            ),
            Conversation(
                id = "conv-dm-organizer",
                title = "Jamie (Organizer)",
                accountId = accountId,
                serverUrl = serverUrl,
                lastMessage = "Hey, welcome aboard!",
                lastMessageDate = Date(now.time - 120_000),
                isGroup = false,
            ),
        )
    }
}
