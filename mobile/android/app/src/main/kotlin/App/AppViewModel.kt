package net.theavalanche.app

import android.content.Context
import android.content.SharedPreferences
import android.content.Intent
import android.net.Uri
import androidx.compose.runtime.mutableStateMapOf
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import uniffi.app_core.ConnectionState
import uniffi.app_core.ContactRowFfi
import uniffi.app_core.DeliveryStatusUpdate
import uniffi.app_core.DecryptedMessage
import uniffi.app_core.GroupEventKind
import uniffi.app_core.IncomingEvent
import uniffi.app_core.MessageRevisionFfi
import uniffi.app_core.AttachmentFfi
import uniffi.app_core.LinkPreviewFfi
import uniffi.app_core.MessageTarget
import uniffi.app_core.PreparedAccount
import uniffi.app_core.ReactionFfi
import uniffi.app_core.StoredMessageFfi
import java.io.File
import java.util.Date
import java.util.UUID

// ---------------------------------------------------------------------------
// ServiceMode — mirrors iOS enum ServiceMode
// ---------------------------------------------------------------------------

enum class ServiceMode(val rawValue: String) {
    MOCK("Mock (no server)"),
    DEV_SERVER("Dev Server");

    companion object {
        fun fromRawValue(rawValue: String): ServiceMode? =
            values().firstOrNull { it.rawValue == rawValue }
    }
}

// ---------------------------------------------------------------------------
// Persistence model classes — private; not exposed outside this file.
// Mirrors iOS PersistedAccount / PersistedServer.
// ---------------------------------------------------------------------------

private data class PersistedServer(
    val id: String,
    val name: String,
    val url: String,
)

private data class PersistedAccount(
    val did: String,
    val displayName: String,
    val dbFilename: String,
    val servers: List<PersistedServer>,
)

// ---------------------------------------------------------------------------
// AppViewModel
//
// Mirrors iOS AppState (ObservableObject). Every @Published field becomes a
// public val StateFlow backed by a private MutableStateFlow. All async work
// runs in viewModelScope; FFI calls are dispatched to Dispatchers.IO.
// ---------------------------------------------------------------------------

/**
 * Top-level ViewModel that owns all app state. Mirrors iOS AppState.swift.
 *
 * Construct via the factory [AppViewModelFactory] so the application [Context]
 * (needed for file I/O and SharedPreferences) is available without leaking an
 * Activity.
 */
