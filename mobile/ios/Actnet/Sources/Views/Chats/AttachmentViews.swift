import SwiftUI
import UIKit

/// Renders a single message attachment (docs/35-attachments.md) inside a
/// message bubble. Images show the inline thumbnail immediately, then load the
/// full-resolution blob (the inline thumbnail is a fast placeholder so the chat
/// scrolls without waiting on a download). Non-image attachments render as a
/// file chip.
struct AttachmentView: View {
    let attachment: AttachmentFfi
    /// Downloads (or loads the cached) decrypted bytes for this attachment.
    let loader: (AttachmentFfi) async -> Data?

    @State private var fullData: Data?
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
            if let fullData, let img = UIImage(data: fullData) {
                Image(uiImage: img).resizable().scaledToFit()
            } else if let img = UIImage(data: attachment.thumbnail) {
                // Inline thumbnail placeholder while the full blob loads.
                ZStack {
                    Image(uiImage: img).resizable().scaledToFit().blur(radius: 2)
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
        guard fullData == nil, !loading else { return }
        loading = true
        fullData = await loader(attachment)
        loading = false
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
