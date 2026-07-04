package net.theavalanche.app

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.History
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.app_core.AttachmentFfi
import uniffi.app_core.ReactionFfi
import kotlin.math.roundToInt

/**
 * Signal-style long-press overlay (docs/33): a dimmed backdrop with a floating
 * reaction bar on top, the tapped message rendered as a focal copy, and the
 * action list below — replacing the old DropdownMenu. Mirrors
 * mobile/ios/Actnet/Sources/Views/Chats/MessageActionsOverlay.swift.
 *
 * The message *lifts from where it sits* to the center: a placeholder the exact
 * size of the source spaces the bar/menu and gives the resting center, while a
 * visible copy sized to the source's natural width is offset-animated from the
 * source bounds to that center (so it starts exactly overlapping the real bubble
 * — no snap). The scrim fades in fast (first on the way in, last on the way out)
 * so the swap between the real bubble and the copy stays hidden.
 */
@Composable
fun MessageActionsOverlay(
    message: Message,
    isMe: Boolean,
    isBot: Boolean,
    senderName: String?,
    isLastInRun: Boolean,
    sourceBounds: Rect,
    reactions: List<ReactionFfi>,
    myDid: String,
    canEdit: Boolean,
    onToggleReaction: (String) -> Unit,
    onMore: () -> Unit,
    onEdit: () -> Unit,
    onDelete: (Boolean) -> Unit,
    onShowHistory: () -> Unit,
    attachmentLoader: suspend (AttachmentFfi) -> ByteArray?,
    onDismiss: () -> Unit,
) {
    val density = LocalDensity.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val colors = LocalAvalancheColors.current

    // 0 = at source bounds, 1 = at resting (centered) slot.
    val progress = remember { Animatable(0f) }
    val scrim = remember { Animatable(0f) }
    var overlayOrigin by remember { mutableStateOf(Offset.Zero) }
    var restingCenter by remember { mutableStateOf<Offset?>(null) }

    val myEmoji = reactions.firstOrNull { it.reactorDid == myDid }?.emoji
    val sourceCenter = sourceBounds.center
    val bubbleWidthDp = with(density) { sourceBounds.width.toDp() }
    val bubbleHeightDp = with(density) { sourceBounds.height.toDp() }

    // `action` (react / edit / delete) runs only *after* the exit finishes:
    // applying it earlier would reflow the timeline (e.g. a new reaction adds
    // height) while the copy is still gliding back to the pre-action bounds, so
    // it would land on the old spot and snap. Deferring lets the copy return
    // onto the unchanged bubble, then the change appears as a normal layout.
    fun dismiss(action: (() -> Unit)? = null) {
        scope.launch {
            // Fade the scrim last, finishing with the glide (its tail overlaps).
            launch {
                delay(150)
                scrim.animateTo(0f, tween(120))
            }
            progress.animateTo(0f, spring(dampingRatio = 0.9f, stiffness = Spring.StiffnessMediumLow))
            action?.invoke()
            onDismiss()
        }
    }

    // Scrim in immediately; the bubble lifts once the resting slot is measured.
    androidx.compose.runtime.LaunchedEffect(Unit) { scrim.animateTo(1f, tween(100)) }
    androidx.compose.runtime.LaunchedEffect(restingCenter) {
        if (restingCenter != null && progress.value == 0f) {
            progress.animateTo(1f, spring(dampingRatio = 0.85f, stiffness = Spring.StiffnessMediumLow))
        }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .onGloballyPositioned { overlayOrigin = it.boundsInWindow().topLeft },
    ) {
        // Dimmed backdrop; tap to dismiss (no ripple).
        Box(
            modifier = Modifier
                .fillMaxSize()
                .graphicsLayer { alpha = scrim.value }
                .background(Color.Black.copy(alpha = 0.4f))
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                ) { dismiss() },
        )

        // Centered layout column: reaction bar, an invisible slot the exact size
        // of the source (spaces the bar/menu, gives the resting center), and the
        // action list.
        Column(
            modifier = Modifier
                .align(Alignment.Center)
                .padding(horizontal = 16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            // --- Reaction bar ---
            Row(
                modifier = Modifier
                    .graphicsLayer {
                        alpha = progress.value
                        scaleX = 0.85f + 0.15f * progress.value
                        scaleY = 0.85f + 0.15f * progress.value
                        transformOrigin = TransformOrigin(0.5f, 1f)
                    }
                    .shadow(6.dp, CircleShape)
                    .clip(CircleShape)
                    .background(colors.paper)
                    .padding(horizontal = 10.dp, vertical = 6.dp),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                EmojiData.quick.forEach { emoji ->
                    Box(
                        modifier = Modifier
                            .size(40.dp)
                            .clip(CircleShape)
                            .background(if (myEmoji == emoji) colors.brand.copy(alpha = 0.25f) else Color.Transparent)
                            .clickable {
                                if (myEmoji != emoji) EmojiRecents.record(context, emoji)
                                dismiss { onToggleReaction(emoji) }
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(text = emoji, fontSize = 24.sp)
                    }
                }
                Box(
                    modifier = Modifier
                        .size(40.dp)
                        .clip(CircleShape)
                        .background(colors.card)
                        .clickable { onMore() },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(Icons.Filled.Add, contentDescription = "More emoji", tint = colors.brand)
                }
            }

            // --- Bubble slot (invisible; measured for the resting center) ---
            Box(
                modifier = Modifier
                    .width(bubbleWidthDp)
                    .height(bubbleHeightDp)
                    .onGloballyPositioned { restingCenter = it.boundsInWindow().center },
            )

            // --- Action list ---
            Column(
                modifier = Modifier
                    .graphicsLayer {
                        alpha = progress.value
                        scaleX = 0.85f + 0.15f * progress.value
                        scaleY = 0.85f + 0.15f * progress.value
                        transformOrigin = TransformOrigin(0.5f, 0f)
                    }
                    .shadow(6.dp, RoundedCornerShape(14.dp))
                    .clip(RoundedCornerShape(14.dp))
                    .background(colors.paper)
                    .width(240.dp),
            ) {
                if (canEdit) {
                    ActionRow("Edit", Icons.Filled.Edit) { dismiss { onEdit() } }
                }
                if (message.editCount > 0) {
                    ActionRow("Edit History", Icons.Filled.History) { dismiss { onShowHistory() } }
                }
                ActionRow("Copy", Icons.Filled.ContentCopy) {
                    dismiss { copyToClipboard(context, message.body) }
                }
                if (isMe) {
                    ActionRow("Delete for Everyone", Icons.Filled.Delete, destructive = true) {
                        dismiss { onDelete(true) }
                    }
                }
                ActionRow("Delete for Me", Icons.Filled.Delete, destructive = true) {
                    dismiss { onDelete(false) }
                }
            }
        }

        // --- The visible, animated copy of the message ---
        val target = restingCenter ?: sourceCenter
        val f = progress.value
        val cx = sourceCenter.x + (target.x - sourceCenter.x) * f
        val cy = sourceCenter.y + (target.y - sourceCenter.y) * f
        Box(
            modifier = Modifier
                .offset {
                    IntOffset(
                        (cx - overlayOrigin.x - sourceBounds.width / 2f).roundToInt(),
                        (cy - overlayOrigin.y - sourceBounds.height / 2f).roundToInt(),
                    )
                }
                .width(bubbleWidthDp),
        ) {
            MessageBubble(
                message = message,
                isMe = isMe,
                isBot = isBot,
                senderName = senderName,
                isLastInRun = isLastInRun,
                reactions = reactions,
                myDid = myDid,
                actionsEnabled = false,
                interactive = false,
                showSideSpacers = false,
                attachmentLoader = attachmentLoader,
            )
        }
    }
}

@Composable
private fun ActionRow(
    title: String,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    destructive: Boolean = false,
    onClick: () -> Unit,
) {
    val color = if (destructive) LocalAvalancheColors.current.error else LocalAvalancheColors.current.ink
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Icon(imageVector = icon, contentDescription = null, tint = color, modifier = Modifier.size(20.dp))
        Text(text = title, color = color, style = MaterialTheme.typography.bodyMedium)
    }
}

private fun copyToClipboard(context: Context, text: String) {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
    clipboard?.setPrimaryClip(ClipData.newPlainText("message", text))
}