class AppViewModel(
    private val applicationContext: Context,
) : ViewModel() {

    // -----------------------------------------------------------------------
    // Tab enum — mirrors iOS AppState.Tab
    // -----------------------------------------------------------------------

    enum class Tab { CHATS, NETWORK }

    // -----------------------------------------------------------------------
    // @Published fields — each becomes a StateFlow
    // -----------------------------------------------------------------------

    private val _accounts = MutableStateFlow<List<Account>>(emptyList())
    val accounts: StateFlow<List<Account>> = _accounts.asStateFlow()

    // Seed from a cheap synchronous check (SharedPreferences read, no Keystore
    // or FFI): if we have persisted accounts we're almost certainly logged in,
    // so start straight on the chats scaffold (MAIN) while `restoreAccounts`
    // finishes in the background — instead of stalling on the splash for the
    // duration of the Keystore unlock + per-account login. Only when there are
    // no persisted accounts do we start on the splash. `restoreAccounts` flips
    // this back to onboarding if the restore actually yields no usable account.
    private val _isOnboarding = MutableStateFlow(loadPersistedAccounts().isEmpty())
    val isOnboarding: StateFlow<Boolean> = _isOnboarding.asStateFlow()

    private val _conversations = MutableStateFlow<List<Conversation>>(emptyList())
    val conversations: StateFlow<List<Conversation>> = _conversations.asStateFlow()

    /**
     * False until the first conversation load completes. Lets the chats list
     * distinguish "still loading on launch" (show a spinner) from "genuinely no
     * conversations" (show the empty state) — without this they're both an empty
     * list and the empty state flashes during restore.
     */
    private val _conversationsLoaded = MutableStateFlow(false)
    val conversationsLoaded: StateFlow<Boolean> = _conversationsLoaded.asStateFlow()

    private val _messagesByConversation = MutableStateFlow<Map<String, List<Message>>>(emptyMap())
    val messagesByConversation: StateFlow<Map<String, List<Message>>> =
        _messagesByConversation.asStateFlow()

    /**
     * Persisted unread count per conversation, seeded from the store on load
     * (`ConversationSummaryFfi.unreadCount`) and kept live as messages arrive or
     * are marked read. The chat-list badge reads this for conversations whose
     * transcript isn't cached in [messagesByConversation], so the badge is
     * correct even for conversations never opened this session.
     * Mirrors iOS AppState.unreadCounts.
     */
    private val _unreadCounts = MutableStateFlow<Map<String, Int>>(emptyMap())
    val unreadCounts: StateFlow<Map<String, Int>> = _unreadCounts.asStateFlow()

    /** Reactions per conversation (docs/33), keyed by conversation id. */
    private val _reactionsByConversation = MutableStateFlow<Map<String, List<ReactionFfi>>>(emptyMap())
    val reactionsByConversation: StateFlow<Map<String, List<ReactionFfi>>> =
        _reactionsByConversation.asStateFlow()

    private val _serviceMode = MutableStateFlow(ServiceMode.DEV_SERVER)
    val serviceMode: StateFlow<ServiceMode> = _serviceMode.asStateFlow()

    private val _selectedTab = MutableStateFlow(Tab.CHATS)
    val selectedTab: StateFlow<Tab> = _selectedTab.asStateFlow()

    private val _navigateToConversation = MutableStateFlow<Conversation?>(null)
    val navigateToConversation: StateFlow<Conversation?> = _navigateToConversation.asStateFlow()

    // An image shared into the app from another app (docs/35), awaiting a
    // destination chat. Non-null drives the share-destination picker. Android-only
    // for now — iOS share-in is blocked on the share-extension sandbox (docs/02).
    private val _pendingSharedImage = MutableStateFlow<PendingSharedImage?>(null)
    val pendingSharedImage: StateFlow<PendingSharedImage?> = _pendingSharedImage.asStateFlow()

    // Image bytes staged for a specific conversation by the share flow, consumed
    // (and cleared) by ConversationView's onAppear. Keyed by conversation id.
    private val _pendingStagedImage = MutableStateFlow<Map<String, ByteArray>>(emptyMap())
    val pendingStagedImage: StateFlow<Map<String, ByteArray>> = _pendingStagedImage.asStateFlow()

    /**
     * A notification tap that arrived before the conversation list finished
     * loading (cold-start launch). Stored as (conversationId, accountId) and
     * flushed by [openConversationById] once `loadConversationsFromStore`
     * populates `_conversations`. Cleared after a successful open.
     */
    @Volatile
    private var pendingOpenConversation: Pair<String, String>? = null

    /**
     * ID of the conversation currently visible on screen, or null.
     * Set by the conversation composable's LaunchedEffect(onAppear/onDisappear).
     * Used to suppress notifications for the chat the user is actively reading.
     */
    private val _currentConversationId = MutableStateFlow<String?>(null)
    val currentConversationId: StateFlow<String?> = _currentConversationId.asStateFlow()

    /**
     * Whether the app's Activity is in the resumed state. Driven by lifecycle
     * observers. Used to decide whether to fire a local notification.
     */
    private val _isAppActive = MutableStateFlow(true)
    val isAppActive: StateFlow<Boolean> = _isAppActive.asStateFlow()

    /**
     * Per-account connection state, keyed by DID.
     * Sourced from the Rust reconnect loop via waitForConnectionStateChange.
     */
    private val _connectionStates = MutableStateFlow<Map<String, ConnectionState>>(emptyMap())
    val connectionStates: StateFlow<Map<String, ConnectionState>> = _connectionStates.asStateFlow()

    /** Pending invite token from a deep link, picked up by the onboarding flow. */
    private val _pendingInviteToken = MutableStateFlow<String?>(null)
    val pendingInviteToken: StateFlow<String?> = _pendingInviteToken.asStateFlow()

    /** A parsed `authorize` deep link awaiting consent (docs/25). */
    private val _pendingLoginRequest = MutableStateFlow<ProjectLoginRequest?>(null)
    val pendingLoginRequest: StateFlow<ProjectLoginRequest?> = _pendingLoginRequest.asStateFlow()
    /** A structured login failure the UI surfaces and a Project/app can hook. */
    private val _loginError = MutableStateFlow<ProjectLoginError?>(null)
    val loginError: StateFlow<ProjectLoginError?> = _loginError.asStateFlow()
    fun clearLoginError() { _loginError.value = null }
    fun cancelLogin() { _pendingLoginRequest.value = null }

    /**
     * The validated invite the onboarding flow is currently acting on. Set once a
     * QR/link is resolved; read by IdentityPickerView / NewAccountView /
     * JoiningServerView so the token does not need to round-trip through route args.
     */
    private val _pendingInvite = MutableStateFlow<InviteToken?>(null)
    val pendingInvite: StateFlow<InviteToken?> = _pendingInvite.asStateFlow()

    // -----------------------------------------------------------------------
    // Mutable state setters (called from composables)
    // -----------------------------------------------------------------------

    fun setSelectedTab(tab: Tab) { _selectedTab.value = tab }
    fun setCurrentConversationId(id: String?) { _currentConversationId.value = id }
    fun setIsAppActive(active: Boolean) {
        _isAppActive.value = active
        // Push the foreground-active state to every core: gates the WS keepalive
        // (foreground-only, for battery) and, on becoming active, triggers an
        // opportunistic reconnect + liveness probe so a socket that died while
        // the app was backgrounded recovers promptly instead of pinning
        // "Reconnecting…". Mirrors iOS's `AppState.setAppActiveAll`.
        viewModelScope.launch {
            val coresSnapshot = cores.values.toList()
            for (core in coresSnapshot) {
                withContext(Dispatchers.IO) { runCatching { core.setAppActive(active) } }
            }
        }
    }
    fun setNavigateToConversation(conv: Conversation?) { _navigateToConversation.value = conv }
    fun setPendingInviteToken(token: String?) { _pendingInviteToken.value = token }
    fun setPendingInvite(invite: InviteToken?) { _pendingInvite.value = invite }

    // -----------------------------------------------------------------------
    // Private state — not exposed to views
    // -----------------------------------------------------------------------

    /** Active AppCore instances, keyed by DID. */
    private val cores: MutableMap<String, AppCoreProtocol> = mutableMapOf()

    /** DIDs with an in-flight recoverAccount. Single-flight guard. */
    private val recoveriesInFlight: MutableSet<String> = mutableSetOf()

    /** Per-account connection-state listener coroutine jobs. Cancelled on logout/mode switch. */
    private val stateJobs: MutableMap<String, Job> = mutableMapOf()

    /** Per-account event-channel listener coroutine jobs. Cancelled on logout/mode switch. */
    private val eventJobs: MutableMap<String, Job> = mutableMapOf()

    /**
     * Cached display names for remote DIDs, keyed by DID. Backed by Compose
     * snapshot state so that when an async resolution lands
     * (`applyResolvedDisplayName`), the conversation-list rows — whose preview
     * text is computed live from this cache via `resolvedName`/`groupEventText`
     * — recompose and replace "Unknown" with the real name. With a plain map the
     * read isn't tracked, so a row resolved only after the list first rendered
     * stays stale until the view is rebuilt (e.g. opening the conversation and
     * coming back). Mirrors iOS's `@Published displayNameCache`.
     */
    private val displayNameCache: MutableMap<String, String> = mutableStateMapOf()

    /** DIDs currently being fetched (to avoid duplicate requests). */
    private val displayNameInFlight: MutableSet<String> = mutableSetOf()

    /**
     * DIDs that resolved to no name this session. Suppresses re-spawning a
     * resolve task on every re-render. Cleared on reconnect so coming back
     * online retries.
     */
    private val unresolvedDids: MutableSet<String> = mutableSetOf()

    /**
     * Cached bot status for remote DIDs, keyed by DID. Snapshot-backed for the
     * same reactivity reason as `displayNameCache`: the row avatar must switch
     * to the bot frame once bot status resolves.
     */
    private val isBotCache: MutableMap<String, Boolean> = mutableStateMapOf()

    /** Cached group titles, keyed by URL-safe-no-pad base64 group_id. */
    private val groupTitleCache: MutableMap<String, String> = mutableMapOf()

    private var _service: ActnetService = makeService(resolveInitialServiceMode())

    val service: ActnetService get() = _service

    /**
     * Preview/testing only: seed the observable state directly, with no I/O,
     * network, or FFI. Used by `rememberPreviewAppViewModel` so @Preview
     * composables can render realistic content. Do not call from app code.
     */
    fun seedForPreview(
        accounts: List<Account> = emptyList(),
        conversations: List<Conversation> = emptyList(),
        messagesByConversation: Map<String, List<Message>> = emptyMap(),
        isOnboarding: Boolean = false,
    ) {
        _accounts.value = accounts
        _conversations.value = conversations
        _messagesByConversation.value = messagesByConversation
        _isOnboarding.value = isOnboarding
    }

    // -----------------------------------------------------------------------
    // Preferences keys
    // -----------------------------------------------------------------------

    private companion object {
        const val SERVICE_MODE_KEY = "serviceMode"
        const val ACCOUNTS_KEY = "persistedAccounts"
        // Device-linking poll loop (docs/04 §4.2): one FFI step per GET, sleeping
        // between steps in a cancellable coroutine.
        const val LINK_TIMEOUT_MS = 180_000L
        const val LINK_POLL_INTERVAL_MS = 1_000L
    }

    private val prefs: SharedPreferences
        get() = applicationContext.getSharedPreferences("appstate_prefs", Context.MODE_PRIVATE)

    // -----------------------------------------------------------------------
    // Init helpers
    // -----------------------------------------------------------------------

    private fun resolveInitialServiceMode(): ServiceMode {
        val saved = prefs.getString(SERVICE_MODE_KEY, null)
        return saved?.let { ServiceMode.fromRawValue(it) } ?: ServiceMode.DEV_SERVER
    }

    private fun makeService(mode: ServiceMode): ActnetService = when (mode) {
        ServiceMode.MOCK -> MockActnetService()
        ServiceMode.DEV_SERVER -> LiveActnetService
    }

    private val dbDir: File
        get() = File(applicationContext.filesDir, "actnet").also { it.mkdirs() }

    // -----------------------------------------------------------------------
    // Deep linking — mirrors iOS AppState.handleDeepLink / isDeepLink
    // -----------------------------------------------------------------------

    /**
     * Handle a deep link URI.
     * Supported:
     *  - https://go.theavalanche.net/conversation/<recipient_did>
     *  - https://go.theavalanche.net/i/<token>  (or legacy /invite/<token>)
     */
    fun handleDeepLink(uri: Uri) {
        AppLog.info("DeepLink", "handleDeepLink: $uri, scheme=${uri.scheme}, host=${uri.host}, path=${uri.path}")
        if (!isDeepLink(uri)) return

        val segments = uri.pathSegments.filter { it != "/" && it.isNotEmpty() }
        // "Sign in with Avalanche" (docs/25): `/authorize?...` carries its params
        // in the query string, so it's handled before the path-segment guard.
        if (segments.firstOrNull() == "authorize") {
            handleAuthorizeDeepLink(uri)
            return
        }
        if (segments.size < 2) return
        val action = segments[0]

        when (action) {
            "conversation" -> {
                val did = segments[1]
                if (did.isEmpty()) return
                val accountId = _accounts.value.firstOrNull()?.id ?: return
                AppLog.info("DeepLink", "navigating to conversation with $did")
                val conv = findOrCreateDMConversation(recipientDid = did, accountId = accountId)
                _selectedTab.value = Tab.CHATS
                _navigateToConversation.value = conv
            }

            "i", "invite" -> {
                val token = segments[1]
                AppLog.info("DeepLink", "handling invite token")
                // Try to decode the token locally to check if we're already on the server.
                val decoded = decodeBase64URL(token)
                val payload = decoded?.let { runCatching { JSONObject(String(it)) }.getOrNull() }
                val serverUrl = payload?.optString("s")
                val inviterDid = payload?.optString("d")

                if (serverUrl != null && inviterDid != null && inviterDid.isNotEmpty()) {
                    val account = _accounts.value.firstOrNull { acct ->
                        acct.servers.any { s ->
                            s.url.toString().trimEnd('/') == serverUrl.trimEnd('/')
                        }
                    }
                    if (account != null) {
                        // Already registered on this server — skip to DM.
                        AppLog.info("DeepLink", "already on server, opening DM with $inviterDid")
                        val conv = findOrCreateDMConversation(
                            recipientDid = inviterDid,
                            accountId = account.id
                        )
                        _selectedTab.value = Tab.CHATS
                        _navigateToConversation.value = conv
                        return
                    }
                }
                // Not on this server — start onboarding flow.
                _pendingInviteToken.value = token
            }

            else -> AppLog.info("DeepLink", "unknown action: $action")
        }
    }

    /** Check if a URI is a deep link for this app. */
    fun isDeepLink(uri: Uri): Boolean = uri.host == "go.theavalanche.net"

    /**
     * Navigate to an existing conversation by its conversation id (the value
     * stored in `message_history.conversation_id`: `dm-<did>` for DMs,
     * `group-<groupId>` for groups). Used by the notification-tap path, which
     * carries `conversationId`/`accountId` extras straight from the
     * notification that scheduled it.
     *
     * Unlike [handleDeepLink]'s `conversation/<did>` action, this never tries to
     * (re)create a DM — it only opens a conversation that already exists in the
     * in-memory list, so a group notification opens the actual group rather than
     * a bogus DM whose "recipient" is the `group-…` id.
     *
     * Mirrors iOS AppDelegate.userNotificationCenter(_:didReceive:) which looks
     * up `conversations.first { id == conversationId && accountId == accountId }`.
     */
    fun openConversationById(conversationId: String, accountId: String) {
        AppLog.info("DeepLink", "openConversationById: $conversationId (account $accountId)")
        val conv = _conversations.value.firstOrNull {
            it.id == conversationId && it.accountId == accountId
        } ?: run {
            // Cold-start tap: conversations not loaded yet. Remember the request
            // and flush it once loadConversationsFromStore populates the list.
            AppLog.info("DeepLink", "conversation not loaded yet; deferring open of $conversationId")
            pendingOpenConversation = conversationId to accountId
            return
        }
        pendingOpenConversation = null
        _selectedTab.value = Tab.CHATS
        _navigateToConversation.value = conv
    }

    // -----------------------------------------------------------------------
    // Shared image (docs/35) — Android share-in routing
    // -----------------------------------------------------------------------

    /** Surface the destination picker for an image shared in from another app. */
    fun setPendingSharedImage(data: ByteArray, contentType: String) {
        _pendingSharedImage.value = PendingSharedImage(data = data, contentType = contentType)
    }

    /** Dismiss the share-destination picker without choosing a chat. */
    fun clearPendingSharedImage() {
        _pendingSharedImage.value = null
    }

    /**
     * Route a shared image to the chosen conversation: stage it for that chat and
     * navigate there, where ConversationView pre-fills the composer for review.
     */
    fun routeSharedImage(conversation: Conversation) {
        val pending = _pendingSharedImage.value ?: return
        _pendingStagedImage.value = _pendingStagedImage.value + (conversation.id to pending.data)
        _pendingSharedImage.value = null
        _selectedTab.value = Tab.CHATS
        _navigateToConversation.value = conversation
    }

    /** Consume (and clear) the image staged for a conversation, if any. */
    fun takePendingStagedImage(conversationId: String): ByteArray? {
        val current = _pendingStagedImage.value
        val data = current[conversationId] ?: return null
        _pendingStagedImage.value = current - conversationId
        return data
    }

    // -----------------------------------------------------------------------
    // Unread count — mirrors iOS AppState.unreadCount(for:)
    // -----------------------------------------------------------------------

    /**
     * Unread count for a conversation's chat-list badge.
     *
     * When the transcript is loaded in memory we count it directly (reflecting
     * optimistic mark-read and freshly-appended messages immediately); otherwise
     * we fall back to the persisted count seeded from the store, so
     * conversations not opened this session still show the right badge.
     */
    fun unreadCount(conversation: Conversation): Int {
        val messages = _messagesByConversation.value[conversation.id]
        if (messages != null) {
            return messages.count { it.readAtMs == null && it.senderAccountId != conversation.accountId }
        }
        return _unreadCounts.value[conversation.id] ?: 0
    }

    // -----------------------------------------------------------------------
    // Account lifecycle
    // -----------------------------------------------------------------------

    /**
     * Attempt to restore persisted accounts on launch.
     * Mirrors iOS AppState.restoreAccounts().
     */
    fun restoreAccounts() {
        viewModelScope.launch {
            val persisted = loadPersistedAccounts()
            if (persisted.isEmpty()) return@launch

            val dbKey = withContext(Dispatchers.IO) {
                runCatching { KeystoreKeyManager.dbPassphrase(applicationContext) }.getOrNull()
            }
            if (dbKey == null) {
                AppLog.error("restore", "Failed to retrieve DB encryption key, cannot restore accounts")
                // We optimistically started on MAIN; with no usable accounts,
                // fall back to the splash/onboarding flow.
                _isOnboarding.value = _accounts.value.isEmpty()
                return@launch
            }

            for (p in persisted) {
                val dbFile = File(dbDir, p.dbFilename)
                if (!dbFile.exists()) {
                    AppLog.warn("restore", "DB file missing for ${p.did}, removing from persisted accounts")
                    removePersistedAccount(p.did)
                    continue
                }

                val serverInfos = p.servers.mapNotNull { s ->
                    runCatching { ServerInfo(id = s.id, name = s.name, url = Uri.parse(s.url)) }.getOrNull()
                }
                val account = Account(
                    id = p.did,
                    displayName = p.displayName,
                    avatarData = null,
                    servers = serverInfos,
                )
                _accounts.update { it + account }

                val core = withContext(Dispatchers.IO) {
                    runCatching { _service.login(dbPath = dbFile.absolutePath, dbKey = dbKey) }.getOrNull()
                }
                if (core == null) {
                    AppLog.warn("restore", "Failed to authenticate account ${p.did} (will show offline)")
                    continue
                }
                // Key by the persisted DID we already trust — avoids a blocking
                // FFI call (core.did()) on the main thread; it equals core.did()
                // for this account anyway.
                cores[p.did] = core

                // Refresh display name from the local profile store — persisted name can be stale.
                val coreName = withContext(Dispatchers.IO) {
                    runCatching { core.ownDisplayName() }.getOrElse { "" }
                }
                if (coreName.isNotEmpty() && coreName != p.displayName) {
                    _accounts.update { list ->
                        list.map { if (it.id == p.did) it.copy(displayName = coreName) else it }
                    }
                    persistAccount(
                        PersistedAccount(
                            did = p.did,
                            displayName = coreName,
                            dbFilename = p.dbFilename,
                            servers = p.servers,
                        )
                    )
                }
            }

            if (_accounts.value.isNotEmpty()) {
                _isOnboarding.value = false

                if (_serviceMode.value == ServiceMode.MOCK) {
                    for (account in _accounts.value) {
                        val seeds = MockData.seedConversations(
                            accountId = account.id,
                            serverUrl = account.servers.firstOrNull()?.id ?: ""
                        )
                        _conversations.update { it + seeds }
                    }
                    _conversationsLoaded.value = true
                } else {
                    loadConversationsFromStore()
                }

                startMessagePolling()
                if (_serviceMode.value != ServiceMode.MOCK) {
                    PushManager.requestPermissionAndRegister(
                        context = applicationContext,
                        appViewModel = this@AppViewModel,
                    )
                }
            } else {
                // Persisted entries existed (so we started on MAIN) but none
                // produced a usable account — e.g. their DB files were gone.
                // Fall back to the splash/onboarding flow.
                _isOnboarding.value = true
            }
        }
    }

    fun logout() {
        deregisterPushBestEffort(cores.values.toList())
        cancelAllListenerJobs()
        _accounts.value = emptyList()
        _conversations.value = emptyList()
        _messagesByConversation.value = emptyMap()
        _reactionsByConversation.value = emptyMap()
        _connectionStates.value = emptyMap()
        cores.clear()
        displayNameCache.clear()
        displayNameInFlight.clear()
        unresolvedDids.clear()
        isBotCache.clear()
        clearPersistedAccounts()
        _conversationsLoaded.value = false
        _isOnboarding.value = true
    }

    fun switchMode(mode: ServiceMode) {
        _serviceMode.value = mode
        prefs.edit().putString(SERVICE_MODE_KEY, mode.rawValue).apply()
        _service = makeService(mode)
        cancelAllListenerJobs()
        _accounts.value = emptyList()
        _conversations.value = emptyList()
        _messagesByConversation.value = emptyMap()
        _reactionsByConversation.value = emptyMap()
        _connectionStates.value = emptyMap()
        cores.clear()
        displayNameCache.clear()
        displayNameInFlight.clear()
        unresolvedDids.clear()
        isBotCache.clear()
        clearPersistedAccounts()
        _conversationsLoaded.value = false
        _isOnboarding.value = true
    }

    /**
     * Create a new account.
     * [prfOutput] is the raw 32-byte WebAuthn PRF output (or hash of a recovery phrase).
     * Pass an empty ByteArray to skip recovery setup.
     * Mirrors iOS AppState.createAccount().
     */
    suspend fun createAccount(
        serverUrl: String,
        serverName: String,
        displayName: String,
        inviteToken: String?,
        prfOutput: ByteArray = ByteArray(0),
    ) {
        val dbFilename = "account-${UUID.randomUUID().toString().take(8)}.db"
        val dbPath = File(dbDir, dbFilename).absolutePath
        val dbKey = withContext(Dispatchers.IO) {
            KeystoreKeyManager.dbPassphrase(applicationContext)
        }
        val core = withContext(Dispatchers.IO) {
            _service.createAccount(
                serverUrl = serverUrl,
                dbPath = dbPath,
                dbKey = dbKey,
                prfOutput = prfOutput,
                displayName = displayName,
                inviteToken = inviteToken,
            )
        }
        finishAccountRegistration(
            core = core,
            serverUrl = serverUrl,
            serverName = serverName,
            displayName = displayName,
            dbFilename = dbFilename,
        )
    }

    /**
     * Prepare a fresh identity (Stage 1 of the passkey signup flow).
     * Mirrors iOS AppState.prepareAccount().
     */
    suspend fun prepareAccount(serverUrl: String, prfOutput: ByteArray): PreparedAccount =
        withContext(Dispatchers.IO) {
            _service.prepareAccount(serverUrl = serverUrl, prfOutput = prfOutput)
        }

    /**
     * Finalize an account previously created by prepareAccount (Stage 2).
     * Mirrors iOS AppState.finalizePreparedAccount().
     */
    suspend fun finalizePreparedAccount(
        prepared: PreparedAccount,
        serverUrl: String,
        serverName: String,
        displayName: String,
        inviteToken: String?,
    ) {
        val dbFilename = "account-${UUID.randomUUID().toString().take(8)}.db"
        val dbPath = File(dbDir, dbFilename).absolutePath
        val dbKey = withContext(Dispatchers.IO) {
            KeystoreKeyManager.dbPassphrase(applicationContext)
        }
        val core = withContext(Dispatchers.IO) {
            _service.finalizeAccount(
                prepared = prepared,
                dbPath = dbPath,
                dbKey = dbKey,
                displayName = displayName,
                inviteToken = inviteToken,
            )
        }
        finishAccountRegistration(
            core = core,
            serverUrl = serverUrl,
            serverName = serverName,
            displayName = displayName,
            dbFilename = dbFilename,
        )
    }

    /**
     * Recover an account from a passkey-protected recovery blob.
     * Mirrors iOS AppState.recoverAccount().
     */
    suspend fun recoverAccount(
        serverUrl: String,
        serverName: String,
        did: String,
        prfOutput: ByteArray,
        displayName: String,
    ) {
        // Single-flight guard: run recovery once per DID.
        if (_accounts.value.any { it.id == did }) return
        if (!recoveriesInFlight.add(did)) return
        try {
            val dbFilename = "account-${UUID.randomUUID().toString().take(8)}.db"
            val dbPath = File(dbDir, dbFilename).absolutePath
            val dbKey = withContext(Dispatchers.IO) {
                KeystoreKeyManager.dbPassphrase(applicationContext)
            }
            val core = withContext(Dispatchers.IO) {
                _service.recoverFromBlob(
                    serverUrl = serverUrl,
                    did = did,
                    prfOutput = prfOutput,
                    dbPath = dbPath,
                    dbKey = dbKey,
                    displayName = displayName,
                )
            }

            val restoredName = withContext(Dispatchers.IO) {
                runCatching { core.ownDisplayName() }.getOrElse { "" }
            }
            val resolvedDisplayName = when {
                restoredName.isNotEmpty() -> restoredName
                displayName.isNotEmpty() -> displayName
                else -> "Account ${did.takeLast(6)}"
            }
            finishAccountRegistration(
                core = core,
                serverUrl = serverUrl,
                serverName = serverName,
                displayName = resolvedDisplayName,
                dbFilename = dbFilename,
            )
        } finally {
            recoveriesInFlight.remove(did)
        }
    }

    // -----------------------------------------------------------------------
    // Device linking (docs/04-multi-device.md §4)
    //
    // Two sides; the bundle always flows existing→new. Role (show vs. scan) is
    // independent of which device is which — the FFI is symmetric.
    // Mirrors iOS AppState device-linking methods.
    // -----------------------------------------------------------------------

    /**
     * Existing-device side: show a pairing code for a new device to scan.
     * Returns the pairing string; follow with [linkSendBundle].
     */
    suspend fun linkCreatePairing(accountId: String): String {
        val core = cores[accountId] ?: throw IllegalStateException("Account not signed in on this device")
        return withContext(Dispatchers.IO) { core.linkCreatePairing(null) }
    }

    /**
     * Existing-device side: ingest a code scanned/pasted from the new device.
     * Follow with [linkSendBundle].
     */
    suspend fun linkAcceptPairing(accountId: String, code: String) {
        val core = cores[accountId] ?: throw IllegalStateException("Account not signed in on this device")
        withContext(Dispatchers.IO) { core.linkAcceptPairing(code) }
    }

    /**
     * Existing-device side: seal and send the provisioning bundle, driving the
     * mailbox poll loop here. Because the loop lives in a coroutine with a
     * cancellable [kotlinx.coroutines.delay], the caller's LaunchedEffect being
     * cancelled (screen dismissed / mode switched) stops polling within ~1s — no
     * orphaned, uncancellable FFI poll (docs/04 §4.2).
     */
    suspend fun linkSendBundle(accountId: String) {
        val core = cores[accountId] ?: throw IllegalStateException("Account not signed in on this device")
        val deadline = System.currentTimeMillis() + LINK_TIMEOUT_MS
        while (true) {
            val done = withContext(Dispatchers.IO) { core.linkSendBundleStep() }
            if (done) return
            if (System.currentTimeMillis() >= deadline) {
                throw IllegalStateException("Timed out waiting for the other device. Try again.")
            }
            delay(LINK_POLL_INTERVAL_MS)
        }
    }

    /**
     * New-device side: create a fresh link handle (this device has no account
     * yet). Drive it via [DeviceLink], then [completeDeviceLink].
     */
    fun makeDeviceLink(): DeviceLink = _service.makeDeviceLink()

    /**
     * New-device side: complete the link and register the resulting account,
     * exiting onboarding. [link] must have had [DeviceLink.createPairing] or
     * [DeviceLink.acceptPairing] called on it first. Blocks until the existing
     * device approves or the attempt times out.
     */
    suspend fun completeDeviceLink(link: DeviceLink) {
        val dbFilename = "account-${UUID.randomUUID().toString().take(8)}.db"
        val dbPath = File(dbDir, dbFilename).absolutePath
        val dbKey = withContext(Dispatchers.IO) {
            KeystoreKeyManager.dbPassphrase(applicationContext)
        }

        // UI-driven poll loop with a cancellable delay (see linkSendBundle).
        val deadline = System.currentTimeMillis() + LINK_TIMEOUT_MS
        val core = run {
            while (true) {
                val c = withContext(Dispatchers.IO) { link.awaitLinkStep(dbPath, dbKey) }
                if (c != null) return@run c
                if (System.currentTimeMillis() >= deadline) {
                    throw IllegalStateException("Timed out waiting for the link to complete. Try again.")
                }
                delay(LINK_POLL_INTERVAL_MS)
            }
            @Suppress("UNREACHABLE_CODE") error("unreachable")
        }

        // The joining device learns its DID and home server from the bundle, so
        // neither is supplied by the user (unlike recovery).
        val did = withContext(Dispatchers.IO) { core.did() }
        if (_accounts.value.any { it.id == did }) return

        val serverUrl = withContext(Dispatchers.IO) { core.homeServer() }
        val serverName = runCatching { Uri.parse(serverUrl).host }.getOrNull() ?: serverUrl
        val restoredName = withContext(Dispatchers.IO) {
            runCatching { core.ownDisplayName() }.getOrElse { "" }
        }
        val displayName = if (restoredName.isNotEmpty()) restoredName else "Account ${did.takeLast(6)}"

        finishAccountRegistration(
            core = core,
            serverUrl = serverUrl,
            serverName = serverName,
            displayName = displayName,
            dbFilename = dbFilename,
        )
    }

    private suspend fun finishAccountRegistration(
        core: AppCoreProtocol,
        serverUrl: String,
        serverName: String,
        displayName: String,
        dbFilename: String,
    ) {
        val did = withContext(Dispatchers.IO) { core.did() }
        cores[did] = core

        val serverUri = runCatching { Uri.parse(serverUrl) }.getOrElse { Uri.EMPTY }
        val account = Account(
            id = did,
            displayName = displayName,
            avatarData = null,
            servers = listOf(ServerInfo(id = serverUrl, name = serverName, url = serverUri)),
        )
        _accounts.update { it + account }

        persistAccount(
            PersistedAccount(
                did = did,
                displayName = displayName,
                dbFilename = dbFilename,
                servers = listOf(PersistedServer(id = serverUrl, name = serverName, url = serverUrl)),
            )
        )

        loadConversationsFromStore()

        if (_serviceMode.value == ServiceMode.MOCK) {
            val seeds = MockData.seedConversations(accountId = did, serverUrl = serverUrl)
            _conversations.update { it + seeds }
        }

        _isOnboarding.value = false
        startMessagePolling()
        if (_serviceMode.value != ServiceMode.MOCK) {
            PushManager.requestPermissionAndRegister(
                context = applicationContext,
                appViewModel = this,
            )
        }
    }

    /** Returns all active core instances. */
    fun activeCores(): List<AppCoreProtocol> = cores.values.toList()

    /**
     * Register [token] with every active core, on the ViewModel's own scope.
     *
     * Called by [PushManager.didReceiveToken], which may run on an FCM callback
     * thread. Snapshotting `cores` inside `viewModelScope.launch` keeps the map
     * read on the main dispatcher (consistent with every other access), and the
     * work is cancelled with the ViewModel rather than leaking a detached scope.
     * The blocking FFI call itself runs on [Dispatchers.IO]. `registerPushToken`
     * is idempotent, so calling on every launch / rotation is safe.
     */
    fun registerPushTokenWithCores(
        token: String,
        platform: String,
        relayUrl: String,
        environment: String,
    ) {
        viewModelScope.launch {
            val coresSnapshot = cores.values.toList()
            for (core in coresSnapshot) {
                withContext(Dispatchers.IO) {
                    runCatching {
                        core.registerPushToken(
                            deviceToken = token,
                            platform = platform,
                            relayUrl = relayUrl,
                            environment = environment,
                        )
                    }
                }.onFailure { error ->
                    AppLog.error(
                        "PushManager",
                        "registerPushToken failed (relay=$relayUrl): ${error.message}",
                    )
                }.onSuccess {
                    AppLog.info(
                        "PushManager",
                        "registerPushToken ok (relay=$relayUrl, env=$environment)",
                    )
                }
            }
        }
    }

    /**
     * Deregister this device's push token for [coresToClear], best-effort, on
     * the ViewModel scope (which outlives a logout — the ViewModel isn't
     * cleared). Callers snapshot `cores` and call this BEFORE clearing the map.
     * Without it, the relay keeps mapping the FCM token to the logged-out
     * account until its GC reaps the stranded pseudonym.
     */
    private fun deregisterPushBestEffort(coresToClear: List<AppCoreProtocol>) {
        if (coresToClear.isEmpty()) return
        val relayUrl = BuildConfig.RELAY_URL
        if (relayUrl.isEmpty()) return
        viewModelScope.launch {
            for (core in coresToClear) {
                withContext(Dispatchers.IO) {
                    runCatching { core.unregisterPushToken(relayUrl = relayUrl) }
                }.onFailure { error ->
                    AppLog.warn("PushManager", "unregisterPushToken failed: ${error.message}")
                }
            }
        }
    }

    /** Look up the AppCore bound to a given account DID. */
    fun core(accountId: String): AppCoreProtocol? = cores[accountId]

    suspend fun joinServer(serverUrl: String, serverName: String, existingAccountId: String) {
        val serverUri = runCatching { Uri.parse(serverUrl) }.getOrElse { Uri.EMPTY }
        _accounts.update { list ->
            list.map { acct ->
                if (acct.id == existingAccountId) {
                    acct.copy(
                        servers = acct.servers + ServerInfo(
                            id = serverUrl,
                            name = serverName,
                            url = serverUri,
                        )
                    )
                } else acct
            }
        }
        _isOnboarding.value = false
    }

    // -----------------------------------------------------------------------
    // Account teardown (docs/53-multi-account-ux.md)
    // -----------------------------------------------------------------------

    /**
     * Leave a server (docs/53 §Leave).
     * Mirrors iOS AppState.leaveServer().
     */
    suspend fun leaveServer(account: Account, server: ServerInfo) {
        val core = cores[account.id]
            ?: throw IllegalStateException("No active connection for this account")
        withContext(Dispatchers.IO) { core.leaveServer() }
        removeAccountLocally(accountId = account.id)
    }

    /**
     * Delete an identity (docs/53 §Delete identity).
     * Mirrors iOS AppState.deleteIdentity().
     */
    suspend fun deleteIdentity(account: Account) {
        val core = cores[account.id]
            ?: throw IllegalStateException("No active connection for this account")
        withContext(Dispatchers.IO) { core.deleteIdentity() }
        removeAccountLocally(accountId = account.id)
    }

    /**
     * Tear down all local state for one account.
     * Mirrors iOS AppState.removeAccountLocally().
     */
    private fun removeAccountLocally(accountId: String) {
        stateJobs[accountId]?.cancel()
        stateJobs.remove(accountId)
        eventJobs[accountId]?.cancel()
        eventJobs.remove(accountId)
        cores.remove(accountId)
        _connectionStates.update { it - accountId }
        _accounts.update { it.filter { a -> a.id != accountId } }

        val convIds = _conversations.value
            .filter { it.accountId == accountId }
            .map { it.id }
        _conversations.update { it.filter { c -> c.accountId != accountId } }
        _messagesByConversation.update { map ->
            map.filterKeys { it !in convIds }
        }
        _reactionsByConversation.update { map ->
            map.filterKeys { it !in convIds }
        }

        // Delete the on-disk SQLCipher files (identity + device + WAL/SHM siblings).
        val filename = persistedDbFilename(accountId)
        if (filename != null) {
            val base = File(dbDir, filename).absolutePath
            for (path in listOf(base, "$base.device")) {
                for (suffix in listOf("", "-wal", "-shm")) {
                    runCatching { File(path + suffix).delete() }
                }
            }
        }

        removePersistedAccount(accountId)
        if (_accounts.value.isEmpty()) {
            _isOnboarding.value = true
        }
    }

    // -----------------------------------------------------------------------
    // Display name resolution — mirrors iOS AppState display name section
    // -----------------------------------------------------------------------

    /**
     * Returns the cached display name for a DID, or the DID itself if unknown.
     * Kicks off a background fetch if not cached yet.
     * Mirrors iOS AppState.displayName(for:accountId:).
     */
    fun displayName(did: String, accountId: String): String {
        // Own accounts: name lives in Account model.
        val ownAccount = _accounts.value.firstOrNull { it.id == did }
        if (ownAccount != null) return ownAccount.displayName
        displayNameCache[did]?.let { return it }
        resolveDisplayName(did = did, accountId = accountId)
        return did
    }

    /**
     * UI-facing display name: the resolved name, or "Unknown" while resolving.
     * Never returns a DID.
     * Mirrors iOS AppState.resolvedName(for:accountId:).
     */
    fun resolvedName(did: String, accountId: String): String {
        val name = displayName(did = did, accountId = accountId)
        return if (name == did) "Unknown" else name
    }

    /**
     * Human-readable line for a group system/metadata event (docs/03 §3.6).
     * Mirrors iOS AppState.groupEventText(_:accountId:).
     */
    fun groupEventText(message: Message, accountId: String): String {
        val ev = message.groupEvent ?: return message.body
        val actor = eventName(ev.actorDid, accountId = accountId, capitalized = true)
        val target = eventName(ev.targetDid, accountId = accountId, capitalized = false)
        return when (ev.event) {
            GroupEventKind.MEMBER_JOINED -> "$actor joined"
            GroupEventKind.MEMBER_JOINED_VIA_LINK -> "$actor joined via invite link"
            GroupEventKind.MEMBER_REQUESTED_TO_JOIN -> "$actor requested to join"
            GroupEventKind.MEMBER_INVITED -> "$actor invited $target"
            GroupEventKind.MEMBER_LEFT -> "$actor left the group"
            GroupEventKind.MEMBER_REMOVED -> "$actor removed $target"
            GroupEventKind.JOIN_REQUEST_APPROVED -> "$actor approved ${target}'s request to join"
            GroupEventKind.JOIN_REQUEST_DENIED -> "$actor declined a join request"
            GroupEventKind.INVITE_DECLINED -> "$actor declined the invitation"
            GroupEventKind.JOIN_REQUEST_CANCELLED -> "$actor cancelled their request to join"
            GroupEventKind.ROLE_CHANGED_TO_ADMIN -> "$actor made $target an admin"
            GroupEventKind.ROLE_CHANGED_TO_MEMBER -> "$actor removed $target as an admin"
            GroupEventKind.TITLE_CHANGED ->
                if (ev.newTitle.isEmpty()) "$actor changed the group name"
                else "$actor changed the group name to “${ev.newTitle}”"
            GroupEventKind.DESCRIPTION_CHANGED -> "$actor changed the group description"
            GroupEventKind.EXPIRY_CHANGED ->
                if (ev.expirySeconds == 0u) "$actor turned off disappearing messages"
                else "$actor set disappearing messages to ${disappearingMessagesLabel(ev.expirySeconds)}"
            GroupEventKind.POLICY_CHANGED -> "$actor changed the group settings"
        }
    }

    private fun eventName(did: String, accountId: String, capitalized: Boolean): String {
        if (did.isEmpty()) return if (capitalized) "Someone" else "someone"
        if (did == accountId) return if (capitalized) "You" else "you"
        return resolvedName(did = did, accountId = accountId)
    }

    /**
     * Whether a DID is a bot account, for avatar/badge presentation (docs/54).
     * Mirrors iOS AppState.isBot(_:accountId:).
     */
    fun isBot(did: String, accountId: String): Boolean {
        if (_accounts.value.any { it.id == did }) return false
        isBotCache[did]?.let { return it }
        resolveDisplayName(did = did, accountId = accountId)
        return false
    }

    /**
     * Seed the name cache with a name a caller already holds.
     * Mirrors iOS AppState.cacheDisplayName(_:for:).
     */
    fun cacheDisplayName(name: String, did: String) {
        if (name.isEmpty() || displayNameCache[did] != null) return
        applyResolvedDisplayName(did = did, name = name)
    }

    /**
     * Resolve a display name for a DID asynchronously.
     * Mirrors iOS AppState.resolveDisplayName(did:accountId:).
     */
    private fun resolveDisplayName(did: String, accountId: String) {
        if (displayNameInFlight.contains(did)) return
        if (unresolvedDids.contains(did)) return
        val core = cores[accountId] ?: return
        displayNameInFlight.add(did)
        viewModelScope.launch {
            val localName = withContext(Dispatchers.IO) {
                runCatching { core.contactDisplayName(did = did) }.getOrElse { "" }
            }
            val serverInfo = if (localName.isEmpty()) {
                withContext(Dispatchers.IO) {
                    runCatching { core.getAccountInfo(did = did) }.getOrNull()
                }
            } else null
            val resolved = localName.ifEmpty { serverInfo?.displayName ?: "" }
            val isBot = serverInfo?.isBot ?: false

            displayNameInFlight.remove(did)
            isBotCache[did] = isBot
            if (resolved.isEmpty()) {
                unresolvedDids.add(did)
            } else {
                applyResolvedDisplayName(did = did, name = resolved)
            }
        }
    }

    /**
     * Cache a resolved name and update any conversation title that doesn't already match.
     * Mirrors iOS AppState.applyResolvedDisplayName(did:name:).
     */
    private fun applyResolvedDisplayName(did: String, name: String) {
        displayNameCache[did] = name
        _conversations.update { list ->
            list.map { conv ->
                if (conv.recipientDid == did && conv.title != name) conv.copy(title = name)
                else conv
            }
        }
    }

    /**
     * Re-fetch a contact's encrypted profile from the homeserver and refresh the cached
     * display name if it changed.
     * Mirrors iOS AppState.refreshContactProfile(did:accountId:).
     */
    fun refreshContactProfile(did: String, accountId: String) {
        val core = cores[accountId] ?: return
        viewModelScope.launch {
            val changed = withContext(Dispatchers.IO) {
                runCatching { core.refreshContactProfile(did = did) }.getOrElse { false }
            }
            if (!changed) return@launch
            val newName = withContext(Dispatchers.IO) {
                runCatching { core.contactDisplayName(did = did) }.getOrElse { "" }
            }
            if (newName.isNotEmpty()) {
                applyResolvedDisplayName(did = did, name = newName)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Abuse handling (docs/12-abuse-handling.md)
    // -----------------------------------------------------------------------

    /** Accept a message request. Mirrors iOS AppState.acceptRequest(did:accountId:). */
    fun acceptRequest(did: String, accountId: String) {
        val core = cores[accountId] ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) { runCatching { core.acceptRequest(did = did) } }
            loadConversationsFromStore()
        }
    }

    /** Delete a message request. Mirrors iOS AppState.deleteRequest(did:accountId:). */
    fun deleteRequest(did: String, accountId: String) {
        val core = cores[accountId] ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) { runCatching { core.deleteRequest(did = did) } }
            loadConversationsFromStore()
        }
    }

    /** Report Spam and Block. Mirrors iOS AppState.reportAndBlock(did:accountId:reason:). */
    fun reportAndBlock(did: String, accountId: String, reason: String = "spam") {
        val core = cores[accountId] ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) { runCatching { core.reportAndBlock(did = did, reason = reason) } }
            loadConversationsFromStore()
        }
    }

    /** Block a contact (docs/12 §2). Mirrors iOS AppState.blockContact(did:accountId:). */
    fun blockContact(did: String, accountId: String) {
        val core = cores[accountId] ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) { runCatching { core.blockContact(did = did) } }
            loadConversationsFromStore()
        }
    }

    /** Unblock a contact (docs/12 §2). Mirrors iOS AppState.unblockContact(did:accountId:). */
    fun unblockContact(did: String, accountId: String) {
        val core = cores[accountId] ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) { runCatching { core.unblockContact(did = did) } }
            loadConversationsFromStore()
        }
    }

    /** The block list for an account. Mirrors iOS AppState.listBlocked(accountId:). */
    suspend fun listBlocked(accountId: String): List<ContactRowFfi> {
        val core = cores[accountId] ?: return emptyList()
        return withContext(Dispatchers.IO) {
            runCatching { core.listBlocked() }.getOrElse { emptyList() }
        }
    }

    // -----------------------------------------------------------------------
    // Messaging
    // -----------------------------------------------------------------------

    /**
     * Unified composer send (docs/35): one message to a DM or group carrying
     * optional [text], an optional already-picked image ([imageData]), and an
     * optional link [preview]. Collapses the former sendMessage / sendGroupMessage
     * / sendAttachment paths onto the core's single sendMessageWithAttachments
     * entry point — which is wire-identical to a plain text send when attachments
     * and previews are empty. Mirrors iOS AppState.sendComposed.
     *
     * The caller has already inserted the optimistic transcript row and bumped
     * the chat list. This resolves the disappearing-messages timer, uploads the
     * image (if any), grafts the pointer + preview onto the optimistic row,
     * persists the sending -> sent / failed lifecycle, and sends.
     */
    suspend fun sendComposed(
        conversation: Conversation,
        text: String,
        imageData: ByteArray? = null,
        imageContentType: String = "image/jpeg",
        imageFileName: String? = "photo.jpg",
        preview: LinkPreviewFfi? = null,
        messageId: String,
        sentAtMs: Long,
    ) {
        val core = cores[conversation.accountId] ?: return
        val target = if (conversation.isGroup) {
            MessageTarget.Group(groupId = conversation.groupId ?: "")
        } else {
            MessageTarget.Dm(recipientDid = conversation.recipientDid ?: "")
        }

        // Stamp the local copy with the conversation's disappearing-messages
        // timer (docs/03 §5): group timer for groups, per-peer timer for DMs.
        val timer = withContext(Dispatchers.IO) {
            if (conversation.isGroup) {
                conversation.groupId?.let { gid -> runCatching { core.groupExpirySeconds(groupId = gid) }.getOrNull() }
            } else {
                conversation.recipientDid?.let { rcpt -> runCatching { core.getConversationTimer(conversationId = rcpt) }.getOrNull() }
            }
        } ?: 0u

        val previews = preview?.let { listOf(it) } ?: emptyList()

        runCatching {
            // Upload the staged image first (docs/35) so its pointer — carrying
            // the inline thumbnail + URL — can ride this same message.
            var attachments: List<AttachmentFfi> = emptyList()
            if (imageData != null) {
                val (thumb, w, h) = makeAttachmentThumbnail(imageData)
                val pointer = withContext(Dispatchers.IO) {
                    core.uploadAttachment(imageData, imageContentType, imageFileName, w, h, 0, thumb, 0)
                }
                attachments = listOf(pointer)
            }

            // Graft the attachment + preview onto the optimistic transcript row,
            // and correct the chat-list preview's attachment type.
            if (attachments.isNotEmpty() || previews.isNotEmpty()) {
                _messagesByConversation.update { map ->
                    val msgs = map[conversation.id] ?: return@update map
                    map + (conversation.id to msgs.map {
                        if (it.id == messageId) it.copy(
                            attachments = if (attachments.isNotEmpty()) attachments else it.attachments,
                            previews = if (previews.isNotEmpty()) previews else it.previews,
                        ) else it
                    })
                }
            }
            _conversations.update { list ->
                list.map { c ->
                    if (c.id == conversation.id) {
                        c.copy(lastMessageAttachmentContentType = if (imageData != null) imageContentType else null)
                    } else c
                }
            }

            // Persist as "sending" up front so a failure is recoverable across launches.
            val pending = StoredMessageFfi(
                id = messageId, conversationId = conversation.id, senderDid = conversation.accountId,
                body = text, sentAtMs = sentAtMs, editedAtMs = null, readAtMs = sentAtMs,
                deliveryStatus = DeliveryStatus.SENDING.code.toUByte(), editCount = 0u, deleted = false,
                kind = 0L, metadata = null, expireTimerSecs = timer, expireAtMs = null,
                attachments = attachments, previews = previews,
            )
            withContext(Dispatchers.IO) { runCatching { core.saveMessage(msg = pending) } }

            withContext(Dispatchers.IO) {
                core.sendMessageWithAttachments(target, text, attachments, previews, sentAtMs)
            }
            updateMessageStatus(messageId = messageId, conversationId = conversation.id, newStatus = DeliveryStatus.SENT)
            val sent = StoredMessageFfi(
                id = messageId, conversationId = conversation.id, senderDid = conversation.accountId,
                body = text, sentAtMs = sentAtMs, editedAtMs = null, readAtMs = sentAtMs,
                deliveryStatus = DeliveryStatus.SENT.code.toUByte(), editCount = 0u, deleted = false,
                kind = 0L, metadata = null, expireTimerSecs = timer, expireAtMs = null,
                attachments = attachments, previews = previews,
            )
            withContext(Dispatchers.IO) { runCatching { core.saveMessage(msg = sent) } }
        }.onFailure { error ->
            AppLog.error("send", "send to ${conversation.id} failed: ${error.message}")
            updateMessageStatus(messageId = messageId, conversationId = conversation.id, newStatus = DeliveryStatus.FAILED)
            val failed = StoredMessageFfi(
                id = messageId, conversationId = conversation.id, senderDid = conversation.accountId,
                body = text, sentAtMs = sentAtMs, editedAtMs = null, readAtMs = sentAtMs,
                deliveryStatus = DeliveryStatus.FAILED.code.toUByte(), editCount = 0u, deleted = false,
                kind = 0L, metadata = null, expireTimerSecs = timer, expireAtMs = null,
                attachments = emptyList(), previews = emptyList(),
            )
            withContext(Dispatchers.IO) { runCatching { core.saveMessage(msg = failed) } }
            throw error
        }
    }

    /**
     * Download (or load the cached) decrypted bytes for an attachment (docs/35),
     * caching the blob in filesDir and recording the path so later loads skip
     * the network. Mirrors iOS AppState.attachmentData.
     */
    suspend fun attachmentData(attachment: AttachmentFfi, accountId: String): ByteArray? {
        attachment.localPath?.let { path ->
            val f = java.io.File(path)
            if (f.exists()) return runCatching { f.readBytes() }.getOrNull()
        }
        val core = cores[accountId] ?: return null
        return withContext(Dispatchers.IO) {
            runCatching {
                val data = core.downloadAttachment(attachment)
                val dir = File(applicationContext.filesDir, "attachments").apply { mkdirs() }
                val name = attachment.id.ifEmpty { UUID.randomUUID().toString() }
                val file = File(dir, name)
                file.writeBytes(data)
                if (attachment.id.isNotEmpty()) {
                    runCatching { core.setAttachmentDownloaded(attachment.id, file.absolutePath) }
                }
                data
            }.getOrNull()
        }
    }

    /**
     * Generate a link-preview card (docs/35 "Link previews") for the first URL
     * in [body], if any: fetch the page's OpenGraph metadata on this device,
     * upload the og:image as an encrypted attachment, and return the pointer.
     * Best-effort — returns `[]` on no URL / failure. Mirrors iOS `linkPreviews`.
     */
    suspend fun linkPreviews(body: String, accountId: String): List<LinkPreviewFfi> {
        val core = cores[accountId] ?: return emptyList()
        val fetched = fetchLinkPreview(body) ?: return emptyList()
        var image: AttachmentFfi? = null
        val data = fetched.imageData
        if (data != null) {
            val (thumb, w, h) = makeAttachmentThumbnail(data)
            image = withContext(Dispatchers.IO) {
                runCatching { core.uploadAttachment(data, "image/jpeg", null, w, h, 0, thumb, 0) }.getOrNull()
            }
        }
        return listOf(
            LinkPreviewFfi(
                url = fetched.url,
                title = fetched.title,
                description = fetched.description,
                dateMs = 0L,
                image = image,
            )
        )
    }

    /** Reflect generated link previews on an in-memory optimistic message row. */
    fun setMessagePreviews(conversationId: String, messageId: String, previews: List<LinkPreviewFfi>) {
        _messagesByConversation.update { map ->
            val msgs = map[conversationId] ?: return@update map
            map + (conversationId to msgs.map {
                if (it.id == messageId) it.copy(previews = previews) else it
            })
        }
    }

    /** Update an in-memory message's delivery status by id. */
    private fun updateMessageStatus(
        messageId: String,
        conversationId: String,
        newStatus: DeliveryStatus,
    ) {
        _messagesByConversation.update { map ->
            val msgs = map[conversationId] ?: return@update map
            val idx = msgs.indexOfFirst { it.id == messageId }
            if (idx < 0) return@update map
            val updated = msgs.toMutableList()
            updated[idx] = updated[idx].copy(deliveryStatus = newStatus)
            map + (conversationId to updated)
        }
    }

    /**
     * Optimistically insert a just-sent message into the UI before the network
     * round-trip completes, and bump the conversation's last-message preview —
     * mirrors how iOS mutates appState.messagesByConversation/appState.conversations
     * in ConversationView.sendMessage. Both updates happen here so the list and the
     * thread stay consistent. A no-op if the message id is already present.
     */
    fun addOptimisticMessage(message: Message, conversation: Conversation) {
        _messagesByConversation.update { map ->
            val existing = map[conversation.id] ?: emptyList()
            if (existing.any { it.id == message.id }) return@update map
            map + (conversation.id to (existing + message))
        }
        _conversations.update { list ->
            list.map { c ->
                if (c.id == conversation.id) {
                    c.copy(
                        lastMessage = message.body,
                        // Mirror the message's attachment type (null for plain
                        // text, which clears any prior "📷 Photo"/"📎 Attachment").
                        lastMessageAttachmentContentType = message.attachments.firstOrNull()?.contentType,
                        lastMessageDate = Date(message.sentAtMs),
                        lastMessageSenderDid = message.senderAccountId,
                    )
                } else c
            }
        }
    }

    /**
     * Mark all messages in a conversation as read.
     * Mirrors iOS AppState.markAllMessagesRead(conversationId:accountId:).
     *
     * Persistence and receipts run off the store, not the in-memory transcript,
     * so this works on the very first open of a conversation — before its
     * transcript has finished loading. (Previously this returned early when the
     * transcript wasn't loaded, silently no-op'ing on first open and inflating
     * the badge over time.)
     */
    fun markAllMessagesRead(conversationId: String, accountId: String) {
        val nowMs = System.currentTimeMillis()

        // Optimistic UI: clear unread in the loaded transcript (if any) and in
        // the persisted-count cache, then recompute the notification badge.
        _messagesByConversation.value[conversationId]?.let { msgs ->
            var changed = false
            val updatedMsgs = msgs.map { msg ->
                if (msg.readAtMs == null && msg.senderAccountId != accountId) {
                    changed = true
                    msg.copy(readAtMs = nowMs)
                } else msg
            }
            if (changed) {
                _messagesByConversation.update { it + (conversationId to updatedMsgs) }
            }
        }
        _unreadCounts.update { it + (conversationId to 0) }
        // The conversation is fully read: cancel its delivered notifications so
        // the launcher drops the app-icon badge (Android derives the badge from
        // active notifications; reading in-app doesn't dismiss them otherwise).
        NotificationPresenter.cancelNotifications(context = applicationContext, conversationId = conversationId)
        NotificationPresenter.updateBadge(context = applicationContext, appViewModel = this)

        val core = cores[accountId] ?: return
        val convId = conversationId
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                // Read the still-unread inbound messages from the store *before*
                // marking, so we know which (sender, timestamp) pairs to ack
                // regardless of whether the transcript was loaded in memory.
                val unread = (runCatching { core.loadMessages(conversationId = convId) }.getOrNull() ?: emptyList())
                    .filter { it.readAtMs == null && it.senderDid != accountId }
                runCatching { core.markMessagesRead(conversationId = convId, upToSentAtMs = nowMs) }
                val timestampsBySender = unread.groupBy { it.senderDid }
                    .mapValues { (_, ms) -> ms.map { it.sentAtMs } }
                for ((senderDid, timestamps) in timestampsBySender) {
                    runCatching {
                        core.sendReadReceipt(recipientDid = senderDid, timestamps = timestamps)
                    }
                }
            }
        }
    }

    /**
     * Mark messages as read up to (and including) the given sentAtMs timestamp.
     * Mirrors iOS AppState.markMessagesReadUpTo(sentAtMs:conversationId:accountId:).
     */
    fun markMessagesReadUpTo(sentAtMs: Long, conversationId: String, accountId: String) {
        val msgs = _messagesByConversation.value[conversationId] ?: return
        val nowMs = System.currentTimeMillis()
        val readTimestampsBySender = mutableMapOf<String, MutableList<Long>>()
        var changed = false

        val updatedMsgs = msgs.map { msg ->
            if (msg.readAtMs == null && msg.senderAccountId != accountId && msg.sentAtMs <= sentAtMs) {
                changed = true
                readTimestampsBySender.getOrPut(msg.senderAccountId) { mutableListOf() }
                    .add(msg.sentAtMs)
                msg.copy(readAtMs = nowMs)
            } else msg
        }
        if (!changed) return

        _messagesByConversation.update { it + (conversationId to updatedMsgs) }
        // If nothing remains unread in this conversation, clear its delivered
        // notifications so the launcher drops the app-icon badge (see
        // markAllMessagesRead / NotificationPresenter.cancelNotifications).
        val stillUnread = updatedMsgs.any { it.readAtMs == null && it.senderAccountId != accountId }
        if (!stillUnread) {
            NotificationPresenter.cancelNotifications(context = applicationContext, conversationId = conversationId)
        }
        NotificationPresenter.updateBadge(context = applicationContext, appViewModel = this)

        val core = cores[accountId] ?: return
        val convId = conversationId
        val timestampsBySender = readTimestampsBySender.toMap()
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                runCatching { core.markMessagesRead(conversationId = convId, upToSentAtMs = sentAtMs) }
                for ((senderDid, timestamps) in timestampsBySender) {
                    runCatching {
                        core.sendReadReceipt(recipientDid = senderDid, timestamps = timestamps)
                    }
                }
            }
        }
    }

    /**
     * Load persisted messages from SQLCipher for a conversation.
     * Mirrors iOS AppState.loadConversationsFromStore().
     */
    private suspend fun loadConversationsFromStore() {
        val pairs = _accounts.value.mapNotNull { acct ->
            cores[acct.id]?.let { acct.id to it }
        }

        data class AccountSummaries(
            val accountId: String,
            val summaries: List<uniffi.app_core.ConversationSummaryFfi>,
            // did -> locally-known display name, resolved up front so the first
            // render shows names instead of raw DIDs / "Unknown".
            val localNames: Map<String, String>,
        )

        val summariesPerAccount: List<AccountSummaries> = pairs.map { (accountId, core) ->
            withContext(Dispatchers.IO) {
                val rows = runCatching { core.loadConversations() }.getOrElse { emptyList() }
                // Warm the display-name cache from local storage (no network) for
                // every DID these rows will render — DM peers, plus group
                // last-message senders and system-event actor/target DIDs.
                // Otherwise those previews fall back to the raw DID / "Unknown"
                // and only correct once the async resolver runs, causing a visible
                // flash on cold launch. One bulk FFI call.
                val dids = displayNameDidsToWarm(rows, accountId)
                val localNames = runCatching { core.cachedDisplayNames(dids) }.getOrElse { emptyMap() }
                AccountSummaries(accountId, rows, localNames)
            }
        }

        // Seed the in-memory cache so both the title computed below and any
        // subsequent reads (e.g. ConversationRow) see the resolved names.
        for (acctSummary in summariesPerAccount) {
            for ((did, name) in acctSummary.localNames) {
                displayNameCache[did] = name
            }
        }

        val newConvs = mutableListOf<Conversation>()
        val newUnread = mutableMapOf<String, Int>()
        val groupsNeedingRefresh = mutableListOf<Pair<String, String>>() // groupId to accountId

        for (acctSummary in summariesPerAccount) {
            val accountId = acctSummary.accountId
            val summaries = acctSummary.summaries
            val serverUrl = _accounts.value.firstOrNull { it.id == accountId }?.servers?.firstOrNull()?.id ?: ""
            for (s in summaries) {
                newUnread[s.conversationId] = s.unreadCount.toInt()
                val lastMsg = s.lastMessage
                val date = lastMsg?.let { Date(it.sentAtMs) }
                val preview = lastMsg?.body
                val lastAttachmentCt = s.lastMessageAttachmentContentType
                val lastKind = lastMsg?.kind?.toInt() ?: 0
                val lastMeta = lastMsg?.metadata
                val lastSender = lastMsg?.senderDid

                val groupId = groupIdFromConversationId(s.conversationId)
                if (groupId != null) {
                    val groupTitle = s.groupTitle
                    if (!groupTitle.isNullOrEmpty()) {
                        groupTitleCache[groupId] = groupTitle
                    }
                    val title = groupTitleCache[groupId] ?: "Group"
                    if (groupTitleCache[groupId] == null) {
                        groupsNeedingRefresh.add(groupId to accountId)
                    }
                    newConvs.add(
                        Conversation(
                            id = s.conversationId,
                            title = title,
                            accountId = accountId,
                            serverUrl = serverUrl,
                            recipientDid = null,
                            groupId = groupId,
                            lastMessage = preview,
                            lastMessageAttachmentContentType = lastAttachmentCt,
                            lastMessageDate = date,
                            lastMessageKind = lastKind,
                            lastMessageMetadata = lastMeta,
                            lastMessageSenderDid = lastSender,
                            isGroup = true,
                        )
                    )
                    continue
                }
                val recipientDid = recipientDidFromConversationId(s.conversationId, accountId)
                val title = recipientDid?.let { displayNameCache[it] } ?: recipientDid ?: s.conversationId
                newConvs.add(
                    Conversation(
                        id = s.conversationId,
                        title = title,
                        accountId = accountId,
                        serverUrl = serverUrl,
                        recipientDid = recipientDid,
                        lastMessage = preview,
                        lastMessageAttachmentContentType = lastAttachmentCt,
                        lastMessageDate = date,
                        isGroup = false,
                        isRequest = s.isRequest,
                        isBlocked = s.isBlocked,
                    )
                )
            }
        }

        val sorted = newConvs.sortedByDescending { it.lastMessageDate?.time ?: Long.MIN_VALUE }
        _conversations.value = sorted
        _unreadCounts.value = newUnread
        _conversationsLoaded.value = true

        // Flush a notification tap that arrived before the list was ready
        // (cold-start launch): now that conversations exist, open the target.
        pendingOpenConversation?.let { (convId, acct) ->
            openConversationById(conversationId = convId, accountId = acct)
        }

        // Kick off async name resolution for any conversation still showing the raw DID.
        for (conv in sorted) {
            val rid = conv.recipientDid
            if (rid != null && conv.title == rid) {
                displayName(did = rid, accountId = conv.accountId)
            }
        }

        // For groups with no locally-cached title, fetch from server.
        for ((groupId, accountId) in groupsNeedingRefresh) {
            refreshGroupTitle(groupId = groupId, accountId = accountId)
        }
    }

    /**
     * The set of DIDs whose display name a batch of conversation summaries will
     * render: the DM peer for each DM, and for each group its last-message
     * sender plus the actor/target of a system-event preview. Used to warm
     * [displayNameCache] from local storage before the rows are built.
     * Mirrors iOS AppState.displayNameDidsToWarm(summaries:accountId:).
     */
    private fun displayNameDidsToWarm(
        summaries: List<uniffi.app_core.ConversationSummaryFfi>,
        accountId: String,
    ): List<String> {
        val dids = mutableSetOf<String>()
        for (s in summaries) {
            if (groupIdFromConversationId(s.conversationId) != null) {
                val last = s.lastMessage ?: continue
                if (last.senderDid.isNotEmpty()) dids.add(last.senderDid)
                // System-event previews resolve actor/target DIDs (e.g.
                // "Alice made Bob an admin"), so warm those too.
                if (last.kind.toInt() > 0) {
                    val m = Message(
                        id = last.id,
                        conversationId = last.conversationId,
                        senderAccountId = last.senderDid,
                        body = last.body,
                        sentAtMs = last.sentAtMs,
                        kind = last.kind.toInt(),
                        metadata = last.metadata,
                    )
                    m.groupEvent?.let { ev ->
                        if (ev.actorDid.isNotEmpty()) dids.add(ev.actorDid)
                        if (ev.targetDid.isNotEmpty()) dids.add(ev.targetDid)
                    }
                }
            } else {
                recipientDidFromConversationId(s.conversationId, accountId)?.let { dids.add(it) }
            }
        }
        return dids.toList()
    }

    /** Parse the recipient DID out of a DM conversation ID. Returns null for non-DM IDs. */
    private fun recipientDidFromConversationId(conversationId: String, accountId: String): String? {
        val prefix = "dm-$accountId-"
        if (!conversationId.startsWith(prefix)) return null
        return conversationId.removePrefix(prefix)
    }

    /** Parse the group_id out of a group conversation ID. Returns null for non-group IDs. */
    private fun groupIdFromConversationId(conversationId: String): String? {
        val prefix = "group-"
        if (!conversationId.startsWith(prefix)) return null
        return conversationId.removePrefix(prefix)
    }

    /**
     * Re-read a group's timeline from the store, but only if already loaded.
     * Mirrors iOS AppState.reloadGroupTimelineIfLoaded(groupId:accountId:).
     */
    fun reloadGroupTimelineIfLoaded(groupId: String, accountId: String) {
        reloadMessagesIfLoaded(conversationId = groupConversationId(groupId), accountId = accountId)
    }

    /**
     * Re-read a conversation's timeline from the store, but only if already loaded.
     * Mirrors iOS AppState.reloadMessagesIfLoaded(conversationId:accountId:).
     */
    fun reloadMessagesIfLoaded(conversationId: String, accountId: String) {
        val core = cores[accountId] ?: return
        if (_messagesByConversation.value[conversationId] == null) return
        viewModelScope.launch {
            val msgs = withContext(Dispatchers.IO) {
                runCatching { core.loadMessages(conversationId = conversationId) }.getOrNull()
            } ?: return@launch
            val messages = msgs.map { messageFromFfi(it) }
            _messagesByConversation.update { it + (conversationId to messages) }
        }
    }

    /** Map a stored FFI message row to the view Message model. */
    fun messageFromFfi(m: StoredMessageFfi): Message = Message(
        id = m.id,
        conversationId = m.conversationId,
        senderAccountId = m.senderDid,
        body = m.body,
        sentAtMs = m.sentAtMs,
        editedAtMs = m.editedAtMs,
        readAtMs = m.readAtMs,
        deliveryStatus = DeliveryStatus.fromCode(m.deliveryStatus.toInt()),
        editCount = m.editCount.toInt(),
        isDeleted = m.deleted,
        kind = m.kind.toInt(),
        metadata = m.metadata,
        expireTimerSecs = m.expireTimerSecs,
        expireAtMs = m.expireAtMs,
        attachments = m.attachments,
        previews = m.previews,
    )

    /**
     * Load persisted messages from SQLCipher for a conversation.
     * Mirrors iOS AppState.loadMessagesFromStore(conversationId:accountId:).
     */
    fun loadMessagesFromStore(conversationId: String, accountId: String) {
        val core = cores[accountId] ?: return
        if (_messagesByConversation.value[conversationId] != null) return
        viewModelScope.launch {
            val msgs = withContext(Dispatchers.IO) {
                runCatching { core.loadMessages(conversationId = conversationId) }.getOrNull()
            } ?: return@launch
            val messages = msgs.map { messageFromFfi(it) }
            // Only write if still not loaded (guard against races).
            if (_messagesByConversation.value[conversationId] == null) {
                _messagesByConversation.update { it + (conversationId to messages) }
            }
        }
    }

    /**
     * Find or create a DM conversation with a recipient DID.
     * Mirrors iOS AppState.findOrCreateDMConversation(recipientDid:accountId:).
     */
    fun findOrCreateDMConversation(recipientDid: String, accountId: String): Conversation {
        _conversations.value.firstOrNull {
            it.accountId == accountId && it.recipientDid == recipientDid
        }?.let { return it }

        val serverUrl = _accounts.value.firstOrNull { it.id == accountId }?.servers?.firstOrNull()?.id ?: ""
        val convId = "dm-$accountId-$recipientDid"
        val title = displayName(did = recipientDid, accountId = accountId)
        val conv = Conversation(
            id = convId,
            title = title,
            accountId = accountId,
            serverUrl = serverUrl,
            recipientDid = recipientDid,
            isGroup = false,
        )
        _conversations.update { it + conv }
        return conv
    }

    /**
     * Find or create a group conversation.
     * Mirrors iOS AppState.findOrCreateGroupConversation(groupId:title:accountId:serverUrl:).
     */
    fun findOrCreateGroupConversation(
        groupId: String,
        title: String,
        accountId: String,
        serverUrl: String,
    ): Conversation {
        val convId = groupConversationId(groupId)
        _conversations.value.firstOrNull { it.id == convId }?.let { return it }

        val conv = Conversation(
            id = convId,
            title = title,
            accountId = accountId,
            serverUrl = serverUrl,
            recipientDid = null,
            groupId = groupId,
            isGroup = true,
        )
        _conversations.update { it + conv }
        groupTitleCache[groupId] = title
        return conv
    }

    /**
     * Whether the current identity is still a member of a group.
     * Mirrors iOS AppState.isGroupMember(groupId:accountId:).
     */
    suspend fun isGroupMember(groupId: String, accountId: String): Boolean {
        val core = cores[accountId] ?: return true
        return withContext(Dispatchers.IO) {
            runCatching { core.isGroupMember(groupId = groupId) }.getOrElse { true }
        }
    }

    /**
     * Refresh the cached title for a group from fetchGroupState.
     * Mirrors iOS AppState.refreshGroupTitle(groupId:accountId:).
     */
    fun refreshGroupTitle(groupId: String, accountId: String) {
        val core = cores[accountId] ?: return
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                runCatching { core.applyPendingGroupChanges(groupId = groupId) }
            }
            val summary = withContext(Dispatchers.IO) {
                runCatching { core.fetchGroupState(groupId = groupId) }.getOrNull()
            } ?: return@launch
            val title = if (summary.title.isEmpty()) "Group" else summary.title
            groupTitleCache[groupId] = title
            val convId = groupConversationId(groupId)
            _conversations.update { list ->
                list.map { if (it.id == convId) it.copy(title = title) else it }
            }
        }
    }

    /**
     * Create a new group and return the conversation.
     * Mirrors iOS AppState.createGroupAndOpen().
     */
    suspend fun createGroupAndOpen(
        accountId: String,
        serverUrl: String,
        title: String,
        recipientDids: List<String>,
        expirySeconds: UInt,
        firstMessage: String?,
    ): Conversation {
        val core = cores[accountId]
            ?: throw IllegalStateException("No core for account")

        val created = withContext(Dispatchers.IO) {
            core.createGroup(title = title, description = "", expirySeconds = expirySeconds)
        }
        val groupId = created.groupId

        // Fan out invites — best-effort.
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                for (did in recipientDids) {
                    runCatching {
                        core.inviteMember(groupId = groupId, recipientDid = did, role = 0)
                    }.onFailure {
                        AppLog.warn("compose", "invite $did to $groupId failed: ${it.message}")
                    }
                }
            }
        }

        val conv = findOrCreateGroupConversation(
            groupId = groupId,
            title = title.ifEmpty { "Group" },
            accountId = accountId,
            serverUrl = serverUrl,
        )

        if (!firstMessage.isNullOrEmpty()) {
            val messageId = UUID.randomUUID().toString()
            val nowMs = System.currentTimeMillis()
            val optimistic = Message(
                id = messageId,
                conversationId = conv.id,
                senderAccountId = accountId,
                body = firstMessage,
                sentAtMs = nowMs,
                readAtMs = nowMs,
                deliveryStatus = DeliveryStatus.SENDING,
            )
            _messagesByConversation.update { map ->
                val existing = map[conv.id] ?: emptyList()
                map + (conv.id to (existing + optimistic))
            }
            sendComposed(
                conversation = conv,
                text = firstMessage,
                messageId = messageId,
                sentAtMs = nowMs,
            )
        }
        return conv
    }

    // -----------------------------------------------------------------------
    // Contacts (docs/52-contacts-and-profiles.md)
    // -----------------------------------------------------------------------

    /**
     * Snapshot of the contact list for the given account.
     * Mirrors iOS AppState.listContacts(accountId:).
     */
    suspend fun listContacts(accountId: String): List<ContactRowFfi> {
        val core = cores[accountId] ?: return emptyList()
        return withContext(Dispatchers.IO) {
            runCatching { core.listContacts() }.getOrElse { emptyList() }
        }
    }

    /**
     * Poll messages for an account.
     * Mirrors iOS AppState.pollMessages(for:).
     */
    suspend fun pollMessages(accountId: String): List<DecryptedMessage> {
        val core = cores[accountId] ?: return emptyList()
        return withContext(Dispatchers.IO) {
            runCatching { core.receiveMessages() }.getOrElse { emptyList() }
        }
    }

    /**
     * Fetch the list of Projects from a server.
     * Mirrors iOS AppState.fetchProjects(serverUrl:).
     */
    suspend fun fetchProjects(serverUrl: String): List<ProjectInfo> {
        val account = _accounts.value.firstOrNull { acct ->
            acct.servers.any { it.id == serverUrl }
        } ?: return emptyList()
        val core = cores[account.id] ?: return emptyList()
        return withContext(Dispatchers.IO) {
            runCatching {
                core.fetchProjects().map { ProjectInfo(name = it.name, url = it.url, description = it.description, clientId = it.clientId, official = it.official) }
            }.onFailure {
                AppLog.warn("projects", "Failed to fetch projects: ${it.message}")
            }.getOrElse { emptyList() }
        }
    }

    /**
     * Request a Project token from the homeserver.
     * Mirrors iOS AppState.requestProjectToken(accountId:projectUrl:).
     */
    suspend fun requestProjectToken(accountId: String, projectUrl: String): String {
        val core = cores[accountId] ?: throw IllegalStateException("No account")
        return withContext(Dispatchers.IO) { core.requestProjectToken(projectUrl = projectUrl) }
    }

    // -----------------------------------------------------------------------
    // Project login ("Sign in with Avalanche", docs/25)
    // Mirrors iOS AppState project-login methods.
    // -----------------------------------------------------------------------

    private fun normalizedServer(s: String): String = s.trimEnd('/')

    /**
     * Parse an `authorize` deep link and, if the user has an account on the
     * target homeserver, stage a consent request; otherwise surface the
     * structured no-account failure.
     */
    fun handleAuthorizeDeepLink(uri: Uri) {
        val clientId = uri.getQueryParameter("client_id")
        val serverUrl = uri.getQueryParameter("server_url")
        if (clientId == null || serverUrl == null) {
            AppLog.info("Login", "authorize link missing client_id/server_url")
            return
        }
        val account = _accounts.value.firstOrNull { acct ->
            acct.servers.any { normalizedServer(it.url.toString()) == normalizedServer(serverUrl) }
        }
        if (account == null) {
            AppLog.info("Login", "no account on $serverUrl")
            _loginError.value = ProjectLoginError.NoAccountOnServer(serverUrl)
            return
        }

        val userCode = uri.getQueryParameter("user_code")
        val redirectUri = uri.getQueryParameter("redirect_uri")
        val codeChallenge = uri.getQueryParameter("code_challenge")
        val flow: ProjectLoginFlow = when {
            userCode != null -> ProjectLoginFlow.Device(userCode)
            redirectUri != null && codeChallenge != null -> ProjectLoginFlow.AuthorizationCode(
                redirectUri = redirectUri,
                codeChallenge = codeChallenge,
                codeChallengeMethod = uri.getQueryParameter("code_challenge_method") ?: "S256",
                state = uri.getQueryParameter("state"),
            )
            else -> {
                AppLog.info("Login", "authorize link missing flow params")
                return
            }
        }

        val req = ProjectLoginRequest(
            clientId = clientId,
            serverUrl = serverUrl,
            accountId = account.id,
            scope = uri.getQueryParameter("scope"),
            flow = flow,
        )
        _pendingLoginRequest.value = req
        viewModelScope.launch { resolveLoginProjectMetadata(req.id, serverUrl, clientId) }
    }

    private suspend fun resolveLoginProjectMetadata(id: String, serverUrl: String, clientId: String) {
        val match = fetchProjects(serverUrl).firstOrNull { it.clientId == clientId } ?: return
        val current = _pendingLoginRequest.value
        if (current?.id == id) {
            _pendingLoginRequest.value = current.copy(
                projectName = match.name,
                projectUrl = match.url,
                official = match.official,
            )
        }
    }

    /**
     * Approve a staged login: mint the code (same-device, then redirect the
     * browser) or approve the device grant (cross-device).
     */
    fun approveLogin(req: ProjectLoginRequest) {
        val core = cores[req.accountId]
        if (core == null) {
            _loginError.value = ProjectLoginError.Failed("No account for this login")
            _pendingLoginRequest.value = null
            return
        }
        viewModelScope.launch {
            try {
                when (val flow = req.flow) {
                    is ProjectLoginFlow.Device -> {
                        withContext(Dispatchers.IO) { core.oauthApproveDevice(flow.userCode, req.clientId) }
                        _pendingLoginRequest.value = null
                    }
                    is ProjectLoginFlow.AuthorizationCode -> {
                        val code = withContext(Dispatchers.IO) {
                            core.oauthIssueCode(req.clientId, flow.redirectUri, flow.codeChallenge, flow.codeChallengeMethod, req.scope)
                        }
                        _pendingLoginRequest.value = null
                        openLoginRedirect(flow.redirectUri, code, flow.state)
                    }
                }
            } catch (e: Exception) {
                _pendingLoginRequest.value = null
                _loginError.value = ProjectLoginError.Failed(e.message ?: "Login failed")
            }
        }
    }

    /** Redirect the browser back to the Project with the authorization code. */
    private fun openLoginRedirect(redirectUri: String, code: String, state: String?) {
        val builder = Uri.parse(redirectUri).buildUpon().appendQueryParameter("code", code)
        if (state != null) builder.appendQueryParameter("state", state)
        val intent = Intent(Intent.ACTION_VIEW, builder.build()).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        runCatching { applicationContext.startActivity(intent) }
            .onFailure { AppLog.error("Login", "failed to open redirect: ${it.message}") }
    }

    // -----------------------------------------------------------------------
    // Connection state + incoming events
    // -----------------------------------------------------------------------

    /**
     * Aggregate connection state across all accounts.
     * Mirrors iOS AppState.aggregateConnectionState.
     */
    val aggregateConnectionState: ConnectionState
        get() {
            val states = _connectionStates.value.values
            if (states.isEmpty()) return ConnectionState.Connected
            if (states.all { it is ConnectionState.Connected }) return ConnectionState.Connected

            var bestReconnect: Long? = null
            var sawConnecting = false
            var sawDisconnected = false
            for (state in states) {
                when (state) {
                    is ConnectionState.Reconnecting -> {
                        val cur = bestReconnect
                        bestReconnect = if (cur != null) minOf(cur, state.nextAttemptAtMs) else state.nextAttemptAtMs
                    }
                    is ConnectionState.Connecting -> sawConnecting = true
                    is ConnectionState.Disconnected -> sawDisconnected = true
                    is ConnectionState.Connected -> Unit
                }
            }
            if (bestReconnect != null) return ConnectionState.Reconnecting(nextAttemptAtMs = bestReconnect)
            if (sawConnecting) return ConnectionState.Connecting
            if (sawDisconnected) return ConnectionState.Disconnected
            return ConnectionState.Connected
        }

    /**
     * Start the per-account state + event listener coroutines for any account
     * that has a live core. Idempotent — restarts only missing jobs.
     * Mirrors iOS AppState.startMessagePolling().
     */
    fun startMessagePolling() {
        for (account in _accounts.value) {
            val accountId = account.id
            if (cores[accountId] == null) continue
            if (stateJobs[accountId] == null) {
                stateJobs[accountId] = viewModelScope.launch {
                    connectionStateLoop(accountId = accountId)
                }
            }
            if (eventJobs[accountId] == null) {
                eventJobs[accountId] = viewModelScope.launch {
                    eventLoop(accountId = accountId)
                }
            }
        }
    }

    /** Cancel all per-account listener coroutines. Called on logout/mode switch. */
    private fun cancelAllListenerJobs() {
        stateJobs.values.forEach { it.cancel() }
        stateJobs.clear()
        eventJobs.values.forEach { it.cancel() }
        eventJobs.clear()
    }

    /**
     * Block on waitForConnectionStateChange in a loop and mirror updates
     * into connectionStates[accountId].
     * Mirrors iOS AppState.connectionStateLoop(accountId:).
     */
    private suspend fun connectionStateLoop(accountId: String) {
        AppLog.info("conn", "starting connection-state listener for $accountId")
        val core = cores[accountId] ?: return

        var last: ConnectionState = withContext(Dispatchers.IO) { core.connectionState() }
        _connectionStates.update { it + (accountId to last) }

        while (true) {
            val currentCore = cores[accountId] ?: break
            val lastSnapshot = last
            val next: ConnectionState = runCatching {
                withContext(Dispatchers.IO) {
                    currentCore.waitForConnectionStateChange(last = lastSnapshot)
                }
            }.getOrElse { error ->
                AppLog.warn("conn", "state listener for $accountId ended: ${error.message}")
                null
            } ?: break

            last = next
            _connectionStates.update { it + (accountId to next) }

            // Coming back online — clear the session negative-name cache so names retry.
            if (next is ConnectionState.Connected) {
                unresolvedDids.clear()
            }
        }

        stateJobs.remove(accountId)
        AppLog.info("conn", "connection-state listener ended for $accountId")
    }

    /**
     * Block on nextEvents in a loop and dispatch each event.
     * Mirrors iOS AppState.eventLoop(accountId:).
     */
    private suspend fun eventLoop(accountId: String) {
        AppLog.info("evt", "starting event listener for $accountId")
        while (true) {
            val core = cores[accountId] ?: break
            val events: List<IncomingEvent> = runCatching {
                withContext(Dispatchers.IO) { core.nextEvents() }
            }.getOrElse { error ->
                AppLog.warn("evt", "event listener for $accountId ended: ${error.message}")
                null
            } ?: break

            val messages = mutableListOf<DecryptedMessage>()
            val receiptUpdates = mutableListOf<DeliveryStatusUpdate>()
            var needsConversationReload = false
            val groupsWithNewEvents = mutableSetOf<String>()

            for (ev in events) {
                when (ev) {
                    is IncomingEvent.Message -> messages.add(ev.msg)
                    is IncomingEvent.ReceiptUpdate -> receiptUpdates.add(ev.update)
                    is IncomingEvent.GroupInvite -> needsConversationReload = true
                    is IncomingEvent.GroupMetadataChanged -> {
                        groupsWithNewEvents.add(ev.event.groupId)
                        needsConversationReload = true
                    }
                    is IncomingEvent.StorageSynced -> {
                        // A background storage sync applied remote durable state
                        // (e.g. a group key synced from another device, or an
                        // updated contact/profile). This never touches message
                        // history, so just rebuild the chat list — newly-synced
                        // groups/contacts appear without a restart.
                        needsConversationReload = true
                    }
                    is IncomingEvent.ConversationUpdated -> {
                        // A `SyncSent`/`SyncRead` transcript from another of my
                        // own devices (docs/04 §5.4) changed exactly this
                        // conversation's stored content (a message I sent, an
                        // edit/delete/reaction I made, or read-state I cleared).
                        // Re-read just this timeline so it surfaces live, and
                        // refresh the chat-list preview.
                        reloadMessagesIfLoaded(
                            conversationId = ev.conversationId,
                            accountId = accountId,
                        )
                        needsConversationReload = true
                    }
                    is IncomingEvent.MessageEdited -> {
                        applyInboundEdit(
                            conversationId = ev.conversationId,
                            authorDid = ev.authorDid,
                            sentAtMs = ev.sentAtMs,
                            newBody = ev.newBody,
                            editedAtMs = ev.editedAtMs,
                        )
                    }
                    is IncomingEvent.MessageDeleted -> {
                        applyInboundDelete(
                            conversationId = ev.conversationId,
                            authorDid = ev.authorDid,
                            sentAtMs = ev.sentAtMs,
                        )
                    }
                    is IncomingEvent.ReactionUpdated -> {
                        applyInboundReaction(
                            conversationId = ev.conversationId,
                            targetAuthor = ev.targetAuthor,
                            targetSentAtMs = ev.targetSentAtMs,
                            reactorDid = ev.reactorDid,
                            emoji = ev.emoji,
                            removed = ev.removed,
                        )
                    }
                    is IncomingEvent.MessagesExpired -> {
                        for (convId in ev.conversationIds) {
                            reloadMessagesIfLoaded(conversationId = convId, accountId = accountId)
                        }
                        needsConversationReload = true
                    }
                    // New IncomingEvent variants are intentionally ignored here until
                    // explicitly handled; this is a statement `when`, so the compiler
                    // does not force exhaustiveness.
                }
            }

            for (msg in messages) {
                handleIncomingMessage(msg, accountId = accountId)
            }
            if (receiptUpdates.isNotEmpty()) {
                applyDeliveryStatusUpdates(receiptUpdates)
            }
            for (groupId in groupsWithNewEvents) {
                reloadGroupTimelineIfLoaded(groupId = groupId, accountId = accountId)
            }
            if (needsConversationReload) {
                loadConversationsFromStore()
            }
        }

        eventJobs.remove(accountId)
        AppLog.info("evt", "event listener ended for $accountId")
    }

    // -----------------------------------------------------------------------
    // Reactions, editing, deletion (docs/33, docs/36)
    // -----------------------------------------------------------------------

    /**
     * Reactions currently on a specific message.
     * Mirrors iOS AppState.reactions(for:).
     */
    fun reactions(message: Message): List<ReactionFfi> =
        (_reactionsByConversation.value[message.conversationId] ?: emptyList()).filter {
            it.targetAuthor == message.senderAccountId && it.targetSentAtMs == message.sentAtMs
        }

    /**
     * Load a conversation's reactions from the store into memory.
     * Mirrors iOS AppState.loadReactions(conversationId:accountId:).
     */
    fun loadReactions(conversationId: String, accountId: String) {
        val core = cores[accountId] ?: return
        viewModelScope.launch {
            val rows = withContext(Dispatchers.IO) {
                runCatching { core.loadReactions(conversationId = conversationId) }.getOrNull()
            } ?: return@launch
            _reactionsByConversation.update { it + (conversationId to rows) }
        }
    }

    /** Where a content op for conversation is directed — the unified MessageTarget. */
    private fun messageTarget(conversation: Conversation): MessageTarget? {
        val groupId = conversation.groupId
        if (groupId != null) return MessageTarget.Group(groupId = groupId)
        val recipientDid = conversation.recipientDid
        if (recipientDid != null) return MessageTarget.Dm(recipientDid = recipientDid)
        return null
    }

    /**
     * Toggle this account's reaction on a message.
     * Mirrors iOS AppState.toggleReaction(message:emoji:conversation:).
     */
    fun toggleReaction(message: Message, emoji: String, conversation: Conversation) {
        val core = cores[conversation.accountId] ?: return
        val target = messageTarget(conversation) ?: return
        val myDid = conversation.accountId
        val convId = conversation.id
        val targetAuthor = message.senderAccountId
        val targetSentAt = message.sentAtMs

        val existingMine = (_reactionsByConversation.value[convId] ?: emptyList()).firstOrNull {
            it.targetAuthor == targetAuthor && it.targetSentAtMs == targetSentAt && it.reactorDid == myDid
        }
        val remove = existingMine?.emoji == emoji
        val nowMs = System.currentTimeMillis()

        // Optimistic in-memory update.
        _reactionsByConversation.update { map ->
            val list = (map[convId] ?: emptyList()).toMutableList()
            list.removeAll { it.targetAuthor == targetAuthor && it.targetSentAtMs == targetSentAt && it.reactorDid == myDid }
            if (!remove) {
                list.add(
                    ReactionFfi(
                        conversationId = convId,
                        targetAuthor = targetAuthor,
                        targetSentAtMs = targetSentAt,
                        reactorDid = myDid,
                        emoji = emoji,
                        reactedAtMs = nowMs,
                    )
                )
            }
            map + (convId to list)
        }

        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                runCatching {
                    core.sendReaction(
                        target = target,
                        targetAuthor = targetAuthor,
                        targetSentAtMs = targetSentAt,
                        emoji = emoji,
                        remove = remove,
                        sentAtMs = nowMs,
                    )
                }
            }
        }
    }

    /**
     * Edit one of my own messages in place (docs/36).
     * Mirrors iOS AppState.editMessage(message:newBody:conversation:).
     */
    fun editMessage(message: Message, newBody: String, conversation: Conversation) {
        val core = cores[conversation.accountId] ?: return
        val target = messageTarget(conversation) ?: return
        val trimmed = newBody.trim()
        if (trimmed.isEmpty() || trimmed == message.body) return
        val nowMs = System.currentTimeMillis()
        val convId = conversation.id

        _messagesByConversation.update { map ->
            val msgs = map[convId]?.toMutableList() ?: return@update map
            val i = msgs.indexOfFirst { it.id == message.id }
            if (i < 0) return@update map
            msgs[i] = msgs[i].copy(body = trimmed, editedAtMs = nowMs, editCount = msgs[i].editCount + 1)
            map + (convId to msgs)
        }

        val targetSentAt = message.sentAtMs
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                runCatching {
                    core.sendEdit(
                        target = target,
                        targetSentAtMs = targetSentAt,
                        newBody = trimmed,
                        sentAtMs = nowMs,
                    )
                }
            }
        }
    }

    /**
     * Delete a message (docs/36).
     * Mirrors iOS AppState.deleteMessage(message:forEveryone:conversation:).
     */
    fun deleteMessage(message: Message, forEveryone: Boolean, conversation: Conversation) {
        val core = cores[conversation.accountId] ?: return
        val target = messageTarget(conversation) ?: return
        val nowMs = System.currentTimeMillis()
        val convId = conversation.id

        _messagesByConversation.update { map ->
            val msgs = map[convId]?.toMutableList() ?: return@update map
            if (forEveryone) {
                val i = msgs.indexOfFirst { it.id == message.id }
                if (i >= 0) {
                    msgs[i] = msgs[i].copy(body = "", isDeleted = true, editedAtMs = null)
                }
            } else {
                msgs.removeAll { it.id == message.id }
            }
            map + (convId to msgs)
        }
        _reactionsByConversation.update { map ->
            val list = (map[convId] ?: emptyList())
                .filterNot { it.targetAuthor == message.senderAccountId && it.targetSentAtMs == message.sentAtMs }
            map + (convId to list)
        }

        val targetAuthor = message.senderAccountId
        val targetSentAt = message.sentAtMs
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                runCatching {
                    core.sendDelete(
                        target = target,
                        targetAuthor = targetAuthor,
                        targetSentAtMs = targetSentAt,
                        forEveryone = forEveryone,
                        sentAtMs = nowMs,
                    )
                }
            }
        }
    }

    /**
     * Load the prior bodies of an edited message for the history sheet (docs/36).
     * Mirrors iOS AppState.loadMessageRevisions(message:conversation:).
     */
    suspend fun loadMessageRevisions(
        message: Message,
        conversation: Conversation,
    ): List<MessageRevisionFfi> {
        val core = cores[conversation.accountId] ?: return emptyList()
        return withContext(Dispatchers.IO) {
            runCatching {
                core.loadMessageRevisions(
                    conversationId = conversation.id,
                    author = message.senderAccountId,
                    sentAtMs = message.sentAtMs,
                )
            }.getOrElse { emptyList() }
        }
    }

    // Inbound op handlers — the store is already updated by app-core; these
    // patch the in-memory model so the open conversation refreshes live.

    private fun applyInboundEdit(
        conversationId: String,
        authorDid: String,
        sentAtMs: Long,
        newBody: String,
        editedAtMs: Long,
    ) {
        _messagesByConversation.update { map ->
            val msgs = map[conversationId]?.toMutableList() ?: return@update map
            val i = msgs.indexOfFirst { it.senderAccountId == authorDid && it.sentAtMs == sentAtMs }
            if (i < 0 || msgs[i].isDeleted) return@update map
            msgs[i] = msgs[i].copy(body = newBody, editedAtMs = editedAtMs, editCount = msgs[i].editCount + 1)
            map + (conversationId to msgs)
        }
    }

    private fun applyInboundDelete(
        conversationId: String,
        authorDid: String,
        sentAtMs: Long,
    ) {
        _messagesByConversation.update { map ->
            val msgs = map[conversationId]?.toMutableList() ?: return@update map
            val i = msgs.indexOfFirst { it.senderAccountId == authorDid && it.sentAtMs == sentAtMs }
            if (i < 0) return@update map
            msgs[i] = msgs[i].copy(body = "", isDeleted = true, editedAtMs = null)
            map + (conversationId to msgs)
        }
        _reactionsByConversation.update { map ->
            val list = (map[conversationId] ?: emptyList())
                .filterNot { it.targetAuthor == authorDid && it.targetSentAtMs == sentAtMs }
            map + (conversationId to list)
        }
    }

    private fun applyInboundReaction(
        conversationId: String,
        targetAuthor: String,
        targetSentAtMs: Long,
        reactorDid: String,
        emoji: String,
        removed: Boolean,
    ) {
        _reactionsByConversation.update { map ->
            val list = (map[conversationId] ?: emptyList()).toMutableList()
            list.removeAll { it.targetAuthor == targetAuthor && it.targetSentAtMs == targetSentAtMs && it.reactorDid == reactorDid }
            if (!removed) {
                val nowMs = System.currentTimeMillis()
                list.add(
                    ReactionFfi(
                        conversationId = conversationId,
                        targetAuthor = targetAuthor,
                        targetSentAtMs = targetSentAtMs,
                        reactorDid = reactorDid,
                        emoji = emoji,
                        reactedAtMs = nowMs,
                    )
                )
            }
            map + (conversationId to list)
        }
    }

    private fun handleIncomingMessage(msg: DecryptedMessage, accountId: String) {
        val senderDid = msg.senderDid
        val text = runCatching { String(msg.plaintext, Charsets.UTF_8) }.getOrElse { "(binary)" }
        // Type of the first attachment, if any (docs/35) — drives the chat-list
        // preview ("📷 Photo" / "📎 Attachment") for a caption-less attachment.
        // `null` for a plain message, which also clears any stale value below.
        val attachmentCt = msg.attachments.firstOrNull()?.contentType

        // Use the sender's timestamp if available, otherwise fall back to local
        // time. This must drive the conversation-row timestamp too (not the
        // arrival time): a late-delivered message — sent hours ago but only
        // decrypted now — would otherwise show "moments ago" in the list while
        // the bubble shows the real send time, and the row would jump back once
        // the list reloads from the store (which sorts by MAX(sent_at)).
        val sentAtMs: Long = msg.sentAtMs ?: System.currentTimeMillis()
        val lastMessageDate = Date(sentAtMs)

        val convId: String

        val groupId = msg.groupId
        if (groupId != null) {
            val serverUrl = _accounts.value.firstOrNull { it.id == accountId }?.servers?.firstOrNull()?.id ?: ""
            val title = groupTitleCache[groupId] ?: "Group"
            val conv = findOrCreateGroupConversation(
                groupId = groupId,
                title = title,
                accountId = accountId,
                serverUrl = serverUrl,
            )
            convId = conv.id
            _conversations.update { list ->
                list.map { c ->
                    if (c.id == convId) c.copy(
                        lastMessage = text,
                        lastMessageAttachmentContentType = attachmentCt,
                        lastMessageDate = lastMessageDate,
                        lastMessageSenderDid = senderDid,
                    ).clearLastMessageEvent()
                    else c
                }
            }
            refreshGroupTitle(groupId = groupId, accountId = accountId)
        } else {
            val existingIdx = _conversations.value.indexOfFirst {
                it.accountId == accountId && it.recipientDid == senderDid
            }
            if (existingIdx >= 0) {
                convId = _conversations.value[existingIdx].id
                _conversations.update { list ->
                    list.mapIndexed { idx, c ->
                        if (idx == existingIdx) c.copy(
                            lastMessage = text,
                            lastMessageAttachmentContentType = attachmentCt,
                            lastMessageDate = lastMessageDate,
                            lastMessageSenderDid = senderDid,
                        ).clearLastMessageEvent()
                        else c
                    }
                }
            } else {
                val serverUrl = _accounts.value.firstOrNull { it.id == accountId }?.servers?.firstOrNull()?.id ?: ""
                convId = "dm-$accountId-$senderDid"
                val dmTitle = displayName(did = senderDid, accountId = accountId)
                val conv = Conversation(
                    id = convId,
                    title = dmTitle,
                    accountId = accountId,
                    serverUrl = serverUrl,
                    recipientDid = senderDid,
                    lastMessage = text,
                    lastMessageAttachmentContentType = attachmentCt,
                    lastMessageDate = lastMessageDate,
                    lastMessageSenderDid = senderDid,
                    isGroup = false,
                )
                _conversations.update { it + conv }
            }
        }

        val messageId = UUID.randomUUID().toString()
        // If the user is currently viewing this conversation, treat the message
        // as read on arrival (and acknowledge it below) rather than flashing an
        // unread badge for something they're already looking at. We stamp
        // readAt directly on the persisted row — rather than a follow-up
        // markMessagesRead — so it can't race the save that writes the row.
        val isActive = _currentConversationId.value == convId
        val readAtMs: Long? = if (isActive) System.currentTimeMillis() else null
        val message = Message(
            id = messageId,
            conversationId = convId,
            senderAccountId = senderDid,
            body = text,
            sentAtMs = sentAtMs,
            readAtMs = readAtMs,
            deliveryStatus = DeliveryStatus.SENT,
            expireTimerSecs = msg.expireTimerSecs,
            attachments = msg.attachments,
            previews = msg.previews,
        )
        // Only append to the in-memory list if it's already loaded; otherwise
        // leave the entry absent so loadMessagesFromStore() does a full DB load
        // when the conversation is next opened. Appending into an absent entry
        // would create a one-element list (just this latest message), and the
        // non-null guard in loadMessagesFromStore() would then skip loading the
        // real history — showing only the latest message until app restart.
        // The message is persisted to SQLCipher below regardless.
        val transcriptLoaded = _messagesByConversation.value.containsKey(convId)
        if (transcriptLoaded) {
            _messagesByConversation.update { map ->
                val existing = map[convId] ?: return@update map
                map + (convId to (existing + message))
            }
        } else if (!isActive) {
            // Transcript not cached: bump the persisted-count cache so the
            // chat-list badge reflects this message without a full reload. (For
            // loaded conversations the badge counts the transcript directly.)
            _unreadCounts.update { it + (convId to ((it[convId] ?: 0) + 1)) }
        }
        NotificationPresenter.updateBadge(context = applicationContext, appViewModel = this)

        // Resolve the sender's name for the notification. A name we already hold
        // (own account or cached) lets us notify immediately; an unknown sender
        // is resolved inside the persistence coroutine below (after the profile
        // fetch) so the banner shows a real name instead of "Unknown".
        val convForNotif = _conversations.value.firstOrNull { it.id == convId }
        val knownName = _accounts.value.firstOrNull { it.id == senderDid }?.displayName
            ?: displayNameCache[senderDid]

        // Persist to SQLCipher in the background.
        val core = cores[accountId]
        if (core != null) {
            val stored = StoredMessageFfi(
                id = messageId,
                conversationId = convId,
                senderDid = senderDid,
                body = text,
                sentAtMs = sentAtMs,
                editedAtMs = null,
                readAtMs = readAtMs,  // read on arrival iff actively viewing
                deliveryStatus = 1u.toUByte(),
                editCount = 0u,
                deleted = false,
                kind = 0L,
                metadata = null,
                expireTimerSecs = msg.expireTimerSecs,
                expireAtMs = null,
                attachments = msg.attachments,
                previews = msg.previews,
            )
            val profileKey = msg.profileKey
            val isRequest = msg.isRequest
            viewModelScope.launch {
                val resolved = withContext(Dispatchers.IO) {
                    runCatching { core.saveMessage(msg = stored) }
                    // Acknowledge after the row is persisted (the FFI gates
                    // receipts to curated senders, so this no-ops for requests).
                    if (isActive) {
                        runCatching { core.sendReadReceipt(recipientDid = senderDid, timestamps = listOf(sentAtMs)) }
                    }
                    runCatching { core.touchContact(did = senderDid, curated = false) }
                    if (isRequest) {
                        runCatching { core.setPendingRequest(did = senderDid, pending = true) }
                    }
                    if (profileKey != null) {
                        runCatching { core.fetchAndCacheProfile(did = senderDid, profileKey = profileKey) }
                    }
                    // Read back the (possibly just-fetched) display name so an
                    // unknown sender's notification shows a real name, not "Unknown".
                    // Falls back to the public account record (getAccountInfo) so
                    // bots — whose names live there, not in an encrypted contact
                    // profile — also resolve. Mirrors resolveDisplayName().
                    val local = runCatching { core.contactDisplayName(did = senderDid) }
                        .getOrNull().orEmpty()
                    local.ifEmpty {
                        runCatching { core.getAccountInfo(did = senderDid) }
                            .getOrNull()?.displayName.orEmpty()
                    }.takeIf { it.isNotEmpty() }
                }
                if (resolved != null) cacheDisplayName(name = resolved, did = senderDid)
                // This branch is the notifier for an unknown sender — now with the
                // freshly-resolved name (or "Unknown" if it still didn't resolve).
                if (knownName == null && convForNotif != null) {
                    NotificationPresenter.present(
                        context = applicationContext,
                        message = message,
                        conversation = convForNotif,
                        senderDisplayName = resolved ?: "Unknown",
                        appViewModel = this@AppViewModel,
                    )
                }
            }
        }

        // Known sender (or no core to fetch with): notify immediately without
        // waiting on the network. present() suppresses the banner when the user
        // is already viewing this conversation and always refreshes the badge;
        // outgoing messages never reach this path.
        if (convForNotif != null && (knownName != null || core == null)) {
            NotificationPresenter.present(
                context = applicationContext,
                message = message,
                conversation = convForNotif,
                senderDisplayName = knownName ?: "Unknown",
                appViewModel = this,
            )
        }
    }

    // -----------------------------------------------------------------------
    // Delivery status updates
    // -----------------------------------------------------------------------

    private fun applyDeliveryStatusUpdates(updates: List<DeliveryStatusUpdate>) {
        for (update in updates) {
            _messagesByConversation.update { map ->
                val msgs = map[update.conversationId] ?: return@update map
                var changed = false
                val updated = msgs.map { msg ->
                    if (msg.sentAtMs == update.sentAtMs) {
                        val newStatus = DeliveryStatus.fromCode(update.deliveryStatus.toInt())
                        if (newStatus.code > msg.deliveryStatus.code) {
                            changed = true
                            msg.copy(deliveryStatus = newStatus)
                        } else msg
                    } else msg
                }
                if (changed) map + (update.conversationId to updated) else map
            }
        }
    }

    // -----------------------------------------------------------------------
    // Persistence helpers — mirrors iOS static AppState persistence section
    // -----------------------------------------------------------------------

    private fun loadPersistedAccounts(): List<PersistedAccount> {
        val json = prefs.getString(ACCOUNTS_KEY, null) ?: return emptyList()
        return runCatching {
            val arr = JSONArray(json)
            (0 until arr.length()).map { i ->
                val obj = arr.getJSONObject(i)
                val serversArr = obj.getJSONArray("servers")
                val servers = (0 until serversArr.length()).map { j ->
                    val s = serversArr.getJSONObject(j)
                    PersistedServer(
                        id = s.getString("id"),
                        name = s.getString("name"),
                        url = s.getString("url"),
                    )
                }
                PersistedAccount(
                    did = obj.getString("did"),
                    displayName = obj.getString("displayName"),
                    dbFilename = obj.getString("dbFilename"),
                    servers = servers,
                )
            }
        }.getOrElse { emptyList() }
    }

    private fun persistAccount(account: PersistedAccount) {
        val existing = loadPersistedAccounts().toMutableList()
        existing.removeAll { it.did == account.did }
        existing.add(account)
        val json = accountsToJson(existing)
        prefs.edit().putString(ACCOUNTS_KEY, json).apply()
    }

    private fun persistedDbFilename(did: String): String? =
        loadPersistedAccounts().firstOrNull { it.did == did }?.dbFilename

    private fun removePersistedAccount(did: String) {
        val existing = loadPersistedAccounts().filter { it.did != did }
        val json = accountsToJson(existing)
        prefs.edit().putString(ACCOUNTS_KEY, json).apply()
    }

    private fun clearPersistedAccounts() {
        prefs.edit().remove(ACCOUNTS_KEY).apply()
    }

    private fun accountsToJson(accounts: List<PersistedAccount>): String {
        val arr = JSONArray()
        for (a in accounts) {
            val obj = JSONObject()
            obj.put("did", a.did)
            obj.put("displayName", a.displayName)
            obj.put("dbFilename", a.dbFilename)
            val serversArr = JSONArray()
            for (s in a.servers) {
                val sObj = JSONObject()
                sObj.put("id", s.id)
                sObj.put("name", s.name)
                sObj.put("url", s.url)
                serversArr.put(sObj)
            }
            obj.put("servers", serversArr)
            arr.put(obj)
        }
        return arr.toString()
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /**
     * Human-readable label for a disappearing messages timer value in seconds.
     * Mirrors iOS DisappearingMessagesPicker.label(for:).
     */
    private fun disappearingMessagesLabel(seconds: UInt): String = when (seconds.toInt()) {
        5 -> "5 seconds"
        10 -> "10 seconds"
        30 -> "30 seconds"
        60 -> "1 minute"
        300 -> "5 minutes"
        1800 -> "30 minutes"
        3600 -> "1 hour"
        86400 -> "1 day"
        604800 -> "1 week"
        else -> "${seconds}s"
    }

    // -----------------------------------------------------------------------
    // ViewModel cleanup
    // -----------------------------------------------------------------------

    override fun onCleared() {
        super.onCleared()
        cancelAllListenerJobs()
    }
}

// ---------------------------------------------------------------------------
// ViewModel factory — needed to pass Context into the ViewModel constructor.
// ---------------------------------------------------------------------------

/**
 * [androidx.lifecycle.ViewModelProvider.Factory] for [AppViewModel].
 * Construct from Application context.
 */
class AppViewModelFactory(
    private val applicationContext: Context,
) : androidx.lifecycle.ViewModelProvider.Factory {
    @Suppress("UNCHECKED_CAST")
    override fun <T : androidx.lifecycle.ViewModel> create(modelClass: Class<T>): T {
        if (modelClass.isAssignableFrom(AppViewModel::class.java)) {
            return AppViewModel(applicationContext) as T
        }
        throw IllegalArgumentException("Unknown ViewModel class: ${modelClass.name}")
    }
}
