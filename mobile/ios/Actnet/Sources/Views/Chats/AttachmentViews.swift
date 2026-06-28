import SwiftUI
import UIKit
import ImageIO

/// Decode `data` into a `UIImage` downsampled so its largest side is at most
/// `maxPixel` pixels, via ImageIO — the full-resolution bitmap is never
/// materialized (a 4000px photo shown at 240pt must not decode at full size).
/// `nonisolated` so callers can run it off the main actor once and cache the
/// result, instead of re-decoding inside a SwiftUI `body` on every re-render
/// (which is what made typing in image-heavy conversations janky).
nonisolated func decodeDownsampledImage(_ data: Data, maxPixel: CGFloat) -> UIImage? {
    let srcOpts = [kCGImageSourceShouldCache: false] as CFDictionary
    guard let src = CGImageSourceCreateWithData(data as CFData, srcOpts) else { return nil }
    let opts: [CFString: Any] = [
        kCGImageSourceCreateThumbnailFromImageAlways: true,
        kCGImageSourceCreateThumbnailWithTransform: true,
        kCGImageSourceShouldCacheImmediately: true,
        kCGImageSourceThumbnailMaxPixelSize: maxPixel,
    ]
    guard let cg = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary) else { return nil }
    return UIImage(cgImage: cg)
}

/// Renders a single message attachment (docs/35-attachments.md) inside a
/// message bubble. Images show the inline thumbnail immediately, then load the
/// full-resolution blob (the inline thumbnail is a fast placeholder so the chat
/// scrolls without waiting on a download). Non-image attachments render as a
/// file chip.
struct AttachmentView: View {
    let attachment: AttachmentFfi
    /// Downloads (or loads the cached) decrypted bytes for this attachment.
    let loader: (AttachmentFfi) async -> Data?

    /// Decoded images, cached so `body` never re-decodes. `thumbImage` is the
    /// inline placeholder; `fullImage` replaces it once the blob downloads.
    @State private var fullImage: UIImage?
    @State private var thumbImage: UIImage?
    @State private var loading = false

    private var isImage: Bool { attachment.contentType.hasPrefix("image/") }

    var body: some View {
        if isImage {
            imageView
        } else {
            fileChip
        }
    }

    @ViewBuilder
    private var imageView: some View {
        Group {
            if let fullImage {
                Image(uiImage: fullImage).resizable().scaledToFit()
            } else if let thumbImage {
                // Inline thumbnail placeholder while the full blob loads.
                ZStack {
                    Image(uiImage: thumbImage).resizable().scaledToFit().blur(radius: 2)
                    if loading { ProgressView() }
                }
            } else {
                ZStack {
                    Rectangle().fill(Color.avMuted.opacity(0.2))
                    if loading { ProgressView() } else { Image(systemName: "photo") }
                }
                .aspectRatio(4.0 / 3.0, contentMode: .fit)
            }
        }
        .frame(maxWidth: 240, maxHeight: 320)
        .clipShape(RoundedRectangle(cornerRadius: 14))
        .task(id: attachment.id) { await load() }
    }

    private var fileChip: some View {
        HStack(spacing: 10) {
            Image(systemName: "doc.fill").font(.title2)
            VStack(alignment: .leading, spacing: 2) {
                Text(attachment.fileName ?? "Attachment").font(.subheadline).lineLimit(1)
                Text(byteSize(attachment.sizeBytes)).font(.caption).foregroundStyle(.secondary)
            }
            if loading { ProgressView() }
        }
        .padding(10)
        .background(Color.avMuted.opacity(0.15))
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .onTapGesture { Task { await load() } }
    }

