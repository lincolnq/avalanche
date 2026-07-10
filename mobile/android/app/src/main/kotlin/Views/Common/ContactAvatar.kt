package net.theavalanche.app

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// Avatar for a contact (someone other than the local user). Shows the
// contact's photo override if one is set; otherwise initials on a tinted shape.
//
// When isBot is true the avatar renders clipped to a hexagon instead of a
// circle (docs/54-bot-presentation.md) — bots are visually distinct from people.
//
// imageData is reserved for the per-contact photo_override field
// (docs/52-contacts-and-profiles.md) and is null until that feature lands —
// callers can wire it through with no further change here.
//
// Mirrors mobile/ios/Actnet/Sources/Views/Common/ContactAvatar.swift.
@Composable
fun ContactAvatar(
    name: String,
    imageData: ByteArray? = null,
    isBot: Boolean = false,
    size: Dp,
    modifier: Modifier = Modifier,
    // Accent used for the placeholder fill + initial. Defaults to the brand
    // plum, which reads on paper/incoming backgrounds. Callers placing the
    // avatar on a dark/plum surface (e.g. a sent contact card) pass a light
    // tint like `AvalancheColors.Sand100` so the initial doesn't vanish.
    tint: Color = LocalAvalancheColors.current.brand,
) {
    val color = tint
    val shape = if (isBot) HexagonShape else CircleShape

    val bitmap = remember(imageData) {
        imageData?.let { bytes ->
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()
        }
    }

    Box(
        modifier = modifier
            .size(size)
            .clip(shape)
            .background(color.copy(alpha = 0.2f)),
        contentAlignment = Alignment.Center,
    ) {
        if (bitmap != null) {
            Image(
                bitmap = bitmap,
                contentDescription = name,
                contentScale = ContentScale.Crop,
                modifier = Modifier
                    .size(size)
                    .clip(shape),
            )
        } else {
            val initial = run {
                val trimmed = name.trim()
                if (trimmed.isEmpty()) "?" else trimmed.first().uppercaseChar().toString()
            }
            Text(
                text = initial,
                // 0.4 * size in sp — approximation; actual size is in dp but the
                // visual result is close enough and matches the iOS formula.
                fontSize = (size.value * 0.4f).sp,
                color = color,
            )
        }
    }
}

// Hexagon Shape used for bot avatars. Draws a flat-topped regular hexagon via a
// GenericShape so it can be used as both clip and background shape, matching the
// iOS AnyShape(Hexagon()) approach. The radius is min(width, height) / 2 about the
// centre, so it stays a regular, centred hexagon at any (including non-square) size.
private val HexagonShape: androidx.compose.ui.graphics.Shape =
    androidx.compose.foundation.shape.GenericShape { size, _ ->
        val w = size.width
        val h = size.height
        val cx = w / 2f
        val cy = h / 2f
        val r = minOf(w, h) / 2f
        // Flat-top hexagon: vertices at 0°, 60°, 120°, 180°, 240°, 300°
        val angles = listOf(0f, 60f, 120f, 180f, 240f, 300f)
        angles.forEachIndexed { i, deg ->
            val rad = Math.toRadians(deg.toDouble())
            val x = (cx + r * Math.cos(rad)).toFloat()
            val y = (cy + r * Math.sin(rad)).toFloat()
            if (i == 0) moveTo(x, y) else lineTo(x, y)
        }
        close()
    }

@Preview(showBackground = true)
@Composable
private fun ContactAvatarPersonPreview() {
    AvalancheTheme {
        Box(modifier = Modifier.background(LocalAvalancheColors.current.paper)) {
            ContactAvatar(name = "Alice Example", size = 40.dp)
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun ContactAvatarBotPreview() {
    AvalancheTheme {
        Box(modifier = Modifier.background(LocalAvalancheColors.current.paper)) {
            ContactAvatar(name = "AdminBot", isBot = true, size = 40.dp)
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun ContactAvatarEmptyNamePreview() {
    AvalancheTheme {
        Box(modifier = Modifier.background(LocalAvalancheColors.current.paper)) {
            ContactAvatar(name = "", size = 40.dp)
        }
    }
}
