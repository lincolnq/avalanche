package net.theavalanche.app

import android.content.Intent
import android.net.Uri
import android.os.Build
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.HelpOutline
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel

// Settings screen listing all accounts (identities) and their servers.
// Mirrors mobile/ios/Actnet/Sources/Views/Settings/AccountsView.swift.
//
// Navigation pattern: instead of an embedded NavigationStack, this composable
// accepts lambda callbacks that the parent NavGraph connects to actual
// destinations. The Done / dismiss action is also a lambda.
@Composable
fun AccountsView(
    viewModel: AppViewModel = viewModel(),
    onDismiss: () -> Unit = {},
    onScanInvite: ((String) -> Unit) -> Unit = {},
    onShowScanner: (() -> Unit) -> Unit = {},
    onNavigateToScanner: ((String) -> Unit) -> Unit = {},
    onNavigateToIdentityDetail: (Account) -> Unit = {},
    onNavigateToServerDetail: (Account, ServerInfo) -> Unit = { _, _ -> },
    onNavigateToAddAccount: () -> Unit = {},
    onOpenLogViewer: () -> Unit = {},
) {
    val accounts by viewModel.accounts.collectAsState()
    val context = LocalContext.current

    // Sort accounts by current order (oldest first — no createdAt yet, preserve list order)
    val sortedAccounts = accounts

    val appVersion = remember {
        val pm = context.packageManager
        val info = runCatching { pm.getPackageInfo(context.packageName, 0) }.getOrNull()
        val version = info?.versionName ?: "—"
        // getLongVersionCode is API 28+; fall back to the deprecated Int form on
        // our minSdk 26/27 floor.
        val build = info?.let {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                it.longVersionCode
            } else {
                @Suppress("DEPRECATION") it.versionCode.toLong()
            }
        }?.toString() ?: "—"
        "Avalanche $version ($build)"
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(AvalancheColors.Paper)
            // This screen is its own nav destination (not inside MainTabView's
            // Scaffold), so it must apply the status-bar inset itself — otherwise
            // the edge-to-edge window draws the top bar under the status bar.
            .statusBarsPadding(),
    ) {
        // Top bar
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 12.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onDismiss) {
                Icon(
                    imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                    contentDescription = "Back",
                    tint = AvalancheColors.Ink,
                )
            }
            Text(
                text = "Accounts",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                color = AvalancheColors.Ink,
            )
            // Balancing spacer so the title stays centered opposite the back button.
            Spacer(modifier = Modifier.width(48.dp))
        }

        HorizontalDivider(color = AvalancheColors.Sand300.copy(alpha = 0.5f))

        LazyColumn(
            modifier = Modifier
                .fillMaxSize()
                .background(AvalancheColors.Paper)
                // Keep the footer clear of the (edge-to-edge) system nav bar.
                .navigationBarsPadding(),
            // Breathing room above the first section (the later sections each add
            // their own top=16.dp), and below the footer.
            contentPadding = PaddingValues(top = 16.dp, bottom = 16.dp),
        ) {
            // ── Section: Scan Invite ────────────────────────────────────────
            item {
                SettingsSectionCard {
                    SettingsRow(
                        leadingIcon = {
                            Icon(
                                imageVector = Icons.Filled.QrCodeScanner,
                                contentDescription = null,
                                tint = AvalancheColors.Brand,
                                modifier = Modifier.size(20.dp),
                            )
                        },
                        label = "Scan Invite",
                        onClick = {
                            // Delegate to the nav lambda; the QR scanner result is
                            // handled by the parent which calls viewModel.handleDeepLink.
                            onNavigateToScanner { scannedValue ->
                                val uri = runCatching { Uri.parse(scannedValue) }.getOrNull()
                                if (uri != null && viewModel.isDeepLink(uri)) {
                                    viewModel.handleDeepLink(uri)
                                    onDismiss()
                                }
                            }
                        },
                    )
                }
            }

            // ── Sections: one per account ───────────────────────────────────
            sortedAccounts.forEach { account ->
                item(key = "account-header-${account.id}") {
                    SettingsSectionCard(
                        modifier = Modifier.padding(top = 16.dp),
                    ) {
                        // Account identity row
                        SettingsRow(
                            leadingIcon = {
                                AccountAvatar(account = account, size = 32.dp)
                            },
                            label = account.displayName,
                            labelStyle = MaterialTheme.typography.titleSmall.copy(
                                fontWeight = FontWeight.SemiBold,
                                fontSize = 17.sp,
                            ),
                            showChevron = true,
                            onClick = { onNavigateToIdentityDetail(account) },
                        )

                        val sortedServers = account.servers.sortedBy { it.name }
                        sortedServers.forEachIndexed { idx, server ->
                            HorizontalDivider(
                                color = AvalancheColors.Sand300.copy(alpha = 0.4f),
                                modifier = Modifier.padding(start = 56.dp),
                            )
                            val isHome = account.servers.firstOrNull()?.id == server.id
                            SettingsRow(
                                leadingIcon = null,
                                label = null,
                                showChevron = true,
                                onClick = { onNavigateToServerDetail(account, server) },
                            ) {
                                ServerRowContent(server = server, isHome = isHome)
                            }
                        }
                    }
                }
            }

            // ── Section: Add account ────────────────────────────────────────
            item {
                SettingsSectionCard(
                    modifier = Modifier.padding(top = 16.dp),
                ) {
                    SettingsRow(
                        leadingIcon = {
                            Icon(
                                imageVector = Icons.Filled.Add,
                                contentDescription = null,
                                tint = AvalancheColors.Brand,
                                modifier = Modifier.size(20.dp),
                            )
                        },
                        label = "Add an account",
                        onClick = onNavigateToAddAccount,
                    )
                }
            }

            // ── Section: About ──────────────────────────────────────────────
            item {
                Column(
                    modifier = Modifier.padding(top = 16.dp),
                ) {
                    Text(
                        text = "About",
                        style = MaterialTheme.typography.labelMedium,
                        color = AvalancheColors.Muted,
                        modifier = Modifier.padding(start = 20.dp, bottom = 6.dp),
                    )
                    SettingsSectionCard {
                        SettingsRow(
                            leadingIcon = {
                                Icon(
                                    imageVector = Icons.AutoMirrored.Filled.HelpOutline,
                                    contentDescription = null,
                                    tint = AvalancheColors.Brand,
                                    modifier = Modifier.size(20.dp),
                                )
                            },
                            label = "Get Help",
                            onClick = {
                                val intent = Intent(
                                    Intent.ACTION_VIEW,
                                    Uri.parse("https://github.com/lincolnq/avalanche/issues"),
                                )
                                context.startActivity(intent)
                            },
                        )
                    }

                    // Footer: version + license link
                    Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(vertical = 12.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Text(
                            text = appVersion,
                            style = MaterialTheme.typography.bodySmall,
                            color = AvalancheColors.Muted,
                            // Debug-only: tap the version to open the in-app log
                            // viewer (mirrors the iOS debug gesture).
                            modifier = if (BuildConfig.DEBUG) {
                                Modifier.clickable { onOpenLogViewer() }
                            } else {
                                Modifier
                            },
                        )
                        Text(
                            text = "Open Source License",
                            style = MaterialTheme.typography.labelSmall,
                            color = AvalancheColors.Brand,
                            modifier = Modifier.clickable {
                                val intent = Intent(
                                    Intent.ACTION_VIEW,
                                    Uri.parse("https://github.com/lincolnq/avalanche/blob/main/LICENSE"),
                                )
                                context.startActivity(intent)
                            },
                        )
                    }
                }
            }
        }
    }
}

