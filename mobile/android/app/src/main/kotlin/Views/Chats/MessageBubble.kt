package net.theavalanche.app

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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccessTime
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Error
import androidx.compose.material.icons.outlined.CheckCircle as OutlinedCheckCircle
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
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.app_core.AttachmentFfi
import uniffi.app_core.ReactionFfi
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlin.math.ceil

/**
 * A single chat message bubble. Mirrors MessageBubble.swift.
 *
 * @param message        The message to render.
 * @param isMe           Whether this message was sent by the local user.
 * @param isBot          Whether the sender is a bot (renders cut-corner shape).
 * @param senderName     Sender's display name, shown above the bubble for
 *                       incoming group messages (Signal-style). Null for DMs,
 *                       own messages, and the 2nd+ message of a run from the
 *                       same sender — ConversationView decides when to pass it.
 * @param isLastInRun    Whether this is the last message of a consecutive run
 *                       from the same sender. Timestamp + delivery only show on
 *                       the last of a run (iMessage-style).
 * @param reactions      List of reactions on this message.
 * @param myDid          The local user's DID, used to highlight own reactions.
 * @param actionsEnabled Whether long-press raises the actions overlay (docs/33).
 * @param interactive    When false the bubble is a static copy (e.g. inside the
 *                       actions overlay): no long-press gesture is attached.
 * @param showSideSpacers When false the side spacers are dropped so the bubble
 *                       sizes to its content — used by the actions overlay, which
 *                       positions the copy itself.
 * @param onToggleReaction Callback when a reaction cluster is tapped.
 * @param onLongPress    Long-press on the bubble — ConversationView raises the
 *                       Signal-style overlay. The Rect is this bubble's content
 *                       bounds in window coords, so the overlay can animate it.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun MessageBubble(
    message: Message,
    isMe: Boolean,
    isBot: Boolean = false,
    senderName: String? = null,
    isLastInRun: Boolean = true,
    reactions: List<ReactionFfi> = emptyList(),
    myDid: String = "",
    actionsEnabled: Boolean = false,
    interactive: Boolean = true,
    showSideSpacers: Boolean = true,
    onToggleReaction: (String) -> Unit = {},
    onLongPress: (Rect) -> Unit = {},
    /** Loads decrypted bytes for an attachment (docs/35); injected by the
     *  conversation screen so the bubble stays free of ViewModel access. */
    attachmentLoader: suspend (AttachmentFfi) -> ByteArray? = { null },
) {
    if (showSideSpacers) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = if (isMe) Arrangement.End else Arrangement.Start,
        ) {
            if (isMe) Spacer(modifier = Modifier.width(60.dp))
            BubbleColumn(message, isMe, isBot, senderName, isLastInRun, reactions, myDid, actionsEnabled, interactive, onToggleReaction, onLongPress, attachmentLoader)
            if (!isMe) Spacer(modifier = Modifier.width(60.dp))
        }
    } else {
        BubbleColumn(message, isMe, isBot, senderName, isLastInRun, reactions, myDid, actionsEnabled, interactive, onToggleReaction, onLongPress, attachmentLoader)
    }
}

