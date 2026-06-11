import SwiftUI

/// A rectangle with chamfered (45°-cut) corners — an octagon when the rect is
/// square, a beveled rectangle at any other aspect ratio. This is the bot
/// message-bubble shape (docs/54-bot-presentation.md): bot bubbles read as
/// angular/geometric, echoing the `Hexagon` avatar frame, while people's
/// bubbles stay rounded.
///
struct CutCornerRectangle: Shape {
    var cut: CGFloat = 12

    func path(in rect: CGRect) -> Path {
        let c = min(cut, min(rect.width, rect.height) / 2)
        var path = Path()
        path.move(to: CGPoint(x: rect.minX + c, y: rect.minY))
        path.addLine(to: CGPoint(x: rect.maxX - c, y: rect.minY))
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.minY + c))
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY - c))
        path.addLine(to: CGPoint(x: rect.maxX - c, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.minX + c, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.minX, y: rect.maxY - c))
        path.addLine(to: CGPoint(x: rect.minX, y: rect.minY + c))
        path.closeSubpath()
        return path
    }
}
