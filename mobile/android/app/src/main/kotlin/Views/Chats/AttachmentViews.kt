package net.theavalanche.app

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import android.media.ExifInterface
import android.net.Uri
import android.util.Log
import android.util.LruCache
import androidx.compose.ui.platform.LocalDensity
import java.util.concurrent.Executors
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.withContext
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
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
import java.io.ByteArrayInputStream
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

// Decode caps (longest edge, px) matching iOS: a photo shown in a ~240dp bubble
// must not decode at full resolution. 960 for message images, 520 for the smaller
// link-preview image (mirrors iOS `decodeDownsampledImage` maxPixel values).
internal const val MESSAGE_IMAGE_MAX_PIXEL = 960
private const val LINK_PREVIEW_MAX_PIXEL = 520

// Image decode is CPU-heavy; cap concurrency to 2 so a fling's worth of decodes
// can't saturate every core and starve the UI thread (gfxinfo "Slow UI thread").
// `internal` so the fullscreen viewer (ImageViewerDialog) shares the same pool.
internal val imageDecodeDispatcher = Executors.newFixedThreadPool(2).asCoroutineDispatcher()

/**
 * Decode [data] downsampled so its longest edge is at most ~[maxPixel] px, via
 * `BitmapFactory.inSampleSize` — the full-resolution bitmap is never materialized.
 * Mirrors iOS `decodeDownsampledImage`. Call OFF the main thread.
 */
fun decodeDownsampledBitmap(data: ByteArray, maxPixel: Int): Bitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(data, 0, data.size, bounds)
    val longest = maxOf(bounds.outWidth, bounds.outHeight)
    if (longest <= 0) return null
    var sample = 1
    while (longest / (sample * 2) >= maxPixel) sample *= 2
    val opts = BitmapFactory.Options().apply { inSampleSize = sample }
    return BitmapFactory.decodeByteArray(data, 0, data.size, opts)
}

/**
 * App-wide LRU cache of decoded message-image bitmaps, keyed by attachment id.
 * Compose's `LazyColumn` disposes off-screen rows, so without this every scroll-back
 * re-decodes the blob (jank + a thumbnail→full flash); the cache makes scroll-back
 * instant and keeps decode work off the hot path. (iOS holds the decoded image in
 * per-row `@State` that its more retentive `List` keeps alive; Compose needs an
 * explicit cache for the same effect.) Sized to ~1/8 of the heap.
 */
object MessageImageCache {
    private val maxKb = (Runtime.getRuntime().maxMemory() / 1024 / 4)
        .coerceIn(8L * 1024, Int.MAX_VALUE.toLong()).toInt()
    private val cache = object : LruCache<String, Bitmap>(maxKb) {
        override fun sizeOf(key: String, value: Bitmap): Int = value.byteCount / 1024
    }
    fun get(key: String): Bitmap? = if (key.isEmpty()) null else cache.get(key)
    fun put(key: String, value: Bitmap) { if (key.isNotEmpty()) cache.put(key, value) }
}

/**
 * Renders a single message attachment (docs/35-attachments.md). Images show the
 * inline thumbnail immediately (a fast placeholder), then load the full blob;
 * non-image attachments render as a file chip. Mirrors iOS `AttachmentView`.
 */
