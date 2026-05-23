import SwiftUI

extension Color {
    // MARK: - Plum (primary brand scale)
    static let plum50  = Color(red: 0.945, green: 0.922, blue: 0.929) // #F1EBED
    static let plum100 = Color(red: 0.863, green: 0.784, blue: 0.808) // #DCC8CE
    static let plum200 = Color(red: 0.784, green: 0.706, blue: 0.741) // #C8B4BD
    static let plum300 = Color(red: 0.694, green: 0.584, blue: 0.624) // #B1959F
    static let plum400 = Color(red: 0.612, green: 0.463, blue: 0.510) // #9C7682
    static let plum500 = Color(red: 0.420, green: 0.243, blue: 0.314) // #6B3E50
    static let plum600 = Color(red: 0.361, green: 0.204, blue: 0.271) // #5C3445
    static let plum700 = Color(red: 0.302, green: 0.161, blue: 0.224) // #4D2939
    static let plum800 = Color(red: 0.235, green: 0.122, blue: 0.176) // #3C1F2D
    static let plum900 = Color(red: 0.165, green: 0.086, blue: 0.125) // #2A1620

    // MARK: - Sand (warm neutral)
    static let sand50  = Color(red: 1.000, green: 0.973, blue: 0.945) // #FFF8F1
    static let sand100 = Color(red: 1.000, green: 0.945, blue: 0.914) // #FFF1E9
    static let sand200 = Color(red: 0.957, green: 0.906, blue: 0.851) // #F4E7D9
    static let sand300 = Color(red: 0.898, green: 0.831, blue: 0.753) // #E5D4C0
    static let sand400 = Color(red: 0.780, green: 0.714, blue: 0.627) // #C7B6A0
    static let sand500 = Color(red: 0.604, green: 0.541, blue: 0.471) // #9A8A78
    static let sand600 = Color(red: 0.431, green: 0.384, blue: 0.345) // #6E6258
    static let sand900 = Color(red: 0.122, green: 0.094, blue: 0.082) // #1F1815

    // MARK: - Accents (reserved, status only)
    static let rose400 = Color(red: 0.820, green: 0.459, blue: 0.475) // #D17579
    static let rose500 = Color(red: 0.780, green: 0.408, blue: 0.439) // #C76870
    static let rose700 = Color(red: 0.600, green: 0.243, blue: 0.278) // #993E47
    static let moss    = Color(red: 0.420, green: 0.498, blue: 0.302) // #6B7F4D
    static let amber   = Color(red: 0.831, green: 0.627, blue: 0.290) // #D4A04A

    // MARK: - Semantic aliases
    static let avBrand          = Color.plum500
    static let avPaper          = Color.sand100
    static let avInk            = Color.sand900
    static let avMuted          = Color.sand600
    static let avOutgoingBubble = Color.plum500
    static let avIncomingBubble = Color.sand200
    static let avDarkSurface    = Color.plum900
    static let avError          = Color.rose700
    static let avNotification   = Color.rose500
    static let avSuccess        = Color.moss
    static let avWarning        = Color.amber
}
