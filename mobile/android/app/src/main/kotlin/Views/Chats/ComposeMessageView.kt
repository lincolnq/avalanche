package net.theavalanche.app

import android.net.Uri
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.AddCircle
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.CenterAlignedTopAppBar
import androidx.compose.material3.InputChip
import androidx.compose.material3.InputChipDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SuggestionChip
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import org.json.JSONObject
import uniffi.app_core.ContactRowFfi

// ---------------------------------------------------------------------------
// ComposeMessageView — new-conversation composer.
//
// A consistent layout: To-field with recipient chips, an always-browsable
// (and typing-filtered) contact list, and two persistent actions: DM
// (enabled at exactly one recipient) and New Group (always available).
//
// Mirrors mobile/ios/Actnet/Sources/Views/Chats/ComposeMessageView.swift.
// See docs/30-mobile-ux.md §Compose.
// ---------------------------------------------------------------------------

/**
 * A confirmed recipient. [displayName] may be empty when the user typed a raw
 * DID we haven't seen before; [label] falls back to a truncated DID.
 * Mirrors the nested Swift Chip struct.
 */
data class ComposeChip(
    val id: String,     // == did
    val did: String,
    val displayName: String,
) {
    /** User-visible text for the chip. Never a raw full DID. */
    val label: String
        get() = if (displayName.isEmpty()) shortenDid(did) else displayName
}

