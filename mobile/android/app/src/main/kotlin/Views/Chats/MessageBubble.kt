package net.theavalanche.app

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccessTime
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Error
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.app_core.ReactionFfi
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

private val quickEmoji = listOf("👍", "❤️", "😂", "😮", "😢", "🙏")

/**
 * A single chat message bubble. Mirrors MessageBubble.swift.
 *
 * @param message        The message to render.
 * @param isMe           Whether this message was sent by the local user.
 * @param isBot          Whether the sender is a bot (renders cut-corner shape).
 * @param reactions      List of reactions on this message.
 * @param myDid          The local user's DID, used to highlight own reactions.
 * @param actionsEnabled Whether long-press context menu is enabled (DMs only).
 * @param canEdit        Whether the Edit option is shown in the context menu.
 * @param onToggleReaction Callback when a reaction emoji is toggled.
 * @param onEdit         Callback when Edit is tapped.
 * @param onDelete       Callback when Delete is tapped; Boolean = deleteForEveryone.
 * @param onShowHistory  Callback when Edit History is tapped.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun MessageBubble(
    message: Message,
    isMe: Boolean,
    isBot: Boolean = false,
    reactions: List<ReactionFfi> = emptyList(),
    myDid: String = "",
    actionsEnabled: Boolean = false,
    canEdit: Boolean = false,
    onToggleReaction: (String) -> Unit = {},
    onEdit: () -> Unit = {},
    onDelete: (Boolean) -> Unit = {},
    onShowHistory: () -> Unit = {},
) {
    var menuExpanded by remember { mutableStateOf(false) }

    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = if (isMe) Arrangement.End else Arrangement.Start,
    ) {
        if (isMe) Spacer(modifier = Modifier.width(60.dp))

        Column(
            horizontalAlignment = if (isMe) Alignment.End else Alignment.Start,
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            // Bubble
            Box {
                BubbleContent(
                    message = message,
                    isMe = isMe,
                    isBot = isBot,
                    actionsEnabled = actionsEnabled,
                    onLongClick = { if (actionsEnabled) menuExpanded = true },
                )

                if (actionsEnabled) {
                    BubbleContextMenu(
                        expanded = menuExpanded,
                        message = message,
                        isMe = isMe,
                        canEdit = canEdit,
                        onDismiss = { menuExpanded = false },
                        onToggleReaction = { emoji ->
                            menuExpanded = false
                            onToggleReaction(emoji)
                        },
                        onEdit = {
                            menuExpanded = false
                            onEdit()
                        },
                        onDelete = { forEveryone ->
                            menuExpanded = false
                            onDelete(forEveryone)
                        },
                        onShowHistory = {
                            menuExpanded = false
                            onShowHistory()
                        },
                    )
                }
            }

            // Reaction clusters
            val clusters = reactionClusters(reactions, myDid)
            if (clusters.isNotEmpty()) {
                ReactionClusterRow(
                    clusters = clusters,
                    onToggleReaction = onToggleReaction,
                )
            }

            // Timestamp + edited + delivery indicator row
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Text(
                    text = formatTime(message.sentAt),
                    style = MaterialTheme.typography.labelSmall,
                    color = AvalancheColors.Muted,
                    fontSize = 10.sp,
                )
                if (message.isEdited && !message.isDeleted) {
                    Text(
                        text = "· Edited",
                        style = MaterialTheme.typography.labelSmall,
                        color = AvalancheColors.Muted,
                        fontSize = 10.sp,
                    )
                }
                if (isMe) {
                    DeliveryIndicator(status = message.deliveryStatus)
                }
            }
        }

        if (!isMe) Spacer(modifier = Modifier.width(60.dp))
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun BubbleContent(
    message: Message,
    isMe: Boolean,
    isBot: Boolean,
    actionsEnabled: Boolean,
    onLongClick: () -> Unit,
) {
    val bubbleShape: Shape = if (isBot) CutCornerRectangle(cut = 12f) else RoundedCornerShape(16.dp)

    if (message.isDeleted) {
        // Dashed border tombstone
        Box(
            modifier = Modifier
                .padding(horizontal = 12.dp, vertical = 8.dp),
        ) {
            // Compose doesn't support dashed borders natively via Modifier.border;
            // draw a dashed rounded rect using Canvas.
            // TODO(opus): replace with Canvas drawRoundRect + PathEffect.dashPathEffect if needed
            Box(
                modifier = Modifier
                    .border(
                        width = 1.dp,
                        color = AvalancheColors.Muted.copy(alpha = 0.4f),
                        shape = RoundedCornerShape(16.dp),
                    )
                    .padding(horizontal = 12.dp, vertical = 8.dp),
            ) {
                Text(
                    text = "This message was deleted",
                    style = MaterialTheme.typography.bodyMedium.copy(fontStyle = FontStyle.Italic),
                    color = AvalancheColors.Muted,
                )
            }
        }
    } else {
        val bgColor = if (isMe) AvalancheColors.OutgoingBubble else AvalancheColors.IncomingBubble
        val fgColor = if (isMe) AvalancheColors.Sand100 else AvalancheColors.Ink

        Text(
            text = message.body,
            style = MaterialTheme.typography.bodyMedium,
            color = fgColor,
            modifier = Modifier
                .clip(bubbleShape)
                .background(bgColor)
                .combinedClickable(
                    onClick = {},
                    onLongClick = onLongClick,
                )
                .padding(horizontal = 12.dp, vertical = 8.dp),
        )
    }
}

@Composable
private fun BubbleContextMenu(
    expanded: Boolean,
    message: Message,
    isMe: Boolean,
    canEdit: Boolean,
    onDismiss: () -> Unit,
    onToggleReaction: (String) -> Unit,
    onEdit: () -> Unit,
    onDelete: (Boolean) -> Unit,
    onShowHistory: () -> Unit,
) {
    val context = LocalContext.current

    DropdownMenu(
        expanded = expanded,
        onDismissRequest = onDismiss,
    ) {
        // Quick-reaction palette row
        Row(
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            quickEmoji.forEach { emoji ->
                androidx.compose.material3.TextButton(
                    onClick = { onToggleReaction(emoji) },
                    modifier = Modifier.size(40.dp),
                ) {
                    Text(text = emoji, fontSize = 18.sp)
                }
            }
        }

        HorizontalDivider()

        if (canEdit) {
            DropdownMenuItem(
                text = { Text("Edit") },
                onClick = onEdit,
            )
        }

        if (message.editCount > 0) {
            DropdownMenuItem(
                text = { Text("Edit History") },
                onClick = onShowHistory,
            )
        }

        DropdownMenuItem(
            text = { Text("Copy") },
            onClick = {
                val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                clipboard?.setPrimaryClip(ClipData.newPlainText("message", message.body))
                onDismiss()
            },
        )

        HorizontalDivider()

        if (isMe) {
            DropdownMenuItem(
                text = { Text("Delete for Everyone", color = AvalancheColors.Error) },
                onClick = { onDelete(true) },
            )
        }

        DropdownMenuItem(
            text = { Text("Delete for Me", color = AvalancheColors.Error) },
            onClick = { onDelete(false) },
        )
    }
}

// ---------------------------------------------------------------------------
// Reaction clusters
// ---------------------------------------------------------------------------

private data class ReactionCluster(val emoji: String, val count: Int, val mine: Boolean)

private fun reactionClusters(reactions: List<ReactionFfi>, myDid: String): List<ReactionCluster> {
    val order = mutableListOf<String>()
    val counts = mutableMapOf<String, Int>()
    val mine = mutableMapOf<String, Boolean>()
    for (r in reactions) {
        if (!counts.containsKey(r.emoji)) order.add(r.emoji)
        counts[r.emoji] = (counts[r.emoji] ?: 0) + 1
        if (r.reactorDid == myDid) mine[r.emoji] = true
    }
    return order.map { ReactionCluster(it, counts[it] ?: 0, mine[it] ?: false) }
}

@Composable
private fun ReactionClusterRow(
    clusters: List<ReactionCluster>,
    onToggleReaction: (String) -> Unit,
) {
    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        clusters.forEach { cluster ->
            val bgColor = if (cluster.mine) AvalancheColors.Brand.copy(alpha = 0.18f) else AvalancheColors.IncomingBubble
            val borderColor = if (cluster.mine) AvalancheColors.Brand.copy(alpha = 0.5f) else Color.Transparent

            Box(
                modifier = Modifier
                    .clip(CircleShape)
                    .background(bgColor)
                    .border(width = 1.dp, color = borderColor, shape = CircleShape)
                    .combinedClickable(onClick = { onToggleReaction(cluster.emoji) })
                    .padding(horizontal = 6.dp, vertical = 3.dp),
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    Text(text = cluster.emoji, fontSize = 12.sp)
                    if (cluster.count > 1) {
                        Text(
                            text = "${cluster.count}",
                            fontSize = 10.sp,
                            color = if (cluster.mine) AvalancheColors.Brand else AvalancheColors.Muted,
                        )
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Delivery indicator
// ---------------------------------------------------------------------------

@Composable
private fun DeliveryIndicator(status: DeliveryStatus) {
    when (status) {
        DeliveryStatus.SENDING -> {
            Icon(
                imageVector = Icons.Filled.AccessTime,
                contentDescription = "Sending",
                tint = AvalancheColors.Muted,
                modifier = Modifier.size(12.dp),
            )
        }
        DeliveryStatus.SENT -> {
            Icon(
                imageVector = Icons.Filled.Check,
                contentDescription = "Sent",
                tint = AvalancheColors.Muted,
                modifier = Modifier.size(12.dp),
            )
        }
        DeliveryStatus.DELIVERED -> {
            // Double-check (delivered) — two overlapping checkmarks
            Box {
                Icon(
                    imageVector = Icons.Filled.Check,
                    contentDescription = "Delivered",
                    tint = AvalancheColors.Muted,
                    modifier = Modifier.size(12.dp),
                )
                Icon(
                    imageVector = Icons.Filled.Check,
                    contentDescription = null,
                    tint = AvalancheColors.Muted,
                    modifier = Modifier
                        .size(12.dp)
                        .offset(x = 4.dp),
                )
            }
        }
        DeliveryStatus.READ -> {
            // Double-check (read) — brand color
            Box {
                Icon(
                    imageVector = Icons.Filled.Check,
                    contentDescription = "Read",
                    tint = AvalancheColors.Brand,
                    modifier = Modifier.size(12.dp),
                )
                Icon(
                    imageVector = Icons.Filled.Check,
                    contentDescription = null,
                    tint = AvalancheColors.Brand,
                    modifier = Modifier
                        .size(12.dp)
                        .offset(x = 4.dp),
                )
            }
        }
        DeliveryStatus.FAILED -> {
            Icon(
                imageVector = Icons.Filled.Error,
                contentDescription = "Failed",
                tint = AvalancheColors.Error,
                modifier = Modifier.size(12.dp),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

private val timeFormatter = SimpleDateFormat("h:mm a", Locale.getDefault())

private fun formatTime(date: Date): String = timeFormatter.format(date)

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

@Preview(showBackground = true)
@Composable
private fun MessageBubblePreview() {
    val incoming = Message(
        id = "1",
        conversationId = "conv1",
        senderAccountId = "other",
        body = "Hey! How are you?",
        sentAtMs = System.currentTimeMillis() - 60_000,
        deliveryStatus = DeliveryStatus.READ,
    )
    val outgoing = Message(
        id = "2",
        conversationId = "conv1",
        senderAccountId = "me",
        body = "Doing great, thanks!",
        sentAtMs = System.currentTimeMillis(),
        deliveryStatus = DeliveryStatus.DELIVERED,
        editedAtMs = System.currentTimeMillis() - 1000,
    )
    val deleted = Message(
        id = "3",
        conversationId = "conv1",
        senderAccountId = "other",
        body = "",
        sentAtMs = System.currentTimeMillis() - 120_000,
        isDeleted = true,
    )

    AvalancheTheme {
        Column(
            modifier = Modifier
                .background(AvalancheColors.Paper)
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            MessageBubble(message = incoming, isMe = false)
            MessageBubble(message = outgoing, isMe = true, actionsEnabled = true, canEdit = true)
            MessageBubble(message = deleted, isMe = false)
            MessageBubble(
                message = incoming.copy(id = "4", body = "Bot message here", senderAccountId = "bot"),
                isMe = false,
                isBot = true,
            )
        }
    }
}
