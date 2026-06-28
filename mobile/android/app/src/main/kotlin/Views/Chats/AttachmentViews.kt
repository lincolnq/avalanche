package net.theavalanche.app

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.Log
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.MaterialTheme
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Description
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.foundation.layout.height
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import java.io.ByteArrayOutputStream
import uniffi.app_core.AttachmentFfi
import uniffi.app_core.LinkPreviewFfi

/**
 * Open [url] in the user's browser via an ACTION_VIEW intent.
 *
 * Uses the Activity [Context] directly (the proven pattern in AccountsView)
 * rather than Compose's `LocalUriHandler`: `AndroidUriHandler.openUri` does a
 * bare `startActivity` without `FLAG_ACTIVITY_NEW_TASK`, and when that throws the
 * failure was being silently swallowed — so chat link previews and hyperlinks
 * appeared inert. `NEW_TASK` is added defensively so the launch resolves a
 * browser regardless of the launching context.
 */
fun openUrlInBrowser(context: Context, rawUrl: String) {
    if (rawUrl.isBlank()) return
    // Schemes are case-insensitive per RFC 3986, but Android's intent matching is
    // case-sensitive and browsers only register lowercase http/https — so a typed
    // "Http://..." resolves to nothing. We hand-roll link detection (linkify), so
    // unlike Signal — which leans on Android's Linkify to canonicalize the scheme
    // for free — we must lowercase it ourselves.
    val url = SCHEME_PREFIX.replace(rawUrl) { it.value.lowercase() }
    runCatching {
        val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url))
        // Mirror Signal's LinkActions: NEW_TASK only when not launching from an
        // Activity (from an Activity it would needlessly start a separate task).
        if (context !is Activity) intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(intent)
    }.onFailure { Log.w("Avalanche", "Failed to open URL: $url", it) }
}

private val SCHEME_PREFIX = Regex("^[a-zA-Z][a-zA-Z0-9+.-]*:")

/**
 * Renders a single message attachment (docs/35-attachments.md). Images show the
 * inline thumbnail immediately (a fast placeholder), then load the full blob;
 * non-image attachments render as a file chip. Mirrors iOS `AttachmentView`.
 */
@Composable
fun AttachmentView(
    attachment: AttachmentFfi,
    loader: suspend (AttachmentFfi) -> ByteArray?,
) {
    val isImage = attachment.contentType.startsWith("image/")
    var fullData by remember(attachment.id) { mutableStateOf<ByteArray?>(null) }
    var loading by remember(attachment.id) { mutableStateOf(false) }

    if (isImage) {
        // Kick off the full-resolution load; the inline thumbnail shows until it
        // arrives.
        LaunchedEffect(attachment.id) {
            if (fullData == null) {
                loading = true
                fullData = loader(attachment)
                loading = false
            }
        }
        val bytes = fullData ?: attachment.thumbnail.takeIf { it.isNotEmpty() }
        val bitmap = remember(bytes) {
            bytes?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
        }
        if (bitmap != null) {
            Image(
                bitmap = bitmap.asImageBitmap(),
                contentDescription = attachment.fileName ?: "Attachment",
                contentScale = ContentScale.Fit,
                modifier = Modifier
                    .sizeIn(maxWidth = 240.dp, maxHeight = 320.dp)
                    .clip(RoundedCornerShape(14.dp)),
            )
        } else if (loading) {
            CircularProgressIndicator(modifier = Modifier.size(32.dp))
        }
    } else {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier
                .clip(RoundedCornerShape(12.dp))
                .background(LocalAvalancheColors.current.muted.copy(alpha = 0.15f))
                .clickable {
                    // Tapping a file chip is a no-op placeholder for opening;
                    // full open/save is a follow-up.
                }
                .padding(10.dp),
        ) {
            Icon(Icons.Filled.Description, contentDescription = null)
            Spacer(Modifier.width(8.dp))
            Column {
                Text(
                    text = attachment.fileName ?: "Attachment",
                    style = MaterialTheme.typography.bodyMedium,
                )
                Text(
                    text = formatBytes(attachment.sizeBytes),
                    style = MaterialTheme.typography.labelSmall,
                    color = LocalAvalancheColors.current.muted,
                )
            }
        }
    }
}

private fun formatBytes(n: Long): String = when {
    n >= 1_000_000 -> "%.1f MB".format(n / 1_000_000.0)
    n >= 1_000 -> "%.0f KB".format(n / 1_000.0)
    else -> "$n B"
}

/**
 * Downscale image [data] to a small inline preview JPEG plus its pixel
 * dimensions, for the attachment pointer's thumbnail/width/height (docs/35).
 * Returns an empty thumbnail and zero dimensions if [data] isn't decodable.
 * Mirrors iOS `makeAttachmentThumbnail`.
 */
fun makeAttachmentThumbnail(data: ByteArray, maxDimension: Int = 320): Triple<ByteArray, Int, Int> {
    val full = BitmapFactory.decodeByteArray(data, 0, data.size) ?: return Triple(ByteArray(0), 0, 0)
    val w = full.width
    val h = full.height
    val scale = minOf(1f, maxDimension.toFloat() / maxOf(w, h))
    val thumb = if (scale < 1f) {
        Bitmap.createScaledBitmap(full, (w * scale).toInt().coerceAtLeast(1), (h * scale).toInt().coerceAtLeast(1), true)
    } else {
        full
    }
    val out = ByteArrayOutputStream()
    thumb.compress(Bitmap.CompressFormat.JPEG, 60, out)
    return Triple(out.toByteArray(), w, h)
}

/**
 * A rich link-preview card (docs/35 "Link previews"): the og:image (if any) on
 * top, then title and source domain. Tapping opens the URL. The image is a
 * normal attachment downloaded via the same [loader] as message attachments.
 * Mirrors iOS `LinkPreviewCard`.
 */
@Composable
fun LinkPreviewCard(
    preview: LinkPreviewFfi,
    isMe: Boolean,
    loader: suspend (AttachmentFfi) -> ByteArray?,
) {
    val colors = LocalAvalancheColors.current
    val context = LocalContext.current
    var imageData by remember(preview.url) { mutableStateOf<ByteArray?>(null) }

    LaunchedEffect(preview.url) {
        val img = preview.image
        if (imageData == null && img != null) imageData = loader(img)
    }
    val bitmap = remember(imageData) {
        imageData?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
    }
    val domain = remember(preview.url) {
        runCatching { java.net.URI(preview.url).host?.removePrefix("www.") }.getOrNull() ?: preview.url
    }

    Column(
        modifier = Modifier
            .sizeIn(maxWidth = 260.dp)
            .clip(RoundedCornerShape(14.dp))
            .background(if (isMe) colors.outgoingBubble.copy(alpha = 0.6f) else colors.incomingBubble)
            .clickable { openUrlInBrowser(context, preview.url) },
    ) {
        if (bitmap != null) {
            Image(
                bitmap = bitmap.asImageBitmap(),
                contentDescription = preview.title.ifEmpty { domain },
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxWidth().height(130.dp),
            )
        }
        Column(modifier = Modifier.padding(10.dp)) {
            if (preview.title.isNotEmpty()) {
                Text(
                    text = preview.title,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Text(
                text = domain,
                style = MaterialTheme.typography.labelSmall,
                color = colors.muted,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}
