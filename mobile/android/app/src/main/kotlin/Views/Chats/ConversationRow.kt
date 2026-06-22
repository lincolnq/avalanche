package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Person
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale
import java.util.concurrent.TimeUnit

/**
 * A single row in the conversations list. Mirrors
 * mobile/ios/Actnet/Sources/Views/Chats/ConversationRow.swift.
 *
 * [conversation] is the conversation to display.
 * [account] is the account that owns this conversation (for the multi-account indicator).
 * [accounts] is the full list of accounts so the indicator is shown only when count > 1.
 * [unreadCount] is the number of unread messages.
 * [isBotConversation] is true when the DM partner is a bot (renders hexagon frame).
 * [previewText] is the formatted preview line ("You: ...", sender-prefixed, etc.).
 */
@Composable
fun ConversationRow(
    conversation: Conversation,
    account: Account?,
    accounts: List<Account>,
    unreadCount: Int,
    isBotConversation: Boolean,
    previewText: String?,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 2.dp)
            .then(modifier),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Avatar placeholder — hexagon frame for bot DMs, circle for everyone else.
        val avatarShape = if (isBotConversation) Hexagon() else CircleShape

        Box(
            modifier = Modifier
                .size(48.dp)
                .background(color = AvalancheColors.Sand200, shape = avatarShape)
                .clip(avatarShape),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = if (conversation.isGroup) Icons.Filled.Group else Icons.Filled.Person,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.6f),
                modifier = Modifier.size(24.dp),
            )
        }

        Spacer(modifier = Modifier.width(12.dp))

        Column(modifier = Modifier.weight(1f)) {
            // Top line: title + relative timestamp
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = conversation.title,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                    color = AvalancheColors.Ink,
                )
                conversation.lastMessageDate?.let { date ->
                    Text(
                        text = relativeTimeLabel(date),
                        style = MaterialTheme.typography.bodySmall,
                        color = AvalancheColors.Muted,
                        modifier = Modifier.padding(start = 4.dp),
                    )
                }
            }

            // Bottom line: preview text or "Message request" badge, account indicator, unread badge
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (conversation.isRequest) {
                    // First contact from an un-curated DID (docs/12 §1).
                    Text(
                        text = "Message request",
                        style = MaterialTheme.typography.bodySmall,
                        fontWeight = FontWeight.Medium,
                        color = AvalancheColors.Brand,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                } else if (previewText != null) {
                    Text(
                        text = previewText,
                        style = MaterialTheme.typography.bodySmall,
                        color = AvalancheColors.Muted,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                } else {
                    Spacer(modifier = Modifier.weight(1f))
                }

                // Multi-account: show which identity this chat belongs to
                if (account != null && accounts.size > 1) {
                    val initial = account.displayName.firstOrNull()?.uppercaseChar()?.toString() ?: "?"
                    Box(
                        modifier = Modifier
                            .padding(start = 4.dp)
                            .size(18.dp)
                            .background(color = AvalancheColors.Brand, shape = CircleShape),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            text = initial,
                            fontSize = 9.sp,
                            fontWeight = FontWeight.Medium,
                            color = Color.White,
                        )
                    }
                }

                if (unreadCount > 0) {
                    Box(
                        modifier = Modifier
                            .padding(start = 4.dp)
                            .background(
                                color = AvalancheColors.Notification,
                                shape = RoundedCornerShape(percent = 50),
                            )
                            .padding(horizontal = 6.dp, vertical = 2.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            text = "$unreadCount",
                            fontSize = 10.sp,
                            fontWeight = FontWeight.Bold,
                            color = Color.White,
                        )
                    }
                }
            }
        }
    }
}

/**
 * Mirrors SwiftUI's `Text(date, style: .relative)` — returns a short human-
 * readable relative label ("just now", "5m", "2h", "Mon", "Jan 3", etc.).
 */
private fun relativeTimeLabel(date: Date): String {
    val nowMs = System.currentTimeMillis()
    val diffMs = nowMs - date.time
    val diffMin = TimeUnit.MILLISECONDS.toMinutes(diffMs)
    val diffHr = TimeUnit.MILLISECONDS.toHours(diffMs)
    val diffDays = TimeUnit.MILLISECONDS.toDays(diffMs)

    return when {
        diffMin < 1 -> "just now"
        diffMin < 60 -> "${diffMin}m"
        diffHr < 24 -> "${diffHr}h"
        diffDays < 7 -> {
            // Day of week abbreviation
            SimpleDateFormat("EEE", Locale.getDefault()).format(date)
        }
        isCurrentYear(date) -> {
            // e.g. "Jan 3"
            SimpleDateFormat("MMM d", Locale.getDefault()).format(date)
        }
        else -> {
            // e.g. "1/3/23"
            SimpleDateFormat("M/d/yy", Locale.getDefault()).format(date)
        }
    }
}

private fun isCurrentYear(date: Date): Boolean {
    val cal = Calendar.getInstance()
    val nowYear = cal.get(Calendar.YEAR)
    cal.time = date
    return cal.get(Calendar.YEAR) == nowYear
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

@Preview(showBackground = true)
@Composable
private fun ConversationRowPreview() {
    AvalancheTheme {
        Column(modifier = Modifier.padding(16.dp)) {
            // Group conversation with unread messages
            ConversationRow(
                conversation = Conversation(
                    id = "group-abc123",
                    title = "Organizing Team",
                    accountId = "did:key:alice",
                    serverUrl = "https://example.com",
                    isGroup = true,
                    lastMessage = "Meeting tomorrow at 9am",
                    lastMessageDate = Date(System.currentTimeMillis() - TimeUnit.MINUTES.toMillis(5)),
                    lastMessageSenderDid = "did:key:bob",
                ),
                account = Account(id = "did:key:alice", displayName = "Alice"),
                accounts = listOf(Account(id = "did:key:alice", displayName = "Alice")),
                unreadCount = 3,
                isBotConversation = false,
                previewText = "Bob: Meeting tomorrow at 9am",
            )

            // DM with a bot
            ConversationRow(
                conversation = Conversation(
                    id = "dm-did:key:alice-did:key:bot",
                    title = "AdminBot",
                    accountId = "did:key:alice",
                    serverUrl = "https://example.com",
                    recipientDid = "did:key:bot",
                    isGroup = false,
                    lastMessage = "Hello! How can I help?",
                    lastMessageDate = Date(System.currentTimeMillis() - TimeUnit.HOURS.toMillis(2)),
                ),
                account = Account(id = "did:key:alice", displayName = "Alice"),
                accounts = listOf(Account(id = "did:key:alice", displayName = "Alice")),
                unreadCount = 0,
                isBotConversation = true,
                previewText = "Hello! How can I help?",
            )

            // Message request
            ConversationRow(
                conversation = Conversation(
                    id = "dm-did:key:alice-did:key:stranger",
                    title = "Stranger",
                    accountId = "did:key:alice",
                    serverUrl = "https://example.com",
                    recipientDid = "did:key:stranger",
                    isGroup = false,
                    isRequest = true,
                    lastMessage = "Hi there!",
                    lastMessageDate = Date(System.currentTimeMillis() - TimeUnit.DAYS.toMillis(1)),
                ),
                account = Account(id = "did:key:alice", displayName = "Alice"),
                accounts = listOf(
                    Account(id = "did:key:alice", displayName = "Alice"),
                    Account(id = "did:key:alice2", displayName = "Bob"),
                ),
                unreadCount = 1,
                isBotConversation = false,
                previewText = null,
            )
        }
    }
}
