import UIKit

/// Turns a cropped square `UIImage` into the compact JPEG bytes app-core stores
/// as an avatar (docs/55). The Rust core caps the *plaintext* at
/// `MAX_AVATAR_BYTES` (60 KiB); we downscale and quality-search to land well
/// under that, so `setOwnAvatar` / `setGroupAvatar` never bounce with
/// `AvatarTooLarge`.
enum AvatarEncoder {
    /// Output square edge in pixels. 512 covers every current render surface
    /// (largest is 80 pt ≈ 240 px @3x) with headroom for a future larger view.
    static let dimension: CGFloat = 512
    /// Target ceiling for the encoded JPEG. Comfortably below the core's 60 KiB
    /// plaintext cap after the GCM nonce+tag.
    static let maxBytes = 48 * 1024

    /// Downscale `image` to a `dimension`-square (aspect-fill; a square input is
    /// just scaled) and JPEG-encode it under `maxBytes` by searching the quality
    /// knob. Returns `nil` only if encoding fails outright.
    static func encode(_ image: UIImage) -> Data? {
        let square = render(image, side: dimension)
        if let data = compress(square, maxBytes: maxBytes) {
            return data
        }
        // Extremely unlikely (a 512px JPEG under 48 KiB is easy), but if even the
        // lowest quality is too big, drop the resolution and try once more.
        let smaller = render(image, side: dimension * 0.66)
        return smaller.jpegData(compressionQuality: 0.5)
    }

    /// Aspect-fill `image` into a `side`×`side` opaque bitmap, normalizing
    /// orientation in the process.
    private static func render(_ image: UIImage, side: CGFloat) -> UIImage {
        let format = UIGraphicsImageRendererFormat.default()
        format.scale = 1
        format.opaque = true
        let renderer = UIGraphicsImageRenderer(size: CGSize(width: side, height: side), format: format)
        return renderer.image { _ in
            let fill = max(side / image.size.width, side / image.size.height)
            let drawSize = CGSize(width: image.size.width * fill, height: image.size.height * fill)
            let origin = CGPoint(x: (side - drawSize.width) / 2, y: (side - drawSize.height) / 2)
            image.draw(in: CGRect(origin: origin, size: drawSize))
        }
    }

    /// Binary-search JPEG quality for the largest data that fits `maxBytes`.
    private static func compress(_ image: UIImage, maxBytes: Int) -> Data? {
        if let hi = image.jpegData(compressionQuality: 0.9), hi.count <= maxBytes {
            return hi
        }
        var lo: CGFloat = 0.2
        var hi: CGFloat = 0.9
        var best: Data?
        for _ in 0..<7 {
            let mid = (lo + hi) / 2
            guard let data = image.jpegData(compressionQuality: mid) else { break }
            if data.count <= maxBytes {
                best = data
                lo = mid
            } else {
                hi = mid
            }
        }
        return best
    }
}
