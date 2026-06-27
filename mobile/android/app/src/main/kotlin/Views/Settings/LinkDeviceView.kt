package net.theavalanche.app

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material3.Button
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
import kotlinx.coroutines.launch

// ---------------------------------------------------------------------------
// LinkDeviceView — existing-device side of device linking (docs/04 §4).
// Mirrors mobile/ios/Actnet/Sources/Views/Settings/LinkDeviceView.swift.
//
// This device already has the account and authorizes a new one by sealing the
// identity bundle to it over an ephemeral mailbox. Role is independent of
// gesture: by default this device *shows* a code for the new device to scan,
// but the user can flip to *scanning* the new device's code instead.
// ---------------------------------------------------------------------------

private enum class LinkMode { SHOW, SCAN }

private sealed interface LinkPhase {
    data object Preparing : LinkPhase
    data object Waiting : LinkPhase
    data object Scanning : LinkPhase
    data object Linking : LinkPhase
    data object Done : LinkPhase
    data class Failed(val message: String) : LinkPhase
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LinkDeviceView(
    accountId: String,
    viewModel: AppViewModel,
    onBack: () -> Unit = {},
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var mode by remember { mutableStateOf(LinkMode.SHOW) }
    var phase by remember { mutableStateOf<LinkPhase>(LinkPhase.Preparing) }
    var pairingCode by remember { mutableStateOf<String?>(null) }
    var attempt by remember { mutableStateOf(0) }

    // Show-mode flow runs automatically; scan-mode just arms the camera and
    // waits for a scan. Re-runs when the mode or retry counter changes.
    LaunchedEffect(mode, attempt) {
        pairingCode = null
        if (mode == LinkMode.SCAN) {
            phase = LinkPhase.Scanning
            return@LaunchedEffect
        }
        phase = LinkPhase.Preparing
        runCatching {
            val code = viewModel.linkCreatePairing(accountId)
            pairingCode = code
            phase = LinkPhase.Waiting
            viewModel.linkSendBundle(accountId)
        }.onSuccess {
            phase = LinkPhase.Done
        }.onFailure { err ->
            phase = LinkPhase.Failed(err.message ?: "Linking failed")
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Link a Device") },
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
                text = "Link the other device to this account. Both devices must stay on this screen until linking finishes.",
                color = AvalancheColors.Muted,
                fontSize = 14.sp,
                textAlign = TextAlign.Center,
            )

            when (mode) {
                LinkMode.SHOW -> ShowSection(phase = phase, pairingCode = pairingCode) { code ->
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    clipboard.setPrimaryClip(ClipData.newPlainText("Pairing code", code))
                }
                LinkMode.SCAN -> ScanSection(phase = phase) { code ->
                    if (phase != LinkPhase.Scanning) return@ScanSection
                    phase = LinkPhase.Linking
                    scope.launch {
                        runCatching {
                            viewModel.linkAcceptPairing(accountId, code)
                            viewModel.linkSendBundle(accountId)
                        }.onSuccess {
                            phase = LinkPhase.Done
                        }.onFailure { err ->
                            phase = LinkPhase.Failed(err.message ?: "Linking failed")
                        }
                    }
                }
            }

            (phase as? LinkPhase.Failed)?.let { failed ->
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

            when {
                phase == LinkPhase.Done -> Button(
                    onClick = onBack,
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(52.dp),
                ) { Text("Done") }
                phase != LinkPhase.Linking -> ModeToggle(mode = mode) {
                    mode = if (mode == LinkMode.SHOW) LinkMode.SCAN else LinkMode.SHOW
                    attempt += 1
                }
            }
        }
    }
}

@Composable
private fun ShowSection(
    phase: LinkPhase,
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
            StatusFooter(phase)
        } else {
            CircularProgressIndicator()
            Text("Preparing…", color = AvalancheColors.Muted, fontSize = 13.sp)
        }
    }
}

@Composable
private fun ScanSection(
    phase: LinkPhase,
    onScanned: (String) -> Unit,
) {
    Column(
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        when (phase) {
            LinkPhase.Linking -> {
                CircularProgressIndicator()
                Text("Linking…", color = AvalancheColors.Muted, fontSize = 13.sp)
            }
            LinkPhase.Done -> StatusFooter(phase)
            else -> {
                QRCodeCameraView(
                    onScanned = onScanned,
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(260.dp)
                        .clip(RoundedCornerShape(12.dp)),
                )
                Text(
                    text = "Point this camera at the code on the other device.",
                    color = AvalancheColors.Muted,
                    fontSize = 13.sp,
                    textAlign = TextAlign.Center,
                )
            }
        }
    }
}

@Composable
private fun StatusFooter(phase: LinkPhase) {
    when (phase) {
        LinkPhase.Waiting -> Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            CircularProgressIndicator(modifier = Modifier.size(20.dp), strokeWidth = 2.dp)
            Text("Waiting for the other device…", color = AvalancheColors.Muted, fontSize = 13.sp)
        }
        LinkPhase.Done -> Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Icon(Icons.Filled.CheckCircle, contentDescription = null, tint = AvalancheColors.Brand)
            Text("Device linked", color = AvalancheColors.Brand, fontSize = 16.sp)
        }
        else -> {}
    }
}

@Composable
private fun ModeToggle(mode: LinkMode, onToggle: () -> Unit) {
    TextButton(onClick = onToggle) {
        Text(
            text = if (mode == LinkMode.SHOW) "Scan the other device instead" else "Show a code instead",
            color = AvalancheColors.Muted,
        )
    }
}
