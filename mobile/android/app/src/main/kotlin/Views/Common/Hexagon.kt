package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import kotlin.math.cos
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * A regular pointy-top hexagon with softly rounded corners, sized to the
 * shorter dimension of its bounds and centered. This is the bot avatar frame
 * (docs/54-bot-presentation.md): people render in a Circle, bots in a
 * Hexagon, so an automated participant is distinguishable from a person at
 * a glance. The shape is applied by the client over whatever image the
 * account supplies, so the bytes can't undo it.
 *
 * Mirrors mobile/ios/Actnet/Sources/Views/Common/Hexagon.swift.
 */
class Hexagon(
    /** Corner rounding as a fraction of the circumradius. ~0.18 reads as a
     *  hexagon without the sharp points looking like a "missing photo" glyph. */
    val cornerRadiusFraction: Float = 0.18f,
) : Shape {

    override fun createOutline(
        size: Size,
        layoutDirection: LayoutDirection,
        density: Density,
    ): Outline {
        return Outline.Generic(buildPath(size))
    }

    private fun buildPath(size: Size): Path {
        val centerX = size.width / 2f
        val centerY = size.height / 2f
        val radius = minOf(size.width, size.height) / 2f
        val corner = radius * cornerRadiusFraction

        // Six vertices, pointy-top (first vertex straight up).
        val vertices = Array(6) { i ->
            val angle = Math.PI / 3.0 * i - Math.PI / 2.0
            floatArrayOf(
                centerX + radius * cos(angle).toFloat(),
                centerY + radius * sin(angle).toFloat(),
            )
        }

        val path = Path()
        for (i in 0 until 6) {
            val current = vertices[i]
            val previous = vertices[(i + 5) % 6]
            val next = vertices[(i + 1) % 6]

            // Pull back from each vertex along both incident edges, then round
            // the corner with a quadratic curve through the original vertex.
            val toPrev = unitVector(current, previous)
            val toNext = unitVector(current, next)
            val startX = current[0] + toPrev[0] * corner
            val startY = current[1] + toPrev[1] * corner
            val endX = current[0] + toNext[0] * corner
            val endY = current[1] + toNext[1] * corner

            if (i == 0) {
                path.moveTo(startX, startY)
            } else {
                path.lineTo(startX, startY)
            }
            path.quadraticTo(current[0], current[1], endX, endY)
        }
        path.close()
        return path
    }

    /** Returns a unit vector pointing from [a] toward [b]. */
    private fun unitVector(a: FloatArray, b: FloatArray): FloatArray {
        val dx = b[0] - a[0]
        val dy = b[1] - a[1]
        val length = maxOf(sqrt(dx * dx + dy * dy), 0.0001f)
        return floatArrayOf(dx / length, dy / length)
    }
}

@Preview(showBackground = true)
@Composable
private fun HexagonPreview() {
    AvalancheTheme {
        Box(
            modifier = Modifier
                .size(80.dp)
                .background(color = AvalancheColors.Brand, shape = Hexagon()),
        )
    }
}
