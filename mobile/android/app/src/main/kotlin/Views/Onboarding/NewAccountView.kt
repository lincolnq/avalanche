package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CameraAlt
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Screen for creating a new identity after accepting an invite.
 *
 * Mirrors iOS Sources/Views/Onboarding/NewAccountView.swift.
 *
 * Navigation callbacks replace SwiftUI's .navigationDestination:
 *   [onNext] — called when the user taps "Next" (navigates to PasskeyExplainerView).
 *   [onRecover] — called when the user taps "Recover an existing identity instead"
 *                 (navigates to RecoveryExplainerView). Only shown when [showRecoverLink]
 *                 is true (default).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NewAccountView(
    inviteToken: InviteToken,
    showRecoverLink: Boolean = true,
    // displayName is passed into PasskeyExplainerView in the iOS impl; we surface it here
    // so the caller can wire the nav callback with the entered name.
    onNext: (displayName: String) -> Unit = {},
    onRecover: () -> Unit = {},
) {
    var displayName by remember { mutableStateOf("") }
    // null = loading; empty string = not configured
    var privacyPolicyUrl by remember { mutableStateOf<String?>(null) }

    // Mirrors SwiftUI .task { privacyPolicyURL = await PublicServerInfo.privacyPolicyURL(...) }
    LaunchedEffect(inviteToken.serverUrl) {
        privacyPolicyUrl = withContext(Dispatchers.IO) {
            // TODO(opus): wire to the real PublicServerInfo.privacyPolicyUrl(forServer:) call
            //  once that top-level UniFFI function or HTTP helper is available on Android.
            runCatching { fetchPublicServerInfoPrivacyPolicy(serverUrl = inviteToken.serverUrl) }
                .getOrNull()
        }
    }

    val uriHandler = LocalUriHandler.current

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(AvalancheColors.Paper)
            .padding(top = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(24.dp),
    ) {
        // Title — mirrors Text("Create a new identity").font(.headline).foregroundStyle(.secondary)
        Text(
            text = "Create a new identity",
            color = AvalancheColors.Muted,
            style = androidx.compose.material3.MaterialTheme.typography.titleMedium,
        )

        // TODO: Avatar photo picker — mirrors the Circle placeholder in iOS
        Box(
            modifier = Modifier
                .size(100.dp)
                .clip(CircleShape)
                .background(AvalancheColors.Sand200),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = Icons.Filled.CameraAlt,
                contentDescription = "Add photo",
                tint = AvalancheColors.Muted,
                modifier = Modifier.size(32.dp),
            )
        }

        // Display name text field — mirrors TextField("Your name", text: $displayName)
        OutlinedTextField(
            value = displayName,
            onValueChange = { displayName = it },
            placeholder = {
                Text(
                    text = "Your name",
                    modifier = Modifier.fillMaxWidth(),
                    textAlign = TextAlign.Center,
                    color = AvalancheColors.Muted,
                )
            },
            singleLine = true,
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 32.dp),
            textStyle = androidx.compose.ui.text.TextStyle(textAlign = TextAlign.Center),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
        )

        // Next button — disabled when displayName is empty
        Button(
            onClick = { onNext(displayName) },
            enabled = displayName.isNotEmpty(),
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 32.dp)
                .height(52.dp),
        ) {
            Text("Next")
        }

        // Privacy policy link — shown only after loading and if a URL was returned
        val policyUrl = privacyPolicyUrl
        if (!policyUrl.isNullOrEmpty()) {
            TextButton(
                onClick = { uriHandler.openUri(policyUrl) },
            ) {
                Text(
                    text = "View ${inviteToken.serverName}'s privacy policy",
                    style = androidx.compose.material3.MaterialTheme.typography.bodySmall,
                    color = AvalancheColors.Brand,
                )
            }
        }

        Spacer(modifier = Modifier.weight(1f))

        if (showRecoverLink) {
            TextButton(
                onClick = onRecover,
                modifier = Modifier.padding(bottom = 16.dp),
            ) {
                Text(
                    text = "Recover an existing identity instead",
                    style = androidx.compose.material3.MaterialTheme.typography.bodySmall,
                    color = AvalancheColors.Muted,
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Stub — replace with real HTTP call or UniFFI generated helper
// ---------------------------------------------------------------------------

/**
 * Fetch the operator's privacy-policy URL from [serverUrl]/v1/info.
 * Returns null if not configured.
 *
 * TODO(opus): implement via the real PublicServerInfo utility once it exists on
 *  Android. iOS calls PublicServerInfo.privacyPolicyURL(forServer:) which hits
 *  <serverUrl>/v1/info and parses the JSON response.
 */
private suspend fun fetchPublicServerInfoPrivacyPolicy(serverUrl: String): String? {
    // TODO(opus): real implementation — make an HTTP GET to $serverUrl/v1/info,
    //  parse JSON, return the "privacy_policy_url" field (or null if absent).
    return null
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

@Preview(showBackground = true)
@Composable
private fun NewAccountPreview() {
    val fakeToken = InviteToken(
        token = "fake-token",
        serverUrl = "https://example.theavalanche.net",
        serverName = "Example Server",
        inviterDid = null,
        postOnboardingRedirect = null,
    )
    AvalancheTheme {
        NewAccountView(
            inviteToken = fakeToken,
            showRecoverLink = true,
        )
    }
}
