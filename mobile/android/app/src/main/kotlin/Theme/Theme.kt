package net.theavalanche.app

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable

// Fixed brand color scheme (no dynamic/Material You theming) so the app looks the
// same on every device, matching the iOS reference. Built from AvalancheColors.
private val LightColors = lightColorScheme(
    primary = AvalancheColors.Brand,
    onPrimary = AvalancheColors.Paper,
    secondary = AvalancheColors.Plum300,
    onSecondary = AvalancheColors.Ink,
    background = AvalancheColors.Paper,
    onBackground = AvalancheColors.Ink,
    surface = AvalancheColors.Paper,
    onSurface = AvalancheColors.Ink,
    error = AvalancheColors.Error,
)

private val DarkColors = darkColorScheme(
    primary = AvalancheColors.Plum200,
    onPrimary = AvalancheColors.Plum900,
    secondary = AvalancheColors.Plum300,
    onSecondary = AvalancheColors.Sand50,
    background = AvalancheColors.DarkSurface,
    onBackground = AvalancheColors.Sand50,
    surface = AvalancheColors.DarkSurface,
    onSurface = AvalancheColors.Sand50,
    error = AvalancheColors.Rose400,
)

@Composable
fun AvalancheTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = if (darkTheme) DarkColors else LightColors,
        typography = Typography,
        content = content,
    )
}
