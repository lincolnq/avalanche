package net.theavalanche.app

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
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
    const val GROUP_DETAIL = "group_detail/{groupId}/{accountId}"
    const val IDENTITY_DETAIL = "identity_detail/{did}"
    const val SERVER_DETAIL = "server_detail/{serverUrl}"
    const val ADD_ACCOUNT = "add_account"
    const val SCANNER = "scanner"                   // QRScannerView (invite scanning)
    const val INVITE_LINK_ENTRY = "invite_link_entry"
    const val RECOVERY_EXPLAINER = "recovery_explainer"
    const val RECOVERY_CONSOLE = "recovery_console"
    const val LOG_VIEWER = "log_viewer"

    // helpers
    fun conversation(id: String) = "conversation/$id"
    fun groupDetail(groupId: String, accountId: String) = "group_detail/$groupId/$accountId"
    fun identityDetail(did: String) = "identity_detail/$did"
    fun serverDetail(serverUrl: String) = "server_detail/${Uri.encode(serverUrl)}"
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

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        // Create the notification channel (no-op on API < 26).
        NotificationPresenter.createNotificationChannel(this)

        val factory = AppViewModelFactory(applicationContext)
        appViewModel = ViewModelProvider(this, factory)[AppViewModel::class.java]
        appViewModel.restoreAccounts()
        intent?.data?.let { appViewModel.handleDeepLink(it) }

        setContent {
            AvalancheTheme {
                AppNavGraph(appViewModel = appViewModel)
            }
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
//   - LogViewerView accessible via debug gesture (two-finger triple-tap on iOS;
//     stubbed here — TODO(opus): hook into a debug shake detector or similar).
// ---------------------------------------------------------------------------

@Composable
fun AppNavGraph(
    appViewModel: AppViewModel,
) {
    val navController: NavHostController = rememberNavController()
    val isOnboarding by appViewModel.isOnboarding.collectAsState()

    // React to isOnboarding changes, mirroring iOS RootView Group { if appState.isOnboarding … }
    LaunchedEffect(isOnboarding) {
        if (isOnboarding) {
            navController.navigate(Route.SPLASH) {
                popUpTo(0) { inclusive = true }
            }
        } else {
            navController.navigate(Route.MAIN) {
                popUpTo(0) { inclusive = true }
            }
        }
    }

    NavHost(
        navController = navController,
        // Start on splash; the LaunchedEffect above immediately re-routes after
        // restoreAccounts() resolves the correct starting state.
        startDestination = Route.SPLASH,
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
                    // ByteArray cannot be a nav arg; stash it in the back-stack
                    // savedStateHandle so RecoveryConsoleView can retrieve it.
                    navController.currentBackStackEntry
                        ?.savedStateHandle
                        ?.set("prfOutput", prfOutput)
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
                onScanned = { rawText ->
                    // raw QR text — pass back to caller or store as pending token
                    appViewModel.setPendingInviteToken(rawText)
                    navController.navigate(Route.INVITE_LINK_ENTRY) {
                        popUpTo(Route.SCANNER) { inclusive = true }
                    }
                },
                onInviteToken = { token ->
                    // InviteToken already decoded — store and navigate
                    // TODO(opus): InviteToken is a data class produced by
                    // QRScannerView; extract the raw token string from it.
                    navController.navigate(Route.INVITE_LINK_ENTRY) {
                        popUpTo(Route.SCANNER) { inclusive = true }
                    }
                },
            )
        }

        // ----------------------------------------------------------------
        // Invite link entry
        // Actual signature: InviteLinkEntryView(onInviteTokenResolved)
        // ----------------------------------------------------------------
        composable(Route.INVITE_LINK_ENTRY) {
            InviteLinkEntryView(
                onInviteTokenResolved = { token ->
                    // InviteToken resolved — navigate into the onboarding flow.
                    // TODO(opus): InviteToken carries server URL / did; wire the
                    // NewAccountView or JoiningServerView with its contents.
                    navController.navigate(Route.MAIN) {
                        popUpTo(0) { inclusive = true }
                    }
                },
            )
        }

        // ----------------------------------------------------------------
        // Recovery console
        // Actual signature: RecoveryConsoleView(prfOutput, did, appViewModel)
        // prfOutput is retrieved from the previous back-stack entry's savedStateHandle
        // (stashed by RecoveryExplainerView before navigating here).
        // did is also sourced from the same savedStateHandle entry since it cannot
        // be cleanly embedded in the route without its own parameterisation.
        // TODO(opus): parameterize the did into the route string once the
        //   integration pass is done (requires Uri.encode / NavType.StringType).
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
                // TODO(opus): wire onNavigateToNameGroup to a NameGroupView destination
                onNavigateToNameGroup = { _, _, _ -> },
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
                        // BlockedContactsView(account, appViewModel, modifier) — no nav route yet.
                        // TODO(opus): add a BLOCKED_CONTACTS route and navigate there.
                    },
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
            )
        }

        // ----------------------------------------------------------------
        // Log viewer (debug — mirrors iOS two-finger triple-tap sheet)
        // Actual signature: LogViewerView(onDismiss)
        // ----------------------------------------------------------------
        composable(Route.LOG_VIEWER) {
            // TODO(opus): LogViewerView has not yet been ported to Android.
            //   Uncomment when the composable exists:
            // LogViewerView(onDismiss = { navController.popBackStack() })
        }
    }
}

