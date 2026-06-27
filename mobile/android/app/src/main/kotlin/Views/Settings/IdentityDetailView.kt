package net.theavalanche.app

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Color as AndroidColor
import androidx.compose.foundation.Image
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
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import kotlinx.coroutines.launch
import java.util.Base64

// ---------------------------------------------------------------------------
// IdentityDetailView — mirrors iOS Sources/Views/Settings/IdentityDetailView.swift
// ---------------------------------------------------------------------------

/**
 * Detail screen for a single identity/account. Shows the QR code for the contact
 * URL, the DID, home server info, blocked contacts link, and a delete button.
 *
 * Navigation is callback-based (no NavController direct dependency), matching the
 * lambda-nav pattern from SplashView. A central NavGraph wires these later.
 *
 * @param account             The account whose details to show.
 * @param viewModel           AppViewModel to call deleteIdentity.
 * @param onBack              Called when the screen should pop (after successful delete
 *                            or via the system back button).
 * @param onNavigateToBlocked Called to push BlockedContactsView for this account.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun IdentityDetailView(
    account: Account,
    viewModel: AppViewModel,
    onBack: () -> Unit = {},
    onNavigateToBlocked: (Account) -> Unit = {},
    onNavigateToLinkDevice: (Account) -> Unit = {},
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var showDeleteConfirmation by remember { mutableStateOf(false) }
    var showStubAlert by remember { mutableStateOf(false) }
    var stubMessage by remember { mutableStateOf("") }
    var isDeleting by remember { mutableStateOf(false) }
    var deleteError by remember { mutableStateOf<String?>(null) }

    val homeServer: ServerInfo? = account.servers.firstOrNull()
    val contactURL: String? = homeServer?.let { server ->
        val token = makeInviteToken(serverUrl = server.url.toString(), inviterDid = account.id)
        "https://go.theavalanche.net/i/$token"
    }
    val qrBitmap: Bitmap? = contactURL?.let { generateQRBitmap(it) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Identity") },
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
            .padding(bottom = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        // Header — avatar + display name
        Spacer(Modifier.height(16.dp))
        AccountAvatar(account = account, size = 72.dp)
        Spacer(Modifier.height(8.dp))
        Text(
            text = account.displayName,
            color = AvalancheColors.Ink,
            fontSize = 20.sp,
            fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold,
        )
        Spacer(Modifier.height(20.dp))

        // QR code + action buttons
        if (qrBitmap != null) {
            Image(
                bitmap = qrBitmap.asImageBitmap(),
                contentDescription = "Contact QR code",
                modifier = Modifier
                    .size(220.dp)
                    .clip(RoundedCornerShape(8.dp)),
            )
            Spacer(Modifier.height(12.dp))
            Row(
                horizontalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                OutlinedButton(onClick = {
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    clipboard.setPrimaryClip(ClipData.newPlainText("Contact URL", contactURL))
                }) {
                    Icon(Icons.Filled.ContentCopy, contentDescription = null)
                    Spacer(Modifier.width(4.dp))
                    Text("Copy")
                }
                OutlinedButton(onClick = {
                    val shareIntent = Intent(Intent.ACTION_SEND).apply {
                        type = "text/plain"
                        putExtra(Intent.EXTRA_TEXT, contactURL)
                    }
                    context.startActivity(Intent.createChooser(shareIntent, "Share contact link"))
                }) {
                    Icon(Icons.Filled.Share, contentDescription = null)
                    Spacer(Modifier.width(4.dp))
                    Text("Share")
                }
            }
            Spacer(Modifier.height(20.dp))
        }

        // DID row + home server row
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            DetailRow(label = "DID", value = account.id, mono = true)

            if (homeServer != null) {
                // "Change home server" row — stubbed out like iOS
                TextButton(
                    onClick = {
                        stubMessage = "Migration / change home server is not implemented yet."
                        showStubAlert = true
                    },
                    modifier = Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(8.dp),
                    colors = ButtonDefaults.textButtonColors(
                        containerColor = AvalancheColors.Sand50,
                        contentColor = AvalancheColors.Ink,
                    ),
                ) {
                    Column(
                        modifier = Modifier
                            .weight(1f)
                            .padding(vertical = 4.dp),
                        horizontalAlignment = Alignment.Start,
                    ) {
                        Text(
                            text = "Home server",
                            color = AvalancheColors.Muted,
                            fontSize = 11.sp,
                        )
                        Text(
                            text = homeServer.name,
                            color = AvalancheColors.Ink,
                        )
                        Text(
                            text = homeServer.url.toString(),
                            color = AvalancheColors.Muted,
                            fontSize = 10.sp,
                        )
                    }
                    Icon(
                        Icons.Filled.ChevronRight,
                        contentDescription = null,
                        tint = AvalancheColors.Muted,
                    )
                }
            }
        }

        Spacer(Modifier.height(12.dp))

        // Privacy disclaimer
        Text(
            text = "Your home server is listed publicly so people can reach you. Your display name, other server memberships, contacts, and messages are not public.",
            color = AvalancheColors.Muted,
            fontSize = 12.sp,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(horizontal = 16.dp),
        )

        Spacer(Modifier.height(20.dp))

        // Link a device + Blocked contacts rows
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            TextButton(
                onClick = { onNavigateToLinkDevice(account) },
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(8.dp),
                colors = ButtonDefaults.textButtonColors(
                    containerColor = AvalancheColors.Sand50,
                    contentColor = AvalancheColors.Ink,
                ),
            ) {
                Text(
                    text = "Link a Device",
                    modifier = Modifier.weight(1f),
                    textAlign = TextAlign.Start,
                )
                Icon(
                    Icons.Filled.ChevronRight,
                    contentDescription = null,
                    tint = AvalancheColors.Muted,
                )
            }

            TextButton(
                onClick = { onNavigateToBlocked(account) },
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(8.dp),
                colors = ButtonDefaults.textButtonColors(
                    containerColor = AvalancheColors.Sand50,
                    contentColor = AvalancheColors.Ink,
                ),
            ) {
                Text(
                    text = "Blocked Contacts",
                    modifier = Modifier.weight(1f),
                    textAlign = TextAlign.Start,
                )
                Icon(
                    Icons.Filled.ChevronRight,
                    contentDescription = null,
                    tint = AvalancheColors.Muted,
                )
            }
        }

        Spacer(Modifier.height(24.dp))

        // Delete identity button
        Button(
            onClick = { showDeleteConfirmation = true },
            enabled = !isDeleting,
            colors = ButtonDefaults.buttonColors(
                containerColor = AvalancheColors.Error,
                contentColor = AvalancheColors.Paper,
            ),
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp)
                .height(52.dp),
        ) {
            if (isDeleting) {
                CircularProgressIndicator(
                    color = AvalancheColors.Paper,
                    strokeWidth = 2.dp,
                    modifier = Modifier.size(20.dp),
                )
            } else {
                Text("Delete identity")
            }
        }
    }
    }

    // -----------------------------------------------------------------------
    // Dialogs
    // -----------------------------------------------------------------------

    // Delete confirmation dialog
    if (showDeleteConfirmation) {
        val serverCount = account.servers.count()
        AlertDialog(
            onDismissRequest = { showDeleteConfirmation = false },
            title = { Text("Delete this identity?") },
            text = {
                Text(
                    "This will delete ${account.displayName} from $serverCount " +
                        "server${if (serverCount == 1) "" else "s"} and mark the identity " +
                        "deleted in the public registry. This cannot be undone. Your other " +
                        "identities on this device will not be affected."
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        showDeleteConfirmation = false
                        isDeleting = true
                        scope.launch {
                            runCatching { viewModel.deleteIdentity(account = account) }
                                .onSuccess { onBack() }
                                .onFailure { err ->
                                    deleteError = err.message ?: "Unknown error"
                                }
                            isDeleting = false
                        }
                    },
                    colors = ButtonDefaults.textButtonColors(contentColor = AvalancheColors.Error),
                ) {
                    Text("Delete")
                }
            },
            dismissButton = {
                TextButton(onClick = { showDeleteConfirmation = false }) {
                    Text("Cancel")
                }
            },
        )
    }

    // Stub / not-implemented alert
    if (showStubAlert) {
        AlertDialog(
            onDismissRequest = { showStubAlert = false },
            title = { Text("Not implemented") },
            text = { Text(stubMessage) },
            confirmButton = {
                TextButton(onClick = { showStubAlert = false }) { Text("OK") }
            },
        )
    }

    // Delete error alert
    if (deleteError != null) {
        AlertDialog(
            onDismissRequest = { deleteError = null },
            title = { Text("Couldn't delete identity") },
            text = { Text(deleteError ?: "") },
            confirmButton = {
                TextButton(onClick = { deleteError = null }) { Text("OK") }
            },
        )
    }
}

// ---------------------------------------------------------------------------
// DetailRow — mirrors iOS private struct DetailRow
// ---------------------------------------------------------------------------

@Composable
private fun DetailRow(
    label: String,
    value: String,
    mono: Boolean = false,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(8.dp))
            .background(AvalancheColors.Sand50)
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Text(
            text = label,
            color = AvalancheColors.Muted,
            fontSize = 11.sp,
        )
        Text(
            text = value,
            color = AvalancheColors.Ink,
            fontSize = if (mono) 12.sp else 16.sp,
            fontFamily = if (mono) FontFamily.Monospace else FontFamily.Default,
        )
    }
}

// ---------------------------------------------------------------------------
// Helpers — mirrors iOS private funcs makeInviteToken / generateQRCode
// ---------------------------------------------------------------------------

/**
 * Encode server URL + inviter DID as a URL-safe base64 JSON token.
 * Mirrors iOS IdentityDetailView.makeInviteToken(serverUrl:inviterDid:).
 */
