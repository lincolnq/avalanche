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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import uniffi.app_core.ContactRowFfi

// Settings -> (Identity) -> Blocked Contacts (docs/12 §7). Lists the DIDs this
// identity has blocked and lets the user unblock them. The block list is
// identity-scoped and synced across the identity's devices.
//
// Mirrors mobile/ios/Actnet/Sources/Views/Settings/BlockedContactsView.swift.
@Composable
fun BlockedContactsView(
    account: Account,
    appViewModel: AppViewModel,
    modifier: Modifier = Modifier,
) {
    var blocked by remember { mutableStateOf<List<ContactRowFfi>>(emptyList()) }
    var loaded by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    suspend fun reload() {
        blocked = appViewModel.listBlocked(accountId = account.id)
        loaded = true
    }

    LaunchedEffect(account.id) {
        reload()
    }

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(AvalancheColors.Paper),
    ) {
        if (!loaded) {
            // Loading state — show placeholder text centered
            Box(
                modifier = Modifier.fillMaxSize(),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = "Loading…",
                    color = AvalancheColors.Muted,
                    fontSize = 15.sp,
                )
            }
        } else if (blocked.isEmpty()) {
            // Empty state
            Box(
                modifier = Modifier.fillMaxSize(),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = "You haven't blocked anyone.",
                    color = AvalancheColors.Muted,
                    fontSize = 15.sp,
                )
            }
        } else {
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
            ) {
                items(blocked, key = { it.did }) { row ->
                    BlockedContactRow(
                        row = row,
                        onUnblock = {
                            scope.launch {
                                appViewModel.unblockContact(did = row.did, accountId = account.id)
                                reload()
                            }
                        },
                    )
                    HorizontalDivider(color = AvalancheColors.Muted.copy(alpha = 0.2f))
                }

                item {
                    Text(
                        text = "Blocked contacts can’t message you, and you can’t message them. Unblocking reverses both.",
                        color = AvalancheColors.Muted,
                        fontSize = 12.sp,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun BlockedContactRow(
    row: ContactRowFfi,
    onUnblock: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val displayName = if (row.displayName.isEmpty()) row.did else row.displayName

    Row(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        ContactAvatar(name = displayName, size = 36.dp)

        Column(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            Text(
                text = displayName,
                fontWeight = FontWeight.Medium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                color = AvalancheColors.Ink,
                fontSize = 15.sp,
            )
            Text(
                text = row.did,
                fontSize = 11.sp,
                color = AvalancheColors.Muted,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }

        OutlinedButton(
            onClick = onUnblock,
            colors = ButtonDefaults.outlinedButtonColors(
                contentColor = AvalancheColors.Brand,
            ),
        ) {
            Text(
                text = "Unblock",
                fontSize = 13.sp,
            )
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun BlockedContactsViewEmptyPreview() {
    AvalancheTheme {
        // Simulate loaded + empty state inline to avoid needing a ViewModel
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(AvalancheColors.Paper),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = "You haven't blocked anyone.",
                color = AvalancheColors.Muted,
                fontSize = 15.sp,
            )
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun BlockedContactRowPreview() {
    AvalancheTheme {
        Box(modifier = Modifier.background(AvalancheColors.Paper)) {
            BlockedContactRow(
                row = ContactRowFfi(
                    did = "did:plc:abc123xyz",
                    displayName = "Alice Example",
                    isCurated = false,
                    lastInteractionAtMs = 0L,
                ),
                onUnblock = {},
            )
        }
    }
}
