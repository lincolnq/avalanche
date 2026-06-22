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
import androidx.compose.runtime.LaunchedEffect
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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.net.URL

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
    // The operator's privacy policy URL, fetched from the server's public
    // /v1/info endpoint when the screen appears. null until loaded, or if the
    // operator hasn't configured one.
    var privacyPolicyUrl by remember { mutableStateOf<String?>(null) }

    val scope = rememberCoroutineScope()
    val uriHandler = LocalUriHandler.current

    // Mirrors iOS .task { await loadServerInfo() }
    LaunchedEffect(inviteToken.serverUrl) {
        privacyPolicyUrl = loadPrivacyPolicyUrl(inviteToken.serverUrl)
    }

    Scaffold(
        topBar = {
            TopAppBar(title = { Text("Join Server") })
        },
        containerColor = AvalancheColors.Paper,
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(AvalancheColors.Paper)
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
                color = AvalancheColors.Ink,
                modifier = Modifier.padding(horizontal = 24.dp),
            )

            // Show privacy policy link if the server provided one.
            if (privacyPolicyUrl != null) {
                TextButton(
                    onClick = {
                        runCatching { uriHandler.openUri(privacyPolicyUrl!!) }
                    }
                ) {
                    Text(
                        text = "View ${inviteToken.serverName}'s privacy policy",
                        color = AvalancheColors.Brand,
                    )
                }
            }

            // Error message (mirrors iOS red callout text).
            if (errorMessage != null) {
                Text(
                    text = errorMessage!!,
                    color = AvalancheColors.Error,
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
                        color = AvalancheColors.Paper,
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

/**
 * Best-effort fetch of the operator's privacy policy URL for [serverUrl].
 * Returns null if the server is unreachable, the endpoint errors, or no
 * policy is configured — mirrors iOS PublicServerInfo.privacyPolicyURL(forServer:).
 */
private suspend fun loadPrivacyPolicyUrl(serverUrl: String): String? {
    return withContext(Dispatchers.IO) {
        runCatching {
            val infoUrl = serverUrl.trimEnd('/') + "/v1/info"
            val connection = URL(infoUrl).openConnection() as java.net.HttpURLConnection
            connection.connectTimeout = 5_000
            connection.readTimeout = 5_000
            if (connection.responseCode != 200) return@runCatching null
            val body = connection.inputStream.bufferedReader().readText()
            val json = JSONObject(body)
            json.optString("privacy_policy_url").takeIf { it.isNotEmpty() }
        }.getOrNull()
    }
}

@Preview(showBackground = true)
@Composable
private fun JoiningServerViewPreview() {
    AvalancheTheme {
        // TODO(opus): wire a real AppViewModel for interactive previews via
        // LocalContext + AppViewModelFactory; using a simplified stub here.
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
        )
        // Preview cannot construct AppViewModel without Context; show the
        // static layout only via a stub lambda approach.
        // TODO(opus): replace with a proper preview ViewModel once a
        // previewable interface exists.
    }
}
