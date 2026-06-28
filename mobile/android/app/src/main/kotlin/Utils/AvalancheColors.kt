package net.theavalanche.app

import android.content.Context
import android.content.res.Configuration
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

// Brand palette. Mirrors mobile/ios/Actnet/Sources/Utils/AvalancheColors.swift —
// keep the two in sync (cross-platform parity rule).
//
// The raw palette (Plum*/Sand*/accents) below is fixed reference swatches. The
// *semantic* colors are dark-mode-adaptive and live in AvalancheSemanticColors /
// LocalAvalancheColors at the bottom of this file — Composable views read those
// (e.g. LocalAvalancheColors.current.paper), mirroring iOS's adaptive Color
// aliases. The `Brand`/`Paper`/... aliases on the object are kept as the *light*
// values for the MaterialTheme scheme (Theme.kt), QR rendering, and previews.
object AvalancheColors {
    // MARK: - Plum (primary brand scale)
    val Plum50 = Color(0xFFF1EBED)
    val Plum100 = Color(0xFFDCC8CE)
    val Plum200 = Color(0xFFC8B4BD)
    val Plum300 = Color(0xFFB1959F)
    val Plum400 = Color(0xFF9C7682)
    val Plum500 = Color(0xFF6B3E50)
    val Plum600 = Color(0xFF5C3445)
    val Plum700 = Color(0xFF4D2939)
    val Plum800 = Color(0xFF3C1F2D)
    val Plum900 = Color(0xFF2A1620)

    // MARK: - Sand (warm neutral)
    val Sand50 = Color(0xFFFFF8F1)
    val Sand100 = Color(0xFFFFF1E9)
    val Sand200 = Color(0xFFF4E7D9)
    val Sand300 = Color(0xFFE5D4C0)
    val Sand400 = Color(0xFFC7B6A0)
    val Sand500 = Color(0xFF9A8A78)
    val Sand600 = Color(0xFF6E6258)
    val Sand900 = Color(0xFF1F1815)

    // MARK: - Accents (reserved, status only)
    val Rose400 = Color(0xFFD17579)
    val Rose500 = Color(0xFFC76870)
    val Rose700 = Color(0xFF993E47)
    val Moss = Color(0xFF6B7F4D)
    val Amber = Color(0xFFD4A04A)

    // MARK: - Semantic aliases
    val Brand = Plum500
    val Paper = Sand100
    val Ink = Sand900
    val Muted = Sand600
    val OutgoingBubble = Plum500
    val IncomingBubble = Sand200
    val DarkSurface = Plum900
    val Error = Rose700
    val Notification = Rose500
    val Success = Moss
    val Warning = Amber
}

/// Dark-mode-adaptive semantic colors. Mirrors the adaptive `av*` aliases in the
/// iOS AvalancheColors.swift. Warm "plum-black" dark theme: deep aubergine
/// surfaces, warm off-white text.
data class AvalancheSemanticColors(
    val brand: Color,
    val paper: Color,
    val ink: Color,
    val muted: Color,
    val incomingBubble: Color,
    val outgoingBubble: Color,
    val card: Color,
    val divider: Color,
    val error: Color,
    val notification: Color,
    val success: Color,
    val warning: Color,
)

val LightAvalancheColors = AvalancheSemanticColors(
    brand = AvalancheColors.Plum500,
    paper = AvalancheColors.Sand100,
    ink = AvalancheColors.Sand900,
    muted = AvalancheColors.Sand600,
    incomingBubble = AvalancheColors.Sand200,
    outgoingBubble = AvalancheColors.Plum500,
    card = AvalancheColors.Sand50,
    divider = AvalancheColors.Sand300,
    error = AvalancheColors.Rose700,
    notification = AvalancheColors.Rose500,
    success = AvalancheColors.Moss,
    warning = AvalancheColors.Amber,
)

val DarkAvalancheColors = AvalancheSemanticColors(
    brand = AvalancheColors.Plum200,
    paper = AvalancheColors.Plum900,
    ink = AvalancheColors.Sand50,
    muted = AvalancheColors.Sand300,
    incomingBubble = AvalancheColors.Plum800,
    outgoingBubble = AvalancheColors.Plum500, // always on plum; stands out in both modes
    card = AvalancheColors.Plum800,
    divider = AvalancheColors.Plum700,
    error = AvalancheColors.Rose400,
    notification = AvalancheColors.Rose500,
    success = AvalancheColors.Moss,
    warning = AvalancheColors.Amber,
)

/// Provided by AvalancheTheme based on the system light/dark setting. Composables
/// read `LocalAvalancheColors.current.<role>`.
val LocalAvalancheColors = staticCompositionLocalOf { LightAvalancheColors }

/// Resolves the semantic palette outside a Composable scope (e.g. inside a custom
/// android.view.View such as RecipientTokenField) from the context's night-mode
/// configuration. The hosting Activity is recreated on a uiMode change, so this is
/// re-read with the correct values after a light/dark toggle.
fun avalancheSemanticColors(context: Context): AvalancheSemanticColors {
    val night = (context.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) ==
        Configuration.UI_MODE_NIGHT_YES
    return if (night) DarkAvalancheColors else LightAvalancheColors
}
