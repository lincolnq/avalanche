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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.app_core.GroupMemberFfi
import uniffi.app_core.GroupPendingFfi
import uniffi.app_core.GroupSummaryFfi

// Group detail screen (docs/03-groups.md): member list, admin role changes,
// disappearing-message timer, and leave-group. Admin-only controls are shown
// only when the current user is an admin; the server enforces the same.
//
// Mirrors mobile/ios/Actnet/Sources/Views/Chats/GroupDetailView.swift.
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun GroupDetailView(
    groupId: String,
    accountId: String,
    appViewModel: AppViewModel,
    onDismiss: () -> Unit = {},
) {
    val scope = rememberCoroutineScope()

    var summary by remember { mutableStateOf<GroupSummaryFfi?>(null) }
    var loading by remember { mutableStateOf(true) }
    var errorMessage by remember { mutableStateOf<String?>(null) }
    // Separate var so we can guard against spurious initial onChange fires.
    var expirySeconds by remember { mutableStateOf(0u) }
    var showRename by remember { mutableStateOf(false) }
    var renameText by remember { mutableStateOf("") }

    // Computed from summary
    val amAdmin = summary?.members?.firstOrNull { it.did == accountId }?.role?.toInt() == 1
    val amMember = summary?.members?.any { it.did == accountId } ?: false

    // -----------------------------------------------------------------------
    // Helpers (mirrors private funcs in iOS)
    // -----------------------------------------------------------------------

    fun orderedMembers(members: List<GroupMemberFfi>): List<GroupMemberFfi> =
        members.sortedByDescending { it.did == accountId }

    fun memberName(member: GroupMemberFfi): String =
        if (member.did == accountId) "You"
        else appViewModel.resolvedName(did = member.did, accountId = accountId)

    fun isBot(member: GroupMemberFfi): Boolean =
        member.did != accountId && appViewModel.isBot(did = member.did, accountId = accountId)

    suspend fun load() {
        loading = true
        val core = appViewModel.core(accountId) ?: run {
            errorMessage = "No core for account"
            loading = false
            return
        }
        val gid = groupId
        try {
            val s = withContext(Dispatchers.IO) { core.fetchGroupState(groupId = gid) }
            summary = s
            expirySeconds = s.expirySeconds
        } catch (e: Exception) {
            // Server fetch is membership-gated; falls back to cached state so the
            // screen renders read-only after leaving (docs/53 §Leave).
            val cached = runCatching {
                withContext(Dispatchers.IO) { core.cachedGroupState(groupId = gid) }
            }.getOrNull()
            if (cached != null) {
                summary = cached
                expirySeconds = cached.expirySeconds
            } else {
                errorMessage = e.localizedMessage ?: e.message ?: "Unknown error"
            }
        }
        loading = false
    }

    fun setExpiry(seconds: UInt) {
        val core = appViewModel.core(accountId) ?: return
        val gid = groupId
        scope.launch {
            try {
                withContext(Dispatchers.IO) { core.setGroupExpiry(groupId = gid, expirySeconds = seconds) }
                load()
            } catch (e: Exception) {
                errorMessage = e.localizedMessage ?: e.message ?: "Unknown error"
                load()  // revert picker to server's value on failure
            }
        }
    }

    fun changeRole(member: GroupMemberFfi, toAdmin: Boolean) {
        val core = appViewModel.core(accountId) ?: return
        val gid = groupId
        val emi = member.encryptedMemberId
        val newRole: Short = if (toAdmin) 1 else 0
        scope.launch {
            try {
                withContext(Dispatchers.IO) {
                    core.changeMemberRole(groupId = gid, encryptedMemberId = emi, newRole = newRole)
                }
                load()
            } catch (e: Exception) {
                errorMessage = e.localizedMessage ?: e.message ?: "Unknown error"
            }
        }
    }

    fun renameGroup(name: String) {
        val trimmed = name.trim()
        val core = appViewModel.core(accountId) ?: return
        if (trimmed.isEmpty() || trimmed == (summary?.title ?: "")) return
        val gid = groupId
        scope.launch {
            try {
                withContext(Dispatchers.IO) { core.setGroupTitle(groupId = gid, newTitle = trimmed) }
                load()
            } catch (e: Exception) {
                errorMessage = e.localizedMessage ?: e.message ?: "Unknown error"
            }
        }
    }

    fun leaveGroup() {
        val core = appViewModel.core(accountId) ?: return
        val gid = groupId
        scope.launch {
            try {
                withContext(Dispatchers.IO) { core.leaveGroup(groupId = gid) }
                appViewModel.reloadGroupTimelineIfLoaded(groupId = gid, accountId = accountId)
                onDismiss()
            } catch (e: Exception) {
                errorMessage = e.localizedMessage ?: e.message ?: "Unknown error"
            }
        }
    }

    // Load on first composition
    LaunchedEffect(groupId, accountId) { load() }

    // -----------------------------------------------------------------------
    // Rename dialog — mirrors SwiftUI .alert("Rename group", ...)
    // -----------------------------------------------------------------------
    if (showRename) {
        AlertDialog(
            onDismissRequest = { showRename = false },
            title = { Text("Rename group") },
            text = {
                OutlinedTextField(
                    value = renameText,
                    onValueChange = { renameText = it },
                    label = { Text("Group name") },
                    singleLine = true,
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        renameGroup(renameText)
                        showRename = false
                    },
                    enabled = renameText.trim().isNotEmpty(),
                ) {
                    Text("Save")
                }
            },
            dismissButton = {
                TextButton(onClick = { showRename = false }) { Text("Cancel") }
            },
        )
    }

    // -----------------------------------------------------------------------
    // Error dialog for action failures
    // -----------------------------------------------------------------------
    val errMsg = errorMessage
    if (errMsg != null) {
        AlertDialog(
            onDismissRequest = { errorMessage = null },
            title = { Text("Error") },
            text = { Text(errMsg) },
            confirmButton = {
                TextButton(onClick = { errorMessage = null }) { Text("OK") }
            },
        )
    }

    // -----------------------------------------------------------------------
    // Screen scaffold
    // -----------------------------------------------------------------------
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Group info") },
                navigationIcon = {
                    IconButton(onClick = onDismiss) {
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
        val s = summary
        when {
            s != null -> {
                LazyColumn(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(LocalAvalancheColors.current.paper)
                        .padding(innerPadding),
                ) {
                    // -------------------------------------------------------
                    // Section 1: title / description / revision
                    // -------------------------------------------------------
                    item {
                        SectionContainer {
                            Column(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(horizontal = 16.dp, vertical = 12.dp),
                                verticalArrangement = Arrangement.spacedBy(4.dp),
                            ) {
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Text(
                                        text = if (s.title.isEmpty()) "Group" else s.title,
                                        style = MaterialTheme.typography.titleMedium,
                                        modifier = Modifier.weight(1f),
                                    )
                                    if (amAdmin) {
                                        TextButton(onClick = {
                                            renameText = s.title
                                            showRename = true
                                        }) {
                                            Text(
                                                text = "Rename",
                                                fontSize = 14.sp,
                                                color = LocalAvalancheColors.current.brand,
                                            )
                                        }
                                    }
                                }
                                if (s.description.isNotEmpty()) {
                                    Text(
                                        text = s.description,
                                        style = MaterialTheme.typography.bodyMedium,
                                        color = LocalAvalancheColors.current.muted,
                                    )
                                }
                                Text(
                                    text = "Revision ${s.revision}",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = LocalAvalancheColors.current.muted,
                                )
                            }
                        }
                        SectionDivider()
                    }

                    // -------------------------------------------------------
                    // Section 2: disappearing messages
                    // -------------------------------------------------------
                    item {
                        SectionHeader("Disappearing messages")
                        SectionContainer {
                            if (amAdmin) {
                                val loadedExpiry = s.expirySeconds
                                DisappearingMessagesPickerView(
                                    seconds = expirySeconds,
                                    onSecondsChange = { newValue ->
                                        // Guard: ignore the initial seeding; only react on a real change.
                                        if (newValue != loadedExpiry) {
                                            expirySeconds = newValue
                                            setExpiry(newValue)
                                        } else {
                                            expirySeconds = newValue
                                        }
                                    },
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .padding(horizontal = 16.dp, vertical = 8.dp),
                                )
                            } else {
                                Row(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .padding(horizontal = 16.dp, vertical = 12.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Text(
                                        text = "Timer",
                                        style = MaterialTheme.typography.bodyMedium,
                                        modifier = Modifier.weight(1f),
                                    )
                                    Text(
                                        text = DisappearingMessagesPicker.label(s.expirySeconds),
                                        style = MaterialTheme.typography.bodyMedium,
                                        color = LocalAvalancheColors.current.muted,
                                    )
                                }
                            }
                        }
                        SectionDivider()
                    }

                    // -------------------------------------------------------
                    // Section 3: members
                    // -------------------------------------------------------
                    item {
                        SectionHeader("Members (${s.members.size})")
                    }
                    items(orderedMembers(s.members), key = { it.encryptedMemberId }) { member ->
                        SectionContainer {
                            MemberRow(
                                name = memberName(member),
                                isBot = isBot(member),
                                role = member.role,
                                isSelf = member.did == accountId,
                                amAdmin = amAdmin,
                                onMakeAdmin = { changeRole(member, toAdmin = true) },
                                onRemoveAdmin = { changeRole(member, toAdmin = false) },
                            )
                        }
                        HorizontalDivider(
                            modifier = Modifier.padding(start = 58.dp),
                            color = LocalAvalancheColors.current.divider,
                        )
                    }

                    // -------------------------------------------------------
                    // Section 4: pending invites (if any)
                    // -------------------------------------------------------
                    if (s.pendingInvites.isNotEmpty()) {
                        item {
                            SectionDivider()
                            SectionHeader("Pending invites (${s.pendingInvites.size})")
                        }
                        items(s.pendingInvites, key = { it.encryptedMemberId }) { pending ->
                            SectionContainer {
                                Text(
                                    text = pending.encryptedMemberId,
                                    style = MaterialTheme.typography.labelSmall,
                                    color = LocalAvalancheColors.current.muted,
                                    maxLines = 1,
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .padding(horizontal = 16.dp, vertical = 12.dp),
                                )
                            }
                        }
                    }

                    // -------------------------------------------------------
                    // Section 5: leave / "you left" notice
                    // -------------------------------------------------------
                    item {
                        SectionDivider()
                        SectionContainer {
                            if (amMember) {
                                TextButton(
                                    onClick = { leaveGroup() },
                                    colors = ButtonDefaults.textButtonColors(
                                        contentColor = LocalAvalancheColors.current.error,
                                    ),
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .padding(horizontal = 8.dp, vertical = 4.dp),
                                ) {
                                    Text("Leave group")
                                }
                            } else {
                                Text(
                                    text = "You left this group.",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = LocalAvalancheColors.current.muted,
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .padding(horizontal = 16.dp, vertical = 12.dp),
                                )
                            }
                        }
                    }
                }
            }

            loading -> {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(innerPadding),
                    contentAlignment = Alignment.Center,
                ) {
                    CircularProgressIndicator(color = LocalAvalancheColors.current.brand)
                }
            }

            else -> {
                // Error / unavailable state
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(innerPadding)
                        .padding(24.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Column(
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Text(
                            text = "Couldn't load group",
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.SemiBold,
                        )
                        if (errMsg != null) {
                            Text(
                                text = errMsg,
                                style = MaterialTheme.typography.bodyMedium,
                                color = LocalAvalancheColors.current.muted,
                            )
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Private composable helpers
// ---------------------------------------------------------------------------

@Composable
private fun SectionHeader(title: String) {
    Text(
        text = title.uppercase(),
        style = MaterialTheme.typography.labelSmall,
        color = LocalAvalancheColors.current.muted,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
    )
}

@Composable
private fun SectionContainer(content: @Composable () -> Unit) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .background(LocalAvalancheColors.current.card),
    ) {
        content()
    }
}

@Composable
private fun SectionDivider() {
    HorizontalDivider(
        color = LocalAvalancheColors.current.divider,
        thickness = 0.5.dp,
        modifier = Modifier.padding(vertical = 8.dp),
    )
}

@Composable
private fun MemberRow(
    name: String,
    isBot: Boolean,
    role: Short,
    isSelf: Boolean,
    amAdmin: Boolean,
    onMakeAdmin: () -> Unit,
    onRemoveAdmin: () -> Unit,
) {
    var menuExpanded by remember { mutableStateOf(false) }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        ContactAvatar(name = name, isBot = isBot, size = 32.dp)
        Text(
            text = name,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.weight(1f),
            maxLines = 1,
        )
        // Admin badge
        if (role == 1.toShort()) {
            Text(
                text = "Admin",
                style = MaterialTheme.typography.labelSmall,
                color = LocalAvalancheColors.current.brand,
                modifier = Modifier
                    .clip(RoundedCornerShape(50))
                    .background(LocalAvalancheColors.current.brand.copy(alpha = 0.15f))
                    .padding(horizontal = 6.dp, vertical = 2.dp),
            )
        }
        // Per-member admin menu — visible only to admins and not for self
        if (amAdmin && !isSelf) {
            Box {
                IconButton(onClick = { menuExpanded = true }) {
                    Icon(
                        imageVector = Icons.Default.MoreVert,
                        contentDescription = "Member options",
                        tint = LocalAvalancheColors.current.muted,
                    )
                }
                DropdownMenu(
                    expanded = menuExpanded,
                    onDismissRequest = { menuExpanded = false },
                ) {
                    if (role == 1.toShort()) {
                        DropdownMenuItem(
                            text = { Text("Remove admin") },
                            onClick = {
                                menuExpanded = false
                                onRemoveAdmin()
                            },
                        )
                    } else {
                        DropdownMenuItem(
                            text = { Text("Make admin") },
                            onClick = {
                                menuExpanded = false
                                onMakeAdmin()
                            },
                        )
                    }
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
private fun GroupDetailViewPreview() {
    AvalancheTheme {
        // The preview ViewModel's mock core returns no group summary, so this
        // renders the loading state — useful for reviewing the scaffold/top bar.
        GroupDetailView(
            groupId = "preview-group",
            accountId = "did:example:alice",
            appViewModel = rememberPreviewAppViewModel(),
        )
    }
}