/**
 * The message content (sender name, attachments, bubble, previews, reactions)
 * without the surrounding side spacers. Shared by the timeline (wrapped in the
 * Row) and the actions overlay copy. Tracks its own bounds so a long-press can
 * hand the overlay a start position for the lift-to-center animation.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun BubbleColumn(
    message: Message,
    isMe: Boolean,
    isBot: Boolean,
    senderName: String?,
    isLastInRun: Boolean,
    reactions: List<ReactionFfi>,
    myDid: String,
    actionsEnabled: Boolean,
    interactive: Boolean,
    onToggleReaction: (String) -> Unit,
    onLongPress: (Rect) -> Unit,
    attachmentLoader: suspend (AttachmentFfi) -> ByteArray?,
) {
    var contentBounds by remember { mutableStateOf(Rect.Zero) }

    Column(
        horizontalAlignment = if (isMe) Alignment.End else Alignment.Start,
        verticalArrangement = Arrangement.spacedBy(4.dp),
        modifier = Modifier.onGloballyPositioned { contentBounds = it.boundsInWindow() },
    ) {
        // Sender name above the bubble (incoming group messages, first of a
        // run). Per-sender color so a member keeps the same color.
        if (senderName != null && !isMe) {
            Text(
                text = senderName,
                style = MaterialTheme.typography.labelMedium,
                fontWeight = FontWeight.SemiBold,
                color = senderColor(message.senderAccountId),
                modifier = Modifier.padding(start = 4.dp),
            )
        }

        // Attachments (docs/35), rendered above the text bubble.
        message.attachments.forEach { att ->
            AttachmentView(attachment = att, loader = attachmentLoader)
        }

        // Bubble — omitted for an attachment-only message (empty body) so a
        // photo doesn't get an empty bubble below it.
        if (message.body.isNotEmpty() || message.attachments.isEmpty() || message.isDeleted) {
            BubbleContent(
                message = message,
                isMe = isMe,
                isBot = isBot,
                isLastInRun = isLastInRun,
                actionsEnabled = actionsEnabled && interactive,
                onLongClick = { if (actionsEnabled && interactive) onLongPress(contentBounds) },
            )
        }

        // Link-preview cards (docs/35) below the text bubble.
        message.previews.forEach { preview ->
            LinkPreviewCard(preview = preview, isMe = isMe, loader = attachmentLoader)
        }

        // Reaction clusters
        val clusters = reactionClusters(reactions, myDid)
        if (clusters.isNotEmpty()) {
            ReactionClusterRow(
                clusters = clusters,
                onToggleReaction = onToggleReaction,
            )
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun BubbleContent(
    message: Message,
    isMe: Boolean,
    isBot: Boolean,
    isLastInRun: Boolean,
    actionsEnabled: Boolean,
    onLongClick: () -> Unit,
) {
    val bubbleShape: Shape = if (isBot) CutCornerRectangle(cut = 12f) else RoundedCornerShape(16.dp)
    // Timestamp + delivery only on the last bubble of a run; "Edited" whenever
    // applicable. When neither, no metadata is laid out at all.
    val showMetadata = isLastInRun || (message.isEdited && !message.isDeleted)
    val metadata: @Composable () -> Unit = {
        MessageMetadata(
            message = message,
            isMe = isMe,
            isLastInRun = isLastInRun,
            color = if (isMe && !message.isDeleted) {
                AvalancheColors.Sand100.copy(alpha = 0.8f)
            } else {
                LocalAvalancheColors.current.muted
            },
        )
    }

    if (message.isDeleted) {
        // Dashed border tombstone. Compose has no dashed Modifier.border, so we
        // stroke a dashed rounded rect ourselves via drawBehind + dashPathEffect.
        val dashColor = LocalAvalancheColors.current.muted.copy(alpha = 0.4f)
        val density = LocalDensity.current
        val cornerPx = with(density) { 16.dp.toPx() }
        val strokePx = with(density) { 1.dp.toPx() }
        Box(
            modifier = Modifier
                .padding(horizontal = 12.dp, vertical = 8.dp),
        ) {
            Box(
                modifier = Modifier
                    .drawBehind {
                        drawRoundRect(
                            color = dashColor,
                            cornerRadius = CornerRadius(cornerPx, cornerPx),
                            style = Stroke(
                                width = strokePx,
                                pathEffect = PathEffect.dashPathEffect(floatArrayOf(10f, 6f), 0f),
                            ),
                        )
                    }
                    .padding(horizontal = 12.dp, vertical = 8.dp),
            ) {
                FlowMessageText(
                    text = AnnotatedString("This message was deleted"),
                    textStyle = MaterialTheme.typography.bodyMedium.copy(fontStyle = FontStyle.Italic),
                    textColor = LocalAvalancheColors.current.muted,
                    showMetadata = showMetadata,
                    metadata = metadata,
                )
            }
        }
    } else {
        val bgColor = if (isMe) LocalAvalancheColors.current.outgoingBubble else LocalAvalancheColors.current.incomingBubble
        val fgColor = if (isMe) AvalancheColors.Sand100 else LocalAvalancheColors.current.ink
        val context = LocalContext.current

        Box(
            modifier = Modifier
                .clip(bubbleShape)
                .background(bgColor)
                .combinedClickable(
                    onClick = {},
                    onLongClick = onLongClick,
                )
                .padding(horizontal = 12.dp, vertical = 8.dp),
        ) {
            FlowMessageText(
                text = linkify(message.body) { url -> openUrlInBrowser(context, url) },
                textStyle = MaterialTheme.typography.bodyMedium,
                textColor = fgColor,
                showMetadata = showMetadata,
                metadata = metadata,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Hyperlinking
// ---------------------------------------------------------------------------

private val URL_REGEX = Regex("""(https?://|www\.)\S+""", RegexOption.IGNORE_CASE)

/**
 * Turn URLs in [body] into tappable links. Mirrors iOS's `NSDataDetector`
 * linkification; trailing punctuation is excluded from the link, and bare `www.`
 * hosts get an `http://` scheme so they resolve.
 *
 * Each link carries an explicit [onClick] listener that launches the URL via
 * [openUrlInBrowser]. We do NOT rely on the default `LocalUriHandler` path: it
 * launched without `FLAG_ACTIVITY_NEW_TASK` and swallowed the resulting failure,
 * so taps appeared inert.
 */
