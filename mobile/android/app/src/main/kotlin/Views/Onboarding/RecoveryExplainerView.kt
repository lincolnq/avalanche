package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Badge
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.app_core.deriveDidFromPasskey
import uniffi.app_core.recoveryPhraseToSeed

/**
 * Mirrors iOS RecoveryExplainerView.
 *
 * Offers two recovery paths:
 *  1. Passkey (Android Credential Manager — see TODO below)
 *  2. Recovery phrase + home server URL
 *
 * On success both paths resolve a DID + PRF seed and navigate to
 * RecoveryConsoleView (not yet ported) via [onNavigateToRecoveryConsole].
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RecoveryExplainerView(
    accounts: List<Account> = emptyList(),
    onNavigateToRecoveryConsole: (prfOutput: ByteArray, did: String) -> Unit = { _, _ -> },
    onNavigateUp: () -> Unit = {},
) {
    var showPhraseEntry by remember { mutableStateOf(false) }
    var errorMessage by remember { mutableStateOf<String?>(null) }

    val scope = rememberCoroutineScope()
    val context = LocalContext.current

    // Helper: shared post-validation logic once we have seed + DID.
    fun finishRecovery(seed: ByteArray, derivedDid: String, onError: (String) -> Unit) {
        if (accounts.any { it.id == derivedDid }) {
            onError("This identity is already signed in on this device.")
            return
        }
        onNavigateToRecoveryConsole(seed, derivedDid)
    }

    fun recoverWithPhrase(phrase: String, serverUrl: String) {
        errorMessage = null
        scope.launch {
            try {
                val seed = withContext(Dispatchers.IO) {
                    recoveryPhraseToSeed(phrase)
                }
                val derivedDid = withContext(Dispatchers.IO) {
                    deriveDidFromPasskey(prfOutput = seed, signupServerUrl = serverUrl)
                }
                finishRecovery(seed, derivedDid) { msg -> errorMessage = msg }
                if (errorMessage == null) {
                    showPhraseEntry = false
                }
            } catch (e: Exception) {
                errorMessage = "Invalid recovery phrase: ${e.message}"
            }
        }
    }

    // Mirrors iOS RecoveryExplainerView.recoverWithPasskey(): run the WebAuthn
    // assertion, recompute the DID deterministically from the PRF output + the
    // signup server URL stored in the userHandle, then hand off to the console.
    fun recoverWithPasskey() {
        errorMessage = null
        scope.launch {
            try {
                val passkey = PasskeyManager.authenticate(context = context.findActivity())
                val derivedDid = withContext(Dispatchers.IO) {
                    deriveDidFromPasskey(
                        prfOutput = passkey.prfOutput,
                        signupServerUrl = passkey.signupServerUrl,
                    )
                }
                finishRecovery(passkey.prfOutput, derivedDid) { msg -> errorMessage = msg }
            } catch (e: PasskeyException.Cancelled) {
                // User cancelled — stay on the screen, no error.
            } catch (e: Exception) {
                errorMessage = e.message ?: "Passkey recovery failed"
            }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Recovery") },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = LocalAvalancheColors.current.paper,
                    titleContentColor = LocalAvalancheColors.current.ink,
                ),
                navigationIcon = {
                    TextButton(onClick = onNavigateUp) {
                        Text("Back", color = LocalAvalancheColors.current.brand)
                    }
                },
            )
        },
        containerColor = LocalAvalancheColors.current.paper,
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding)
                .windowInsetsPadding(WindowInsets.systemBars),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Spacer(Modifier.weight(1f))

            Icon(
                imageVector = Icons.Filled.Badge,
                contentDescription = null,
                tint = LocalAvalancheColors.current.brand,
                modifier = Modifier.size(64.dp),
            )

            Spacer(Modifier.height(16.dp))

            Text(
                text = "Recover an identity",
                style = MaterialTheme.typography.titleLarge,
                color = LocalAvalancheColors.current.ink,
            )

            Spacer(Modifier.height(8.dp))

            Text(
                text = "Use a passkey or recovery phrase to restore an identity you created on another device.",
                style = MaterialTheme.typography.bodyMedium,
                color = LocalAvalancheColors.current.muted,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(horizontal = 32.dp),
            )

            if (errorMessage != null) {
                Spacer(Modifier.height(12.dp))
                Text(
                    text = errorMessage!!,
                    style = MaterialTheme.typography.bodyMedium,
                    color = LocalAvalancheColors.current.error,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 32.dp),
                )
            }

            Spacer(Modifier.weight(1f))

            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 32.dp)
                    .padding(bottom = 48.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Button(
                    onClick = { recoverWithPasskey() },
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(52.dp),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = LocalAvalancheColors.current.brand,
                        contentColor = LocalAvalancheColors.current.paper,
                    ),
                ) {
                    Icon(Icons.Filled.Badge, contentDescription = null)
                    Spacer(Modifier.padding(horizontal = 4.dp))
                    Text("Recover using Passkey")
                }

                TextButton(onClick = { showPhraseEntry = true }) {
                    Text(
                        text = "Enter your recovery phrase instead",
                        style = MaterialTheme.typography.bodyMedium,
                        color = LocalAvalancheColors.current.brand,
                    )
                }
            }
        }
    }

    // Recovery phrase sheet — shown as a dialog to mirror the iOS .sheet() presentation.
    if (showPhraseEntry) {
        RecoveryPhraseEntryView(
            onComplete = { phrase, serverUrl -> recoverWithPhrase(phrase, serverUrl) },
            onDismiss = { showPhraseEntry = false },
        )
    }
}

// ---------------------------------------------------------------------------
// RecoveryPhraseEntryView
//
// Mirrors the private iOS RecoveryPhraseEntryView sheet — shown as a
// full-screen dialog on Android since Compose sheets are modal and NavigationStack
// children work differently. The parent controls dismissal via [onDismiss].
// ---------------------------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun RecoveryPhraseEntryView(
    onComplete: (phrase: String, serverUrl: String) -> Unit,
    onDismiss: () -> Unit,
) {
    var phrase by remember { mutableStateOf("") }
    // Mirror iOS DEBUG default: pre-fill localhost in debug, blank in release.
    var serverUrl by remember {
        mutableStateOf(
            if (BuildConfig.DEBUG) "http://localhost:3000" else ""
        )
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = {
            Button(
                onClick = { onComplete(phrase, serverUrl.trim()) },
                enabled = phrase.isNotBlank() && serverUrl.isNotBlank(),
                colors = ButtonDefaults.buttonColors(
                    containerColor = LocalAvalancheColors.current.brand,
                    contentColor = LocalAvalancheColors.current.paper,
                ),
            ) {
                Text("Recover")
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("Cancel", color = LocalAvalancheColors.current.brand)
            }
        },
        title = { Text("Recovery Phrase", style = MaterialTheme.typography.titleMedium) },
        text = {
            Column(
                verticalArrangement = Arrangement.spacedBy(16.dp),
                modifier = Modifier.verticalScroll(rememberScrollState()),
            ) {
                Text(
                    text = "Enter your recovery phrase",
                    style = MaterialTheme.typography.bodyMedium,
                    color = LocalAvalancheColors.current.ink,
                )

                OutlinedTextField(
                    value = phrase,
                    onValueChange = { phrase = it },
                    label = { Text("Recovery phrase") },
                    minLines = 3,
                    maxLines = 6,
                    modifier = Modifier.fillMaxWidth(),
                    keyboardOptions = KeyboardOptions(
                        capitalization = KeyboardCapitalization.None,
                        autoCorrectEnabled = false,
                        imeAction = ImeAction.Next,
                    ),
                )

                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(
                        text = "Home server",
                        style = MaterialTheme.typography.labelSmall,
                        color = LocalAvalancheColors.current.muted,
                    )
                    OutlinedTextField(
                        value = serverUrl,
                        onValueChange = { serverUrl = it },
                        label = { Text("https://server.example") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                        keyboardOptions = KeyboardOptions(
                            capitalization = KeyboardCapitalization.None,
                            autoCorrectEnabled = false,
                            keyboardType = KeyboardType.Uri,
                            imeAction = ImeAction.Done,
                        ),
                    )
                }
            }
        },
        containerColor = LocalAvalancheColors.current.paper,
    )
}

// ---------------------------------------------------------------------------
// Previews
// ---------------------------------------------------------------------------

@Preview(showBackground = true)
@Composable
private fun RecoveryExplainerPreview() {
    AvalancheTheme {
        RecoveryExplainerView()
    }
}

@Preview(showBackground = true)
@Composable
private fun RecoveryPhraseEntryPreview() {
    AvalancheTheme {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(LocalAvalancheColors.current.paper),
        ) {
            RecoveryPhraseEntryView(
                onComplete = { _, _ -> },
                onDismiss = {},
            )
        }
    }
}
