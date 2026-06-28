package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch

// ---------------------------------------------------------------------------
// NameGroupView
//
// Signal-style "Name Group" screen, pushed from the composer's New Group
// button. Collects the group name, the disappearing-messages timer, and (for
// an empty group) the hosting server, then creates the group and opens its
// thread.
//
// Mirrors mobile/ios/Actnet/Sources/Views/Chats/NameGroupView.swift.
//
// Navigation: the screen receives lambda callbacks for navigation actions
// (matching the project's lambda-nav pattern). onCreated is called with the
// newly-created Conversation once the group is created.
// ---------------------------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NameGroupView(
    members: List<RecipientChip>,
    accountId: String,
    /// Servers the active identity belongs to; the first is its home server.
    servers: List<ServerInfo>,
    /// Called once the group is created.
    onCreated: (Conversation) -> Unit = {},
    onDismiss: () -> Unit = {},
    viewModel: AppViewModel,
) {
    // -----------------------------------------------------------------------
    // Local state — mirrors @State vars in the Swift source
    // -----------------------------------------------------------------------

    var name by remember { mutableStateOf("") }
    var expirySeconds by remember { mutableStateOf(0u) }
    var selectedServerId by remember { mutableStateOf("") }
    var creating by remember { mutableStateOf(false) }
    var errorMessage by remember { mutableStateOf<String?>(null) }

    // Seed selectedServerId from the home server on first composition.
    LaunchedEffect(servers) {
        if (selectedServerId.isEmpty()) {
            selectedServerId = servers.firstOrNull()?.id ?: ""
        }
    }

    // -----------------------------------------------------------------------
    // Derived values — mirrors Swift computed vars
    // -----------------------------------------------------------------------

    val homeServer: ServerInfo? = servers.firstOrNull()

    // Only an empty group may choose a server; with recipients the founder's
    // home server is used. A picker is also pointless with a single server.
    val canChooseServer: Boolean = members.isEmpty() && servers.size > 1

    val resolvedServer: ServerInfo? =
        servers.firstOrNull { it.id == selectedServerId } ?: homeServer

    val trimmedName: String = name.trim()

    // -----------------------------------------------------------------------
    // Coroutine scope for the Create action
    // -----------------------------------------------------------------------

    val scope = rememberCoroutineScope()

    fun create() {
        if (creating || trimmedName.isEmpty()) return
        creating = true
        errorMessage = null
        val title = trimmedName
        val serverUrl = resolvedServer?.id ?: ""
        val recipientDids = members.map { it.did }
        val expiry = expirySeconds
        scope.launch {
            try {
                val conv = viewModel.createGroupAndOpen(
                    accountId = accountId,
                    serverUrl = serverUrl,
                    title = title,
                    recipientDids = recipientDids,
                    expirySeconds = expiry,
                    firstMessage = null,
                )
                onCreated(conv)
            } catch (e: Exception) {
                errorMessage = e.localizedMessage ?: e.message ?: "Unknown error"
            } finally {
                creating = false
            }
        }
    }

    // -----------------------------------------------------------------------
    // UI
    // -----------------------------------------------------------------------

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Name Group") },
                navigationIcon = {
                    IconButton(onClick = onDismiss) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    Button(
                        onClick = { create() },
                        enabled = !creating && trimmedName.isNotEmpty(),
                        colors = ButtonDefaults.buttonColors(
                            containerColor = LocalAvalancheColors.current.brand,
                        ),
                        modifier = Modifier.padding(end = 8.dp),
                    ) {
                        Text("Create")
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = LocalAvalancheColors.current.paper,
                ),
            )
        },
        containerColor = LocalAvalancheColors.current.paper,
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            // ----------------------------------------------------------------
            // Group name section
            // ----------------------------------------------------------------

            FormSection {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("Group Name (required)") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
            }

            // ----------------------------------------------------------------
            // Server section
            // ----------------------------------------------------------------

            FormSection(label = "Server") {
                if (canChooseServer) {
                    ServerPicker(
                        servers = servers,
                        selectedServerId = selectedServerId,
                        homeServerId = homeServer?.id,
                        onSelectionChange = { selectedServerId = it },
                    )
                    Text(
                        text = "Creating on another server isn't supported yet.",
                        fontSize = 12.sp,
                        color = LocalAvalancheColors.current.muted,
                        modifier = Modifier.padding(top = 4.dp),
                    )
                } else if (resolvedServer != null) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(resolvedServer.displayHost, color = LocalAvalancheColors.current.ink)
                        Text(
                            text = "Home",
                            fontSize = 12.sp,
                            color = LocalAvalancheColors.current.muted,
                        )
                    }
                }
            }

            // ----------------------------------------------------------------
            // Disappearing messages section
            // ----------------------------------------------------------------

            FormSection {
                DisappearingMessagesPickerView(
                    seconds = expirySeconds,
                    onSecondsChange = { expirySeconds = it },
                )
            }

            // ----------------------------------------------------------------
            // Members section
            // ----------------------------------------------------------------

            FormSection(label = "Members (${members.size})") {
                if (members.isEmpty()) {
                    Text(
                        text = "No members yet — you can add people after creating the group.",
                        fontSize = 13.sp,
                        color = LocalAvalancheColors.current.muted,
                    )
                } else {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        members.forEachIndexed { index, member ->
                            if (index > 0) {
                                HorizontalDivider(color = LocalAvalancheColors.current.muted.copy(alpha = 0.2f))
                            }
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                modifier = Modifier.fillMaxWidth(),
                            ) {
                                val isBot = viewModel.isBot(member.did, accountId = accountId)
                                ContactAvatar(
                                    name = member.label,
                                    isBot = isBot,
                                    size = 32.dp,
                                )
                                Spacer(Modifier.width(10.dp))
                                Text(
                                    text = member.label,
                                    color = LocalAvalancheColors.current.ink,
                                    maxLines = 1,
                                    modifier = Modifier.weight(1f),
                                )
                            }
                        }
                    }
                }
            }

            // ----------------------------------------------------------------
            // Error section
            // ----------------------------------------------------------------

            if (errorMessage != null) {
                FormSection {
                    Text(
                        text = errorMessage!!,
                        fontSize = 12.sp,
                        color = LocalAvalancheColors.current.error,
                    )
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RecipientChip
//
// Mirrors the iOS ComposeMessageView.Chip nested struct. The recipient-chip
// shape is shared across the Chats package via this single declaration;
// ComposeMessageView uses its own recipient model, so there is no duplication.
// ---------------------------------------------------------------------------

data class RecipientChip(
    val id: String,   // == did
    val did: String,
    val displayName: String,
) {
    // User-visible text. Never a raw full DID.
    val label: String
        get() = if (displayName.isEmpty()) shortenDid(did) else displayName
}

// shortenDid lives in ComposeMessageView.kt (same package) — single definition shared across Chats.

// ---------------------------------------------------------------------------
// FormSection — lightweight container that mirrors SwiftUI's Form/Section
// visual grouping (card-style surface with optional label).
// ---------------------------------------------------------------------------

@Composable
private fun FormSection(
    label: String? = null,
    content: @Composable () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(
                color = LocalAvalancheColors.current.incomingBubble.copy(alpha = 0.5f),
                shape = androidx.compose.foundation.shape.RoundedCornerShape(8.dp),
            )
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (label != null) {
            Text(
                text = label.uppercase(),
                fontSize = 11.sp,
                fontWeight = FontWeight.Medium,
                color = LocalAvalancheColors.current.muted,
                letterSpacing = 0.5.sp,
            )
        }
        content()
    }
}

// ---------------------------------------------------------------------------
// ServerPicker — inline exposed dropdown for the server selector.
// Non-home servers are shown but disabled (matching the iOS .disabled() gate).
// ---------------------------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ServerPicker(
    servers: List<ServerInfo>,
    selectedServerId: String,
    homeServerId: String?,
    onSelectionChange: (String) -> Unit,
) {
    var expanded by remember { mutableStateOf(false) }
    val selectedServer = servers.firstOrNull { it.id == selectedServerId }
    val displayValue = selectedServer?.displayHost ?: selectedServerId

    ExposedDropdownMenuBox(
        expanded = expanded,
        onExpandedChange = { expanded = it },
        modifier = Modifier.fillMaxWidth(),
    ) {
        OutlinedTextField(
            value = displayValue,
            onValueChange = {},
            readOnly = true,
            label = { Text("Server") },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = expanded) },
            colors = ExposedDropdownMenuDefaults.outlinedTextFieldColors(),
            modifier = Modifier
                .fillMaxWidth()
                .menuAnchor(androidx.compose.material3.ExposedDropdownMenuAnchorType.PrimaryNotEditable),
        )

        ExposedDropdownMenu(
            expanded = expanded,
            onDismissRequest = { expanded = false },
        ) {
            servers.forEach { server ->
                val isHome = server.id == homeServerId
                DropdownMenuItem(
                    text = {
                        Row(
                            horizontalArrangement = Arrangement.SpaceBetween,
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Text(
                                text = server.displayHost,
                                color = if (isHome) LocalAvalancheColors.current.ink else LocalAvalancheColors.current.muted,
                            )
                            if (isHome) {
                                Text(
                                    text = "Home",
                                    fontSize = 12.sp,
                                    color = LocalAvalancheColors.current.muted,
                                )
                            }
                        }
                    },
                    onClick = {
                        if (isHome) {
                            // Non-home creation isn't wired in the core yet; only
                            // allow selecting the home server (matching iOS .disabled()).
                            onSelectionChange(server.id)
                        }
                        expanded = false
                    },
                    enabled = isHome,
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Previews
// ---------------------------------------------------------------------------

private val previewServers = listOf(
    ServerInfo(
        id = "https://home.example.com",
        name = "Home",
        url = android.net.Uri.parse("https://home.example.com"),
    ),
    ServerInfo(
        id = "https://work.example.com",
        name = "Work",
        url = android.net.Uri.parse("https://work.example.com"),
    ),
)

@Preview(showBackground = true, name = "Group with members")
@Composable
private fun NameGroupWithMembersPreview() {
    AvalancheTheme {
        NameGroupView(
            members = listOf(
                RecipientChip(id = "did:example:bob", did = "did:example:bob", displayName = "Bob"),
                RecipientChip(id = "did:example:carol", did = "did:example:carol", displayName = "Carol"),
            ),
            accountId = "did:example:alice",
            servers = previewServers,
            viewModel = rememberPreviewAppViewModel(),
        )
    }
}

@Preview(showBackground = true, name = "Empty group, multi-server")
@Composable
private fun NameGroupEmptyPreview() {
    AvalancheTheme {
        NameGroupView(
            members = emptyList(),
            accountId = "did:example:alice",
            servers = previewServers,
            viewModel = rememberPreviewAppViewModel(),
        )
    }
}
