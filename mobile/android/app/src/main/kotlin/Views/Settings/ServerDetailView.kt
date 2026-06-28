package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Home
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

/**
 * Detail screen for a single server entry in an account's server list.
 * Mirrors iOS Sources/Views/Settings/ServerDetailView.swift.
 *
 * Navigation: callback-lambda pattern (no NavController dependency here).
 * [onDismiss] is called after a successful leave or when the back button is pressed.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ServerDetailView(
    account: Account,
    server: ServerInfo,
    appViewModel: AppViewModel,
    onDismiss: () -> Unit = {},
) {
    val coroutineScope = rememberCoroutineScope()

    // Whether this server is the home server (first registered server).
    val isHome = account.servers.firstOrNull()?.id == server.id
    val homeServerName = account.servers.firstOrNull()?.name ?: "your home server"

    var showLeaveConfirmation by remember { mutableStateOf(false) }
    var isLeaving by remember { mutableStateOf(false) }
    var leaveError by remember { mutableStateOf<String?>(null) }

    fun leaveServer() {
        isLeaving = true
        coroutineScope.launch {
            runCatching {
                appViewModel.leaveServer(account = account, server = server)
                onDismiss()
            }.onFailure { error ->
                leaveError = error.localizedMessage ?: error.message ?: "Unknown error"
            }
            isLeaving = false
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(server.name) },
                navigationIcon = {
                    IconButton(onClick = onDismiss) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { innerPadding ->
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(LocalAvalancheColors.current.paper)
            .padding(innerPadding)
            .verticalScroll(rememberScrollState())
            .padding(bottom = 32.dp),
    ) {
        // Server name + URL header
        Column(
            modifier = Modifier
                .padding(horizontal = 16.dp)
                .padding(top = 16.dp),
        ) {
            Text(
                text = server.name,
                style = MaterialTheme.typography.titleLarge,
                color = LocalAvalancheColors.current.ink,
            )
            Spacer(Modifier.height(4.dp))
            SelectionContainer {
                Text(
                    text = server.url.toString(),
                    style = MaterialTheme.typography.bodySmall,
                    color = LocalAvalancheColors.current.muted,
                )
            }
        }

        Spacer(Modifier.height(16.dp))

        // Home server notice card — only shown when this IS the home server.
        if (isHome) {
            Column(
                modifier = Modifier
                    .padding(horizontal = 16.dp)
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(8.dp))
                    .background(LocalAvalancheColors.current.card)
                    .padding(12.dp),
            ) {
                // House icon + label row
                androidx.compose.foundation.layout.Row(
                    verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
                ) {
                    Icon(
                        imageVector = Icons.Filled.Home,
                        contentDescription = null,
                        tint = LocalAvalancheColors.current.brand,
                    )
                    Spacer(Modifier.padding(start = 6.dp))
                    Text(
                        text = "Home server for ${account.displayName}",
                        style = MaterialTheme.typography.bodyMedium,
                        color = LocalAvalancheColors.current.brand,
                    )
                }
                Spacer(Modifier.height(4.dp))
                Text(
                    text = "To change your home server or delete this identity, open the identity detail screen.",
                    style = MaterialTheme.typography.bodySmall,
                    color = LocalAvalancheColors.current.muted,
                )
            }
        }

        Spacer(Modifier.height(16.dp))

        // Leave button — only shown when this is NOT the home server.
        if (!isHome) {
            OutlinedButton(
                onClick = { showLeaveConfirmation = true },
                enabled = !isLeaving,
                colors = ButtonDefaults.outlinedButtonColors(
                    contentColor = MaterialTheme.colorScheme.error,
                ),
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp),
            ) {
                if (isLeaving) {
                    CircularProgressIndicator(
                        modifier = Modifier
                            .height(20.dp)
                            .padding(horizontal = 4.dp),
                        color = MaterialTheme.colorScheme.error,
                        strokeWidth = 2.dp,
                    )
                } else {
                    Text("Leave this server")
                }
            }
        }
    }
    }

    // Leave confirmation dialog — mirrors iOS .confirmationDialog(...)
    if (showLeaveConfirmation) {
        AlertDialog(
            onDismissRequest = { showLeaveConfirmation = false },
            title = { Text("Leave ${server.name}?") },
            text = {
                Text(
                    "You'll be removed from any groups and Projects on ${server.name}. " +
                        "People you share other servers with will still be able to reach you there. " +
                        "New contacts will reach you at $homeServerName."
                )
            },
            confirmButton = {
                Button(
                    onClick = {
                        showLeaveConfirmation = false
                        leaveServer()
                    },
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.error,
                    ),
                ) {
                    Text("Leave")
                }
            },
            dismissButton = {
                TextButton(onClick = { showLeaveConfirmation = false }) {
                    Text("Cancel")
                }
            },
        )
    }

    // Error alert — mirrors iOS .alert("Couldn't leave server", ...)
    if (leaveError != null) {
        AlertDialog(
            onDismissRequest = { leaveError = null },
            title = { Text("Couldn't leave server") },
            text = { Text(leaveError ?: "") },
            confirmButton = {
                TextButton(onClick = { leaveError = null }) {
                    Text("OK")
                }
            },
        )
    }
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

@Preview(showBackground = true)
@Composable
private fun ServerDetailViewPreview() {
    AvalancheTheme {
        // Preview with a non-home server so the Leave button is visible.
        val homeServer = ServerInfo(
            id = "https://home.example.com",
            name = "Home Server",
            url = android.net.Uri.parse("https://home.example.com"),
        )
        val otherServer = ServerInfo(
            id = "https://other.example.com",
            name = "Other Server",
            url = android.net.Uri.parse("https://other.example.com"),
        )
        val account = Account(
            id = "did:example:123",
            displayName = "Alice",
            servers = listOf(homeServer, otherServer),
        )
        ServerDetailView(
            account = account,
            server = otherServer,
            appViewModel = rememberPreviewAppViewModel(accounts = listOf(account)),
            onDismiss = {},
        )
    }
}
