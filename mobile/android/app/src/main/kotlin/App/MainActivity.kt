package net.theavalanche.app

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Chat
import androidx.compose.material.icons.filled.Dns
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.lifecycle.ViewModelProvider
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navDeepLink

// ---------------------------------------------------------------------------
// Route constants — used by NavHost and all push-navigation call sites.
// ---------------------------------------------------------------------------

private object Route {
    const val SPLASH = "splash"
    const val MAIN = "main"                         // tab container (chats + network)
    const val CONVERSATION = "conversation/{id}"    // ConversationView
    const val ACCOUNTS = "accounts"                 // AccountsView
    const val COMPOSE = "compose"                   // ComposeMessageView
    const val NAME_GROUP = "name_group"             // NameGroupView
    const val GROUP_DETAIL = "group_detail/{groupId}/{accountId}"
    const val IDENTITY_DETAIL = "identity_detail/{did}"
    const val BLOCKED_CONTACTS = "blocked_contacts/{did}"
    const val SERVER_DETAIL = "server_detail/{serverUrl}"
    const val ADD_ACCOUNT = "add_account"
    const val SCANNER = "scanner"                   // QRScannerView (invite scanning)
    const val INVITE_LINK_ENTRY = "invite_link_entry"
    const val IDENTITY_PICKER = "identity_picker"
    const val NEW_ACCOUNT = "new_account"
    const val PASSKEY_EXPLAINER = "passkey_explainer/{displayName}"
    const val RECOVERY_PHRASE_SETUP = "recovery_phrase_setup/{displayName}"
    const val JOINING_SERVER = "joining_server/{did}"
    const val RECOVERY_EXPLAINER = "recovery_explainer"
    const val RECOVERY_CONSOLE = "recovery_console"
    const val LOG_VIEWER = "log_viewer"
    const val LINK_DEVICE = "link_device/{did}"        // existing-device side
    const val LINK_NEW_DEVICE = "link_new_device"      // new (joining) device side

    // helpers
    fun conversation(id: String) = "conversation/$id"
    fun groupDetail(groupId: String, accountId: String) = "group_detail/$groupId/$accountId"
    fun identityDetail(did: String) = "identity_detail/${Uri.encode(did)}"
    fun linkDevice(did: String) = "link_device/${Uri.encode(did)}"
    fun blockedContacts(did: String) = "blocked_contacts/${Uri.encode(did)}"
    fun serverDetail(serverUrl: String) = "server_detail/${Uri.encode(serverUrl)}"
    fun passkeyExplainer(displayName: String) = "passkey_explainer/${Uri.encode(displayName)}"
    fun recoveryPhraseSetup(displayName: String) = "recovery_phrase_setup/${Uri.encode(displayName)}"
    fun joiningServer(did: String) = "joining_server/${Uri.encode(did)}"
}

// ---------------------------------------------------------------------------
// MainActivity
//
// Hosts the root Compose UI. Mirrors the role of iOS ActnetApp / AppDelegate.
//
// The NavGraph replaces iOS NavigationStack + sheet navigation. Each screen
// composable receives callback lambdas; MainActivity wires them here.
// ---------------------------------------------------------------------------

class MainActivity : ComponentActivity() {

    private lateinit var appViewModel: AppViewModel

    // Android 13+ POST_NOTIFICATIONS runtime permission. The result is forwarded
    // to PushManager; FCM token registration happens regardless (data wakeups do
    // not need the permission — it only gates whether banners are shown).
    private val notificationPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            PushManager.onPermissionResult(granted = granted, appViewModel = appViewModel)
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Fully transparent system navigation bar so the app's background shows
        // through it edge-to-edge. `light` selects dark back/home icons for our
        // light (Paper) UI; the transparent scrims keep it clear on every API.
        enableEdgeToEdge(
            navigationBarStyle = SystemBarStyle.light(
                android.graphics.Color.TRANSPARENT,
                android.graphics.Color.TRANSPARENT,
            ),
        )

        // Create the notification channel (no-op on API < 26).
        NotificationPresenter.createNotificationChannel(this)

