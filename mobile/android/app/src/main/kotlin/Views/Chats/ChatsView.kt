package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Create
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.util.Date
import java.util.Locale
import java.util.concurrent.TimeUnit

// ---------------------------------------------------------------------------
// ChatsView
//
// Mirrors iOS Sources/Views/Chats/ChatsView.swift.
// Navigation callbacks replace NavigationStack — the central NavGraph wires them.
// ---------------------------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatsView(
    viewModel: AppViewModel,
    onOpenConversation: (Conversation) -> Unit = {},
    onOpenAccounts: () -> Unit = {},
    onOpenCompose: () -> Unit = {},
) {
    val conversations by viewModel.conversations.collectAsState()
    val conversationsLoaded by viewModel.conversationsLoaded.collectAsState()
    val navigateToConversation by viewModel.navigateToConversation.collectAsState()

    // Mirror iOS onChange(of: appState.navigateToConversation)
    LaunchedEffect(navigateToConversation) {
        val conv = navigateToConversation ?: return@LaunchedEffect
        viewModel.setNavigateToConversation(null)
        onOpenConversation(conv)
    }

    val sortedConversations = remember(conversations) {
        conversations.sortedByDescending { it.lastMessageDate?.time ?: Long.MIN_VALUE }
    }

    // TODO(opus): hasRecoveryKey — check via Rust core once the FFI method exists.
    val hasRecoveryKey = false

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("") },
                navigationIcon = {
                    IconButton(onClick = onOpenAccounts) {
                        Icon(
                            imageVector = Icons.Filled.Settings,
                            contentDescription = "Accounts",
                        )
                    }
                },
                actions = {
                    IconButton(onClick = onOpenCompose) {
                        Icon(
                            imageVector = Icons.Filled.Create,
                            contentDescription = "Compose",
                        )
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = LocalAvalancheColors.current.paper,
                    titleContentColor = LocalAvalancheColors.current.ink,
                    navigationIconContentColor = LocalAvalancheColors.current.ink,
                    actionIconContentColor = LocalAvalancheColors.current.ink,
                ),
            )
        },
        containerColor = LocalAvalancheColors.current.paper,
    ) { innerPadding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding),
        ) {
            if (sortedConversations.isEmpty() && !conversationsLoaded) {
                // Initial load still in flight — show a spinner instead of the
                // empty state, so "No conversations yet" doesn't flash on launch.
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center,
                ) {
                    CircularProgressIndicator(color = LocalAvalancheColors.current.muted)
                }
            } else if (sortedConversations.isEmpty()) {
                // Mirrors iOS ContentUnavailableView
                Column(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(32.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    Icon(
                        imageVector = Icons.Filled.Create,
                        contentDescription = null,
                        tint = LocalAvalancheColors.current.muted,
                        modifier = Modifier.size(48.dp),
                    )
                    androidx.compose.foundation.layout.Spacer(Modifier.size(16.dp))
                    Text(
                        text = "No conversations yet",
                        color = LocalAvalancheColors.current.ink,
                        fontSize = 18.sp,
                        fontWeight = FontWeight.SemiBold,
                    )
                    androidx.compose.foundation.layout.Spacer(Modifier.size(8.dp))
                    Text(
                        text = "Messages from all your servers will appear here.",
                        color = LocalAvalancheColors.current.muted,
                        fontSize = 14.sp,
                    )
                }
            } else {
                Column(modifier = Modifier.fillMaxSize()) {
                    // Recovery key banner (mirrors iOS .overlay alignment: .top)
                    if (!hasRecoveryKey) {
                        RecoveryKeyBanner()
                        HorizontalDivider(color = LocalAvalancheColors.current.divider)
                    }

                    LazyColumn(
                        modifier = Modifier
                            .fillMaxSize()
                            .background(LocalAvalancheColors.current.paper),
                    ) {
                        items(
                            items = sortedConversations,
                            key = { it.id },
                        ) { conversation ->
                            val accounts by viewModel.accounts.collectAsState()
                            val account = accounts.firstOrNull { it.id == conversation.accountId }
                            val unreadCount = viewModel.unreadCount(conversation)
                            val recipientDid = conversation.recipientDid
                            val isBot = !conversation.isGroup &&
                                recipientDid != null &&
                                viewModel.isBot(recipientDid, conversation.accountId)
                            // Preview text — mirrors iOS ConversationRow.previewText.
                            // Computed live (not `remember`d) so the snapshot reads of
                            // `displayNameCache` inside resolvedName/groupEventText are
                            // tracked: when a name resolves async, this row recomposes and
                            // "Unknown" becomes the real name.
                            val previewText: String? = run {
                                if (conversation.lastMessageKind > 0) {
                                    val msg = Message(
                                        id = conversation.id,
                                        conversationId = conversation.id,
                                        senderAccountId = conversation.lastMessageSenderDid ?: "",
                                        body = conversation.lastMessage ?: "",
                                        sentAtMs = 0L,
                                        readAtMs = null,
                                        deliveryStatus = DeliveryStatus.SENT,
                                        kind = conversation.lastMessageKind,
                                        metadata = conversation.lastMessageMetadata,
                                    )
                                    viewModel.groupEventText(msg, conversation.accountId)
                                } else {
                                    val body = conversation.lastMessage
                                    if (body == null) {
                                        null
                                    } else {
                                        val sender = conversation.lastMessageSenderDid
                                        when {
                                            sender != null && sender == conversation.accountId -> "You: $body"
                                            conversation.isGroup && sender != null && sender.isNotEmpty() ->
                                                "${viewModel.resolvedName(sender, conversation.accountId)}: $body"
                                            else -> body
                                        }
                                    }
                                }
                            }
                            ConversationRow(
                                conversation = conversation,
                                account = account,
                                accounts = accounts,
                                unreadCount = unreadCount,
                                isBotConversation = isBot,
                                previewText = previewText,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clickable { onOpenConversation(conversation) }
                                    .padding(horizontal = 16.dp, vertical = 10.dp),
                            )
                            HorizontalDivider(
                                color = LocalAvalancheColors.current.divider.copy(alpha = 0.5f),
                                modifier = Modifier.padding(start = 76.dp),
                            )
                        }
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
private fun ChatsViewEmptyPreview() {
    AvalancheTheme {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(LocalAvalancheColors.current.paper),
            contentAlignment = Alignment.Center,
        ) {
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text(
                    text = "No conversations yet",
                    color = LocalAvalancheColors.current.ink,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = 18.sp,
                )
                Text(
                    text = "Messages from all your servers will appear here.",
                    color = LocalAvalancheColors.current.muted,
                    fontSize = 14.sp,
                )
            }
        }
    }
}

