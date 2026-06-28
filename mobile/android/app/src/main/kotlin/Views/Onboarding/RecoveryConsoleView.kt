package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// Recovery console view shown during passkey/phrase-based account recovery.
// Mirrors mobile/ios/Actnet/Sources/Views/Onboarding/RecoveryConsoleView.swift.
//
// prfOutput — raw 32-byte PRF output from the passkey ceremony (or derived
//             from a recovery phrase). Passed as ByteArray (Swift Data).
// did       — the DID embedded in the passkey, or empty for phrase-based
//             recovery that has no DID yet.
// appViewModel — top-level ViewModel; recovery calls appViewModel.recoverAccount.
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RecoveryConsoleView(
    prfOutput: ByteArray,
    did: String,
    appViewModel: AppViewModel,
    onBack: () -> Unit = {},
) {
    // Mutable list of console lines — appended from coroutines on the main thread.
    val logLines = remember { mutableStateListOf<String>() }

    // Server URL input — pre-fill localhost in debug builds like iOS does.
    var serverUrlInput by rememberSaveable {
        mutableStateOf(if (BuildConfig.DEBUG) "http://localhost:3000" else "")
    }

    // Whether to show the manual server URL prompt.
    var needsServerUrl by remember { mutableStateOf(false) }

    // Single-flight guard: mirrors iOS @State var didStart = false.
    var didStart by remember { mutableStateOf(false) }

    val scope = rememberCoroutineScope()
    val listState = rememberLazyListState()

    // Scroll to bottom whenever a new line is appended.
    LaunchedEffect(logLines.size) {
        if (logLines.isNotEmpty()) {
            listState.animateScrollToItem(logLines.size - 1)
        }
    }

    // Helpers that log a line and mirror Swift AppLog calls.
    fun log(line: String) {
        logLines.add(line)
        when {
            line.startsWith("[!]") ->
                AppLog.error("recovery", line.drop(3).trim())
            line.startsWith("[ok]") ->
                AppLog.ok("recovery", line.drop(4).trim())
            else ->
                AppLog.info("recovery", line)
        }
    }

    // Stage 2: connect to a known server URL and attempt blob download + restore.
    suspend fun performRecoveryWithServer(serverUrl: String) {
        log("Connecting to $serverUrl...")
        delay(300L)

        if (did.isEmpty()) {
            // Phrase-based flow: we don't know the DID without a server lookup.
            log("[!] Recovery phrase flow requires knowing your DID.")
            log("[!] This flow is not yet fully implemented.")
            log("Please use passkey recovery instead, which embeds your DID.")
            return
        }

        log("Downloading recovery blob for $did...")
        try {
            appViewModel.recoverAccount(
                serverUrl = serverUrl,
                serverName = serverUrl,
                did = did,
                prfOutput = prfOutput,
                displayName = "",
            )
            log("[ok] Identity restored. Replacing device on home server...")
            log("[ok] Signed in!")
            // recoverAccount flips appViewModel.isOnboarding = false,
            // which causes the root NavGraph to swap to MainTabView.
            // No explicit navigation callback needed here.
        } catch (e: Exception) {
            log("[!] Recovery failed: ${e.message ?: "unknown error"}")
            log("Check that the server URL and recovery key are correct.")
        }
    }

    // Stage 1: determine server URL (via PLC directory lookup or manual entry).
    suspend fun performRecovery() {
        if (did.isEmpty()) {
            // Phrase-based recovery — no DID from passkey.
            log("Recovery phrase entered.")
            log("We need your home server URL to find your recovery blob.")
            needsServerUrl = true
            return
        }

        log("DID: $did")

        // did:local:* (bot/test accounts) have no PLC entry — prompt manually.
        if (!did.startsWith("did:plc:")) {
            log("DID is not a did:plc:* — manual home server URL required.")
            needsServerUrl = true
            return
        }

        log("Resolving DID from PLC directory...")
        val resolved: String = try {
            withContext(Dispatchers.IO) {
                // TODO(opus): implement resolveHomeserverFromPlc(did) — mirrors
                // iOS resolveHomeserverFromPlc(did:) which does an HTTP GET to
                // https://plc.directory/<did> and parses the serviceEndpoint.
                resolveHomeserverFromPlc(did = did)
            }
        } catch (e: Exception) {
            log("[!] PLC lookup failed: ${e.message ?: "unknown error"}")
            log("Please enter your home server URL to continue.")
            needsServerUrl = true
            return
        }

        log("[ok] Home server: $resolved")
        performRecoveryWithServer(serverUrl = resolved)
    }

    // Fire performRecovery once when the composable first enters the composition.
    LaunchedEffect(Unit) {
        if (!didStart) {
            didStart = true
            performRecovery()
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Recovering...") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = LocalAvalancheColors.current.paper,
                    titleContentColor = LocalAvalancheColors.current.ink,
                ),
            )
        },
        containerColor = LocalAvalancheColors.current.paper,
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(LocalAvalancheColors.current.paper)
                .padding(innerPadding),
        ) {
            // Scrollable console log — takes all remaining space.
            LazyColumn(
                state = listState,
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .padding(horizontal = 12.dp, vertical = 8.dp),
            ) {
                itemsIndexed(logLines) { _, line ->
                    Text(
                        text = line,
                        fontSize = 12.sp,
                        fontFamily = FontFamily.Monospace,
                        color = lineColor(line),
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(vertical = 1.dp),
                        softWrap = true,
                    )
                }
            }

            // Server URL prompt — shown when automatic PLC resolution fails or
            // when doing phrase-based recovery that has no embedded DID.
            if (needsServerUrl) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(16.dp),
                ) {
                    Text(
                        text = "Enter your home server URL:",
                        fontSize = 14.sp,
                        color = LocalAvalancheColors.current.ink,
                    )
                    Spacer(Modifier.height(8.dp))
                    OutlinedTextField(
                        value = serverUrlInput,
                        onValueChange = { serverUrlInput = it },
                        placeholder = { Text("https://server.example", color = LocalAvalancheColors.current.muted) },
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(
                            capitalization = KeyboardCapitalization.None,
                            keyboardType = KeyboardType.Uri,
                            imeAction = ImeAction.Done,
                        ),
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Spacer(Modifier.height(12.dp))
                    Button(
                        onClick = {
                            needsServerUrl = false
                            scope.launch {
                                performRecoveryWithServer(serverUrl = serverUrlInput)
                            }
                        },
                        enabled = serverUrlInput.isNotEmpty(),
                        colors = ButtonDefaults.buttonColors(
                            containerColor = LocalAvalancheColors.current.brand,
                        ),
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text("Continue")
                    }
                    Spacer(Modifier.height(8.dp))
                }
            }
        }
    }
}

