package net.theavalanche.app

import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlin.math.abs
import kotlin.math.roundToInt
import kotlinx.coroutines.withContext
import uniffi.app_core.AttachmentFfi

// Full-resolution decode cap for the fullscreen viewer. Higher than the inline
// MESSAGE_IMAGE_MAX_PIXEL so zoom stays sharp; matches the outgoing cap so it's
// effectively full-res for images we send, while bounding memory for larger
// inbound ones. (iOS decodes at 4096; Android caps lower for tighter heaps.)
private const val VIEWER_IMAGE_MAX_PIXEL = 2048
private const val MAX_ZOOM = 5f

/**
 * Fullscreen image viewer (docs/35): pages between every image attachment in a
 * conversation in timeline order, with pinch-to-zoom + pan per image. The pager
 * only swipes while the current image is unzoomed; once zoomed, one-finger drags
 * pan within the image. Swipe down (when unzoomed) or the close button dismiss.
 * Mirrors iOS `ImageViewerView`.
 */
@Composable
fun ImageViewerDialog(
    images: List<AttachmentFfi>,
    startId: String,
    loader: suspend (AttachmentFfi) -> ByteArray?,
    onDismiss: () -> Unit,
) {
    if (images.isEmpty()) return
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        val startIndex = remember(startId) { images.indexOfFirst { it.id == startId }.coerceAtLeast(0) }
        val pagerState = rememberPagerState(initialPage = startIndex) { images.size }

        // Whether the current page is zoomed — gates pager swiping.
        var currentZoomed by remember { mutableStateOf(false) }
        // Vertical offset for the swipe-down-to-dismiss gesture.
        var dragOffsetY by remember { mutableStateOf(0f) }

        val backdropAlpha = (1f - abs(dragOffsetY) / 900f).coerceIn(0f, 1f)

        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color.Black.copy(alpha = backdropAlpha)),
        ) {
            HorizontalPager(
                state = pagerState,
                userScrollEnabled = !currentZoomed,
                modifier = Modifier
                    .fillMaxSize()
                    .offset { IntOffset(0, dragOffsetY.roundToInt()) },
            ) { page ->
                ZoomablePage(
                    attachment = images[page],
                    loader = loader,
                    isCurrentPage = page == pagerState.currentPage,
                    onZoomChanged = { zoomed -> if (page == pagerState.currentPage) currentZoomed = zoomed },
                    onVerticalDrag = { delta -> dragOffsetY += delta },
                    onVerticalDragEnd = {
                        if (abs(dragOffsetY) > 240f) onDismiss() else dragOffsetY = 0f
                    },
                )
            }

            // Chrome: close button (top-left).
            IconButton(
                onClick = onDismiss,
                modifier = Modifier
                    .align(Alignment.TopStart)
                    .padding(8.dp)
                    .background(Color.Black.copy(alpha = 0.45f * backdropAlpha), CircleShape),
            ) {
                Icon(Icons.Filled.Close, contentDescription = "Close", tint = Color.White)
            }
        }

    }
}

