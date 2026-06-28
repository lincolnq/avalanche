package net.theavalanche.app

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Dns
import androidx.compose.material.icons.automirrored.filled.Message
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview

// ---------------------------------------------------------------------------
// MainTabView
//
// Mirrors iOS Sources/Views/Common/MainTabView.swift.
//
// The iOS implementation is a TabView with two tabs:
//   - Chats  (message bubble icon → Message on Material)
//   - Network (server.rack icon → Dns on Material)
//
// On Android we render a Scaffold with a Material 3 NavigationBar (bottom nav).
// The offline banner is overlaid on top of the content, mirroring iOS RootView's
// ZStack { MainTabView() \n OfflineBanner() }.
//
// Navigation callbacks replace NavigationStack/sheet navigation — the central
// NavGraph in MainActivity wires them.
// ---------------------------------------------------------------------------

@Composable
fun MainTabView(
    appViewModel: AppViewModel,
    // ---- Chats tab callbacks ----
    onOpenConversation: (Conversation) -> Unit = {},
    onOpenAccounts: () -> Unit = {},
    onOpenCompose: () -> Unit = {},
    // ---- Network tab has no extra callbacks for now ----
) {
    val selectedTab by appViewModel.selectedTab.collectAsState()

    // Box so we can overlay the OfflineBanner on top of the Scaffold,
    // mirroring iOS RootView's ZStack.
    Box(modifier = Modifier.fillMaxSize()) {
        Scaffold(
            containerColor = LocalAvalancheColors.current.paper,
            bottomBar = {
                NavigationBar(
                    containerColor = LocalAvalancheColors.current.paper,
                ) {
                    NavigationBarItem(
                        selected = selectedTab == AppViewModel.Tab.CHATS,
                        onClick = { appViewModel.setSelectedTab(AppViewModel.Tab.CHATS) },
                        icon = {
                            Icon(
                                imageVector = Icons.AutoMirrored.Filled.Message,
                                contentDescription = "Chats",
                            )
                        },
                        label = { Text("Chats") },
                        colors = NavigationBarItemDefaults.colors(
                            selectedIconColor = LocalAvalancheColors.current.brand,
                            selectedTextColor = LocalAvalancheColors.current.brand,
                            indicatorColor = LocalAvalancheColors.current.paper,
                            unselectedIconColor = LocalAvalancheColors.current.muted,
                            unselectedTextColor = LocalAvalancheColors.current.muted,
                        ),
                    )
                    NavigationBarItem(
                        selected = selectedTab == AppViewModel.Tab.NETWORK,
                        onClick = { appViewModel.setSelectedTab(AppViewModel.Tab.NETWORK) },
                        icon = {
                            Icon(
                                imageVector = Icons.Filled.Dns,
                                contentDescription = "Network",
                            )
                        },
                        label = { Text("Network") },
                        colors = NavigationBarItemDefaults.colors(
                            selectedIconColor = LocalAvalancheColors.current.brand,
                            selectedTextColor = LocalAvalancheColors.current.brand,
                            indicatorColor = LocalAvalancheColors.current.paper,
                            unselectedIconColor = LocalAvalancheColors.current.muted,
                            unselectedTextColor = LocalAvalancheColors.current.muted,
                        ),
                    )
                }
            },
        ) { innerPadding ->
            Box(modifier = Modifier.padding(innerPadding)) {
                when (selectedTab) {
                    AppViewModel.Tab.CHATS -> ChatsView(
                        viewModel = appViewModel,
                        onOpenConversation = onOpenConversation,
                        onOpenAccounts = onOpenAccounts,
                        onOpenCompose = onOpenCompose,
                    )
                    AppViewModel.Tab.NETWORK -> NetworkView(
                        appViewModel = appViewModel,
                    )
                }
            }
        }

        // Offline banner overlaid at the top-center — mirrors iOS RootView
        // ZStack(alignment: .top) + OfflineBanner(). Without the explicit
        // align it would default to the Box's top-start (top-left) corner.
        Box(
            modifier = Modifier
                .align(Alignment.TopCenter)
                // The overlay sits outside the Scaffold, so it must apply the
                // status-bar inset itself — otherwise the pill floats under the
                // status bar in the edge-to-edge window.
                .statusBarsPadding(),
        ) {
            OfflineBanner(appViewModel = appViewModel)
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun MainTabViewPreview() {
    AvalancheTheme {
        MainTabView(appViewModel = rememberPreviewAppViewModel())
    }
}