private fun makeInviteToken(serverUrl: String, inviterDid: String): String {
    // Single-char wire keys (s=server_url, d=inviter_did) keep the QR low-density.
    val json = """{"s":"$serverUrl","d":"$inviterDid"}"""
    val encoded = Base64.getEncoder().encodeToString(json.toByteArray(Charsets.UTF_8))
    return encoded
        .replace("+", "-")
        .replace("/", "_")
        .replace("=", "")
}

/**
 * Render a QR code bitmap from [content].
 * Colors mirror iOS: foreground = Plum800, background = Paper.
 *
 * Mirrors iOS IdentityDetailView.generateQRCode(from:).
 */
private fun generateQRBitmap(content: String, sizePx: Int = 512): Bitmap? {
    return runCatching {
        val hints = mapOf(EncodeHintType.MARGIN to 1)
        val bitMatrix = QRCodeWriter().encode(content, BarcodeFormat.QR_CODE, sizePx, sizePx, hints)
        val fgColor = AvalancheColors.Plum800.toArgb()
        val bgColor = AvalancheColors.Paper.toArgb()
        val bitmap = Bitmap.createBitmap(sizePx, sizePx, Bitmap.Config.RGB_565)
        for (x in 0 until sizePx) {
            for (y in 0 until sizePx) {
                bitmap.setPixel(x, y, if (bitMatrix[x, y]) fgColor else bgColor)
            }
        }
        bitmap
    }.getOrNull()
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

@Preview(showBackground = true)
@Composable
private fun IdentityDetailPreview() {
    AvalancheTheme {
        val account = Account(
            id = "did:example:alice",
            displayName = "Alice",
            servers = listOf(
                ServerInfo(
                    id = "https://home.example.com",
                    name = "Home Server",
                    url = android.net.Uri.parse("https://home.example.com"),
                ),
            ),
        )
        IdentityDetailView(
            account = account,
            viewModel = rememberPreviewAppViewModel(accounts = listOf(account)),
        )
    }
}