        val factory = AppViewModelFactory(applicationContext)
        appViewModel = ViewModelProvider(this, factory)[AppViewModel::class.java]
        // Publish the live ViewModel so ActnetFirebaseMessagingService can reach it.
        (application as? ActnetApplication)?.appViewModel = appViewModel
        appViewModel.restoreAccounts()
        intent?.let { handleIntent(it) }

        maybeRequestNotificationPermission()

        setContent {
            AvalancheTheme {
                AppNavGraph(appViewModel = appViewModel)
            }
        }
    }

    /** Request POST_NOTIFICATIONS on Android 13+ if not already granted. */
    private fun maybeRequestNotificationPermission() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
        val granted = ContextCompat.checkSelfPermission(
            this,
            Manifest.permission.POST_NOTIFICATIONS,
        ) == PackageManager.PERMISSION_GRANTED
        if (!granted) {
            notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }

    override fun onResume() {
        super.onResume()
        appViewModel.setIsAppActive(true)
    }

    override fun onPause() {
        super.onPause()
        appViewModel.setIsAppActive(false)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleIntent(intent)
    }

    /**
     * Route an incoming intent. A notification tap carries `conversationId` /
     * `accountId` extras (set in [NotificationPresenter.present]) and is opened
     * by direct lookup — this correctly opens group conversations, whose id is
     * `group-<groupId>` rather than a DM recipient DID. A genuine external deep
     * link (QR scan, invite URL) has no such extras and goes through
     * [AppViewModel.handleDeepLink].
     *
     * Mirrors iOS: AppDelegate.userNotificationCenter(_:didReceive:) opens the
     * notification's `conversationId`; onOpenURL handles external links.
     */
    private fun handleIntent(intent: Intent) {
        val conversationId = intent.getStringExtra("conversationId")
        val accountId = intent.getStringExtra("accountId")
        if (conversationId != null && accountId != null) {
            appViewModel.openConversationById(conversationId = conversationId, accountId = accountId)
            return
        }
        intent.data?.let { appViewModel.handleDeepLink(it) }
    }
}

// ---------------------------------------------------------------------------
// AppNavGraph
//
// Central NavHost. Mirrors iOS RootView + NavigationStack + sheet navigation.
//
// RootView logic:
//   - if isOnboarding -> SplashView (onboarding flow)
//   - else -> MainTabView (chats + network)
//   - OfflineBanner overlaid on top
//   - LogViewerView is reachable in debug builds by tapping the app-version
//     footer in AccountsView (the Android analog of the iOS debug gesture).
// ---------------------------------------------------------------------------

