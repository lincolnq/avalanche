package net.theavalanche.app

import android.net.Uri
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

/**
 * Registers an existing DID with a new server.
 *
 * Mirrors iOS Sources/Views/Onboarding/JoiningServerView.swift.
 *
 * Navigation is callback-based: the caller (NavGraph) provides [onJoinComplete]
 * which is invoked when the join succeeds. The screen is assumed to be
 * presented inside a scaffold that supplies the back arrow; [onBack] handles
 * the optional back-press if the host needs to know.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun JoiningServerView(
    inviteToken: InviteToken,
    existingAccount: Account,
    appViewModel: AppViewModel,
    onJoinComplete: () -> Unit = {},
    onBack: () -> Unit = {},
) {
    var isJoining by remember { mutableStateOf(false) }
    var errorMessage by remember { mutableStateOf<String?>(null) }
    // The operator's privacy policy URL comes from invite validation (resolved
    // by the core), so there's no separate server call here. null/blank when the
    // operator hasn't configured one — the link is hidden in that case.
    val privacyPolicyUrl = inviteToken.privacyPolicyUrl

    val scope = rememberCoroutineScope()
    val uriHandler = LocalUriHandler.current

    Scaffold(
        topBar = {
            TopAppBar(title = { Text("Join Server") })
        },
        containerColor = LocalAvalancheColors.current.paper,
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(LocalAvalancheColors.current.paper)
                .padding(innerPadding)
                .padding(top = 48.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(24.dp),
        ) {
            AccountAvatar(account = existingAccount, size = 80.dp)

            Text(
                text = "Join ${inviteToken.serverName} as ${existingAccount.displayName}?",
                style = androidx.compose.material3.MaterialTheme.typography.headlineSmall,
                textAlign = TextAlign.Center,
                color = LocalAvalancheColors.current.ink,
                modifier = Modifier.padding(horizontal = 24.dp),
            )

            // Show privacy policy link if the server provided one. Guard against
            // blank values so we never call openUri("") (which would crash).
            if (!privacyPolicyUrl.isNullOrBlank()) {
                TextButton(
                    onClick = {
                        runCatching { uriHandler.openUri(privacyPolicyUrl) }
                    }
                ) {
                    Text(
                        text = "View ${inviteToken.serverName}'s privacy policy",
                        color = LocalAvalancheColors.current.brand,
                    )
                }
            }

            // Error message (mirrors iOS red callout text).
            if (errorMessage != null) {
                Text(
                    text = errorMessage!!,
                    color = LocalAvalancheColors.current.error,
                    style = androidx.compose.material3.MaterialTheme.typography.bodyMedium,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 24.dp),
                )
            }

            // Join button — shows a spinner while in-flight.
            Button(
                onClick = {
                    if (!isJoining) {
                        isJoining = true
                        errorMessage = null
                        scope.launch {
                            runCatching {
                                appViewModel.joinServer(
                                    serverUrl = inviteToken.serverUrl,
                                    serverName = inviteToken.serverName,
                                    existingAccountId = existingAccount.id,
                                )
                            }.onSuccess {
                                onJoinComplete()
                            }.onFailure { error ->
                                errorMessage = error.localizedMessage ?: error.message ?: "Unknown error"
                                isJoining = false
                            }
                        }
                    }
                },
                enabled = !isJoining,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 32.dp)
                    .height(52.dp),
            ) {
                if (isJoining) {
                    CircularProgressIndicator(
                        modifier = Modifier.height(24.dp),
                        color = LocalAvalancheColors.current.paper,
                        strokeWidth = 2.dp,
                    )
                } else {
                    Text("Join")
                }
            }

            Spacer(modifier = Modifier.weight(1f))
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun JoiningServerViewPreview() {
    AvalancheTheme {
        val fakeAccount = Account(
            id = "did:example:alice",
            displayName = "Alice",
        )
        val fakeToken = InviteToken(
            token = "abc123",
            serverUrl = "https://demo.theavalanche.net",
            serverName = "Demo Server",
            inviterDid = null,
            postOnboardingRedirect = null,
            privacyPolicyUrl = "https://demo.theavalanche.net/privacy",
        )
        JoiningServerView(
            inviteToken = fakeToken,
            existingAccount = fakeAccount,
            appViewModel = rememberPreviewAppViewModel(accounts = listOf(fakeAccount)),
        )
    }
}