/** Shorten a DID for display. Mirrors RecipientTokenField.swift shortenDid(). */
fun shortenDid(did: String): String {
    return if (did.length > 18) "${did.take(12)}…${did.takeLast(4)}" else did
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun ComposeMessageView(
    viewModel: AppViewModel,
    initialChips: List<ComposeChip> = emptyList(),
    onDismiss: () -> Unit = {},
    onNavigateToConversation: (Conversation) -> Unit = {},
    onNavigateToNameGroup: (members: List<ComposeChip>, accountId: String, servers: List<ServerInfo>) -> Unit = { _, _, _ -> },
) {
    val accounts by viewModel.accounts.collectAsStateWithLifecycle()

    // Local state
    val chips = remember { mutableStateListOf<ComposeChip>().also { it.addAll(initialChips) } }
    var query by remember { mutableStateOf("") }
    var selectedAccountId by remember { mutableStateOf<String?>(null) }
    var allContacts by remember { mutableStateOf<List<ContactRowFfi>>(emptyList()) }
    var errorMessage by remember { mutableStateOf<String?>(null) }
    var showingContactPicker by remember { mutableStateOf(false) }

    val scope = rememberCoroutineScope()

    val activeAccountId: String? = selectedAccountId ?: accounts.firstOrNull()?.id

    val activeAccountServers: List<ServerInfo> = run {
        val id = activeAccountId ?: return@run emptyList()
        accounts.firstOrNull { it.id == id }?.servers ?: emptyList()
    }
    val activeServer: ServerInfo? = activeAccountServers.firstOrNull()

    // Helper: contact name resolution via ViewModel
    fun contactName(c: ContactRowFfi): String {
        val id = activeAccountId ?: return if (c.displayName.isEmpty()) "Unknown" else c.displayName
        return viewModel.resolvedName(did = c.did, accountId = id)
    }

    fun isBot(c: ContactRowFfi): Boolean {
        val id = activeAccountId ?: return false
        return viewModel.isBot(did = c.did, accountId = id)
    }

    // Filtered contact lists mirroring Swift computed vars
    val trimmedQuery = query.trim()
    val queryLooksLikeDid = trimmedQuery.startsWith("did:")

    val peopleResults: List<ContactRowFfi> = run {
        val q = trimmedQuery.lowercase()
        allContacts.filter { c ->
            if (!c.isCurated) return@filter false
            if (chips.any { it.did == c.did }) return@filter false
            if (q.isEmpty()) return@filter true
            c.displayName.lowercase().contains(q) || c.did.lowercase().contains(q)
        }
    }

    val otherResults: List<ContactRowFfi> = run {
        val q = trimmedQuery.lowercase()
        allContacts.filter { c ->
            if (c.isCurated) return@filter false
            if (chips.any { it.did == c.did }) return@filter false
            if (q.isEmpty()) return@filter true
            c.displayName.lowercase().contains(q) || c.did.lowercase().contains(q)
        }
    }

    val newGroupTitle: String = if (chips.isEmpty()) "New Empty Group" else "New Group (${chips.count()})"

    // Load contacts when the active account changes
    LaunchedEffect(activeAccountId) {
        val id = activeAccountId ?: return@LaunchedEffect
        val rows = viewModel.listContacts(accountId = id)
        allContacts = rows
        for (c in rows) {
            viewModel.cacheDisplayName(name = c.displayName, did = c.did)
        }
    }

    // Watch query for contact links
    LaunchedEffect(query) {
        val trimmed = query.trim()
        if (trimmed.isNotEmpty()) {
            handleContactLink(trimmed, chips, activeAccountId)
        }
    }

    fun addChip(did: String, displayName: String) {
        if (chips.none { it.did == did }) {
            chips.add(ComposeChip(id = did, did = did, displayName = displayName))
            query = ""
        }
    }

    fun commitQueryAsChip() {
        if (queryLooksLikeDid) {
            addChip(did = trimmedQuery, displayName = "")
        } else {
            val first = peopleResults.firstOrNull() ?: otherResults.firstOrNull()
            if (first != null) {
                addChip(did = first.did, displayName = contactName(first))
            }
        }
    }

    fun dmTapped() {
        if (chips.size != 1) return
        val accountId = activeAccountId ?: return
        val conv = viewModel.findOrCreateDMConversation(
            recipientDid = chips[0].did,
            accountId = accountId,
        )
        viewModel.setNavigateToConversation(conv)
        onDismiss()
    }

    Scaffold(
        topBar = {
            CenterAlignedTopAppBar(
                title = {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text("New Conversation", style = MaterialTheme.typography.titleMedium)
                        if (activeServer != null) {
                            Text(
                                "at ${activeServer.displayHost}",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                },
                navigationIcon = {
                    IconButton(onClick = onDismiss) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
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
                .padding(innerPadding),
        ) {
            // Account picker (only shown when there are multiple accounts)
            if (accounts.size > 1) {
                AccountPickerRow(
                    accounts = accounts,
                    selectedAccountId = selectedAccountId,
                    onAccountSelected = { selectedAccountId = it },
                )
                HorizontalDivider()
            }

            // Recipient field with chips
            RecipientFieldRow(
                chips = chips,
                query = query,
                onQueryChange = { query = it },
                onChipRemove = { chips.remove(it) },
                onAddTapped = { showingContactPicker = true },
                onSubmit = { commitQueryAsChip() },
            )
            HorizontalDivider()

            // Autocomplete list — always browsable
            LazyColumn(modifier = Modifier.weight(1f)) {
                // Inline DID add button
                if (queryLooksLikeDid && trimmedQuery.isNotEmpty()) {
                    item {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable { addChip(did = trimmedQuery, displayName = "") }
                                .padding(horizontal = 16.dp, vertical = 12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(
                                Icons.Filled.Person,
                                contentDescription = null,
                                tint = LocalAvalancheColors.current.brand,
                                modifier = Modifier.size(20.dp),
                            )
                            Spacer(Modifier.width(10.dp))
                            Text("Add $trimmedQuery", maxLines = 1, fontSize = 14.sp)
                        }
                    }
                }

                // People section
                if (peopleResults.isNotEmpty()) {
                    item {
                        Text(
                            "People",
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp),
                            style = MaterialTheme.typography.labelSmall,
                            color = LocalAvalancheColors.current.muted,
                        )
                    }
                    items(peopleResults, key = { it.did }) { c ->
                        ContactRowItem(
                            name = contactName(c),
                            isBot = isBot(c),
                            onClick = { addChip(did = c.did, displayName = contactName(c)) },
                        )
                    }
                }

                // Other section
                if (otherResults.isNotEmpty()) {
                    item {
                        Text(
                            "Other",
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp),
                            style = MaterialTheme.typography.labelSmall,
                            color = LocalAvalancheColors.current.muted,
                        )
                    }
                    items(otherResults, key = { it.did }) { c ->
                        ContactRowItem(
                            name = contactName(c),
                            isBot = isBot(c),
                            onClick = { addChip(did = c.did, displayName = contactName(c)) },
                        )
                    }
                }

                // Empty state
                if (peopleResults.isEmpty() && otherResults.isEmpty() && !queryLooksLikeDid) {
                    item {
                        Text(
                            "No more contacts to add.",
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                            style = MaterialTheme.typography.bodySmall,
                            color = LocalAvalancheColors.current.muted,
                        )
                    }
                }
            }

            // Action bar
            ActionBar(
                chips = chips,
                newGroupTitle = newGroupTitle,
                errorMessage = errorMessage,
                onDmTapped = { dmTapped() },
                onNewGroupTapped = {
                    val id = activeAccountId ?: return@ActionBar
                    onNavigateToNameGroup(chips.toList(), id, activeAccountServers)
                },
            )
        }
    }

    // Contact picker bottom sheet
    if (showingContactPicker) {
        ContactPickerSheet(
            contacts = allContacts,
            excludedDids = chips.map { it.did }.toSet(),
            nameFor = { contactName(it) },
            isBotFor = { isBot(it) },
            onSelect = { c ->
                addChip(did = c.did, displayName = contactName(c))
                showingContactPicker = false
            },
            onScanLink = { raw ->
                val did = recipientDidFromContactLink(raw) ?: return@ContactPickerSheet false
                if (chips.none { it.did == did }) {
                    chips.add(ComposeChip(id = did, did = did, displayName = ""))
                }
                showingContactPicker = false
                true
            },
            onDismiss = { showingContactPicker = false },
        )
    }
}

// ---------------------------------------------------------------------------
// Account picker row
// ---------------------------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AccountPickerRow(
    accounts: List<Account>,
    selectedAccountId: String?,
    onAccountSelected: (String?) -> Unit,
) {
    val selected = accounts.firstOrNull { it.id == selectedAccountId } ?: accounts.firstOrNull()
    var expanded by remember { mutableStateOf(false) }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("From", color = LocalAvalancheColors.current.muted, modifier = Modifier.padding(end = 8.dp))
        ExposedDropdownMenuBox(
            expanded = expanded,
            onExpandedChange = { expanded = it },
            modifier = Modifier.weight(1f),
        ) {
            OutlinedTextField(
                value = selected?.displayName ?: "",
                onValueChange = {},
                readOnly = true,
                singleLine = true,
                trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = expanded) },
                colors = ExposedDropdownMenuDefaults.outlinedTextFieldColors(),
                modifier = Modifier
                    .fillMaxWidth()
                    .menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable),
            )
            ExposedDropdownMenu(
                expanded = expanded,
                onDismissRequest = { expanded = false },
            ) {
                accounts.forEach { account ->
                    DropdownMenuItem(
                        text = { Text(account.displayName) },
                        onClick = {
                            onAccountSelected(account.id)
                            expanded = false
                        },
                    )
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Recipient field with chips and query input
// ---------------------------------------------------------------------------

@OptIn(ExperimentalLayoutApi::class, ExperimentalMaterial3Api::class)
@Composable
private fun RecipientFieldRow(
    chips: SnapshotStateList<ComposeChip>,
    query: String,
    onQueryChange: (String) -> Unit,
    onChipRemove: (ComposeChip) -> Unit,
    onAddTapped: () -> Unit,
    onSubmit: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.Top,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Text(
                    "To:",
                    modifier = Modifier.align(Alignment.CenterVertically),
                    color = LocalAvalancheColors.current.muted,
                )
                chips.forEach { chip ->
                    InputChip(
                        selected = false,
                        onClick = { onChipRemove(chip) },
                        label = { Text(chip.label, maxLines = 1) },
                        trailingIcon = {
                            Icon(
                                Icons.Filled.Close,
                                contentDescription = "Remove ${chip.label}",
                                modifier = Modifier.size(InputChipDefaults.AvatarSize),
                            )
                        },
                        colors = InputChipDefaults.inputChipColors(
                            containerColor = LocalAvalancheColors.current.brand.copy(alpha = 0.15f),
                            labelColor = LocalAvalancheColors.current.ink,
                        ),
                    )
                }
                OutlinedTextField(
                    value = query,
                    onValueChange = onQueryChange,
                    placeholder = { Text("Type a name", color = LocalAvalancheColors.current.muted) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { onSubmit() }),
                    modifier = Modifier.width(180.dp),
                    // Borderless to match the iOS UITextView-style field that blends
                    // into the recipient flow row (no visible outline).
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedBorderColor = Color.Transparent,
                        unfocusedBorderColor = Color.Transparent,
                        disabledBorderColor = Color.Transparent,
                        errorBorderColor = Color.Transparent,
                    ),
                )
            }
        }
        IconButton(onClick = onAddTapped) {
            Icon(
                Icons.Filled.AddCircle,
                contentDescription = "Add recipient",
                tint = LocalAvalancheColors.current.brand,
                modifier = Modifier.size(28.dp),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Single contact row in the autocomplete list
// ---------------------------------------------------------------------------

@Composable
private fun ContactRowItem(
    name: String,
    isBot: Boolean,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ContactAvatar(name = name, isBot = isBot, size = 32.dp)
        Spacer(Modifier.width(10.dp))
        Text(name, maxLines = 1)
    }
}

// ---------------------------------------------------------------------------
// Action bar (DM + New Group)
// ---------------------------------------------------------------------------

@Composable
private fun ActionBar(
    chips: List<ComposeChip>,
    newGroupTitle: String,
    errorMessage: String?,
    onDmTapped: () -> Unit,
    onNewGroupTapped: () -> Unit,
) {
    Column {
        if (errorMessage != null) {
            Text(
                errorMessage,
                style = MaterialTheme.typography.bodySmall,
                color = LocalAvalancheColors.current.error,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
        }
        HorizontalDivider()
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            // DM button — prominent when exactly one recipient
            if (chips.size == 1) {
                Button(
                    onClick = onDmTapped,
                    modifier = Modifier.weight(1f).height(48.dp),
                    colors = ButtonDefaults.buttonColors(containerColor = LocalAvalancheColors.current.brand),
                ) {
                    Text("DM")
                }
            } else {
                OutlinedButton(
                    onClick = onDmTapped,
                    modifier = Modifier.weight(1f).height(48.dp),
                    enabled = chips.size == 1,
                    colors = ButtonDefaults.outlinedButtonColors(contentColor = LocalAvalancheColors.current.brand),
                ) {
                    Text("DM")
                }
            }

            // New Group button — prominent when 2+ recipients
            if (chips.size >= 2) {
                Button(
                    onClick = onNewGroupTapped,
                    modifier = Modifier.weight(1f).height(48.dp),
                    colors = ButtonDefaults.buttonColors(containerColor = LocalAvalancheColors.current.brand),
                ) {
                    Text(newGroupTitle, maxLines = 1)
                }
            } else {
                OutlinedButton(
                    onClick = onNewGroupTapped,
                    modifier = Modifier.weight(1f).height(48.dp),
                    colors = ButtonDefaults.outlinedButtonColors(contentColor = LocalAvalancheColors.current.brand),
                ) {
                    Text(newGroupTitle, maxLines = 1)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ContactPickerSheet — full-list contact picker from the "+" button.
//
// Mirrors the private Swift struct ContactPickerSheet.
// ---------------------------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ContactPickerSheet(
    contacts: List<ContactRowFfi>,
    excludedDids: Set<String>,
    nameFor: (ContactRowFfi) -> String,
    isBotFor: (ContactRowFfi) -> Boolean,
    onSelect: (ContactRowFfi) -> Unit,
    onScanLink: (String) -> Boolean,
    onDismiss: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var search by remember { mutableStateOf("") }
    var showingScanner by remember { mutableStateOf(false) }
    var scanError by remember { mutableStateOf<String?>(null) }

    val filtered: List<ContactRowFfi> = run {
        val q = search.trim().lowercase()
        contacts.filter { c ->
            if (excludedDids.contains(c.did)) return@filter false
            if (q.isEmpty()) return@filter true
            nameFor(c).lowercase().contains(q) || c.did.lowercase().contains(q)
        }
    }
    val people = filtered.filter { it.isCurated }
    val other = filtered.filter { !it.isCurated }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = LocalAvalancheColors.current.paper,
    ) {
        Column(modifier = Modifier.fillMaxSize()) {
            // Header row
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "Add Recipient",
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.weight(1f),
                )
                IconButton(onClick = onDismiss) {
                    Icon(Icons.Filled.Close, contentDescription = "Close")
                }
            }

            // Search field
            OutlinedTextField(
                value = search,
                onValueChange = { search = it },
                placeholder = { Text("Search contacts") },
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 4.dp),
                singleLine = true,
            )

            LazyColumn(modifier = Modifier.weight(1f)) {
                // Scan QR code button
                item {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable {
                                scanError = null
                                showingScanner = true
                            }
                            .padding(horizontal = 16.dp, vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(
                            Icons.Filled.QrCodeScanner,
                            contentDescription = null,
                            tint = LocalAvalancheColors.current.brand,
                            modifier = Modifier.size(32.dp),
                        )
                        Spacer(Modifier.width(10.dp))
                        Text("Scan QR Code", color = LocalAvalancheColors.current.brand)
                    }
                }

                // People section
                if (people.isNotEmpty()) {
                    item {
                        Text(
                            "People",
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp),
                            style = MaterialTheme.typography.labelSmall,
                            color = LocalAvalancheColors.current.muted,
                        )
                    }
                    items(people, key = { it.did }) { c ->
                        ContactRowItem(
                            name = nameFor(c),
                            isBot = isBotFor(c),
                            onClick = { onSelect(c) },
                        )
                    }
                }

                // Other section
                if (other.isNotEmpty()) {
                    item {
                        Text(
                            "Other",
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp),
                            style = MaterialTheme.typography.labelSmall,
                            color = LocalAvalancheColors.current.muted,
                        )
                    }
                    items(other, key = { it.did }) { c ->
                        ContactRowItem(
                            name = nameFor(c),
                            isBot = isBotFor(c),
                            onClick = { onSelect(c) },
                        )
                    }
                }

                // Empty state
                if (filtered.isEmpty()) {
                    item {
                        Text(
                            "No contacts to add.",
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                            style = MaterialTheme.typography.bodySmall,
                            color = LocalAvalancheColors.current.muted,
                        )
                    }
                }
            }
        }
    }

    // QR scanner sheet
    if (showingScanner) {
        ModalBottomSheet(
            onDismissRequest = { showingScanner = false },
            containerColor = Color.Black,
        ) {
            QRCodeCameraView(
                onScanned = { value ->
                    showingScanner = false
                    if (onScanLink(value)) {
                        // onScanLink returns true → chip added, dismiss picker
                    } else {
                        scanError = "That QR code isn't an Avalanche contact link."
                    }
                },
                modifier = Modifier.height(400.dp),
            )
        }
    }

    // Scan error dialog
    if (scanError != null) {
        AlertDialog(
            onDismissRequest = { scanError = null },
            title = { Text("Couldn't add contact") },
            text = { Text(scanError ?: "") },
            confirmButton = {
                TextButton(onClick = { scanError = null }) { Text("OK") }
            },
        )
    }
}

