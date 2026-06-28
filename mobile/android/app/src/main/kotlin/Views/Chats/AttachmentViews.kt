package net.theavalanche.app

import android.graphics.Bitmap
import android.graphics.BitmapFactory
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import java.io.ByteArrayOutputStream
import uniffi.app_core.AttachmentFfi

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
                .background(AvalancheColors.Muted.copy(alpha = 0.15f))
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
                    color = AvalancheColors.Muted,
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
