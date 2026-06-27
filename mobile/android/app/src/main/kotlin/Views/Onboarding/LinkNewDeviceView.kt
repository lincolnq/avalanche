package net.theavalanche.app

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// ---------------------------------------------------------------------------
// LinkNewDeviceView — new-device (joining) side of device linking (docs/04 §4).
// Mirrors mobile/ios/Actnet/Sources/Views/Onboarding/LinkNewDeviceView.swift.
//
// This device has no account yet; it receives the identity bundle from an
// already-signed-in device over an ephemeral mailbox. By default it *scans* the
// existing device's code, but the user can flip to *showing* its own (mailbox
// defaults to the built-in host, so no server URL is needed). On success,
// AppViewModel exits onboarding and the nav graph swaps to the main app.
// ---------------------------------------------------------------------------

private enum class JoinMode { SCAN, SHOW }

private sealed interface JoinPhase {
    data object Scanning : JoinPhase
    data object Preparing : JoinPhase
    data object Showing : JoinPhase
    data object Linking : JoinPhase
    data class Failed(val message: String) : JoinPhase
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LinkNewDeviceView(
    viewModel: AppViewModel,
    onBack: () -> Unit = {},
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var mode by remember { mutableStateOf(JoinMode.SCAN) }
    var phase by remember { mutableStateOf<JoinPhase>(JoinPhase.Scanning) }
    var pairingCode by remember { mutableStateOf<String?>(null) }
    var attempt by remember { mutableStateOf(0) }

    // Show-mode flow runs automatically; scan-mode arms the camera and waits.
    LaunchedEffect(mode, attempt) {
        pairingCode = null
        if (mode == JoinMode.SCAN) {
            phase = JoinPhase.Scanning
            return@LaunchedEffect
        }
        phase = JoinPhase.Preparing
        runCatching {
            val link = viewModel.makeDeviceLink()
            val code = withContext(Dispatchers.IO) { link.createPairing(null) }
            pairingCode = code
            phase = JoinPhase.Showing
            viewModel.completeDeviceLink(link)
            // Success flips isOnboarding; the nav graph tears this screen down.
        }.onFailure { err ->
            phase = JoinPhase.Failed(err.message ?: "Linking failed")
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Link to a Device") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(AvalancheColors.Paper)
                .padding(innerPadding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp, vertical = 16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(24.dp),
        ) {
            Text(
                text = "Link this device to an account you're already signed in to on another device. Keep both devices on this screen until linking finishes.",
                color = AvalancheColors.Muted,
                fontSize = 14.sp,
                textAlign = TextAlign.Center,
            )

            when (mode) {
                JoinMode.SCAN -> ScanSection(phase = phase) { code ->
                    if (phase != JoinPhase.Scanning) return@ScanSection
                    phase = JoinPhase.Linking
                    scope.launch {
                        runCatching {
                            val link = viewModel.makeDeviceLink()
                            withContext(Dispatchers.IO) { link.acceptPairing(code) }
                            viewModel.completeDeviceLink(link)
                        }.onFailure { err ->
                            phase = JoinPhase.Failed(err.message ?: "Linking failed")
                        }
                    }
                }
                JoinMode.SHOW -> ShowSection(pairingCode = pairingCode) { code ->
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    clipboard.setPrimaryClip(ClipData.newPlainText("Pairing code", code))
                }
            }

            (phase as? JoinPhase.Failed)?.let { failed ->
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Text(
                        text = failed.message,
                        color = AvalancheColors.Error,
                        fontSize = 13.sp,
                        textAlign = TextAlign.Center,
                    )
                    OutlinedButton(onClick = { attempt += 1 }) { Text("Try Again") }
                }
            }

            if (phase != JoinPhase.Linking) {
                TextButton(onClick = {
                    mode = if (mode == JoinMode.SCAN) JoinMode.SHOW else JoinMode.SCAN
                    attempt += 1
                }) {
                    Text(
                        text = if (mode == JoinMode.SCAN) "Show a code instead" else "Scan the other device instead",
                        color = AvalancheColors.Muted,
                    )
                }
            }
        }
    }
}

@Composable
private fun ScanSection(
    phase: JoinPhase,
    onScanned: (String) -> Unit,
) {
    Column(
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        if (phase == JoinPhase.Linking) {
            CircularProgressIndicator()
            Text("Linking…", color = AvalancheColors.Muted, fontSize = 13.sp)
        } else {
            QRCodeCameraView(
                onScanned = onScanned,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(260.dp)
                    .clip(RoundedCornerShape(12.dp)),
            )
            Text(
                text = "Point this camera at the code shown on the other device.",
                color = AvalancheColors.Muted,
                fontSize = 13.sp,
                textAlign = TextAlign.Center,
            )
        }
    }
}

@Composable
private fun ShowSection(
    pairingCode: String?,
    onCopy: (String) -> Unit,
) {
    Column(
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        if (pairingCode != null) {
            QRCodeImage(text = pairingCode)
            Text(
                text = pairingCode,
                color = AvalancheColors.Muted,
                fontSize = 12.sp,
                fontFamily = FontFamily.Monospace,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            OutlinedButton(onClick = { onCopy(pairingCode) }) {
                Icon(Icons.Filled.ContentCopy, contentDescription = null)
                Spacer(Modifier.width(4.dp))
                Text("Copy code")
            }
            CircularProgressIndicator()
            Text("Waiting for the other device…", color = AvalancheColors.Muted, fontSize = 13.sp)
        } else {
            CircularProgressIndicator()
            Text("Preparing…", color = AvalancheColors.Muted, fontSize = 13.sp)
        }
    }
}
