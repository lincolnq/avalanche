import SwiftUI
import CoreImage.CIFilterBuiltins

/// Renders a string as a QR code image, themed to match the app. Used by the
/// invite and device-linking flows. Falls back to empty space if encoding
/// fails (e.g. the payload is too large for a single QR symbol).
struct QRCodeView: View {
    let text: String
    var size: CGFloat = 220

    var body: some View {
        if let image = Self.generate(from: text) {
            Image(uiImage: image)
                .interpolation(.none)
                .resizable()
                .scaledToFit()
                .frame(width: size, height: size)
        } else {
            Color.clear.frame(width: size, height: size)
        }
    }

    /// Encode `string` into a themed QR `UIImage`. Mirrors the invite-QR
    /// rendering in `IdentityDetailView`.
    static func generate(from string: String) -> UIImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(string.utf8)
        filter.correctionLevel = "M"
        guard let ciImage = filter.outputImage else { return nil }
        let falseColor = CIFilter.falseColor()
        falseColor.inputImage = ciImage
        falseColor.color0 = CIColor(color: UIColor(Color.plum800))
        falseColor.color1 = CIColor(color: UIColor(Color.avPaper))
        guard let colored = falseColor.outputImage else { return nil }
        let scaled = colored.transformed(by: CGAffineTransform(scaleX: 10, y: 10))
        let context = CIContext()
        guard let cgImage = context.createCGImage(scaled, from: scaled.extent) else { return nil }
        return UIImage(cgImage: cgImage)
    }
}