    private func load() async {
        // Decode the inline thumbnail once for an instant placeholder.
        if isImage, thumbImage == nil, !attachment.thumbnail.isEmpty {
            let thumb = attachment.thumbnail
            thumbImage = await Task.detached(priority: .userInitiated) {
                decodeDownsampledImage(thumb, maxPixel: 960)
            }.value
        }
        guard fullImage == nil, !loading else { return }
        loading = true
        defer { loading = false }
        guard let data = await loader(attachment) else { return }
        if isImage {
            // Decode + downsample off the main thread, then cache — so a render
            // pass (e.g. per keystroke in the composer) never touches the bitmap.
            fullImage = await Task.detached(priority: .userInitiated) {
                decodeDownsampledImage(data, maxPixel: 960)
            }.value
        }
    }

    private func byteSize(_ n: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: n, countStyle: .file)
    }
}

/// Downscale image `data` to a small inline preview JPEG plus its pixel
/// dimensions, for the attachment pointer's `thumbnail`/`width`/`height`
/// (docs/35). Returns an empty thumbnail and zero dimensions if `data` isn't a
/// decodable image.
func makeAttachmentThumbnail(_ data: Data, maxDimension: CGFloat = 320) -> (thumbnail: Data, width: Int32, height: Int32) {
    guard let image = UIImage(data: data) else { return (Data(), 0, 0) }
    let size = image.size
    let scale = min(1, maxDimension / max(size.width, size.height))
    let target = CGSize(width: size.width * scale, height: size.height * scale)
    let renderer = UIGraphicsImageRenderer(size: target)
    let thumb = renderer.image { _ in image.draw(in: CGRect(origin: .zero, size: target)) }
    let jpeg = thumb.jpegData(compressionQuality: 0.6) ?? Data()
    return (jpeg, Int32(size.width), Int32(size.height))
}

/// Prepare raw image `data` for sending (docs/35): decode, then re-encode upright,
/// resolution-capped, and metadata-stripped via `UIImage.preparedForSending` —
/// matching Signal's outgoing-image policy. Falls back to the original bytes only
/// if the data can't be decoded. Used by the photo-picker path; paste and the
/// share extension call `preparedForSending` on their `UIImage` directly.
func prepareImageForSending(_ data: Data) -> Data {
    guard let image = UIImage(data: data) else { return data }
    return image.preparedForSending() ?? data
}

/// A rich link-preview card (docs/35 "Link previews"): the og:image (if any) on
/// top, then the title and source domain. Tapping opens the URL. The image is a
/// normal attachment, downloaded via the same `loader` as message attachments.
struct LinkPreviewCard: View {
    let preview: LinkPreviewFfi
    let isMe: Bool
    let loader: (AttachmentFfi) async -> Data?

    @State private var image: UIImage?
    @Environment(\.openURL) private var openURL

    private var domain: String {
        guard let host = URL(string: preview.url)?.host else { return preview.url }
        return host.hasPrefix("www.") ? String(host.dropFirst(4)) : host
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let image {
                Image(uiImage: image)
                    .resizable()
                    .scaledToFill()
                    .frame(maxWidth: 260, maxHeight: 140)
                    .clipped()
            }
            VStack(alignment: .leading, spacing: 2) {
                if !preview.title.isEmpty {
                    Text(preview.title)
                        .font(.subheadline.weight(.semibold))
                        .lineLimit(2)
                }
                Text(domain)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(10)
        }
        .frame(maxWidth: 260, alignment: .leading)
        .background(isMe ? Color.avOutgoingBubble.opacity(0.6) : Color.avIncomingBubble)
        .clipShape(RoundedRectangle(cornerRadius: 14))
        .overlay(
            RoundedRectangle(cornerRadius: 14)
                .strokeBorder(Color.avMuted.opacity(0.25), lineWidth: 1)
        )
        .contentShape(Rectangle())
        .onTapGesture { if let url = URL(string: preview.url) { openURL(url) } }
        .task(id: preview.url) {
            if image == nil, let imageAttachment = preview.image, let data = await loader(imageAttachment) {
                image = await Task.detached(priority: .userInitiated) {
                    decodeDownsampledImage(data, maxPixel: 520)
                }.value
            }
        }
    }
}
