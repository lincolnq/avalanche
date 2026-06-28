package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.runtime.setValue
import kotlinx.coroutines.delay
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import uniffi.app_core.MessageRevisionFfi
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/// Edit-history sheet for a message (docs/36-message-editing-deletion.md):
/// prior bodies oldest-first, then the current version. Reached from a
/// message's long-press menu when it has been edited at least once.
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EditHistorySheet(
    current: Message,
    revisions: List<MessageRevisionFfi>,
    onDismiss: () -> Unit = {},
) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Edit History") },
                actions = {
                    TextButton(onClick = onDismiss) {
                        Text("Done", color = LocalAvalancheColors.current.brand)
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
        LazyColumn(
            modifier = Modifier
                .fillMaxSize()
                .background(LocalAvalancheColors.current.paper)
                .padding(innerPadding),
        ) {
            itemsIndexed(revisions) { index, rev ->
                EditHistoryRow(
                    body = rev.body,
                    atMs = rev.replacedAtMs,
                    label = "Edited",
                )
                HorizontalDivider(color = LocalAvalancheColors.current.divider)
            }
            item {
                EditHistoryRow(
                    body = current.body,
                    atMs = current.editedAtMs ?: current.sentAtMs,
                    label = "Current",
                )
            }
        }
    }
}

@Composable
private fun EditHistoryRow(
    body: String,
    atMs: Long,
    label: String,
) {
    // Live-updating relative time, matching iOS Text(..., style: .relative). A
    // ticker re-derives the label once a minute (the finest granularity this
    // formatter shows) so "2 minutes ago" advances without reopening the sheet.
    var relativeTime by remember(atMs) { mutableStateOf(formatRelativeTime(atMs)) }
    LaunchedEffect(atMs) {
        while (true) {
            relativeTime = formatRelativeTime(atMs)
            delay(60_000L)
        }
    }

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelSmall,
            color = LocalAvalancheColors.current.muted,
        )
        Text(
            text = body,
            style = MaterialTheme.typography.bodyMedium,
            color = LocalAvalancheColors.current.ink,
        )
        Text(
            text = relativeTime,
            style = MaterialTheme.typography.labelSmall,
            color = LocalAvalancheColors.current.muted,
        )
    }
}

/// Formats a unix-millis timestamp as a human-readable relative string.
/// Callers (EditHistoryRow) re-invoke this from a one-minute LaunchedEffect
/// ticker so the label live-updates, matching iOS `Text(..., style: .relative)`.
private fun formatRelativeTime(atMs: Long): String {
    val now = System.currentTimeMillis()
    val diffMs = now - atMs
    val diffSecs = diffMs / 1000
    val diffMins = diffSecs / 60
    val diffHours = diffMins / 60
    val diffDays = diffHours / 24
    return when {
        diffSecs < 60 -> "Just now"
        diffMins < 60 -> "$diffMins minutes ago"
        diffHours < 24 -> "$diffHours hours ago"
        diffDays < 7 -> "$diffDays days ago"
        else -> {
            val fmt = SimpleDateFormat("MMM d, yyyy", Locale.getDefault())
            fmt.format(Date(atMs))
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun EditHistorySheetPreview() {
    AvalancheTheme {
        val current = Message(
            id = "msg-1",
            conversationId = "conv-1",
            senderAccountId = "account-1",
            body = "This is the current message body.",
            sentAtMs = System.currentTimeMillis() - 300_000L,
            editedAtMs = System.currentTimeMillis() - 60_000L,
            editCount = 2,
        )
        val revisions = listOf(
            MessageRevisionFfi(
                body = "This was the original message.",
                replacedAtMs = System.currentTimeMillis() - 300_000L,
            ),
            MessageRevisionFfi(
                body = "This was the first edit.",
                replacedAtMs = System.currentTimeMillis() - 120_000L,
            ),
        )
        EditHistorySheet(
            current = current,
            revisions = revisions,
        )
    }
}