@Composable
fun AppNavGraph(
    appViewModel: AppViewModel,
) {
    val navController: NavHostController = rememberNavController()
    val isOnboarding by appViewModel.isOnboarding.collectAsState()

    // Start on the destination implied by the *initial* onboarding state, which
    // the ViewModel seeds synchronously from persisted accounts. Hardcoding
    // SPLASH here would flash the splash for a frame on every logged-in launch
    // before the effect below re-routes. Captured once so it doesn't change.
    val startDestination = remember {
        if (appViewModel.isOnboarding.value) Route.SPLASH else Route.MAIN
    }

    // React to later isOnboarding changes (login / logout), mirroring iOS
    // RootView Group { if appState.isOnboarding … }. Skip navigating when we're
    // already on the right destination — otherwise the first emission would pop
    // and re-push the start destination, reintroducing the flicker.
    LaunchedEffect(isOnboarding) {
        val target = if (isOnboarding) Route.SPLASH else Route.MAIN
        if (navController.currentDestination?.route != target) {
            navController.navigate(target) {
                popUpTo(0) { inclusive = true }
            }
        }
    }

    NavHost(
        navController = navController,
        startDestination = startDestination,
    ) {

        // ----------------------------------------------------------------
        // Splash / onboarding root
        // ----------------------------------------------------------------
        composable(Route.SPLASH) {
            SplashView(
                onScanInvite = { navController.navigate(Route.SCANNER) },
                onEnterLink = { navController.navigate(Route.INVITE_LINK_ENTRY) },
                // Recovery flows through the explainer screen first (mirrors iOS SplashView).
                onRecover = { navController.navigate(Route.RECOVERY_EXPLAINER) },
                onLinkDevice = { navController.navigate(Route.LINK_NEW_DEVICE) },
            )
        }

        // ----------------------------------------------------------------
        // Link to an existing device — new (joining) device side (docs/04 §4).
        // On success AppViewModel flips isOnboarding, re-routing to MAIN.
        // ----------------------------------------------------------------
        composable(Route.LINK_NEW_DEVICE) {
            LinkNewDeviceView(
                viewModel = appViewModel,
                onBack = { navController.popBackStack() },
            )
        }

        // ----------------------------------------------------------------
        // Recovery explainer — entry point for the account-recovery flow.
        // Actual signature: RecoveryExplainerView(accounts, onNavigateToRecoveryConsole,
        //   onNavigateUp)
        // ----------------------------------------------------------------
        composable(Route.RECOVERY_EXPLAINER) {
            val accounts by appViewModel.accounts.collectAsState()
            RecoveryExplainerView(
                accounts = accounts,
                onNavigateToRecoveryConsole = { prfOutput, did ->
                    // ByteArray cannot be a nav arg; stash both the PRF output and
                    // the DID in the back-stack savedStateHandle so RecoveryConsoleView
                    // can retrieve them after navigation.
                    navController.currentBackStackEntry?.savedStateHandle?.apply {
                        set("prfOutput", prfOutput)
                        set("did", did)
                    }
                    navController.navigate(Route.RECOVERY_CONSOLE)
                },
                onNavigateUp = { navController.popBackStack() },
            )
        }

        // ----------------------------------------------------------------
        // QR Scanner (invite scanning)
        // Actual signature: QRScannerView(onScanned, onInviteToken)
        // ----------------------------------------------------------------
        composable(Route.SCANNER) {
            QRScannerView(
                onInviteToken = { token ->
                    // QRScannerView decodes + validates the scanned URL itself, so a
                    // successful scan hands us a ready InviteToken — record it and go
                    // straight to the identity picker (which offers create / join /
                    // recover), matching iOS's navigationDestination(item:) flow.
                    appViewModel.setPendingInvite(token)
                    navController.navigate(Route.IDENTITY_PICKER) {
                        popUpTo(Route.SCANNER) { inclusive = true }
                    }
                },
                onBack = { navController.popBackStack() },
            )
        }

        // ----------------------------------------------------------------
        // Invite link entry
        // Actual signature: InviteLinkEntryView(onInviteTokenResolved)
        // ----------------------------------------------------------------
        composable(Route.INVITE_LINK_ENTRY) {
            InviteLinkEntryView(
                onBack = { navController.popBackStack() },
                onInviteTokenResolved = { token ->
                    // InviteToken resolved — record it and choose an identity to
                    // join with (existing) or create a new one.
                    appViewModel.setPendingInvite(token)
                    navController.navigate(Route.IDENTITY_PICKER) {
                        popUpTo(Route.INVITE_LINK_ENTRY) { inclusive = true }
                    }
                },
            )
        }

        // ----------------------------------------------------------------
        // Identity picker — pick an existing identity to join the invited server,
        // or create / recover one. Reads the resolved invite from the ViewModel.
        // ----------------------------------------------------------------
        composable(Route.IDENTITY_PICKER) {
            val invite by appViewModel.pendingInvite.collectAsState()
            invite?.let { token ->
                IdentityPickerView(
                    inviteToken = token,
                    appViewModel = appViewModel,
                    onPickExistingAccount = { account ->
                        navController.navigate(Route.joiningServer(account.id))
                    },
                    onCreateNewAccount = { navController.navigate(Route.NEW_ACCOUNT) },
                    onRecoverIdentity = { navController.navigate(Route.RECOVERY_EXPLAINER) },
                    // No identities: the picker has nothing to show, so replace it
                    // on the back stack. Back from the create screen then returns
                    // to Splash instead of re-triggering the auto-skip (the loop).
                    onSkipToNewAccount = {
                        navController.navigate(Route.NEW_ACCOUNT) {
                            popUpTo(Route.IDENTITY_PICKER) { inclusive = true }
                        }
                    },
                )
            }
        }

        // ----------------------------------------------------------------
        // New account — enter a display name for a brand-new identity, then set
        // up recovery (passkey). Reads the resolved invite from the ViewModel.
        // ----------------------------------------------------------------
        composable(Route.NEW_ACCOUNT) {
            val invite by appViewModel.pendingInvite.collectAsState()
            invite?.let { token ->
                NewAccountView(
                    inviteToken = token,
                    onNext = { displayName ->
                        navController.navigate(Route.passkeyExplainer(displayName))
                    },
                    onRecover = { navController.navigate(Route.RECOVERY_EXPLAINER) },
                    // Back arrow / system back: pop. Via the auto-skip path the
                    // back stack is [SPLASH, NEW_ACCOUNT] so this lands on Splash;
                    // via the manual "create" path it returns to the picker.
                    onBack = { navController.popBackStack() },
                )
            }
        }

        // ----------------------------------------------------------------
        // Passkey explainer — offers passkey creation or a recovery phrase.
        // (The passkey ceremony itself is still a deferred TODO inside the view.)
        // ----------------------------------------------------------------
        composable(Route.PASSKEY_EXPLAINER) { backStackEntry ->
            val displayName = backStackEntry.arguments?.getString("displayName") ?: ""
            val invite by appViewModel.pendingInvite.collectAsState()
            invite?.let { token ->
                PasskeyExplainerView(
                    inviteToken = token,
                    displayName = displayName,
                    viewModel = appViewModel,
                    onNavigateToRecoveryPhraseSetup = { _, name ->
                        navController.navigate(Route.recoveryPhraseSetup(name))
                    },
                    onHandleDeepLink = { url ->
                        runCatching { Uri.parse(url) }.getOrNull()?.let { appViewModel.handleDeepLink(it) }
                    },
                )
            }
        }

        // ----------------------------------------------------------------
        // Recovery phrase setup — alternative to a passkey for a new identity.
        // ----------------------------------------------------------------
        composable(Route.RECOVERY_PHRASE_SETUP) { backStackEntry ->
            val displayName = backStackEntry.arguments?.getString("displayName") ?: ""
            val invite by appViewModel.pendingInvite.collectAsState()
            invite?.let { token ->
                RecoveryPhraseSetupView(
                    appViewModel = appViewModel,
                    inviteToken = token,
                    displayName = displayName,
                    onComplete = {
                        appViewModel.setPendingInvite(null)
                        navController.navigate(Route.MAIN) {
                            popUpTo(0) { inclusive = true }
                        }
                    },
                )
            }
        }

        // ----------------------------------------------------------------
        // Joining server — an existing identity joins the invited server.
        // ----------------------------------------------------------------
        composable(Route.JOINING_SERVER) { backStackEntry ->
            val did = backStackEntry.arguments?.getString("did") ?: return@composable
            val accounts by appViewModel.accounts.collectAsState()
            val account = accounts.firstOrNull { it.id == did }
            val invite by appViewModel.pendingInvite.collectAsState()
            if (account != null && invite != null) {
                JoiningServerView(
                    inviteToken = invite!!,
                    existingAccount = account,
                    appViewModel = appViewModel,
                    onJoinComplete = {
                        appViewModel.setPendingInvite(null)
                        navController.navigate(Route.MAIN) {
                            popUpTo(0) { inclusive = true }
                        }
                    },
                    onBack = { navController.popBackStack() },
                )
            }
        }

        // ----------------------------------------------------------------
        // Recovery console
        // Actual signature: RecoveryConsoleView(prfOutput, did, appViewModel)
        // Both prfOutput (a ByteArray, which cannot be a route arg) and did are
        // retrieved from the previous back-stack entry's savedStateHandle, stashed
        // by RecoveryExplainerView before navigating here.
        // ----------------------------------------------------------------
        composable(Route.RECOVERY_CONSOLE) {
            val prfOutput: ByteArray = navController.previousBackStackEntry
                ?.savedStateHandle
                ?.get<ByteArray>("prfOutput")
                ?: ByteArray(0)
            val did: String = navController.previousBackStackEntry
                ?.savedStateHandle
                ?.get<String>("did")
                ?: ""
            RecoveryConsoleView(
                prfOutput = prfOutput,
                did = did,
                appViewModel = appViewModel,
                onBack = { navController.popBackStack() },
            )
        }

        // ----------------------------------------------------------------
        // Main tab container (Chats + Network)
        // ----------------------------------------------------------------
        composable(Route.MAIN) {
            MainTabView(
                appViewModel = appViewModel,
                onOpenConversation = { conv ->
                    navController.navigate(Route.conversation(conv.id))
                },
                onOpenAccounts = { navController.navigate(Route.ACCOUNTS) },
                onOpenCompose = { navController.navigate(Route.COMPOSE) },
            )
        }

        // ----------------------------------------------------------------
        // Conversation
        // ----------------------------------------------------------------
        composable(
            route = Route.CONVERSATION,
            deepLinks = listOf(
                navDeepLink {
                    uriPattern = "https://go.theavalanche.net/conversation/{id}"
                },
            ),
        ) { backStackEntry ->
            val conversationId = backStackEntry.arguments?.getString("id") ?: return@composable
            val conversations by appViewModel.conversations.collectAsState()
            val conv = conversations.firstOrNull { it.id == conversationId }

            if (conv != null) {
                ConversationView(
                    conversation = conv,
                    viewModel = appViewModel,
                    onNavigateToGroupDetail = { groupId, accountId ->
                        navController.navigate(Route.groupDetail(groupId, accountId))
                    },
                    onBack = { navController.popBackStack() },
                )
            }
        }

        // ----------------------------------------------------------------
        // Compose / New message
        // ----------------------------------------------------------------
        composable(Route.COMPOSE) {
            ComposeMessageView(
                viewModel = appViewModel,
                onDismiss = { navController.popBackStack() },
                onNavigateToConversation = { conv ->
                    navController.popBackStack()
                    navController.navigate(Route.conversation(conv.id))
                },
                onNavigateToNameGroup = { members, accountId, servers ->
                    // Complex objects can't be route args; stash the group draft in
                    // this entry's savedStateHandle for NameGroupView to read.
                    navController.currentBackStackEntry?.savedStateHandle?.apply {
                        set(
                            "ng_members",
                            ArrayList(members.map { RecipientChip(it.id, it.did, it.displayName) }),
                        )
                        set("ng_accountId", accountId)
                        set("ng_servers", ArrayList(servers))
                    }
                    navController.navigate(Route.NAME_GROUP)
                },
            )
        }

        // ----------------------------------------------------------------
        // Name group (final step of multi-recipient compose)
        // ----------------------------------------------------------------
        composable(Route.NAME_GROUP) {
            val handle = navController.previousBackStackEntry?.savedStateHandle
            val members = handle?.get<ArrayList<RecipientChip>>("ng_members") ?: arrayListOf()
            val accountId = handle?.get<String>("ng_accountId") ?: ""
            val servers = handle?.get<ArrayList<ServerInfo>>("ng_servers") ?: arrayListOf()
            NameGroupView(
                members = members,
                accountId = accountId,
                servers = servers,
                viewModel = appViewModel,
                onCreated = { conv ->
                    navController.navigate(Route.conversation(conv.id)) {
                        // Drop the compose + name-group screens from the back stack.
                        popUpTo(Route.MAIN)
                    }
                },
                onDismiss = { navController.popBackStack() },
            )
        }

        // ----------------------------------------------------------------
        // Accounts / Settings
        // ----------------------------------------------------------------
        composable(Route.ACCOUNTS) {
            AccountsView(
                viewModel = appViewModel,
                onDismiss = { navController.popBackStack() },
                onScanInvite = { _ -> navController.navigate(Route.SCANNER) },
                onShowScanner = { _ -> navController.navigate(Route.SCANNER) },
                onNavigateToScanner = { _ -> navController.navigate(Route.SCANNER) },
                onNavigateToIdentityDetail = { account ->
                    navController.navigate(Route.identityDetail(account.id))
                },
                onNavigateToServerDetail = { account, server ->
                    navController.navigate(Route.serverDetail(server.id))
                },
                onNavigateToAddAccount = { navController.navigate(Route.ADD_ACCOUNT) },
                onOpenLogViewer = { navController.navigate(Route.LOG_VIEWER) },
            )
        }

        // ----------------------------------------------------------------
        // Group detail
        // ----------------------------------------------------------------
        composable(Route.GROUP_DETAIL) { backStackEntry ->
            val groupId = backStackEntry.arguments?.getString("groupId") ?: return@composable
            val accountId = backStackEntry.arguments?.getString("accountId") ?: return@composable
            GroupDetailView(
                groupId = groupId,
                accountId = accountId,
                appViewModel = appViewModel,
                onDismiss = { navController.popBackStack() },
            )
        }

        // ----------------------------------------------------------------
        // Identity detail (Settings > identity row)
        // Actual signature: IdentityDetailView(account, viewModel, onBack, onNavigateToBlocked)
        // ----------------------------------------------------------------
        composable(Route.IDENTITY_DETAIL) { backStackEntry ->
            val did = backStackEntry.arguments?.getString("did") ?: return@composable
            val accounts by appViewModel.accounts.collectAsState()
            val account = accounts.firstOrNull { it.id == did }
            if (account != null) {
                IdentityDetailView(
                    account = account,
                    viewModel = appViewModel,
                    onBack = { navController.popBackStack() },
                    onNavigateToBlocked = { acct ->
                        navController.navigate(Route.blockedContacts(acct.id))
                    },
                    onNavigateToLinkDevice = { acct ->
                        navController.navigate(Route.linkDevice(acct.id))
                    },
                )
            }
        }

        // ----------------------------------------------------------------
        // Link a device — existing-device side (docs/04 §4).
        // ----------------------------------------------------------------
        composable(Route.LINK_DEVICE) { backStackEntry ->
            val did = backStackEntry.arguments?.getString("did") ?: return@composable
            LinkDeviceView(
                accountId = did,
                viewModel = appViewModel,
                onBack = { navController.popBackStack() },
            )
        }

        // ----------------------------------------------------------------
        // Blocked contacts (Settings > identity > Blocked Contacts)
        // ----------------------------------------------------------------
        composable(Route.BLOCKED_CONTACTS) { backStackEntry ->
            val did = backStackEntry.arguments?.getString("did") ?: return@composable
            val accounts by appViewModel.accounts.collectAsState()
            val account = accounts.firstOrNull { it.id == did }
            if (account != null) {
                BlockedContactsView(
                    account = account,
                    appViewModel = appViewModel,
                    onBack = { navController.popBackStack() },
                )
            }
        }

        // ----------------------------------------------------------------
        // Server detail (Settings > server row)
        // Actual signature: ServerDetailView(account, server, appViewModel, onDismiss)
        // Route embeds the server URL (which may contain slashes) encoded via Uri.encode.
        // ----------------------------------------------------------------
        composable(Route.SERVER_DETAIL) { backStackEntry ->
            val serverUrl = Uri.decode(backStackEntry.arguments?.getString("serverUrl") ?: "")
            val accounts by appViewModel.accounts.collectAsState()
            var foundAccount: Account? = null
            var foundServer: ServerInfo? = null
            for (acct in accounts) {
                val srv = acct.servers.firstOrNull { it.id == serverUrl }
                if (srv != null) {
                    foundAccount = acct
                    foundServer = srv
                    break
                }
            }
            if (foundAccount != null && foundServer != null) {
                ServerDetailView(
                    account = foundAccount,
                    server = foundServer,
                    appViewModel = appViewModel,
                    onDismiss = { navController.popBackStack() },
                )
            }
        }

        // ----------------------------------------------------------------
        // Add account
        // Actual signature: AddAccountView(onScanInvite, onEnterLink, onRecover)
        // ----------------------------------------------------------------
        composable(Route.ADD_ACCOUNT) {
            AddAccountView(
                onScanInvite = { navController.navigate(Route.SCANNER) },
                onEnterLink = { navController.navigate(Route.INVITE_LINK_ENTRY) },
                // Recovery starts at the explainer, not the console directly.
                onRecover = { navController.navigate(Route.RECOVERY_EXPLAINER) },
                onBack = { navController.popBackStack() },
            )
        }

        // ----------------------------------------------------------------
        // Log viewer (debug — mirrors iOS two-finger triple-tap sheet)
        // Actual signature: LogViewerView(onDismiss)
        // ----------------------------------------------------------------
        composable(Route.LOG_VIEWER) {
            LogViewerView(onDismiss = { navController.popBackStack() })
        }
    }
}