// ── Private composable helpers ──────────────────────────────────────────────

// Card-style rounded surface that groups settings rows — matches iOS grouped list section.
@Composable
private fun SettingsSectionCard(
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    Surface(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp),
        shape = RoundedCornerShape(12.dp),
        color = AvalancheColors.Sand50,
        tonalElevation = 0.dp,
        shadowElevation = 0.dp,
    ) {
        Column { content() }
    }
}

// A single tappable row inside a SettingsSectionCard.
// [leadingIcon] is shown at the start (nullable — omitted if null for indent).
// [label] is the primary text; pass null when [rowContent] provides all content.
// [rowContent] lets the caller override the content area entirely.
@Composable
private fun SettingsRow(
    onClick: () -> Unit,
    leadingIcon: (@Composable () -> Unit)?,
    label: String?,
    labelStyle: androidx.compose.ui.text.TextStyle = MaterialTheme.typography.bodyLarge,
    showChevron: Boolean = false,
    modifier: Modifier = Modifier,
    rowContent: (@Composable () -> Unit)? = null,
) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (leadingIcon != null) {
            Box(
                contentAlignment = Alignment.Center,
                modifier = Modifier
                    .size(24.dp)
                    .padding(end = 2.dp),
            ) {
                leadingIcon()
            }
            Spacer(Modifier.width(12.dp))
        } else {
            // Indent matching rows without an icon.
            Spacer(Modifier.width(36.dp))
        }

        if (rowContent != null) {
            Box(modifier = Modifier.weight(1f)) { rowContent() }
        } else {
            Text(
                text = label ?: "",
                style = labelStyle,
                color = AvalancheColors.Ink,
                modifier = Modifier.weight(1f),
            )
        }

        if (showChevron) {
            Icon(
                imageVector = Icons.Filled.ChevronRight,
                contentDescription = null,
                tint = AvalancheColors.Muted,
                modifier = Modifier.size(18.dp),
            )
        }
    }
}

