package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.app_core.MessageRevisionFfi
import java.util.UUID

// ---------------------------------------------------------------------------
// ConversationView — the full-screen message thread.
//
// Mirrors mobile/ios/Actnet/Sources/Views/Chats/ConversationView.swift.
// Navigation is callback-based (lambda params); a central NavGraph wires them.
// ---------------------------------------------------------------------------

/**
 * Full conversation screen for a DM or group chat.
 *
 * @param conversation The conversation whose thread is being shown.
 * @param viewModel    The shared AppViewModel.
 * @param onNavigateToGroupDetail Called when the group title/avatar is tapped to open
 *   GroupDetailView. Only called for group conversations.
 * @param onBack  Called to pop this screen off the back-stack.
 */
@Composable
fun ConversationView(
    conversation: Conversation,
    viewModel: AppViewModel,
    onNavigateToGroupDetail: (groupId: String, accountId: String) -> Unit = { _, _ -> },
    onBack: () -> Unit = {},
) {
    val messagesByConversation by viewModel.messagesByConversation.collectAsState()
    val conversations by viewModel.conversations.collectAsState()
    val reactionsByConversation by viewModel.reactionsByConversation.collectAsState()

    // The live row from conversations so request/blocked state stays reactive
    // after an Accept / Block / Report action; falls back to the passed-in
    // value (e.g. previews) when not in the list.
    val liveConv = conversations.firstOrNull { it.id == conversation.id } ?: conversation

    val messages = messagesByConversation[conversation.id] ?: emptyList()

    var messageText by remember { mutableStateOf("") }
    var errorMessage by remember { mutableStateOf<String?>(null) }

    // Non-null while editing an existing message (docs/36): the composer turns
    // into an edit bar prefilled with its body.
    var editingMessage by remember { mutableStateOf<Message?>(null) }

    // The message whose edit-history sheet is showing, plus its loaded revisions.
    var historyMessage by remember { mutableStateOf<Message?>(null) }
    var historyRevisions by remember { mutableStateOf<List<MessageRevisionFfi>>(emptyList()) }

    // Whether we're still a member of this group (docs/53 §Leave). Non-members
    // keep the readable transcript but lose the composer. Always true for DMs.
    var isGroupMember by remember { mutableStateOf(true) }

    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()

    // Human edit/delete-for-everyone window (docs/36): 24h from send.
    val editWindowMs: Long = 24 * 60 * 60 * 1000L

    fun canEdit(message: Message): Boolean =
        message.senderAccountId == conversation.accountId
            && !message.isDeleted
            && (System.currentTimeMillis() - message.sentAtMs) <= editWindowMs

    // Whether an incoming message's sender is a bot, for the octagon-ish
    // bubble shape (docs/54-bot-presentation.md). Own messages are never bots.
    fun isBotSender(message: Message): Boolean =
        message.senderAccountId != conversation.accountId
            && viewModel.isBot(message.senderAccountId, accountId = conversation.accountId)

    fun startEditing(message: Message) {
        editingMessage = message
        messageText = message.body
    }

    fun cancelEdit() {
        editingMessage = null
        messageText = ""
    }

    fun applyEdit() {
        val msg = editingMessage ?: return
        viewModel.editMessage(message = msg, newBody = messageText, conversation = conversation)
        editingMessage = null
        messageText = ""
    }

    fun showHistory(message: Message) {
        scope.launch {
            historyRevisions = viewModel.loadMessageRevisions(
                message = message,
                conversation = conversation,
            )
            historyMessage = message
        }
    }

    fun sendMessage() {
        val text = messageText.trim()
        if (text.isEmpty()) return
        messageText = ""
        errorMessage = null

        // Optimistically add to UI.
        val messageId = UUID.randomUUID().toString()
        val nowMs = System.currentTimeMillis()
        val optimistic = Message(
            id = messageId,
            conversationId = conversation.id,
            senderAccountId = conversation.accountId,
            body = text,
            sentAtMs = nowMs,
            readAtMs = nowMs,  // outgoing = immediately read
            deliveryStatus = DeliveryStatus.SENDING,
        )
        // TODO(opus): AppViewModel does not expose a direct addOptimisticMessage method.
        // The iOS counterpart mutates appState.messagesByConversation and appState.conversations
        // directly (they are @Published vars). On Android these are private MutableStateFlows.
        // A follow-up pass should add AppViewModel.addOptimisticMessage(message, conversation)
        // that does the same two updates atomically. For now the message will appear when the
        // send completes and the store is reloaded; the UX is slightly less snappy than iOS.

        scope.launch {
            // Scroll to bottom after optimistic insert.
            if (messages.isNotEmpty()) {
                listState.animateScrollToItem(messages.size)
            }

            try {
                if (conversation.isGroup) {
                    viewModel.sendGroupMessage(
                        conversation = conversation,
                        text = text,
                        messageId = messageId,
                        sentAtMs = nowMs,
                    )
                } else {
                    val recipientDid = conversation.recipientDid
                    if (recipientDid == null) {
                        errorMessage = "Cannot send: no recipient"
                        return@launch
                    }
                    viewModel.sendMessage(
                        conversationId = conversation.id,
                        text = text,
                        recipientDid = recipientDid,
                        senderAccountId = conversation.accountId,
                        messageId = messageId,
                        sentAtMs = nowMs,
                    )
                }
            } catch (e: Exception) {
                errorMessage = "Failed to send: ${e.message}"
            }
        }
    }

    // onAppear: load messages, reactions, mark read, set current conv.
    LaunchedEffect(conversation.id) {
        viewModel.setCurrentConversationId(conversation.id)
        viewModel.loadMessagesFromStore(
            conversationId = conversation.id,
            accountId = conversation.accountId,
        )
        viewModel.loadReactions(
            conversationId = conversation.id,
            accountId = conversation.accountId,
        )
        viewModel.markAllMessagesRead(
            conversationId = conversation.id,
            accountId = conversation.accountId,
        )

        // Re-fetch the contact's encrypted profile and update cached display name.
        conversation.recipientDid?.let { did ->
            viewModel.refreshContactProfile(did = did, accountId = conversation.accountId)
        }
        conversation.groupId?.let { groupId ->
            viewModel.refreshGroupTitle(groupId = groupId, accountId = conversation.accountId)
            isGroupMember = viewModel.isGroupMember(
                groupId = groupId,
                accountId = conversation.accountId,
            )
        }

        // After messages load, scroll to first unread (or bottom if all read).
        delay(100)
        val msgs = viewModel.messagesByConversation.value[conversation.id] ?: emptyList()
        val firstUnread = msgs.indexOfFirst {
            it.readAtMs == null && it.senderAccountId != conversation.accountId
        }
        if (firstUnread >= 0) {
            listState.scrollToItem(firstUnread)
        } else if (msgs.isNotEmpty()) {
            listState.scrollToItem(msgs.size - 1)
        }
    }

    // onDisappear: clear current conversation id.
    DisposableEffect(conversation.id) {
        onDispose {
            if (viewModel.currentConversationId.value == conversation.id) {
                viewModel.setCurrentConversationId(null)
            }
        }
    }

    // Auto-scroll when new messages arrive.
    val messageCount = messages.size
    LaunchedEffect(messageCount) {
        if (messageCount > 0) {
            listState.animateScrollToItem(messageCount - 1)
            viewModel.markAllMessagesRead(
                conversationId = conversation.id,
                accountId = conversation.accountId,
            )
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(AvalancheColors.Paper),
    ) {
        // --- Message list ---
        LazyColumn(
            state = listState,
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
                .padding(horizontal = 8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item { Spacer(Modifier.size(8.dp)) }
            items(messages, key = { it.sentAtMs }) { message ->
                if (message.isSystemEvent) {
                    // Group membership/metadata event (docs/03 §3.6) —
                    // a centered grey line, not a chat bubble.
                    GroupSystemEventRow(
                        text = viewModel.groupEventText(
                            message = message,
                            accountId = conversation.accountId,
                        )
                    )
                } else {
                    MessageBubble(
                        message = message,
                        isMe = message.senderAccountId == conversation.accountId,
                        isBot = isBotSender(message),
                        reactions = viewModel.reactions(message),
                        myDid = conversation.accountId,
                        actionsEnabled = true,
                        canEdit = canEdit(message),
                        onToggleReaction = { emoji ->
                            viewModel.toggleReaction(
                                message = message,
                                emoji = emoji,
                                conversation = conversation,
                            )
                        },
                        onEdit = { startEditing(message) },
                        onDelete = { forEveryone ->
                            viewModel.deleteMessage(
                                message = message,
                                forEveryone = forEveryone,
                                conversation = conversation,
                            )
                        },
                        onShowHistory = { showHistory(message) },
                    )
                }
            }
            item { Spacer(Modifier.size(8.dp)) }
        }

        // Error message
        errorMessage?.let { err ->
            Text(
                text = err,
                style = MaterialTheme.typography.labelSmall,
                color = AvalancheColors.Error,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
        }

        HorizontalDivider(color = AvalancheColors.Sand300)

        // Bottom bar: a blocked DM shows an unblock prompt, an un-accepted
        // request shows the Accept/Delete/Report gate (docs/12 §1), and an
        // accepted DM or group shows the normal composer.
        when {
            liveConv.isBlocked && liveConv.recipientDid != null ->
                BlockedBar(
                    did = liveConv.recipientDid!!,
                    accountId = conversation.accountId,
                    viewModel = viewModel,
                )
            liveConv.isRequest && liveConv.recipientDid != null ->
                MessageRequestGate(
                    did = liveConv.recipientDid!!,
                    title = conversation.title,
                    accountId = conversation.accountId,
                    viewModel = viewModel,
                    onDismiss = onBack,
                )
            conversation.isGroup && !isGroupMember ->
                LeftGroupBar()
            else ->
                Composer(
                    messageText = messageText,
                    onMessageTextChange = { messageText = it },
                    editingMessage = editingMessage,
                    onSend = { sendMessage() },
                    onApplyEdit = { applyEdit() },
                    onCancelEdit = { cancelEdit() },
                )
        }
    }

    // Edit history sheet — shown as a full-screen dialog.
    historyMessage?.let { msg ->
        Dialog(
            onDismissRequest = { historyMessage = null },
            properties = DialogProperties(usePlatformDefaultWidth = false),
        ) {
            EditHistorySheet(
                current = msg,
                revisions = historyRevisions,
                onDismiss = { historyMessage = null },
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Sub-composables (private helpers extracted from the main body)
// ---------------------------------------------------------------------------

/**
 * Shown in place of the composer once you've left the group (docs/53 §Leave).
 * The transcript stays readable above; you just can't post.
 */
@Composable
private fun LeftGroupBar() {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 12.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = "You left this group",
            style = MaterialTheme.typography.labelSmall,
            color = AvalancheColors.Muted,
        )
    }
}

/**
 * The normal text composer (with the inline edit bar when editing).
 */
@Composable
private fun Composer(
    messageText: String,
    onMessageTextChange: (String) -> Unit,
    editingMessage: Message?,
    onSend: () -> Unit,
    onApplyEdit: () -> Unit,
    onCancelEdit: () -> Unit,
) {
    Column {
        // Inline edit bar — shown above the text field when editing.
        if (editingMessage != null) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp)
                    .padding(top = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(
                    imageVector = Icons.Filled.Edit,
                    contentDescription = "Editing",
                    tint = AvalancheColors.Brand,
                    modifier = Modifier.size(16.dp),
                )
                Spacer(Modifier.width(6.dp))
                Text(
                    text = "Editing message",
                    style = MaterialTheme.typography.labelSmall,
                    color = AvalancheColors.Muted,
                    modifier = Modifier.weight(1f),
                )
                IconButton(onClick = onCancelEdit) {
                    Icon(
                        imageVector = Icons.Filled.Close,
                        contentDescription = "Cancel edit",
                        tint = AvalancheColors.Muted,
                    )
                }
            }
        }

        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalAlignment = Alignment.Bottom,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            OutlinedTextField(
                value = messageText,
                onValueChange = onMessageTextChange,
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = 40.dp, max = 120.dp),
                placeholder = {
                    Text(
                        text = if (editingMessage == null) "Message" else "Edit message",
                        color = AvalancheColors.Muted,
                    )
                },
                keyboardOptions = KeyboardOptions(
                    capitalization = KeyboardCapitalization.Sentences,
                ),
                maxLines = 5,
            )

            IconButton(
                onClick = { if (editingMessage != null) onApplyEdit() else onSend() },
                enabled = messageText.trim().isNotEmpty(),
            ) {
                Icon(
                    imageVector = if (editingMessage != null) Icons.Filled.Check else Icons.Filled.KeyboardArrowUp,
                    contentDescription = if (editingMessage != null) "Apply edit" else "Send",
                    tint = if (messageText.trim().isNotEmpty()) AvalancheColors.Brand else AvalancheColors.Muted,
                )
            }
        }
    }
}

/**
 * The message-request gate (docs/12 §1): a stranger's first contact is
 * read-only until the user Accepts, Deletes, or Reports & Blocks.
 */
@Composable
private fun MessageRequestGate(
    did: String,
    title: String,
    accountId: String,
    viewModel: AppViewModel,
    onDismiss: () -> Unit,
) {
    val scope = rememberCoroutineScope()

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(
            text = "Let $title message you and share your name with them?",
            style = MaterialTheme.typography.labelSmall,
            color = AvalancheColors.Muted,
            modifier = Modifier.fillMaxWidth(),
        )
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            // Block (Report & Block)
            OutlinedButton(
                onClick = { viewModel.reportAndBlock(did = did, accountId = accountId) },
                modifier = Modifier.weight(1f),
                colors = ButtonDefaults.outlinedButtonColors(
                    contentColor = AvalancheColors.Error,
                ),
            ) {
                Text("Block")
            }

            // Delete the request
            OutlinedButton(
                onClick = {
                    viewModel.deleteRequest(did = did, accountId = accountId)
                    onDismiss()
                },
                modifier = Modifier.weight(1f),
                colors = ButtonDefaults.outlinedButtonColors(
                    contentColor = AvalancheColors.Error,
                ),
            ) {
                Text("Delete")
            }

            // Accept
            Button(
                onClick = { viewModel.acceptRequest(did = did, accountId = accountId) },
                modifier = Modifier.weight(1f),
            ) {
                Text("Accept")
            }
        }
    }
}

