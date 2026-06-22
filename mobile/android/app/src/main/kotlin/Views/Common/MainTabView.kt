package net.theavalanche.app

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
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
            containerColor = AvalancheColors.Paper,
            bottomBar = {
                NavigationBar(
                    containerColor = AvalancheColors.Paper,
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
                            selectedIconColor = AvalancheColors.Brand,
                            selectedTextColor = AvalancheColors.Brand,
                            indicatorColor = AvalancheColors.Paper,
                            unselectedIconColor = AvalancheColors.Muted,
                            unselectedTextColor = AvalancheColors.Muted,
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
                            selectedIconColor = AvalancheColors.Brand,
                            selectedTextColor = AvalancheColors.Brand,
                            indicatorColor = AvalancheColors.Paper,
                            unselectedIconColor = AvalancheColors.Muted,
                            unselectedTextColor = AvalancheColors.Muted,
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

        // Offline banner overlaid at the top — mirrors iOS RootView ZStack + OfflineBanner().
        OfflineBanner(appViewModel = appViewModel)
    }
}

@Preview(showBackground = true)
@Composable
private fun MainTabViewPreview() {
    AvalancheTheme {
        // Preview without a real ViewModel — tab switching not functional.
        // TODO(opus): wire up a mock AppViewModel for richer previews.
    }
}
