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

    // ------------------------------------------------------------------
    // Register with a passkey (Android Credential Manager analog of the
    // iOS ASAuthorization passkey ceremony).
    // ------------------------------------------------------------------
    fun registerWithPasskey() {
        isRegistering = true
        errorMessage = null
        scope.launch {
            try {
                // TODO(opus): Android passkey registration uses the Credential Manager API
                // (androidx.credentials.CreatePublicKeyCredentialRequest). The iOS flow calls
                // PasskeyManager.register() which drives an ASAuthorization sheet.
                // On Android, launch CredentialManager.createCredential() here, parse the
                // CBOR attestation, extract the PRF output (or HMAC-Secret extension), and
                // derive the rotation key exactly as iOS does.
                //
                // Stub: skip to the "no passkey" path until Credential Manager is wired.
                throw UnsupportedOperationException(
                    "Passkey registration via Android Credential Manager is not yet implemented. " +
                        "Use 'Recovery phrase' or 'Skip' for now."
                )

                // When implemented the code should be:
                //
                // val passkeyResult = PasskeyManager.register(
                //     serverUrl = inviteToken.serverUrl,
                //     displayName = "$displayName @ ${inviteToken.serverName}",
                //     activity = <Activity>,        // TODO(opus): inject via LocalContext
                // )
                //
                // val prepared = viewModel.prepareAccount(
                //     serverUrl = inviteToken.serverUrl,
                //     prfOutput = passkeyResult.prfOutput,
                // )
                //
                // viewModel.finalizePreparedAccount(
                //     prepared = prepared,
                //     serverUrl = inviteToken.serverUrl,
                //     serverName = inviteToken.serverName,
                //     displayName = displayName,
                //     inviteToken = inviteToken.token,
                // )
                //
                // inviteToken.postOnboardingRedirect?.let { onHandleDeepLink(it) }
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
            .background(AvalancheColors.Paper)
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
            color = AvalancheColors.Ink,
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
                    .background(AvalancheColors.Sand200),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = displayName.firstOrNull()?.uppercaseChar()?.toString() ?: "",
                    style = MaterialTheme.typography.headlineMedium,
                    color = AvalancheColors.Muted,
                )
            }
            Text(
                text = displayName,
                style = MaterialTheme.typography.titleMedium,
                color = AvalancheColors.Ink,
            )
        }

        // Explainer body
        Text(
            text = "Passkeys are stored securely in your password manager or Google account, " +
                "and synced across all your devices. You’ll use it to sign back into this " +
                "identity if you lose this device.",
            style = MaterialTheme.typography.bodySmall,
            color = AvalancheColors.Muted,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 24.dp),
        )

        // Error message
        errorMessage?.let { error ->
            Text(
                text = error,
                color = AvalancheColors.Error,
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
                        color = AvalancheColors.Paper,
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
                    color = AvalancheColors.Brand,
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
                    color = AvalancheColors.Muted,
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
    // AppViewModel requires a Context; preview uses a minimal stub ViewModel.
    // TODO(opus): use a LocalInspectionMode-aware factory if previews need a real VM.
    AvalancheTheme {
        // Render the pure layout with placeholder values — callback lambdas are no-ops.
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(AvalancheColors.Paper)
                .padding(horizontal = 32.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Spacer(modifier = Modifier.weight(1f))

            Text(
                text = "Create a passkey to protect this identity",
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
                textAlign = TextAlign.Center,
                color = AvalancheColors.Ink,
            )

            Column(
                modifier = Modifier.padding(top = 24.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Box(
                    modifier = Modifier
                        .size(64.dp)
                        .clip(CircleShape)
                        .background(AvalancheColors.Sand200),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = "A",
                        style = MaterialTheme.typography.headlineMedium,
                        color = AvalancheColors.Muted,
                    )
                }
                Text(
                    text = "Alice",
                    style = MaterialTheme.typography.titleMedium,
                    color = AvalancheColors.Ink,
                )
            }

            Text(
                text = "Passkeys are stored securely in your password manager or Google account, " +
                    "and synced across all your devices. You’ll use it to sign back into this " +
                    "identity if you lose this device.",
                style = MaterialTheme.typography.bodySmall,
                color = AvalancheColors.Muted,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(top = 24.dp),
            )

            Spacer(modifier = Modifier.weight(1f))

            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(bottom = 48.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Button(onClick = {}, modifier = Modifier.fillMaxWidth()) {
                    Icon(imageVector = Icons.Filled.Key, contentDescription = null)
                    Text(text = "Create Passkey", modifier = Modifier.padding(start = 8.dp))
                }
                TextButton(onClick = {}) {
                    Text(
                        text = "Use a recovery phrase instead",
                        style = MaterialTheme.typography.bodySmall,
                        color = AvalancheColors.Brand,
                    )
                }
                TextButton(onClick = {}) {
                    Text(
                        text = "Skip recovery setup",
                        style = MaterialTheme.typography.bodySmall,
                        color = AvalancheColors.Muted,
                    )
                }
            }
        }
    }
}