/**
 * Shown in place of the composer for a blocked DM (docs/12 §2).
 */
@Composable
private fun BlockedBar(
    did: String,
    accountId: String,
    viewModel: AppViewModel,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            text = "You blocked this contact.",
            style = MaterialTheme.typography.labelSmall,
            color = AvalancheColors.Muted,
            modifier = Modifier.weight(1f),
        )
        OutlinedButton(
            onClick = { viewModel.unblockContact(did = did, accountId = accountId) },
        ) {
            Text("Unblock")
        }
    }
}

// ---------------------------------------------------------------------------
// GroupSystemEventRow — public so ConversationView can use it from the same file.
//
// A centered grey system line in the conversation timeline for a group
// membership/metadata event (docs/03 §3.6) — "Alice added Bob", "Bob left", etc.
//
// Also mirrors the iOS GroupSystemEventRow struct (at the bottom of
// ConversationView.swift), kept in the same file for parity.
// ---------------------------------------------------------------------------

@Composable
fun GroupSystemEventRow(
    text: String,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.labelSmall,
            color = AvalancheColors.Muted,
            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
        )
    }
}

// ---------------------------------------------------------------------------
// Previews
// ---------------------------------------------------------------------------

@Preview(showBackground = true, name = "GroupSystemEventRow")
@Composable
private fun GroupSystemEventRowPreview() {
    AvalancheTheme {
        GroupSystemEventRow(text = "Alice added Bob")
    }
}

