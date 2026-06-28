import UIKit

/// Outgoing-image processing policy (docs/35). Like Signal, we re-encode every
/// sent image rather than shipping the original: this bakes in EXIF orientation,
/// caps the resolution, and strips EXIF/metadata (GPS, device, timestamps) as a
/// side effect of the re-encode. Single quality tier for now — tune here.
/// (Signal's "standard" tier is ~1600px longest edge; we allow a bit more.)
enum OutgoingImage {
    /// Longest-edge cap, in pixels.
    static let maxDimension: CGFloat = 2048
    static let jpegQuality: CGFloat = 0.9
}

extension UIImage {
    /// Prepare for sending (docs/35): redraw upright, downscale so the longest edge
    /// is at most `maxDimension`, and JPEG-encode — which drops the source's
    /// EXIF/metadata. Returns nil only if encoding fails.
    func preparedForSending(
        maxDimension: CGFloat = OutgoingImage.maxDimension,
        quality: CGFloat = OutgoingImage.jpegQuality
    ) -> Data? {
        let longest = max(size.width, size.height)
        let scale = longest > 0 ? min(1, maxDimension / longest) : 1
        let target = CGSize(
            width: (size.width * scale).rounded(),
            height: (size.height * scale).rounded()
        )
        let format = UIGraphicsImageRendererFormat.default()
        format.scale = 1          // target size is already in pixels
        format.opaque = true      // JPEG has no alpha
        // `draw` honors imageOrientation, so the rendered pixels are upright.
        let rendered = UIGraphicsImageRenderer(size: target, format: format).image { _ in
            draw(in: CGRect(origin: .zero, size: target))
        }
        return rendered.jpegData(compressionQuality: quality)
    }
}