fun linkify(body: String, onClick: (String) -> Unit): AnnotatedString = buildAnnotatedString {
    var idx = 0
    for (m in URL_REGEX.findAll(body)) {
        append(body.substring(idx, m.range.first))
        val raw = m.value
        val trailing = raw.takeLastWhile { it in ".,;:!?)]}\"'" }
        val linkText = raw.dropLast(trailing.length)
        if (linkText.isNotEmpty()) {
            val url = if (linkText.startsWith("www", ignoreCase = true)) "http://$linkText" else linkText
            withLink(
                LinkAnnotation.Url(
                    url,
                    TextLinkStyles(SpanStyle(textDecoration = TextDecoration.Underline)),
                ) { link ->
                    onClick((link as LinkAnnotation.Url).url)
                }
            ) {
                append(linkText)
            }
        }
        append(trailing)
        idx = m.range.last + 1
    }
    append(body.substring(idx))
}

// ---------------------------------------------------------------------------
// Inline timestamp flow layout (Signal-style)
// ---------------------------------------------------------------------------

/**
 * Lays out [text] with a trailing [metadata] cluster (timestamp + delivery)
 * that tucks into the bottom-right of the bubble. If the metadata fits after
 * the last line of text it sits there; otherwise it wraps to its own line,
 * right-aligned, extending the bubble by one line. Mirrors the iOS reservation/
 * overlay trick via a measure of the text's last-line width.
 */
@Composable
private fun FlowMessageText(
    text: AnnotatedString,
    textStyle: TextStyle,
    textColor: Color,
    showMetadata: Boolean,
    metadata: @Composable () -> Unit,
    modifier: Modifier = Modifier,
) {
    if (!showMetadata) {
        Text(text = text, style = textStyle, color = textColor, modifier = modifier)
        return
    }

    val gapPx = with(LocalDensity.current) { 8.dp.roundToPx() }

    SubcomposeLayout(modifier) { constraints ->
        val metaPlaceable = subcompose("meta", metadata)
            .first()
            .measure(constraints.copy(minWidth = 0, minHeight = 0))

        var layoutResult: TextLayoutResult? = null
        val textPlaceable = subcompose("text") {
            Text(
                text = text,
                style = textStyle,
                color = textColor,
                onTextLayout = { layoutResult = it },
            )
        }.first().measure(constraints)

        val maxW = constraints.maxWidth
        val lastLineRight = layoutResult?.let {
            ceil(it.getLineRight(it.lineCount - 1)).toInt()
        } ?: textPlaceable.width

        val fitsOnLastLine = lastLineRight + gapPx + metaPlaceable.width <= maxW

        if (fitsOnLastLine) {
            val width = maxOf(textPlaceable.width, lastLineRight + gapPx + metaPlaceable.width)
                .coerceAtMost(maxW)
            val height = textPlaceable.height
            layout(width, height) {
                textPlaceable.place(0, 0)
                metaPlaceable.place(width - metaPlaceable.width, height - metaPlaceable.height)
            }
        } else {
            val width = maxOf(textPlaceable.width, metaPlaceable.width).coerceAtMost(maxW)
            val height = textPlaceable.height + metaPlaceable.height
            layout(width, height) {
                textPlaceable.place(0, 0)
                metaPlaceable.place(width - metaPlaceable.width, textPlaceable.height)
            }
        }
    }
}

/**
 * The inline metadata cluster: an optional "Edited" marker, the compact
 * timestamp, and (own messages) the delivery glyph. Timestamp + delivery only
 * appear on the last message of a run; "Edited" always shows when applicable.
 */
