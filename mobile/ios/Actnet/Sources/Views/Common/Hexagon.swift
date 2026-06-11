import SwiftUI

/// A regular pointy-top hexagon with softly rounded corners, sized to the
/// shorter dimension of its frame and centered. This is the bot avatar frame
/// (docs/54-bot-presentation.md): people render in a `Circle`, bots in a
/// `Hexagon`, so an automated participant is distinguishable from a person at
/// a glance. The shape is applied by the client over whatever image the
/// account supplies, so the bytes can't undo it.
struct Hexagon: Shape {
    /// Corner rounding as a fraction of the circumradius. ~0.18 reads as a
    /// hexagon without the sharp points looking like a "missing photo" glyph.
    var cornerRadiusFraction: CGFloat = 0.18

    func path(in rect: CGRect) -> Path {
        let center = CGPoint(x: rect.midX, y: rect.midY)
        let radius = min(rect.width, rect.height) / 2
        let corner = radius * cornerRadiusFraction

        // Six vertices, pointy-top (first vertex straight up).
        let vertices: [CGPoint] = (0..<6).map { i in
            let angle = Double.pi / 3 * Double(i) - Double.pi / 2
            return CGPoint(
                x: center.x + radius * CGFloat(cos(angle)),
                y: center.y + radius * CGFloat(sin(angle))
            )
        }

        var path = Path()
        for i in 0..<vertices.count {
            let current = vertices[i]
            let previous = vertices[(i + vertices.count - 1) % vertices.count]
            let next = vertices[(i + 1) % vertices.count]

            // Pull back from each vertex along both incident edges, then round
            // the corner with a quadratic curve through the original vertex.
            let toPrev = unitVector(from: current, to: previous)
            let toNext = unitVector(from: current, to: next)
            let start = CGPoint(x: current.x + toPrev.x * corner, y: current.y + toPrev.y * corner)
            let end = CGPoint(x: current.x + toNext.x * corner, y: current.y + toNext.y * corner)

            if i == 0 {
                path.move(to: start)
            } else {
                path.addLine(to: start)
            }
            path.addQuadCurve(to: end, control: current)
        }
        path.closeSubpath()
        return path
    }

    private func unitVector(from a: CGPoint, to b: CGPoint) -> CGPoint {
        let dx = b.x - a.x
        let dy = b.y - a.y
        let length = max(CGFloat(sqrt(dx * dx + dy * dy)), 0.0001)
        return CGPoint(x: dx / length, y: dy / length)
    }
}
