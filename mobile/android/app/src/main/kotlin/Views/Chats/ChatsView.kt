package net.theavalanche.app

import android.graphics.BitmapFactory
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.LinearOutSlowInEasing
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.indication
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Create
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.PlatformTextStyle
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// ---------------------------------------------------------------------------
// ChatsView
//
// Mirrors iOS Sources/Views/Chats/ChatsView.swift (docs/37-chat-organization):
// no top app bar — a custom header hosts the per-account tab strip (only when
// the user has >1 account; Signal-baseline progressive disclosure) with the
// compose button on the trailing side. Selection on the strip is an avBrand
// ring around the account's avatar (iOS-style), animated between tabs. The
// conversation list sits on `card` (chrome stays `paper`), rows have a
// press highlight, and previews are collapsed to their first line.
//
// Navigation callbacks replace NavigationStack — the central NavGraph wires
// them. Settings moved to the bottom NavigationBar (MainTabView).
// ---------------------------------------------------------------------------

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
    val accounts by viewModel.accounts.collectAsState()

    // Mirror iOS onChange(of: appState.navigateToConversation)
    LaunchedEffect(navigateToConversation) {
        val conv = navigateToConversation ?: return@LaunchedEffect
        viewModel.setNavigateToConversation(null)
        onOpenConversation(conv)
    }

    // Whether the per-account tab strip is shown at all. Progressive
    // disclosure: a single-account user sees a plain unified inbox (docs/37).
    val showsAccountTabs = accounts.size > 1

    // The account (identity) whose conversations are shown when the strip is
    // visible. Held in the ViewModel (survives bottom-tab switches — local
    // state would reset and flash an unfiltered frame); the effective value is
    // derived *synchronously* so no frame ever renders with a stale/absent
    // selection: fall back to the first account if unset or vanished.
    val rawSelectedAccountTab by viewModel.selectedAccountTab.collectAsState()
    val selectedAccountTab = rawSelectedAccountTab
        ?.takeIf { id -> accounts.any { it.id == id } }
        ?: accounts.firstOrNull()?.id

    val sortedConversations = remember(conversations) {
        conversations.sortedByDescending { it.lastMessageDate?.time ?: Long.MIN_VALUE }
    }
    val visibleConversations = remember(sortedConversations, showsAccountTabs, selectedAccountTab) {
        val selected = selectedAccountTab
        if (showsAccountTabs && selected != null) {
            sortedConversations.filter { it.accountId == selected }
        } else {
            sortedConversations
        }
    }

    // TODO(opus): hasRecoveryKey — check via Rust core once the FFI method exists.
    val hasRecoveryKey = false

    // Per-account unread totals for the tab badges: the sum of unread counts
    // across every conversation homed in that account's tab.
    val unreadByAccount = accounts.associate { acct ->
        acct.id to sortedConversations
            .filter { it.accountId == acct.id }
            .sumOf { viewModel.unreadCount(it) }
    }

    Scaffold(
        topBar = {
            ChatsHeader(
                accounts = accounts,
                showsAccountTabs = showsAccountTabs,
                selectedAccountTab = selectedAccountTab,
                unreadByAccount = unreadByAccount,
                onSelectAccountTab = { viewModel.setSelectedAccountTab(it) },
                onOpenCompose = onOpenCompose,
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
                    modifier = Modifier
                        .fillMaxSize()
                        .background(LocalAvalancheColors.current.card),
                    contentAlignment = Alignment.Center,
                ) {
                    CircularProgressIndicator(color = LocalAvalancheColors.current.muted)
                }
            } else if (visibleConversations.isEmpty()) {
                // Mirrors iOS ContentUnavailableView
                Column(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(LocalAvalancheColors.current.card)
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
                    Spacer(Modifier.size(16.dp))
                    Text(
                        text = "No conversations yet",
                        color = LocalAvalancheColors.current.ink,
                        fontSize = 18.sp,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Spacer(Modifier.size(8.dp))
                    Text(
                        text = "Messages from all your servers will appear here.",
                        color = LocalAvalancheColors.current.muted,
                        fontSize = 14.sp,
                    )
                }
            } else {
                Column(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(LocalAvalancheColors.current.card),
                ) {
                    // Recovery key banner (mirrors iOS placement below the header)
                    if (!hasRecoveryKey) {
                        RecoveryKeyBanner()
                        HorizontalDivider(color = LocalAvalancheColors.current.divider)
                    }

                    LazyColumn(
                        modifier = Modifier.fillMaxSize(),
                        // Content scrolls behind the floating bottom bar; the
                        // inset lets the last row scroll clear of it.
                        contentPadding = PaddingValues(bottom = FloatingBarContentPadding),
                    ) {
                        items(
                            items = visibleConversations,
                            key = { it.id },
                        ) { conversation ->
                            val account = accounts.firstOrNull { it.id == conversation.accountId }
                            val unreadCount = viewModel.unreadCount(conversation)
                            val recipientDid = conversation.recipientDid
                            val isBot = !conversation.isGroup &&
                                recipientDid != null &&
                                viewModel.isBot(recipientDid, conversation.accountId)
                            ConversationRow(
                                conversation = conversation,
                                account = account,
                                accounts = accounts,
                                unreadCount = unreadCount,
                                isBotConversation = isBot,
                                previewText = conversationPreviewText(viewModel, conversation),
                                avatarData = conversationAvatar(viewModel, conversation),
                                inviterName = conversation.inviterDid?.let {
                                    viewModel.displayName(did = it, accountId = conversation.accountId)
                                },
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .chatRowPressHighlight { onOpenConversation(conversation) }
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
// Header: account tab strip + compose
// ---------------------------------------------------------------------------

/**
 * Custom top header replacing the old top app bar: the per-account tab strip
 * (only with >1 account) on the leading side, compose on the trailing side.
 * Compose is deliberately *not* part of the tab strip (docs/37).
 */
@Composable
private fun ChatsHeader(
    accounts: List<Account>,
    showsAccountTabs: Boolean,
    selectedAccountTab: String?,
    unreadByAccount: Map<String, Int>,
    onSelectAccountTab: (String) -> Unit,
    onOpenCompose: () -> Unit,
) {
    // background *then* statusBarsPadding: paper paints up behind the status
    // bar, content starts below it. (No outer Scaffold supplies this inset —
    // the floating-bar layout owns the whole window.)
    Column(
        modifier = Modifier
            .background(LocalAvalancheColors.current.paper)
            .statusBarsPadding(),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp)
                // No bottom padding: the tab strip's sliding underline sits at
                // the bottom of this row, flush with the header's bottom edge.
                .padding(top = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (showsAccountTabs) {
                // Material-idiom selection: a brand underline that slides
                // between tabs (Android-y), instead of iOS's avatar ring.
                // Tabs are fixed-width (68dp + 8dp spacing), so the indicator
                // offset is a simple animated multiple of the slot width.
                val selectedIndex = accounts.indexOfFirst { it.id == selectedAccountTab }
                val slotWidth = 68.dp + 8.dp
                // Modern M3 "primary" indicator: a 3dp pill with rounded top
                // corners, sized to the selected tab's *label text* and
                // centered under it. Label widths are measured per tab (they
                // vary by name) and reported up via onLabelWidth.
                val labelWidths = remember { mutableStateMapOf<String, Dp>() }
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .horizontalScroll(rememberScrollState()),
                ) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        accounts.forEach { account ->
                            AccountTab(
                                account = account,
                                selected = selectedAccountTab == account.id,
                                unreadCount = unreadByAccount[account.id] ?: 0,
                                onClick = { onSelectAccountTab(account.id) },
                                onLabelWidth = { labelWidths[account.id] = it },
                            )
                        }
                    }
                    // The indicator (and its animated values) only enters
                    // composition once the selected label's width has been
                    // measured — animateDpAsState initializes *at* its first
                    // target, so the first render lands at final size/position
                    // with no settle wiggle; later tab switches animate.
                    val measuredWidth = selectedAccountTab?.let { labelWidths[it] }
                    if (selectedIndex >= 0 && measuredWidth != null) {
                        val indicatorWidth by animateDpAsState(
                            targetValue = measuredWidth,
                            animationSpec = tween(durationMillis = 130, easing = LinearOutSlowInEasing),
                            label = "accountTabIndicatorWidth",
                        )
                        val indicatorOffset by animateDpAsState(
                            targetValue = slotWidth * selectedIndex +
                                (68.dp - indicatorWidth) / 2,
                            animationSpec = tween(durationMillis = 130, easing = LinearOutSlowInEasing),
                            label = "accountTabIndicator",
                        )
                        Box(
                            modifier = Modifier
                                .align(Alignment.BottomStart)
                                .padding(start = indicatorOffset)
                                .width(indicatorWidth)
                                .height(3.dp)
                                .background(
                                    color = LocalAvalancheColors.current.brand,
                                    shape = RoundedCornerShape(
                                        topStart = 3.dp,
                                        topEnd = 3.dp,
                                    ),
                                ),
                        )
                    }
                }
            } else {
                Spacer(modifier = Modifier.weight(1f))
            }
            IconButton(onClick = onOpenCompose) {
                Icon(
                    imageVector = Icons.Filled.Create,
                    contentDescription = "New message",
                    tint = LocalAvalancheColors.current.brand,
                )
            }
        }
        // No divider/elevation below the header — the paper→card tint change
        // is the separation; a hairline under the indicator reads as a thick
        // double border.
    }
}

/**
 * One account tab: the identity's avatar (monogram fallback) above its name.
 * Selection is shown Material-style — the sliding brand underline drawn by the
 * strip (not iOS's avatar ring); the selected avatar is full-color and the
 * others dimmed, animated with a short no-overshoot tween. Auto-derived so a
 * per-account tab is meaningful with zero config; user-created/topic tabs will
 * carry a chosen symbol instead (tab organizer, deferred — docs/37).
 */
@Composable
private fun AccountTab(
    account: Account,
    selected: Boolean,
    // Total unread messages across this account's conversations — rendered as
    // a notification badge on the avatar (matches the row badge style).
    unreadCount: Int = 0,
    onClick: () -> Unit,
    // Reports the measured width of the label text, so the strip can size its
    // sliding indicator pill to match.
    onLabelWidth: (Dp) -> Unit = {},
) {
    val density = LocalDensity.current
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    val pressScale by animateFloatAsState(
        targetValue = if (pressed) 0.93f else 1f,
        animationSpec = tween(durationMillis = 120),
        label = "accountTabPress",
    )

    val avatarAlpha by animateFloatAsState(
        targetValue = if (selected) 1f else 0.45f,
        animationSpec = tween(durationMillis = 130, easing = LinearOutSlowInEasing),
        label = "accountTabAlpha",
    )
    val avatarScale by animateFloatAsState(
        targetValue = if (selected) 1f else 0.9f,
        animationSpec = tween(durationMillis = 130, easing = LinearOutSlowInEasing),
        label = "accountTabScale",
    )

    val label = accountTabLabel(account)

    Column(
        modifier = Modifier
            .scale(pressScale)
            // Shaped background rather than clip-then-fill: the rounded press
            // highlight is preserved, but children may overflow the bounds —
            // the unread badge rides above the avatar (offset y: -4) with only
            // 2dp of headroom, and a real clip shears its top (same bug as
            // iOS's ScrollView clip, fixed there with .scrollClipDisabled()).
            .background(
                color = if (pressed) LocalAvalancheColors.current.ink.copy(alpha = 0.08f)
                else Color.Transparent,
                shape = RoundedCornerShape(14.dp),
            )
            .clickable(
                interactionSource = interaction,
                indication = null,
                onClick = onClick,
            )
            .width(68.dp)
            // Bottom padding leaves room for the strip's sliding underline.
            .padding(top = 2.dp, bottom = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        // The identity's avatar photo when set (docs/55); monogram fallback.
        val avatarBitmap = remember(account.avatarData) {
            account.avatarData?.let { bytes ->
                BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()
            }
        }
        Box {
            Box(
                modifier = Modifier
                    .size(30.dp)
                    .alpha(avatarAlpha)
                    .scale(avatarScale)
                    .background(
                        color = LocalAvalancheColors.current.card,
                        shape = CircleShape,
                    )
                    .clip(CircleShape),
                contentAlignment = Alignment.Center,
            ) {
                if (avatarBitmap != null) {
                    Image(
                        bitmap = avatarBitmap,
                        contentDescription = null,
                        contentScale = ContentScale.Crop,
                        modifier = Modifier.fillMaxSize(),
                    )
                } else {
                    Text(
                        text = label.firstOrNull()?.uppercaseChar()?.toString() ?: "?",
                        fontSize = 14.sp,
                        fontWeight = FontWeight.SemiBold,
                        color = LocalAvalancheColors.current.brand,
                    )
                }
            }
            // Unread badge — sum across the tab's conversations, same
            // notification/white style as the row badge, riding the avatar's
            // top-trailing corner.
            if (unreadCount > 0) {
                Text(
                    text = "$unreadCount",
                    fontSize = 9.sp,
                    fontWeight = FontWeight.Bold,
                    color = Color.White,
                    // Tight line box to match iOS's capsule: Compose's default
                    // line height plus Android font padding otherwise inflates
                    // 9sp glyphs into a visibly taller pill than the iOS badge.
                    lineHeight = 9.sp,
                    style = LocalTextStyle.current.copy(
                        platformStyle = PlatformTextStyle(includeFontPadding = false),
                    ),
                    modifier = Modifier
                        .align(Alignment.TopEnd)
                        .offset(x = 9.dp, y = (-4).dp)
                        .background(
                            color = LocalAvalancheColors.current.notification,
                            shape = RoundedCornerShape(percent = 50),
                        )
                        .padding(horizontal = 4.dp, vertical = 1.dp),
                )
            }
        }
        Text(
            text = label,
            fontSize = 11.sp,
            fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal,
            // Selected label matches the indicator (brand) — M3 primary-tab
            // idiom. TODO(design): consider a brighter accent if plum reads
            // too quiet here.
            color = if (selected) LocalAvalancheColors.current.brand else LocalAvalancheColors.current.muted,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            onTextLayout = { layout ->
                onLabelWidth(with(density) { layout.size.width.toDp() })
            },
        )
    }
}

/**
 * A short label for an account tab: the identity's display name, falling back
 * to its first server's compact host when the name is unset.
 */
private fun accountTabLabel(account: Account): String {
    if (account.displayName.isNotEmpty()) return account.displayName
    return account.servers.firstOrNull()?.let { it.url.host ?: it.name } ?: "Account"
}

// ---------------------------------------------------------------------------
// Shared helpers (also used by ConversationSearchView)
// ---------------------------------------------------------------------------

/**
 * Press feedback for a chat row: a subtle ink highlight while the finger is
 * down (over the row's `card` background). Mirrors iOS ChatRowButtonStyle.
 */
@Composable
fun Modifier.chatRowPressHighlight(onClick: () -> Unit): Modifier {
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    return this
        .background(
            if (pressed) LocalAvalancheColors.current.ink.copy(alpha = 0.06f)
            else Color.Transparent,
        )
        .clickable(
            interactionSource = interaction,
            indication = null,
            onClick = onClick,
        )
}

/**
 * The avatar photo for a conversation row (docs/55): the group avatar for
 * groups, the peer's avatar for DMs. `null` (placeholder) while unresolved.
 * Mirrors iOS ConversationRow.avatarData. Computed live (not `remember`d) so
 * the snapshot-backed cache read is tracked — the row recomposes when the
 * avatar resolves.
 */
@Composable
fun conversationAvatar(viewModel: AppViewModel, conversation: Conversation): ByteArray? {
    return if (conversation.isGroup) {
        conversation.groupId?.let { viewModel.groupAvatar(it, conversation.accountId) }
    } else {
        conversation.recipientDid?.let { viewModel.avatar(it, conversation.accountId) }
    }
}

/**
 * Preview text for the latest message — mirrors iOS ConversationRow.previewText.
 * For a group system event we render it reactively (resolving DIDs to names at
 * display time). Normal messages compose the content decoration (📷/📎/👤) with
 * the body and the "You:"/sender prefix. Collapsed to the first line — bodies
 * can contain hard line breaks and the chat list must never render past line
 * one (docs/37).
 *
 * Computed live (not `remember`d) so the snapshot reads of `displayNameCache`
 * inside resolvedName/groupEventText are tracked: when a name resolves async,
 * the row recomposes and "Unknown" becomes the real name.
 */
@Composable
fun conversationPreviewText(viewModel: AppViewModel, conversation: Conversation): String? {
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
        return firstLinePreview(viewModel.groupEventText(msg, conversation.accountId))
    }
    // Compose the content decoration (📷/📎/👤) with the body (docs/35): a
    // caption shows "📷 caption", a caption-less content message shows "📷
    // Photo", and plain text shows just the body. Kept in sync between live
    // and post-restart previews via `lastMessagePreview`.
    val rawBody = firstLinePreview(conversation.lastMessage ?: "")
    val deco = lastMessagePreviewDecoration(conversation.lastMessagePreview)
    val body: String = when {
        deco != null -> if (rawBody.isEmpty()) "${deco.first} ${deco.second}" else "${deco.first} $rawBody"
        rawBody.isNotEmpty() -> rawBody
        else -> return null
    }
    val sender = conversation.lastMessageSenderDid
    return when {
        sender != null && sender == conversation.accountId -> "You: $body"
        conversation.isGroup && !sender.isNullOrEmpty() ->
            "${viewModel.resolvedName(sender, conversation.accountId)}: $body"
        else -> body
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
                .background(LocalAvalancheColors.current.card),
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