@Preview(showBackground = true, name = "LeftGroupBar")
@Composable
private fun LeftGroupBarPreview() {
    AvalancheTheme {
        LeftGroupBar()
    }
}

@Preview(showBackground = true, name = "BlockedBar")
@Composable
private fun BlockedBarPreview() {
    AvalancheTheme {
        // TODO(opus): BlockedBar preview requires a real AppViewModel; shown as stub.
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 10.dp),
        ) {
            Text(
                text = "You blocked this contact. [Unblock]",
                style = MaterialTheme.typography.labelSmall,
                color = AvalancheColors.Muted,
            )
        }
    }
}

@Preview(showBackground = true, name = "Composer")
@Composable
private fun ComposerPreview() {
    AvalancheTheme {
        Composer(
            messageText = "",
            onMessageTextChange = {},
            editingMessage = null,
            onSend = {},
            onApplyEdit = {},
            onCancelEdit = {},
        )
    }
}

@Preview(showBackground = true, name = "ComposerEditing")
@Composable
private fun ComposerEditingPreview() {
    AvalancheTheme {
        val msg = Message(
            id = "m1",
            conversationId = "c1",
            senderAccountId = "did:plc:me",
            body = "Original text",
            sentAtMs = System.currentTimeMillis(),
        )
        Composer(
            messageText = "Edited text",
            onMessageTextChange = {},
            editingMessage = msg,
            onSend = {},
            onApplyEdit = {},
            onCancelEdit = {},
        )
    }
}