@Composable
fun AttachmentView(
    attachment: AttachmentFfi,
    loader: suspend (AttachmentFfi) -> ByteArray?,
    /** Tapping an image opens the fullscreen viewer (docs/35); null disables tap. */
    onImageClick: (() -> Unit)? = null,
) {
    val isImage = attachment.contentType.startsWith("image/")

    if (isImage) {
        // Stable cache key (local row id, falling back to the blob URL for
        // not-yet-persisted rows).
        val cacheKey = attachment.id.ifEmpty { attachment.url }

        // Reserve a fixed display box from the attachment's aspect ratio so the
        // bubble does NOT resize when the full image swaps in for the thumbnail —
        // that resize is what shifts scroll position and makes images "grow" on
        // scroll-back. Mirrors iOS, where `scaledToFit` inside a maxWidth/maxHeight
        // frame derives the size from the aspect ratio (identical for thumb and
        // full). Falls back to 4:3 when dimensions are unknown (older messages).
        val (boxW, boxH) = remember(attachment.id, attachment.width, attachment.height) {
            val maxW = 240f
            val maxH = 320f
            val aspect = if (attachment.width > 0 && attachment.height > 0) {
                attachment.width.toFloat() / attachment.height.toFloat()
            } else {
                4f / 3f
            }
            if (maxW / aspect <= maxH) maxW.dp to (maxW / aspect).dp
            else (maxH * aspect).dp to maxH.dp
        }
        // Decode only to the box's pixel size (capped), not a fixed 960px: a multi-MB
        // photo shown in a ~240dp bubble was decoding to a multi-MB bitmap — slow,
        // GC-heavy, and so big the cache thrashed. Sizing the decode to the display
        // box cuts bitmap memory several-fold.
        val density = LocalDensity.current
        val targetPx = remember(boxW, boxH, density) {
            with(density) { maxOf(boxW.toPx(), boxH.toPx()).toInt() }
                .coerceIn(1, MESSAGE_IMAGE_MAX_PIXEL)
        }

        // Decoded bitmaps held in state + an app-wide cache so neither a
        // recomposition (typing) nor a scroll-back re-decodes — mirrors iOS, which
        // decodes downsampled, off the main thread, and holds the result. Seeded
        // from the cache so a scroll-back shows the full image instantly.
        var fullBitmap by remember(attachment.id) { mutableStateOf(MessageImageCache.get(cacheKey)) }
        var thumbBitmap by remember(attachment.id) { mutableStateOf<Bitmap?>(null) }
        var loading by remember(attachment.id) { mutableStateOf(false) }

        LaunchedEffect(attachment.id) {
            // Instant low-res placeholder from the inline thumbnail (decoded off the
            // main thread, capped concurrency), unless the full image is cached.
            if (fullBitmap == null && attachment.thumbnail.isNotEmpty()) {
                thumbBitmap = withContext(imageDecodeDispatcher) {
                    decodeDownsampledBitmap(attachment.thumbnail, targetPx)
                }
            }
            if (fullBitmap == null) {
                loading = true
                val data = loader(attachment)
                val decoded = data?.let {
                    withContext(imageDecodeDispatcher) { decodeDownsampledBitmap(it, targetPx) }
                }
                if (decoded != null) {
                    MessageImageCache.put(cacheKey, decoded)
                    fullBitmap = decoded
                }
                loading = false
            }
        }

        val shape = RoundedCornerShape(14.dp)
        val shown = fullBitmap ?: thumbBitmap
        if (shown != null) {
            Image(
                bitmap = shown.asImageBitmap(),
                contentDescription = attachment.fileName ?: "Attachment",
                contentScale = ContentScale.Fit,
                modifier = Modifier
                    .size(boxW, boxH)
                    .clip(shape)
                    .then(if (onImageClick != null) Modifier.clickable { onImageClick() } else Modifier),
            )
        } else {
            // Reserve the same space before any bitmap is ready, so the row height
            // is stable from the first layout pass.
            Box(
                modifier = Modifier
                    .size(boxW, boxH)
                    .clip(shape)
                    .background(LocalAvalancheColors.current.muted.copy(alpha = 0.15f)),
                contentAlignment = Alignment.Center,
            ) {
                if (loading) CircularProgressIndicator(modifier = Modifier.size(32.dp))
            }
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

// Outgoing-image policy (docs/35), matching the iOS `OutgoingImage` tier and
// Signal's pipeline: re-encode every sent image to bake in orientation, cap the
// resolution, and strip EXIF/metadata (GPS, device) as a side effect. Tune here.
// (Signal's "standard" tier is ~1600px longest edge; we allow a bit more.)
private const val OUTGOING_MAX_DIMENSION = 2048
private const val OUTGOING_JPEG_QUALITY = 90

/**
 * Prepare an image for sending (docs/35): bake in EXIF orientation (which Android's
 * `BitmapFactory` otherwise ignores → sideways photos), cap the longest edge to
 * [OUTGOING_MAX_DIMENSION], and JPEG-encode — which drops the source's
 * EXIF/metadata. Always re-encodes (Signal-style). Returns the original bytes only
 * if it can't be decoded. Call off the main thread.
 */
fun processOutgoingImage(data: ByteArray): ByteArray {
    val orientation = runCatching {
        ExifInterface(ByteArrayInputStream(data))
            .getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL)
    }.getOrDefault(ExifInterface.ORIENTATION_NORMAL)

    val src = BitmapFactory.decodeByteArray(data, 0, data.size) ?: return data
    return runCatching {
        val matrix = Matrix()
        val longest = maxOf(src.width, src.height)
        if (longest > OUTGOING_MAX_DIMENSION) {
            val scale = OUTGOING_MAX_DIMENSION.toFloat() / longest
            matrix.postScale(scale, scale)
        }
        when (orientation) {
            ExifInterface.ORIENTATION_ROTATE_90 -> matrix.postRotate(90f)
            ExifInterface.ORIENTATION_ROTATE_180 -> matrix.postRotate(180f)
            ExifInterface.ORIENTATION_ROTATE_270 -> matrix.postRotate(270f)
            ExifInterface.ORIENTATION_FLIP_HORIZONTAL -> matrix.postScale(-1f, 1f)
            ExifInterface.ORIENTATION_FLIP_VERTICAL -> matrix.postScale(1f, -1f)
            ExifInterface.ORIENTATION_TRANSPOSE -> { matrix.postRotate(90f); matrix.postScale(-1f, 1f) }
            ExifInterface.ORIENTATION_TRANSVERSE -> { matrix.postRotate(270f); matrix.postScale(-1f, 1f) }
        }
        val result = if (!matrix.isIdentity) {
            Bitmap.createBitmap(src, 0, 0, src.width, src.height, matrix, true)
        } else {
            src
        }
        val out = ByteArrayOutputStream()
        result.compress(Bitmap.CompressFormat.JPEG, OUTGOING_JPEG_QUALITY, out)
        if (result != src) result.recycle()
        out.toByteArray()
    }.getOrDefault(data).also { src.recycle() }
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
    // Decode the og:image downsampled and off the main thread, hold it, AND cache
    // it — same anti-jank treatment as message images. Seeding from the cache is
    // what stops the preview from vanishing and re-decoding on every scroll-back
    // (Compose disposes the off-screen row). Mirrors iOS LinkPreviewCard.
    var bitmap by remember(preview.url) {
        mutableStateOf(preview.image?.let { MessageImageCache.get(it.url) })
    }

    LaunchedEffect(preview.url) {
        val img = preview.image
        if (bitmap == null && img != null) {
            val data = loader(img)
            val decoded = data?.let {
                withContext(imageDecodeDispatcher) { decodeDownsampledBitmap(it, LINK_PREVIEW_MAX_PIXEL) }
            }
            if (decoded != null) {
                MessageImageCache.put(img.url, decoded)
                bitmap = decoded
            }
        }
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
        val previewBitmap = bitmap
        if (previewBitmap != null) {
            Image(
                bitmap = previewBitmap.asImageBitmap(),
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
