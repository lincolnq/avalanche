package net.theavalanche.app

import android.net.Uri
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Key
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

/**
 * Explains passkey-based recovery and lets the user create a passkey, use a
 * recovery phrase, or skip recovery entirely.
 *
 * Mirrors iOS PasskeyExplainerView.swift. Navigation is via callback lambdas;
 * a central NavGraph wires them.
 *
 * @param inviteToken    The validated invite token driving this onboarding flow.
 * @param displayName    The display name the user chose earlier in onboarding.
 * @param viewModel      The app-level ViewModel. Passkey and account calls go through it.
 * @param onNavigateToRecoveryPhraseSetup  Called to push RecoveryPhraseSetupView onto the
 *                                          back stack. The caller receives inviteToken and
 *                                          displayName so it can construct the destination.
 * @param onHandleDeepLink  Called after account creation when the invite carries a
 *                          post-onboarding redirect URL. Caller converts the string to a
 *                          [Uri] and delegates to AppViewModel.handleDeepLink.
 */
@Composable
fun PasskeyExplainerView(
    inviteToken: InviteToken,
    displayName: String,
    viewModel: AppViewModel,
    onNavigateToRecoveryPhraseSetup: (inviteToken: InviteToken, displayName: String) -> Unit = { _, _ -> },
    onHandleDeepLink: (url: String) -> Unit = {},
) {
    var isRegistering by remember { mutableStateOf(false) }
    var errorMessage by remember { mutableStateOf<String?>(null) }

    val scope = rememberCoroutineScope()
    val context = LocalContext.current

    // ------------------------------------------------------------------
    // Register with a passkey (Android Credential Manager analog of the
    // iOS ASAuthorization passkey ceremony). Mirrors iOS
    // PasskeyExplainerView.registerWithPasskey().
    // ------------------------------------------------------------------
    fun registerWithPasskey() {
        isRegistering = true
        errorMessage = null
        scope.launch {
            try {
                // Stage 1: run the passkey ceremony first. The credential's
                // user.id is set to the signup server URL — that's what lets
                // recovery recompute the DID later without prompting the user.
                val passkey = PasskeyManager.register(
                    context = context.findActivity(),
                    signupServerUrl = inviteToken.serverUrl,
                    displayName = "$displayName @ ${inviteToken.serverName}",
                )

                // Stage 2: derive the rotation key from the PRF output and build
                // both PLC ops. The DID drops out of this.
                val prepared = viewModel.prepareAccount(
                    serverUrl = inviteToken.serverUrl,
                    prfOutput = passkey.prfOutput,
                )

                // Stage 3: submit the PLC ops, encrypt the recovery blob with the
                // PRF-derived key, and register with the homeserver.
                viewModel.finalizePreparedAccount(
                    prepared = prepared,
                    serverUrl = inviteToken.serverUrl,
                    serverName = inviteToken.serverName,
                    displayName = displayName,
                    inviteToken = inviteToken.token,
                )
                // finalizePreparedAccount sets isOnboarding = false, which
                // navigates to MainTabView.
                inviteToken.postOnboardingRedirect?.let { onHandleDeepLink(it) }
            } catch (e: PasskeyException.Cancelled) {
                // User cancelled — don't show an error, just re-enable buttons.
                isRegistering = false
            } catch (e: Exception) {
                errorMessage = e.message ?: "Passkey registration failed"
                isRegistering = false
            }
        }
    }

    // ------------------------------------------------------------------
    // Register without a passkey (empty prfOutput = unrecoverable, or
    // called after the recovery-phrase screen supplies a real prfOutput).
    // ------------------------------------------------------------------
    fun register(prfOutput: ByteArray) {
        isRegistering = true
        errorMessage = null
        scope.launch {
            try {
                viewModel.createAccount(
                    serverUrl = inviteToken.serverUrl,
                    serverName = inviteToken.serverName,
                    displayName = displayName,
                    inviteToken = inviteToken.token,
                    prfOutput = prfOutput,
                )
                // createAccount sets isOnboarding = false, which navigates to MainTabView.
                inviteToken.postOnboardingRedirect?.let { onHandleDeepLink(it) }
            } catch (e: Exception) {
                errorMessage = e.message ?: "Account creation failed"
                isRegistering = false
            }
        }
    }

    // ------------------------------------------------------------------
    // UI
    // ------------------------------------------------------------------
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(LocalAvalancheColors.current.paper)
            .padding(horizontal = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(modifier = Modifier.weight(1f))

        // Title
        Text(
            text = "Create a passkey to protect this identity",
            style = MaterialTheme.typography.titleLarge,
            fontWeight = FontWeight.SemiBold,
            textAlign = TextAlign.Center,
            color = LocalAvalancheColors.current.ink,
        )

        // Profile preview
        Column(
            modifier = Modifier.padding(top = 24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Box(
                modifier = Modifier
                    .size(64.dp)
                    .clip(CircleShape)
                    .background(LocalAvalancheColors.current.card),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = displayName.firstOrNull()?.uppercaseChar()?.toString() ?: "",
                    style = MaterialTheme.typography.headlineMedium,
                    color = LocalAvalancheColors.current.muted,
                )
            }
            Text(
                text = displayName,
                style = MaterialTheme.typography.titleMedium,
                color = LocalAvalancheColors.current.ink,
            )
        }

        // Explainer body
        Text(
            text = "Passkeys are stored securely in your password manager or Google account, " +
                "and synced across all your devices. You’ll use it to sign back into this " +
                "identity if you lose this device.",
            style = MaterialTheme.typography.bodySmall,
            color = LocalAvalancheColors.current.muted,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 24.dp),
        )

        // Error message
        errorMessage?.let { error ->
            Text(
                text = error,
                color = LocalAvalancheColors.current.error,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(top = 12.dp),
            )
        }

        Spacer(modifier = Modifier.weight(1f))

        // Action buttons
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(bottom = 48.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            // Primary: Create Passkey
            Button(
                onClick = { registerWithPasskey() },
                modifier = Modifier.fillMaxWidth(),
                enabled = !isRegistering,
            ) {
                if (isRegistering) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(20.dp),
                        strokeWidth = 2.dp,
                        color = LocalAvalancheColors.current.paper,
                    )
                } else {
                    Icon(
                        imageVector = Icons.Filled.Key,
                        contentDescription = null,
                    )
                    Text(
                        text = "Create Passkey",
                        modifier = Modifier.padding(start = 8.dp),
                    )
                }
            }

            // Secondary: Use a recovery phrase instead
            TextButton(
                onClick = { onNavigateToRecoveryPhraseSetup(inviteToken, displayName) },
                enabled = !isRegistering,
            ) {
                Text(
                    text = "Use a recovery phrase instead",
                    style = MaterialTheme.typography.bodySmall,
                    color = LocalAvalancheColors.current.brand,
                )
            }

            // Tertiary: Skip recovery setup entirely
            TextButton(
                onClick = { register(ByteArray(0)) },
                enabled = !isRegistering,
            ) {
                Text(
                    text = "Skip recovery setup",
                    style = MaterialTheme.typography.bodySmall,
                    color = LocalAvalancheColors.current.muted,
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

@Preview(showBackground = true)
@Composable
private fun PasskeyExplainerPreview() {
    AvalancheTheme {
        val token = InviteToken(
            token = "abc123",
            serverUrl = "https://demo.theavalanche.net",
            serverName = "Demo Server",
            inviterDid = null,
            postOnboardingRedirect = null,
            privacyPolicyUrl = null,
        )
        PasskeyExplainerView(
            inviteToken = token,
            displayName = "Alice",
            viewModel = rememberPreviewAppViewModel(),
        )
    }
}
