package net.theavalanche.app

import android.content.ClipboardManager
import android.content.Context
import android.graphics.BitmapFactory
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.material.icons.filled.AddCircle
import androidx.compose.material.icons.filled.ContentPaste
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
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
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.app_core.AttachmentFfi
import uniffi.app_core.LinkPreviewFfi
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
/**
 * A message plus the context the actions overlay needs to reproduce and animate
 * its bubble (docs/33). Mirrors the iOS ConversationView.ActionTarget struct.
 */
private data class ChatActionTarget(
    val message: Message,
    /** Source bubble content bounds in window coords (animation start point). */
    val bounds: Rect,
    val senderName: String?,
    val isLastInRun: Boolean,
    val isMe: Boolean,
    val isBot: Boolean,
)

@OptIn(ExperimentalMaterial3Api::class)
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

    // Reactions on a message, read from the *observed* state above so Compose
    // recomposes when they change. (viewModel.reactions() reads the flow's
    // .value directly, which isn't a snapshot read — so an optimistic
    // toggleReaction that touches only this flow would otherwise never
    // recompose, and your own reaction wouldn't appear until some other state
    // change forced a redraw.)
    fun reactionsFor(m: Message) =
        (reactionsByConversation[m.conversationId] ?: emptyList()).filter {
            it.targetAuthor == m.senderAccountId && it.targetSentAtMs == m.sentAtMs
        }

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

    // The image attachment tapped to open the fullscreen viewer (docs/35); the
    // viewer pages through every image in the conversation starting here.
    var imageViewerStartId by remember { mutableStateOf<String?>(null) }

    // Whether we're still a member of this group (docs/53 §Leave). Non-members
    // keep the readable transcript but lose the composer. Always true for DMs.
    var isGroupMember by remember { mutableStateOf(true) }

    // Staged composer attachments (docs/35): an image waiting to send (raw bytes,
    // uploaded on Send) and/or a link-preview card generated as you type. Mirrors
    // iOS ConversationView. `dismissedPreviewUrl` suppresses re-adding a preview
    // the user x'd while that URL is still in the text.
    var stagedImageData by remember { mutableStateOf<ByteArray?>(null) }
    var stagedPreview by remember { mutableStateOf<LinkPreviewFfi?>(null) }
    var stagedPreviewUrl by remember { mutableStateOf<String?>(null) }
    var dismissedPreviewUrl by remember { mutableStateOf<String?>(null) }

    fun clearStaged() {
        stagedImageData = null
        stagedPreview = null
        stagedPreviewUrl = null
        dismissedPreviewUrl = null
    }

    // The message under the Signal-style long-press actions overlay (docs/33),
    // with its on-screen bounds + resolved context so the overlay can lift the
    // exact bubble to center. Null when no overlay is showing.
    var actionTarget by remember { mutableStateOf<ChatActionTarget?>(null) }
    // Whether the full emoji picker sheet is up (opened from the overlay's "+").
    var showEmojiPicker by remember { mutableStateOf(false) }

    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()

    // Gate the "auto-scroll to bottom on new message" effect until initial
    // positioning (scroll to first unread / bottom) has run. Without this, the
    // async 0 -> N population of `messages` on load is mistaken for a new
    // message and animates to the bottom, then 100ms later the initial effect
    // jumps to the first unread — a visible flash-then-jump. Resets per
    // conversation. (iOS gets this for free: SwiftUI .onChange(of:) doesn't
    // fire on initial appearance; Compose's LaunchedEffect does.)
    var initialScrollDone by remember(conversation.id) { mutableStateOf(false) }

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

    // Stage raw image bytes in the composer — shared by the photo picker, the
    // clipboard paste button, and an incoming share (docs/35). Mirrors iOS
    // ConversationView.stageImageData.
    fun stageImageBytes(data: ByteArray) {
        stagedImageData = data
    }

    // Photo attachment picker (docs/35): load the picked image and *stage* it in
    // the composer; nothing is uploaded or sent until Send. Mirrors iOS.
    val context = LocalContext.current
    val photoPicker = rememberLauncherForActivityResult(
        ActivityResultContracts.PickVisualMedia()
    ) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val data = withContext(kotlinx.coroutines.Dispatchers.IO) {
                runCatching { context.contentResolver.openInputStream(uri)?.use { it.readBytes() } }
                    .getOrNull()
                    ?.let { processOutgoingImage(it) }
            } ?: return@launch
            stageImageBytes(data)
        }
    }

    // Clipboard image paste (docs/35): show a paste button when the clipboard
    // holds an image, and stage it on tap. Android only lets the focused app read
    // the clipboard, so we refresh on resume (return from another app where the
    // user copied) and on primary-clip change. Mirrors iOS's paste button.
    val clipboard = remember { context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager }
    var canPasteImage by remember { mutableStateOf(false) }
    fun refreshPasteAvailability() {
        canPasteImage = clipboard.primaryClipDescription?.let { desc ->
            (0 until desc.mimeTypeCount).any { desc.getMimeType(it).startsWith("image/") }
        } ?: false
    }
    fun pasteImage() {
        val clip = clipboard.primaryClip ?: return
        if (clip.itemCount == 0) return
        val uri = clip.getItemAt(0).uri ?: return
        scope.launch {
            val data = withContext(kotlinx.coroutines.Dispatchers.IO) {
                runCatching { context.contentResolver.openInputStream(uri)?.use { it.readBytes() } }
                    .getOrNull()
                    ?.let { processOutgoingImage(it) }
            } ?: return@launch
            stageImageBytes(data)
        }
    }
    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner, clipboard) {
        val clipListener = ClipboardManager.OnPrimaryClipChangedListener { refreshPasteAvailability() }
        clipboard.addPrimaryClipChangedListener(clipListener)
        val lifecycleObserver = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) refreshPasteAvailability()
        }
        lifecycleOwner.lifecycle.addObserver(lifecycleObserver)
        refreshPasteAvailability()
        onDispose {
            clipboard.removePrimaryClipChangedListener(clipListener)
            lifecycleOwner.lifecycle.removeObserver(lifecycleObserver)
        }
    }

    // Debounced link-preview generation (docs/35): ~0.6s after the last keystroke,
    // if the text contains a new URL we haven't staged or dismissed, fetch its
    // preview and stage it; clear the staged preview when the URL leaves the text.
    // Mirrors iOS ConversationView.schedulePreviewFetch.
    LaunchedEffect(messageText, editingMessage) {
        if (editingMessage != null) return@LaunchedEffect
        delay(600)
        val url = firstUrlIn(messageText)
        if (url == null) {
            stagedPreview = null
            stagedPreviewUrl = null
            dismissedPreviewUrl = null
            return@LaunchedEffect
        }
        if (url != dismissedPreviewUrl) dismissedPreviewUrl = null
        if (url == stagedPreviewUrl || url == dismissedPreviewUrl) return@LaunchedEffect
        val previews = viewModel.linkPreviews(messageText, conversation.accountId)
        // Only adopt it if the text still ends on this same URL and it wasn't dismissed.
        if (firstUrlIn(messageText) != url || url == dismissedPreviewUrl) return@LaunchedEffect
        previews.firstOrNull()?.let {
            stagedPreview = it
            stagedPreviewUrl = url
        }
    }

    fun sendMessage() {
        val text = messageText
        val image = stagedImageData
        val preview = stagedPreview
        if (text.trim().isEmpty() && image == null && preview == null) return
        messageText = ""
        clearStaged()
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
        // Insert into the UI immediately so the send feels instant (mirrors iOS).
        // addOptimisticMessage also bumps the chat-list row (incl. the staged
        // image's attachment type, via the message's attachments — empty here, so
        // sendComposed corrects it after upload).
        viewModel.addOptimisticMessage(message = optimistic, conversation = conversation)

        // Scroll-to-bottom is a UI nicety and must NEVER gate the send. Run it
        // as a separate, failure-tolerant coroutine — `animateScrollToItem` was
        // suspending here and the send never ran, leaving the message stuck on
        // the "sending" clock (and never reaching the server, so adminbot never
        // saw it).
        scope.launch {
            runCatching {
                if (messages.isNotEmpty()) listState.animateScrollToItem(messages.size)
            }
        }

        scope.launch {
            try {
                viewModel.sendComposed(
                    conversation = conversation,
                    text = text,
                    imageData = image,
                    preview = preview,
                    messageId = messageId,
                    sentAtMs = nowMs,
                )
            } catch (e: Exception) {
                errorMessage = "Failed to send: ${e.message}"
            }
        }
    }

    // onAppear: load messages, reactions, mark read, set current conv.
    LaunchedEffect(conversation.id) {
        viewModel.setCurrentConversationId(conversation.id)
        // An image shared/routed into this chat (docs/35): pre-stage it in the
        // composer for review before sending.
        viewModel.takePendingStagedImage(conversation.id)?.let { stageImageBytes(it) }
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
        initialScrollDone = true
    }

    // onDisappear: clear current conversation id.
    DisposableEffect(conversation.id) {
        onDispose {
            if (viewModel.currentConversationId.value == conversation.id) {
                viewModel.setCurrentConversationId(null)
            }
        }
    }

    // Mark read (and auto-scroll) when the transcript loads or grows.
    val messageCount = messages.size
    LaunchedEffect(messageCount) {
        if (messageCount == 0) return@LaunchedEffect
        // Mark read whenever the transcript (re)loads or grows — mirrors iOS
        // ConversationView's `.onChange(of: messages.count)`. Crucially this is
        // NOT gated on initialScrollDone: on first open the transcript loads
        // asynchronously *after* onAppear's markAllMessagesRead has already run
        // (when the in-memory transcript was still empty, so its optimistic
        // update was a no-op). Re-marking here, once the messages are actually
        // in memory, is what clears the chat-list badge — otherwise it only
        // cleared on the second visit.
        viewModel.markAllMessagesRead(
            conversationId = conversation.id,
            accountId = conversation.accountId,
        )
        // Auto-scroll to the newest message only for messages arriving after the
        // initial positioning; the initial load is positioned by the effect above.
        if (initialScrollDone) {
            listState.animateScrollToItem(messageCount - 1)
        }
    }

    // Group titles are a tappable avatar + name that open the group detail
    // screen (mirrors the iOS principal toolbar item); DMs show a plain title.
    val groupId = conversation.groupId
    Box(modifier = Modifier.fillMaxSize()) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    if (conversation.isGroup && groupId != null) {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                            modifier = Modifier.clickable {
                                onNavigateToGroupDetail(groupId, conversation.accountId)
                            },
                        ) {
                            ContactAvatar(name = liveConv.title, size = 28.dp)
                            Text(
                                text = liveConv.title,
                                style = MaterialTheme.typography.titleMedium,
                                color = LocalAvalancheColors.current.ink,
                            )
                        }
                    } else {
                        Text(
                            text = liveConv.title,
                            style = MaterialTheme.typography.titleMedium,
                            color = LocalAvalancheColors.current.ink,
                        )
                    }
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = LocalAvalancheColors.current.paper,
                    titleContentColor = LocalAvalancheColors.current.ink,
                    navigationIconContentColor = LocalAvalancheColors.current.ink,
                ),
            )
        },
        containerColor = LocalAvalancheColors.current.paper,
        // Zero the content insets so the Scaffold doesn't pad the bottom nav bar
        // itself — we apply the bottom inset explicitly below. Otherwise the
        // nav-bar padding and the IME padding would stack (sum), leaving a
        // nav-bar-tall gap above the keyboard. The top app bar still handles the
        // status bar via its own insets, so innerPadding still carries the top.
        contentWindowInsets = WindowInsets(0, 0, 0, 0),
    ) { innerPadding ->
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(innerPadding)
            // Edge-to-edge (enableEdgeToEdge) opts out of the system's automatic
            // adjustResize, so consume the bottom inset here: the LARGER of the
            // keyboard (IME) and the navigation bar. union() takes the max per
            // side, so when the keyboard is up the composer sits flush above it,
            // and when it's down it clears the nav bar — no double counting.
            .windowInsetsPadding(WindowInsets.ime.union(WindowInsets.navigationBars))
            .background(LocalAvalancheColors.current.paper),
    ) {
        // --- Message list ---
        LazyColumn(
            state = listState,
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
                .padding(horizontal = 8.dp),
            // Anchor messages to the bottom: when the thread is shorter than the
            // viewport the bubbles sit at the bottom (chat idiom, matching iOS)
            // rather than floating at the top; longer threads scroll normally.
            verticalArrangement = Arrangement.spacedBy(8.dp, Alignment.Bottom),
        ) {
            item { Spacer(Modifier.size(8.dp)) }
            itemsIndexed(messages, key = { _, m -> m.id }) { index, message ->
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
                    // Sender name above incoming group bubbles, only on the
                    // first message of a consecutive run (a system event also
                    // breaks a run). Mirrors ConversationView.swift.
                    val isMe = message.senderAccountId == conversation.accountId
                    val firstOfRun = index == 0 ||
                        messages[index - 1].isSystemEvent ||
                        messages[index - 1].senderAccountId != message.senderAccountId
                    val senderName = if (conversation.isGroup && !isMe && firstOfRun) {
                        viewModel.resolvedName(message.senderAccountId, conversation.accountId)
                    } else {
                        null
                    }
                    // Last of a run: timestamp/delivery collapse to this bubble.
                    val isLastInRun = index == messages.lastIndex ||
                        messages[index + 1].isSystemEvent ||
                        messages[index + 1].senderAccountId != message.senderAccountId

                    MessageBubble(
                        message = message,
                        isMe = isMe,
                        isBot = isBotSender(message),
                        senderName = senderName,
                        isLastInRun = isLastInRun,
                        reactions = reactionsFor(message),
                        myDid = conversation.accountId,
                        actionsEnabled = true,
                        onToggleReaction = { emoji ->
                            viewModel.toggleReaction(
                                message = message,
                                emoji = emoji,
                                conversation = conversation,
                            )
                        },
                        onLongPress = { bounds ->
                            actionTarget = ChatActionTarget(
                                message = message,
                                bounds = bounds,
                                senderName = senderName,
                                isLastInRun = isLastInRun,
                                isMe = isMe,
                                isBot = isBotSender(message),
                            )
                        },
                        attachmentLoader = { att ->
                            viewModel.attachmentData(att, accountId = conversation.accountId)
                        },
                        onImageClick = { att -> imageViewerStartId = att.id },
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
                color = LocalAvalancheColors.current.error,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
        }

        HorizontalDivider(color = LocalAvalancheColors.current.divider)

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
                    stagedImageData = stagedImageData,
                    stagedPreview = stagedPreview,
                    previewLoader = { att -> viewModel.attachmentData(att, conversation.accountId) },
                    onRemoveImage = { stagedImageData = null },
                    onRemovePreview = {
                        dismissedPreviewUrl = stagedPreviewUrl
                        stagedPreview = null
                        stagedPreviewUrl = null
                    },
                    onSend = { sendMessage() },
                    onApplyEdit = { applyEdit() },
                    onCancelEdit = { cancelEdit() },
                    onAttach = {
                        photoPicker.launch(
                            PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)
                        )
                    },
                    canPasteImage = canPasteImage,
                    onPaste = { pasteImage() },
                )
        }
    }
    }

        // Signal-style long-press actions overlay (docs/33), drawn above the
        // whole screen so its scrim dims the app bar too.
        actionTarget?.let { target ->
            MessageActionsOverlay(
                message = target.message,
                isMe = target.isMe,
                isBot = target.isBot,
                senderName = target.senderName,
                isLastInRun = target.isLastInRun,
                sourceBounds = target.bounds,
                reactions = reactionsFor(target.message),
                myDid = conversation.accountId,
                canEdit = canEdit(target.message),
                onToggleReaction = { emoji ->
                    viewModel.toggleReaction(message = target.message, emoji = emoji, conversation = conversation)
                },
                onMore = { showEmojiPicker = true },
                onEdit = { startEditing(target.message) },
                onDelete = { forEveryone ->
                    viewModel.deleteMessage(message = target.message, forEveryone = forEveryone, conversation = conversation)
                },
                onShowHistory = { showHistory(target.message) },
                attachmentLoader = { att -> viewModel.attachmentData(att, accountId = conversation.accountId) },
                onDismiss = { actionTarget = null },
            )
        }
    }

    // Full emoji picker (docs/33), opened from the overlay's "+". Keeps
    // actionTarget alive underneath so the picked emoji has a target.
    if (showEmojiPicker) {
        EmojiPickerSheet(
            onDismiss = { showEmojiPicker = false },
            onPick = { emoji ->
                actionTarget?.let {
                    viewModel.toggleReaction(message = it.message, emoji = emoji, conversation = conversation)
                }
                actionTarget = null
            },
        )
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

    // Fullscreen image viewer (docs/35): pages through every image in the
    // conversation in timeline order, starting from the tapped one.
    imageViewerStartId?.let { startId ->
        val conversationImages = messages
            .flatMap { it.attachments }
            .filter { it.contentType.startsWith("image/") }
        ImageViewerDialog(
            images = conversationImages,
            startId = startId,
            loader = { att -> viewModel.attachmentData(att, accountId = conversation.accountId) },
            onDismiss = { imageViewerStartId = null },
        )
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
            color = LocalAvalancheColors.current.muted,
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
    stagedImageData: ByteArray? = null,
    stagedPreview: LinkPreviewFfi? = null,
    previewLoader: suspend (AttachmentFfi) -> ByteArray? = { null },
    onRemoveImage: () -> Unit = {},
    onRemovePreview: () -> Unit = {},
    onSend: () -> Unit,
    onApplyEdit: () -> Unit,
    onCancelEdit: () -> Unit,
    onAttach: () -> Unit = {},
    canPasteImage: Boolean = false,
    onPaste: () -> Unit = {},
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
                    tint = LocalAvalancheColors.current.brand,
                    modifier = Modifier.size(16.dp),
                )
                Spacer(Modifier.width(6.dp))
                Text(
                    text = "Editing message",
                    style = MaterialTheme.typography.labelSmall,
                    color = LocalAvalancheColors.current.muted,
                    modifier = Modifier.weight(1f),
                )
                IconButton(onClick = onCancelEdit) {
                    Icon(
                        imageVector = Icons.Filled.Close,
                        contentDescription = "Cancel edit",
                        tint = LocalAvalancheColors.current.muted,
                    )
                }
            }
        }

        // Staging strip (docs/35): pending image and/or link-preview card, each
        // removable with an x. Hidden while editing.
        if (editingMessage == null && (stagedImageData != null || stagedPreview != null)) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp)
                    .padding(top = 6.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalAlignment = Alignment.Top,
            ) {
                if (stagedImageData != null) {
                    val bmp = remember(stagedImageData) {
                        BitmapFactory.decodeByteArray(stagedImageData, 0, stagedImageData.size)
                    }
                    Box {
                        if (bmp != null) {
                            Image(
                                bitmap = bmp.asImageBitmap(),
                                contentDescription = "Staged image",
                                contentScale = ContentScale.Crop,
                                modifier = Modifier.size(60.dp).clip(RoundedCornerShape(10.dp)),
                            )
                        }
                        StagedRemoveButton(onClick = onRemoveImage, modifier = Modifier.align(Alignment.TopEnd))
                    }
                }
                if (stagedPreview != null) {
                    Box {
                        LinkPreviewCard(preview = stagedPreview, isMe = true, loader = previewLoader)
                        StagedRemoveButton(onClick = onRemovePreview, modifier = Modifier.align(Alignment.TopEnd))
                    }
                }
            }
        }

        val canSend = messageText.trim().isNotEmpty() || stagedImageData != null || stagedPreview != null

        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.Bottom,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            // Attachment picker (docs/35) — hidden while editing a message.
            if (editingMessage == null) {
                IconButton(onClick = onAttach) {
                    Icon(
                        imageVector = Icons.Filled.AddCircle,
                        contentDescription = "Attach",
                        tint = LocalAvalancheColors.current.brand,
                    )
                }
                // Paste an image from the clipboard (docs/35) — shown only when the
                // clipboard holds an image.
                if (canPasteImage) {
                    IconButton(onClick = onPaste) {
                        Icon(
                            imageVector = Icons.Filled.ContentPaste,
                            contentDescription = "Paste image",
                            tint = LocalAvalancheColors.current.brand,
                        )
                    }
                }
            }

            // Rounded, borderless "pill" input that sits on a soft fill — replaces
            // the boxy default outline. Grows up to a few lines, then scrolls.
            OutlinedTextField(
                value = messageText,
                onValueChange = onMessageTextChange,
                modifier = Modifier
                    .weight(1f)
                    .heightIn(max = 120.dp),
                placeholder = {
                    Text(
                        text = if (editingMessage == null) "Message" else "Edit message",
                        color = LocalAvalancheColors.current.muted,
                    )
                },
                keyboardOptions = KeyboardOptions(
                    capitalization = KeyboardCapitalization.Sentences,
                ),
                maxLines = 5,
                shape = RoundedCornerShape(24.dp),
                colors = OutlinedTextFieldDefaults.colors(
                    focusedContainerColor = LocalAvalancheColors.current.card,
                    unfocusedContainerColor = LocalAvalancheColors.current.card,
                    disabledContainerColor = LocalAvalancheColors.current.card,
                    focusedBorderColor = Color.Transparent,
                    unfocusedBorderColor = Color.Transparent,
                    disabledBorderColor = Color.Transparent,
                    cursorColor = LocalAvalancheColors.current.brand,
                    focusedTextColor = LocalAvalancheColors.current.ink,
                    unfocusedTextColor = LocalAvalancheColors.current.ink,
                ),
            )

            // Circular send button: filled with the brand color when there is text,
            // muted/disabled otherwise (iMessage-style up arrow, check when editing).
            IconButton(
                onClick = { if (editingMessage != null) onApplyEdit() else onSend() },
                enabled = canSend,
                modifier = Modifier
                    .padding(bottom = 4.dp)
                    .size(40.dp)
                    .background(
                        color = if (canSend) LocalAvalancheColors.current.brand else LocalAvalancheColors.current.divider,
                        shape = CircleShape,
                    ),
            ) {
                Icon(
                    imageVector = if (editingMessage != null) Icons.Filled.Check else Icons.Filled.ArrowUpward,
                    contentDescription = if (editingMessage != null) "Apply edit" else "Send",
                    tint = LocalAvalancheColors.current.paper,
                )
            }
        }
    }
}

