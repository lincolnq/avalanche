package net.theavalanche.app

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.Key
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * Shown when the user has scanned/entered an invite.
 * If accounts exist, lets them pick an existing identity or create a new one.
 * If no accounts exist, goes straight to the new account flow (NewAccountView).
 *
 * Mirrors iOS Sources/Views/Onboarding/IdentityPickerView.swift.
 *
 * Navigation is passed as lambdas following the SplashView pattern.
 *
 * @param inviteToken         The validated invite token.
 * @param appViewModel        Top-level ViewModel providing the accounts list.
 * @param onPickExistingAccount Called when the user picks an account to join with.
 *                            Navigates to JoiningServerView.
 * @param onCreateNewAccount  Called when the user wants a brand-new identity.
 *                            Navigates to NewAccountView(showRecoverLink = false).
 * @param onRecoverIdentity   Called when the user wants to recover an existing identity.
 *                            Navigates to RecoveryExplainerView.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun IdentityPickerView(
    inviteToken: InviteToken,
    appViewModel: AppViewModel,
    onPickExistingAccount: (account: Account) -> Unit = {},
    onCreateNewAccount: () -> Unit = {},
    onRecoverIdentity: () -> Unit = {},
) {
    val accounts by appViewModel.accounts.collectAsState()

    if (accounts.isEmpty()) {
        // No existing identities — skip straight to the create-new flow,
        // matching iOS which renders NewAccountView(inviteToken:showRecoverLink:true) here.
        // The parent NavGraph should detect this and navigate, but we also render a
        // direct "create" shortcut so the user is never stuck on a blank screen.
        // TODO(opus): The iOS implementation renders NewAccountView inline; here we
        //   delegate via the lambda so the NavGraph can push the correct destination.
        onCreateNewAccount()
        return
    }

    Scaffold(
        topBar = {
            TopAppBar(title = { Text("Choose Identity") })
        }
    ) { innerPadding ->
        LazyColumn(
            modifier = Modifier
                .padding(innerPadding)
                .fillMaxWidth(),
        ) {
            // ---- Section header ----------------------------------------
            item {
                Text(
                    text = "Join ${inviteToken.serverName} as…",
                    color = AvalancheColors.Muted,
                    fontSize = 13.sp,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                )
            }

            // ---- Existing accounts rows --------------------------------
            items(accounts.size) { idx ->
                val account = accounts[idx]
                ExistingAccountRow(
                    account = account,
                    onClick = { onPickExistingAccount(account) },
                )
                if (idx < accounts.size - 1) {
                    HorizontalDivider(modifier = Modifier.padding(start = 72.dp))
                }
            }

            // ---- Section spacer ----------------------------------------
            item {
                Spacer(modifier = Modifier.height(24.dp))
            }

            // ---- Create / Recover rows ----------------------------------
            item {
                ActionRow(
                    label = "Create a new identity",
                    iconTint = AvalancheColors.Brand,
                    leadingIcon = {
                        Icon(
                            imageVector = Icons.Filled.Add,
                            contentDescription = null,
                            tint = AvalancheColors.Brand,
                        )
                    },
                    onClick = onCreateNewAccount,
                )
                HorizontalDivider(modifier = Modifier.padding(start = 16.dp))
                ActionRow(
                    label = "Recover an identity",
                    iconTint = Color(0xFFFF9500), // iOS .orange
                    leadingIcon = {
                        Icon(
                            imageVector = Icons.Filled.Key,
                            contentDescription = null,
                            tint = Color(0xFFFF9500),
                        )
                    },
                    onClick = onRecoverIdentity,
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

@Composable
private fun ExistingAccountRow(
    account: Account,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        AccountAvatar(account = account, size = 40.dp)
        Spacer(modifier = Modifier.width(12.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = account.displayName,
                fontWeight = FontWeight.Medium,
                color = AvalancheColors.Ink,
            )
            val serverNames = account.servers.joinToString(", ") { it.name }
            if (serverNames.isNotEmpty()) {
                Text(
                    text = serverNames,
                    fontSize = 12.sp,
                    color = AvalancheColors.Muted,
                )
            }
        }
        Icon(
            imageVector = Icons.Filled.ChevronRight,
            contentDescription = null,
            tint = AvalancheColors.Muted,
        )
    }
}

@Composable
private fun ActionRow(
    label: String,
    iconTint: Color,
    leadingIcon: @Composable () -> Unit,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        leadingIcon()
        Spacer(modifier = Modifier.width(12.dp))
        Text(
            text = label,
            color = iconTint,
            modifier = Modifier.weight(1f),
        )
        Icon(
            imageVector = Icons.Filled.ChevronRight,
            contentDescription = null,
            tint = AvalancheColors.Muted,
        )
    }
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

@Preview(showBackground = true)
@Composable
private fun IdentityPickerPreview() {
    // Preview with accounts — shows the picker list.
    AvalancheTheme {
        val sampleAccounts = listOf(
            Account(
                id = "did:example:alice",
                displayName = "Alice",
                servers = listOf(
                    ServerInfo(
                        id = "https://server1.example.com",
                        name = "Server One",
                        url = android.net.Uri.parse("https://server1.example.com"),
                    )
                ),
            ),
            Account(
                id = "did:example:bob",
                displayName = "Bob",
                servers = listOf(
                    ServerInfo(
                        id = "https://server2.example.com",
                        name = "Server Two",
                        url = android.net.Uri.parse("https://server2.example.com"),
                    )
                ),
            ),
        )
        val inviteToken = InviteToken(
            token = "sample-token",
            serverUrl = "https://example.theavalanche.net",
            serverName = "Example Org",
            inviterDid = null,
            postOnboardingRedirect = null,
        )
        // TODO(opus): AppViewModel requires a Context — wire a fake/preview VM here.
        // For now the preview demonstrates the composable structure; run on a device
        // or emulator to see it with a real ViewModel.
    }
}