// ---------------------------------------------------------------------------
// Link parsing helpers — mirrors ComposeMessageView static methods in Swift.
// ---------------------------------------------------------------------------

/**
 * Extract a recipient DID from a contact link, or null if it isn't one.
 * Mirrors ComposeMessageView.recipientDid(fromContactLink:) in Swift.
 *
 * Supported shapes:
 *   https://go.theavalanche.net/conversation/<did>
 *   https://go.theavalanche.net/i/<base64url {"d":…}>
 */
fun recipientDidFromContactLink(raw: String): String? {
    val trimmed = raw.trim()
    val uri = runCatching { Uri.parse(trimmed) }.getOrNull() ?: return null
    if (uri.host != "go.theavalanche.net") return null
    val parts = uri.pathSegments.filter { it.isNotEmpty() }
    if (parts.size < 2) return null
    return when (parts[0]) {
        "conversation" -> {
            val candidate = parts[1]
            if (candidate.startsWith("did:")) candidate else null
        }
        "i", "invite" -> {
            val data = decodeBase64URL(parts[1]) ?: return null
            val payload = runCatching {
                JSONObject(String(data, Charsets.UTF_8))
            }.getOrNull() ?: return null
            val did = payload.optString("d")
            if (did.startsWith("did:")) did else null
        }
        else -> null
    }
}

/**
 * Attempt to parse a contact link from [raw] and add a chip if successful.
 * Returns true if a DID was extracted and added (or was already present).
 */
private fun handleContactLink(
    raw: String,
    chips: SnapshotStateList<ComposeChip>,
    activeAccountId: String?,
): Boolean {
    val did = recipientDidFromContactLink(raw) ?: return false
    if (chips.none { it.did == did }) {
        chips.add(ComposeChip(id = did, did = did, displayName = ""))
    }
    return true
}

// ---------------------------------------------------------------------------
// Previews
// ---------------------------------------------------------------------------

@Preview(showBackground = true)
@Composable
private fun ComposeMessageViewPreview() {
    AvalancheTheme {
        val account = Account(
            id = "did:example:alice",
            displayName = "Alice",
            servers = listOf(
                ServerInfo(
                    id = "https://home.example.com",
                    name = "Home",
                    url = android.net.Uri.parse("https://home.example.com"),
                ),
            ),
        )
        ComposeMessageView(
            viewModel = rememberPreviewAppViewModel(accounts = listOf(account)),
        )
    }
}
