package net.theavalanche.app

import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter

// ---------------------------------------------------------------------------
// QRCodeImage — themed QR rendering shared by the invite and device-linking
// flows. Mirrors the iOS QRCodeView. Foreground = Plum800, background = Paper.
// ---------------------------------------------------------------------------

/** Render [text] as a themed QR code, or nothing if encoding fails. */
@Composable
fun QRCodeImage(text: String, size: Dp = 220.dp, modifier: Modifier = Modifier) {
    val bitmap = remember(text) { generateQrCodeBitmap(text) }
    if (bitmap != null) {
        Image(
            bitmap = bitmap.asImageBitmap(),
            contentDescription = "QR code",
            modifier = modifier
                .size(size)
                .clip(RoundedCornerShape(8.dp)),
        )
    }
}

/**
 * Encode [content] into a themed QR bitmap. Mirrors the invite-QR rendering in
 * IdentityDetailView (foreground = Plum800, background = Paper).
 */
fun generateQrCodeBitmap(content: String, sizePx: Int = 512): Bitmap? = runCatching {
    val hints = mapOf(EncodeHintType.MARGIN to 1)
    val bitMatrix = QRCodeWriter().encode(content, BarcodeFormat.QR_CODE, sizePx, sizePx, hints)
    val fg = AvalancheColors.Plum800.toArgb()
    val bg = AvalancheColors.Paper.toArgb()
    val bitmap = Bitmap.createBitmap(sizePx, sizePx, Bitmap.Config.RGB_565)
    for (x in 0 until sizePx) {
        for (y in 0 until sizePx) {
            bitmap.setPixel(x, y, if (bitMatrix[x, y]) fg else bg)
        }
    }
    bitmap
}.getOrNull()
