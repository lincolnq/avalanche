package net.theavalanche.app

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.text.selection.LocalTextSelectionColors
import androidx.compose.foundation.text.selection.TextSelectionColors
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider

// Fixed brand color scheme (no dynamic/Material You theming) so the app looks the
// same on every device, matching the iOS reference. Built from AvalancheColors.
//
// Every role is set explicitly. Anything left unspecified falls back to
// Material3's baseline scheme, which is a cool purple-gray — and against our warm
// Paper/Sand palette those neutrals read distinctly blue (e.g. selection
// highlights, dividers, text-field and menu backgrounds). Mapping the neutral and
// container roles onto the Sand/Plum scales keeps every surface warm.
private val LightColors = lightColorScheme(
    primary = AvalancheColors.Brand,
    onPrimary = AvalancheColors.Paper,
    primaryContainer = AvalancheColors.Plum100,
    onPrimaryContainer = AvalancheColors.Plum900,
    inversePrimary = AvalancheColors.Plum200,

    secondary = AvalancheColors.Plum300,
    onSecondary = AvalancheColors.Ink,
    secondaryContainer = AvalancheColors.Sand200,
    onSecondaryContainer = AvalancheColors.Ink,

    tertiary = AvalancheColors.Plum400,
    onTertiary = AvalancheColors.Paper,
    tertiaryContainer = AvalancheColors.Plum100,
    onTertiaryContainer = AvalancheColors.Plum900,

    background = AvalancheColors.Paper,
    onBackground = AvalancheColors.Ink,

    surface = AvalancheColors.Paper,
    onSurface = AvalancheColors.Ink,
    surfaceVariant = AvalancheColors.Sand200,
    onSurfaceVariant = AvalancheColors.Muted,
    surfaceTint = AvalancheColors.Brand,

    inverseSurface = AvalancheColors.Ink,
    inverseOnSurface = AvalancheColors.Sand50,

    surfaceBright = AvalancheColors.Sand50,
    surfaceDim = AvalancheColors.Sand300,
    surfaceContainerLowest = AvalancheColors.Sand50,
    surfaceContainerLow = AvalancheColors.Sand100,
    surfaceContainer = AvalancheColors.Sand100,
    surfaceContainerHigh = AvalancheColors.Sand200,
    surfaceContainerHighest = AvalancheColors.Sand300,

    outline = AvalancheColors.Sand400,
    outlineVariant = AvalancheColors.Sand300,

    error = AvalancheColors.Error,
    onError = AvalancheColors.Sand50,
)

private val DarkColors = darkColorScheme(
    primary = AvalancheColors.Plum200,
    onPrimary = AvalancheColors.Plum900,
    primaryContainer = AvalancheColors.Plum700,
    onPrimaryContainer = AvalancheColors.Plum50,
    inversePrimary = AvalancheColors.Plum500,

    secondary = AvalancheColors.Plum300,
    onSecondary = AvalancheColors.Sand50,
    secondaryContainer = AvalancheColors.Plum700,
    onSecondaryContainer = AvalancheColors.Sand50,

    tertiary = AvalancheColors.Plum200,
    onTertiary = AvalancheColors.Plum900,
    tertiaryContainer = AvalancheColors.Plum700,
    onTertiaryContainer = AvalancheColors.Plum50,

    background = AvalancheColors.DarkSurface,
    onBackground = AvalancheColors.Sand50,

    surface = AvalancheColors.DarkSurface,
    onSurface = AvalancheColors.Sand50,
    surfaceVariant = AvalancheColors.Plum800,
    onSurfaceVariant = AvalancheColors.Sand300,
    surfaceTint = AvalancheColors.Plum200,

    inverseSurface = AvalancheColors.Sand100,
    inverseOnSurface = AvalancheColors.Ink,

    surfaceBright = AvalancheColors.Plum700,
    surfaceDim = AvalancheColors.Plum900,
    surfaceContainerLowest = AvalancheColors.Plum900,
    surfaceContainerLow = AvalancheColors.Plum900,
    surfaceContainer = AvalancheColors.Plum800,
    surfaceContainerHigh = AvalancheColors.Plum800,
    surfaceContainerHighest = AvalancheColors.Plum700,

    outline = AvalancheColors.Plum400,
    outlineVariant = AvalancheColors.Plum700,

    error = AvalancheColors.Rose400,
    onError = AvalancheColors.Plum900,
)

@Composable
fun AvalancheTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val colors = if (darkTheme) DarkColors else LightColors
    // Text-selection handles + the highlight behind selected text default to the
    // primary color; pin them to the brand plum (with a translucent highlight) so
    // selecting text in the composer doesn't flash the stock blue.
    val selectionColors = TextSelectionColors(
        handleColor = colors.primary,
        backgroundColor = colors.primary.copy(alpha = 0.4f),
    )
    MaterialTheme(
        colorScheme = colors,
        typography = Typography,
    ) {
        CompositionLocalProvider(
            LocalTextSelectionColors provides selectionColors,
            content = content,
        )
    }
}