/** Small "x" overlay used to drop a staged composer attachment (docs/35). */
@Composable
private fun StagedRemoveButton(onClick: () -> Unit, modifier: Modifier = Modifier) {
    Box(
        modifier = modifier
            .padding(2.dp)
            .size(20.dp)
            .background(Color.Black.copy(alpha = 0.55f), CircleShape)
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            imageVector = Icons.Filled.Close,
            contentDescription = "Remove",
            tint = Color.White,
            modifier = Modifier.size(14.dp),
        )
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
            color = LocalAvalancheColors.current.muted,
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
                    contentColor = LocalAvalancheColors.current.error,
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
                    contentColor = LocalAvalancheColors.current.error,
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
            color = LocalAvalancheColors.current.muted,
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
            color = LocalAvalancheColors.current.muted,
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
        BlockedBar(
            did = "did:example:blocked",
            accountId = "did:example:alice",
            viewModel = rememberPreviewAppViewModel(),
        )
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

// ---------------------------------------------------------------------------
// Full-conversation previews — mirror the iOS #Preview("DM") / #Preview("Group")
// in ConversationView.swift. The host seeds a preview AppViewModel with a "Me"
// account, the conversation, and its messages (no network/FFI).
// ---------------------------------------------------------------------------

@Composable
private fun ConversationPreviewHost(conversation: Conversation, messages: List<Message>) {
    val me = Account(
        id = "did:plc:me",
        displayName = "Me",
        servers = listOf(
            ServerInfo(
                id = "https://server.example",
                name = "Example",
                url = android.net.Uri.parse("https://server.example"),
            ),
        ),
    )
    val viewModel = rememberPreviewAppViewModel(
        accounts = listOf(me),
        conversations = listOf(conversation),
        messagesByConversation = mapOf(conversation.id to messages),
    )
    AvalancheTheme {
        ConversationView(conversation = conversation, viewModel = viewModel)
    }
}

@Preview(showBackground = true, name = "Conversation — DM")
@Composable
private fun ConversationDMPreview() {
    val conv = Conversation(
        id = "dm-bob",
        title = "Bob Chena",
        accountId = "did:plc:me",
        serverUrl = "https://server.example",
        recipientDid = "did:plc:bob",
    )
    ConversationPreviewHost(
        conversation = conv,
        messages = listOf(
            Message(
                id = "m1",
                conversationId = conv.id,
                senderAccountId = "did:plc:bob",
                body = "Are we still meeting at noon?",
                sentAtMs = 1_700_000_000_000,
                readAtMs = 1_700_000_001_000,
                deliveryStatus = DeliveryStatus.DELIVERED,
            ),
            Message(
                id = "m2",
                conversationId = conv.id,
                senderAccountId = "did:plc:me",
                body = "Yes — I'll be at the front entrance.",
                sentAtMs = 1_700_000_060_000,
                readAtMs = 1_700_000_061_000,
                deliveryStatus = DeliveryStatus.READ,
            ),
        ),
    )
}

@Preview(showBackground = true, name = "Conversation — Group")
@Composable
private fun ConversationGroupPreview() {
    val conv = Conversation(
        id = "group-grp1",
        title = "March Logistics",
        accountId = "did:plc:me",
        serverUrl = "https://server.example",
        groupId = "grp1",
        isGroup = true,
    )
    ConversationPreviewHost(
        conversation = conv,
        messages = listOf(
            Message(
                id = "m1",
                conversationId = conv.id,
                senderAccountId = "did:plc:bob",
                body = "Crew — check in when you arrive.",
                sentAtMs = 1_700_000_000_000,
                readAtMs = 1_700_000_001_000,
                deliveryStatus = DeliveryStatus.DELIVERED,
            ),
            Message(
                id = "m2",
                conversationId = conv.id,
                senderAccountId = "did:plc:bob",
                body = "Bring water, it's hot out.",
                sentAtMs = 1_700_000_010_000,
                readAtMs = 1_700_000_011_000,
                deliveryStatus = DeliveryStatus.DELIVERED,
            ),
            Message(
                id = "m3",
                conversationId = conv.id,
                senderAccountId = "did:plc:carol",
                body = "Almost there!",
                sentAtMs = 1_700_000_020_000,
                readAtMs = 1_700_000_021_000,
                deliveryStatus = DeliveryStatus.DELIVERED,
            ),
            Message(
                id = "m4",
                conversationId = conv.id,
                senderAccountId = "did:plc:me",
                body = "On site 👍",
                sentAtMs = 1_700_000_060_000,
                readAtMs = 1_700_000_061_000,
                deliveryStatus = DeliveryStatus.READ,
            ),
        ),
    )
}