@Composable
private fun MessageMetadata(
    message: Message,
    isMe: Boolean,
    isLastInRun: Boolean,
    color: Color,
) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        if (message.isEdited && !message.isDeleted) {
            Text(
                text = "Edited",
                style = MaterialTheme.typography.labelSmall,
                color = color,
                fontSize = 10.sp,
            )
        }
        if (isLastInRun) {
            Text(
                text = shortTimestamp(message.sentAt),
                style = MaterialTheme.typography.labelSmall,
                color = color,
                fontSize = 10.sp,
            )
            if (isMe && !message.isDeleted) {
                InlineDeliveryIcon(status = message.deliveryStatus, color = color)
            }
        }
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
            val bgColor = if (cluster.mine) LocalAvalancheColors.current.brand.copy(alpha = 0.18f) else LocalAvalancheColors.current.incomingBubble
            val borderColor = if (cluster.mine) LocalAvalancheColors.current.brand.copy(alpha = 0.5f) else Color.Transparent

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
                            color = if (cluster.mine) LocalAvalancheColors.current.brand else LocalAvalancheColors.current.muted,
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

/**
 * Inline delivery glyph drawn next to the timestamp (Signal-style, mirroring
 * the iOS checkmark.circle / checkmark.circle.fill pair): a bare check for
 * sent, an outline check-circle for delivered, a filled check-circle for read.
 * Glyphs only ride the outgoing (plum) bubble, so they share the light cluster
 * [color] — read is distinguished by the filled circle, not the brand color
 * (which equals the bubble color and would be invisible). Failed stays red.
 */
@Composable
private fun InlineDeliveryIcon(status: DeliveryStatus, color: Color) {
    val icon = when (status) {
        DeliveryStatus.SENDING -> Icons.Filled.AccessTime
        DeliveryStatus.SENT -> Icons.Filled.Check
        DeliveryStatus.DELIVERED -> Icons.Outlined.OutlinedCheckCircle
        DeliveryStatus.READ -> Icons.Filled.CheckCircle
        DeliveryStatus.FAILED -> Icons.Filled.Error
    }
    val tint = if (status == DeliveryStatus.FAILED) LocalAvalancheColors.current.error else color
    Icon(
        imageVector = icon,
        contentDescription = status.name.lowercase(Locale.US),
        tint = tint,
        modifier = Modifier.size(12.dp),
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

private val timeFormatter = SimpleDateFormat("h:mm a", Locale.getDefault())

/**
 * Compact, Signal-style timestamp: "now" under a minute, "32m" within the hour,
 * otherwise the locale short time ("5:13 PM"). Computed at render — it doesn't
 * live-tick between renders.
 */
private fun shortTimestamp(date: Date): String {
    val secs = (System.currentTimeMillis() - date.time) / 1000
    return when {
        secs < 60 -> "now"
        secs < 3600 -> "${secs / 60}m"
        else -> timeFormatter.format(date)
    }
}

/**
 * Per-sender name color for group bubbles. Picked from a fixed palette by a
 * stable FNV-style hash of the sender DID so a member always gets the same
 * color across launches. Mirrors MessageBubble.swift's senderColor.
 */
private val senderPalette = listOf(
    Color(0xFF1E88E5), // blue
    Color(0xFF8E24AA), // purple
    Color(0xFFD81B60), // pink
    Color(0xFFF4511E), // orange
    Color(0xFF00897B), // teal
    Color(0xFF3949AB), // indigo
    Color(0xFF43A047), // green
    AvalancheColors.Plum400, // plum - fixed (decorative palette read outside composition)
)

private fun senderColor(did: String): Color {
    var hash = 5381L
    for (b in did.toByteArray()) {
        hash = hash * 33 + b
    }
    val idx = ((hash % senderPalette.size) + senderPalette.size) % senderPalette.size
    return senderPalette[idx.toInt()]
}

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
                .background(LocalAvalancheColors.current.paper)
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            MessageBubble(message = incoming, isMe = false)
            MessageBubble(message = outgoing, isMe = true, actionsEnabled = true)
            MessageBubble(message = deleted, isMe = false)
            MessageBubble(
                message = incoming.copy(id = "4", body = "Bot message here", senderAccountId = "bot"),
                isMe = false,
                isBot = true,
            )
        }
    }
}
