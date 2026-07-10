package net.theavalanche.app

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.Message
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import org.json.JSONObject
import uniffi.app_core.SharedContactFfi

/**
 * Clipboard codec for the "Copy contact -> paste into a message" flow (docs/35).
 * Mirrors iOS's `ContactPasteboard`.
 *
 * A contact card is copied to the system clipboard under a private MIME type
 * carrying `{did,name}` JSON, so paste reconstructs it losslessly in-app. Unlike
 * iOS (whose pasteboard item holds a separate plain-text representation), an
 * Android `ClipData.Item` has a single text slot, so the JSON doubles as the
 * `text/plain` fallback — pasting into another app yields the JSON string. The
 * composer detects the private MIME type to surface its paste affordance.
 */
object ContactClipboard {
    const val MIME_TYPE = "application/net.theavalanche.contact"

    private fun clipboardOf(context: Context): ClipboardManager =
        context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager

    /** Copy a contact card to the system clipboard. */
    fun write(context: Context, did: String, name: String) {
        val json = JSONObject().apply {
            put("did", did)
            put("name", name)
        }.toString()
        val clip = ClipData(
            ClipDescription("Contact", arrayOf(MIME_TYPE, ClipDescription.MIMETYPE_TEXT_PLAIN)),
            ClipData.Item(json),
        )
        clipboardOf(context).setPrimaryClip(clip)
    }

    /** True when the clipboard holds a contact card (drives the paste button). */
    fun hasContact(clipboard: ClipboardManager): Boolean =
        clipboard.primaryClipDescription?.hasMimeType(MIME_TYPE) == true

    /** Read a contact card off the clipboard, or null if none / malformed. */
    fun read(clipboard: ClipboardManager): SharedContactFfi? {
        if (!hasContact(clipboard)) return null
        val text = clipboard.primaryClip
            ?.takeIf { it.itemCount > 0 }
            ?.getItemAt(0)?.text?.toString()
            ?: return null
        return runCatching {
            val obj = JSONObject(text)
            val did = obj.optString("did", "")
            if (did.isEmpty()) null else SharedContactFfi(did = did, name = obj.optString("name", ""))
        }.getOrNull()
    }
}

/**
 * A shared contact card rendered inside a message bubble (docs/35). Shows the
 * name the sender knows the person by, plus a "Save" action that adds them to
 * the recipient's contact book. The sender's own copy shows the same card
 * without a Save action (they already know the contact). Long-press raises a
 * menu to message the person or re-copy the card. Mirrors iOS SharedContactCard.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun SharedContactCard(
    contact: SharedContactFfi,
    isMe: Boolean,
    // True when this DID is already a curated contact — the card then shows a
    // non-interactive "Saved" state instead of an active Save button.
    alreadySaved: Boolean = false,
    onSave: () -> Unit = {},
    onMessage: () -> Unit = {},
    onCopy: () -> Unit = {},
) {
    val colors = LocalAvalancheColors.current
    val displayName = remember(contact.name, contact.did) {
        contact.name.trim().ifEmpty { contact.did.takeLast(8) }
    }
    var menuExpanded by remember { mutableStateOf(false) }

    Box {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            modifier = Modifier
                .sizeIn(maxWidth = 280.dp)
                // Mirror the message bubble: full outgoing plum for your own card,
                // incoming tone for a received one — paired with the light-on-plum
                // text colors below for legible contrast in both light and dark.
                .clip(RoundedCornerShape(14.dp))
                .background(if (isMe) colors.outgoingBubble else colors.incomingBubble)
                // Border tint follows the side, like the text/avatar: `muted` reads
                // on the incoming card, but is a mid-tone that disappears on the
                // plum sent card — use a light stroke there instead.
                .border(
                    width = 1.dp,
                    color = if (isMe) AvalancheColors.Sand100.copy(alpha = 0.5f) else colors.muted.copy(alpha = 0.25f),
                    shape = RoundedCornerShape(14.dp),
                )
                // Long-press -> menu: message the person, or copy the card to
                // re-share it into another conversation.
                .combinedClickable(onClick = {}, onLongClick = { menuExpanded = true })
                .padding(10.dp),
        ) {
            // On the plum sent card the brand-plum avatar would vanish; use a
            // light tint there. Received cards keep the default brand tint.
            ContactAvatar(
                name = displayName,
                isBot = false,
                size = 40.dp,
                tint = if (isMe) AvalancheColors.Sand100 else colors.brand,
            )
            Column(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                Text(
                    text = displayName,
                    style = MaterialTheme.typography.bodyMedium,
                    fontWeight = FontWeight.SemiBold,
                    color = if (isMe) AvalancheColors.Sand100 else colors.ink,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    text = "Contact",
                    style = MaterialTheme.typography.labelSmall,
                    color = if (isMe) AvalancheColors.Sand100.copy(alpha = 0.8f) else colors.muted,
                )
            }
            Spacer(modifier = Modifier.width(8.dp))
            // The sender already has this contact; only recipients get Save. A
            // recipient who already has the DID curated sees a "Saved" state.
            if (!isMe) {
                if (alreadySaved) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(2.dp),
                    ) {
                        Icon(
                            imageVector = Icons.Filled.Check,
                            contentDescription = null,
                            tint = colors.muted,
                            modifier = Modifier.width(14.dp),
                        )
                        Text(
                            text = "Saved",
                            style = MaterialTheme.typography.labelMedium,
                            fontWeight = FontWeight.SemiBold,
                            color = colors.muted,
                        )
                    }
                } else {
                    // A compact pill, not a Material3 Button — Button enforces a
                    // ~40dp min height that reads much taller than iOS's small
                    // `.borderedProminent` control. Brand fill with a `paper` label
                    // (the luminance-inverse of brand) stays legible in both modes.
                    Text(
                        text = "Save",
                        style = MaterialTheme.typography.labelMedium,
                        fontWeight = FontWeight.SemiBold,
                        color = colors.paper,
                        modifier = Modifier
                            .clip(RoundedCornerShape(50))
                            .background(colors.brand)
                            .clickable(onClick = onSave)
                            .padding(horizontal = 12.dp, vertical = 5.dp),
                    )
                }
            }
        }
        DropdownMenu(
            expanded = menuExpanded,
            onDismissRequest = { menuExpanded = false },
        ) {
            DropdownMenuItem(
                text = { Text("Message $displayName") },
                leadingIcon = { Icon(Icons.Outlined.Message, contentDescription = null) },
                onClick = {
                    menuExpanded = false
                    onMessage()
                },
            )
            DropdownMenuItem(
                text = { Text("Copy contact") },
                leadingIcon = { Icon(Icons.Outlined.ContentCopy, contentDescription = null) },
                onClick = {
                    menuExpanded = false
                    onCopy()
                },
            )
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun SharedContactCardReceivedPreview() {
    AvalancheTheme {
        Box(modifier = Modifier.background(LocalAvalancheColors.current.paper).padding(16.dp)) {
            SharedContactCard(
                contact = SharedContactFfi(did = "did:plc:carol", name = "Carol (canvass lead)"),
                isMe = false,
            )
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun SharedContactCardSentPreview() {
    AvalancheTheme {
        Box(modifier = Modifier.background(LocalAvalancheColors.current.paper).padding(16.dp)) {
            SharedContactCard(
                contact = SharedContactFfi(did = "did:plc:carol", name = "Carol (canvass lead)"),
                isMe = true,
            )
        }
    }
}