// Color-code a console log line exactly as iOS does:
//   [!]  -> avError  (rose/red)
//   [ok] -> avBrand  (plum)
//   else -> default text
@Composable
private fun lineColor(line: String): Color = when {
    line.startsWith("[!]") -> LocalAvalancheColors.current.error
    line.startsWith("[ok]") -> LocalAvalancheColors.current.brand
    else -> LocalAvalancheColors.current.ink
}

// Stub for the PLC directory HTTP lookup.
// TODO(opus): implement fully — iOS uses resolveHomeserverFromPlc(did:) from
// IosHelpers.swift which fetches https://plc.directory/<did>, parses the JSON
// response, and returns the `serviceEndpoint` URL for the atproto PDS service.
// The Android equivalent should do the same via OkHttp or java.net.URL on
// Dispatchers.IO (already on IO when called from performRecovery).
private fun resolveHomeserverFromPlc(did: String): String {
    // TODO(opus): implement PLC directory HTTP GET and JSON parsing.
    throw UnsupportedOperationException("resolveHomeserverFromPlc not yet implemented on Android")
}

@Preview(showBackground = true)
@Composable
private fun RecoveryConsolePreview() {
    AvalancheTheme {
        RecoveryConsoleView(
            prfOutput = ByteArray(32),
            did = "did:example:alice",
            appViewModel = rememberPreviewAppViewModel(),
        )
    }
}
