package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp

/// A rectangle with chamfered (45°-cut) corners — an octagon when the rect is
/// square, a beveled rectangle at any other aspect ratio. This is the bot
/// message-bubble shape (docs/54-bot-presentation.md): bot bubbles read as
/// angular/geometric, echoing the Hexagon avatar frame, while people's bubbles
/// stay rounded.
class CutCornerRectangle(private val cut: Float = 12f) : Shape {
    override fun createOutline(
        size: Size,
        layoutDirection: LayoutDirection,
        density: Density,
    ): Outline {
        val c = minOf(cut, minOf(size.width, size.height) / 2f)
        val path = Path().apply {
            moveTo(c, 0f)
            lineTo(size.width - c, 0f)
            lineTo(size.width, c)
            lineTo(size.width, size.height - c)
            lineTo(size.width - c, size.height)
            lineTo(c, size.height)
            lineTo(0f, size.height - c)
            lineTo(0f, c)
            close()
        }
        return Outline.Generic(path)
    }
}

@Preview(showBackground = true)
@Composable
private fun CutCornerRectanglePreview() {
    AvalancheTheme {
        Box(
            modifier = Modifier
                .size(120.dp, 60.dp)
                .clip(CutCornerRectangle(cut = 16f))
                .background(AvalancheColors.Brand),
        )
    }
}