/** One zoomable, pannable page inside the viewer's pager. */
@Composable
private fun ZoomablePage(
    attachment: AttachmentFfi,
    loader: suspend (AttachmentFfi) -> ByteArray?,
    isCurrentPage: Boolean,
    onZoomChanged: (Boolean) -> Unit,
    onVerticalDrag: (Float) -> Unit,
    onVerticalDragEnd: () -> Unit,
) {
    var scale by remember(attachment.id) { mutableStateOf(1f) }
    var offsetX by remember(attachment.id) { mutableStateOf(0f) }
    var offsetY by remember(attachment.id) { mutableStateOf(0f) }

    // Reset zoom when this page scrolls out of view so returning starts fresh.
    LaunchedEffect(isCurrentPage) {
        if (!isCurrentPage) { scale = 1f; offsetX = 0f; offsetY = 0f }
    }
    LaunchedEffect(scale) { onZoomChanged(scale > 1.01f) }

    val cacheKey = "viewer:" + attachment.id.ifEmpty { attachment.url }
    var bitmap by remember(attachment.id) { mutableStateOf<Bitmap?>(MessageImageCache.get(cacheKey)) }
    var thumb by remember(attachment.id) { mutableStateOf<Bitmap?>(null) }

    LaunchedEffect(attachment.id) {
        if (bitmap == null && attachment.thumbnail.isNotEmpty()) {
            thumb = withContext(imageDecodeDispatcher) {
                decodeDownsampledBitmap(attachment.thumbnail, MESSAGE_IMAGE_MAX_PIXEL)
            }
        }
        if (bitmap == null) {
            val data = loader(attachment)
            val decoded = data?.let {
                withContext(imageDecodeDispatcher) { decodeDownsampledBitmap(it, VIEWER_IMAGE_MAX_PIXEL) }
            }
            if (decoded != null) {
                MessageImageCache.put(cacheKey, decoded)
                bitmap = decoded
            }
        }
    }

    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        val shown = bitmap ?: thumb
        if (shown != null) {
            Image(
                bitmap = shown.asImageBitmap(),
                contentDescription = attachment.fileName ?: "Image",
                contentScale = ContentScale.Fit,
                modifier = Modifier
                    .fillMaxSize()
                    .graphicsLayer {
                        scaleX = scale
                        scaleY = scale
                        translationX = offsetX
                        translationY = offsetY
                    }
                    .pointerInput(attachment.id) {
                        detectTapGestures(onDoubleTap = {
                            if (scale > 1f) {
                                scale = 1f; offsetX = 0f; offsetY = 0f
                            } else {
                                scale = 3f
                            }
                        })
                    }
                    .pointerInput(attachment.id) {
                        // Custom arbitration (docs/35): pinch (2 fingers) always
                        // zooms; a 1-finger drag pans when zoomed; when unzoomed a
                        // vertical drag dismisses while a horizontal drag is left
                        // UNCONSUMED so the pager can page.
                        val slop = viewConfiguration.touchSlop
                        awaitEachGesture {
                            awaitFirstDown(requireUnconsumed = false)
                            var mode = 0 // 0 undecided, 1 dismiss, 2 pager, 3 zoom
                            var accX = 0f
                            var accY = 0f
                            while (true) {
                                val event = awaitPointerEvent(PointerEventPass.Main)
                                val pressed = event.changes.count { it.pressed }
                                if (pressed == 0) break
                                if (pressed >= 2) {
                                    mode = 3
                                    val zoom = event.calculateZoom()
                                    val pan = event.calculatePan()
                                    val newScale = (scale * zoom).coerceIn(1f, MAX_ZOOM)
                                    scale = newScale
                                    if (newScale > 1f) {
                                        val maxX = size.width * (newScale - 1f) / 2f
                                        val maxY = size.height * (newScale - 1f) / 2f
                                        offsetX = (offsetX + pan.x).coerceIn(-maxX, maxX)
                                        offsetY = (offsetY + pan.y).coerceIn(-maxY, maxY)
                                    } else {
                                        offsetX = 0f; offsetY = 0f
                                    }
                                    event.changes.forEach { it.consume() }
                                } else if (mode != 2) {
                                    val pan = event.calculatePan()
                                    if (scale > 1f) {
                                        val maxX = size.width * (scale - 1f) / 2f
                                        val maxY = size.height * (scale - 1f) / 2f
                                        offsetX = (offsetX + pan.x).coerceIn(-maxX, maxX)
                                        offsetY = (offsetY + pan.y).coerceIn(-maxY, maxY)
                                        event.changes.forEach { it.consume() }
                                    } else if (mode != 3) {
                                        accX += pan.x; accY += pan.y
                                        if (mode == 0) {
                                            if (abs(accY) > slop && abs(accY) > abs(accX)) mode = 1
                                            else if (abs(accX) > slop) mode = 2 // yield to pager
                                        }
                                        if (mode == 1) {
                                            onVerticalDrag(pan.y)
                                            event.changes.forEach { it.consume() }
                                        }
                                    }
                                }
                            }
                            if (mode == 1) onVerticalDragEnd()
                        }
                    },
            )
        } else {
            CircularProgressIndicator(color = Color.White, modifier = Modifier.size(36.dp))
        }
    }
}
