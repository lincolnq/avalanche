package net.theavalanche.app

import androidx.compose.ui.graphics.Color

// Brand palette. Mirrors mobile/ios/Actnet/Sources/Utils/AvalancheColors.swift —
// keep the two in sync (cross-platform parity rule).
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