// Content of a server row — name + optional "home" badge.
// Mirrors the private ServerRow view in the iOS source.
@Composable
private fun ServerRowContent(server: ServerInfo, isHome: Boolean) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            text = server.name,
            style = MaterialTheme.typography.bodyMedium,
            color = AvalancheColors.Ink,
        )
        if (isHome) {
            Surface(
                shape = RoundedCornerShape(50),
                color = AvalancheColors.Brand.copy(alpha = 0.15f),
            ) {
                Text(
                    text = "home",
                    style = MaterialTheme.typography.labelSmall,
                    color = AvalancheColors.Brand,
                    modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
                )
            }
        }
        Spacer(modifier = Modifier.weight(1f))
    }
}

// ── Preview ─────────────────────────────────────────────────────────────────

@Preview(showBackground = true)
@Composable
private fun AccountsViewPreview() {
    AvalancheTheme {
        // Stand-alone preview with mock data — no real ViewModel.
        val accounts = listOf(
            Account(
                id = "did:example:alice",
                displayName = "Alice",
                servers = listOf(
                    ServerInfo(
                        id = "https://home.example.com",
                        name = "Home Server",
                        url = android.net.Uri.parse("https://home.example.com"),
                    ),
                    ServerInfo(
                        id = "https://other.example.com",
                        name = "Other Server",
                        url = android.net.Uri.parse("https://other.example.com"),
                    ),
                ),
            ),
        )
        // Preview using a simplified layout that doesn't need a ViewModel.
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(AvalancheColors.Paper),
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 12.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Spacer(modifier = Modifier.width(56.dp))
                Text(
                    text = "Accounts",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    color = AvalancheColors.Ink,
                )
                TextButton(onClick = {}) {
                    Text("Done", color = AvalancheColors.Brand)
                }
            }
            HorizontalDivider(color = AvalancheColors.Sand300.copy(alpha = 0.5f))

            accounts.forEach { account ->
                SettingsSectionCard(modifier = Modifier.padding(top = 16.dp)) {
                    SettingsRow(
                        leadingIcon = { AccountAvatar(account = account, size = 32.dp) },
                        label = account.displayName,
                        labelStyle = MaterialTheme.typography.titleSmall.copy(
                            fontWeight = FontWeight.SemiBold,
                            fontSize = 17.sp,
                        ),
                        showChevron = true,
                        onClick = {},
                    )
                    account.servers.sortedBy { it.name }.forEach { server ->
                        HorizontalDivider(
                            color = AvalancheColors.Sand300.copy(alpha = 0.4f),
                            modifier = Modifier.padding(start = 56.dp),
                        )
                        val isHome = account.servers.firstOrNull()?.id == server.id
                        SettingsRow(
                            leadingIcon = null,
                            label = null,
                            showChevron = true,
                            onClick = {},
                        ) {
                            ServerRowContent(server = server, isHome = isHome)
                        }
                    }
                }
            }
        }
    }
}
