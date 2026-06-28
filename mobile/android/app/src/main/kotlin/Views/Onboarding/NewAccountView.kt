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
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.CameraAlt
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
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
 *   [onBack] — called when the user taps the app-bar back arrow. Mirrors the
 *              system back button iOS gets from `.navigationTitle` + the
 *              NavigationStack. The host pops the back stack.
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
    onBack: () -> Unit = {},
) {
    var displayName by remember { mutableStateOf("") }
    // The privacy policy URL comes from invite validation (resolved by the
    // core), so there's no separate server call. null/blank when not configured.
    val privacyPolicyUrl = inviteToken.privacyPolicyUrl
    val uriHandler = LocalUriHandler.current

    Scaffold(
        topBar = {
            // iOS shows an inline `.navigationTitle("New Identity")` with the
            // system back button; mirror that with an explicit app bar + back
            // arrow (Compose has no implicit nav chrome).
            TopAppBar(
                title = { Text("New Identity") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
        containerColor = LocalAvalancheColors.current.paper,
    ) { innerPadding ->
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(LocalAvalancheColors.current.paper)
            .padding(innerPadding)
            .padding(top = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(24.dp),
    ) {
        // Title — mirrors Text("Create a new identity").font(.headline).foregroundStyle(.secondary)
        Text(
            text = "Create a new identity",
            color = LocalAvalancheColors.current.muted,
            style = androidx.compose.material3.MaterialTheme.typography.titleMedium,
        )

        // TODO: Avatar photo picker — mirrors the Circle placeholder in iOS
        Box(
            modifier = Modifier
                .size(100.dp)
                .clip(CircleShape)
                .background(LocalAvalancheColors.current.card),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = Icons.Filled.CameraAlt,
                contentDescription = "Add photo",
                tint = LocalAvalancheColors.current.muted,
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
                    color = LocalAvalancheColors.current.muted,
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

        // Privacy policy link — shown only if the operator configured one.
        // Guard against blank values so we never call openUri("").
        if (!privacyPolicyUrl.isNullOrBlank()) {
            TextButton(
                onClick = { uriHandler.openUri(privacyPolicyUrl) },
            ) {
                Text(
                    text = "View ${inviteToken.serverName}'s privacy policy",
                    style = androidx.compose.material3.MaterialTheme.typography.bodySmall,
                    color = LocalAvalancheColors.current.brand,
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
                    color = LocalAvalancheColors.current.muted,
                )
            }
        }
    }
    }
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
        privacyPolicyUrl = "https://example.theavalanche.net/privacy",
    )
    AvalancheTheme {
        NewAccountView(
            inviteToken = fakeToken,
            showRecoverLink = true,
        )
    }
}
