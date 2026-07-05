import SwiftUI
import UIKit

/// Shared cache of full-resolution decoded images for the viewer, keyed by
/// attachment id. Without it, paging back/forth (TabView disposes off-screen
/// pages) and reopening the viewer re-read bytes from disk and re-decode at full
/// resolution every time — the source of the "spends a while loading" lag. Mirrors
/// Android's `MessageImageCache` (viewer-keyed). NSCache is thread-safe and evicts
/// under memory pressure.
private enum ViewerImageCache {
    // NSCache is documented thread-safe; `nonisolated(unsafe)` opts it out of the
    // Sendable check (it has no unsafe shared mutable state of our own).
    nonisolated(unsafe) static let shared = NSCache<NSString, UIImage>()
    static func get(_ id: String) -> UIImage? {
        id.isEmpty ? nil : shared.object(forKey: id as NSString)
    }
    static func set(_ id: String, _ image: UIImage) {
        guard !id.isEmpty else { return }
        shared.setObject(image, forKey: id as NSString)
    }
}

/// Fullscreen image viewer (docs/35): pages between every image attachment in a
/// conversation in timeline order, with pinch-to-zoom + pan per image. Swiping
/// left/right pages only while the current image is unzoomed; once zoomed, drags
/// pan within the image. Swipe down (when unzoomed) or the close button dismiss.
///
/// Paging is a `TabView(.page)` (UIPageViewController-backed); each page is a
/// `ZoomableScrollView` wrapping a `UIScrollView`. Gesture arbitration between the
/// pager and the inner scroll views yields the "page-when-unzoomed / pan-when-
/// zoomed" behavior natively (the standard iOS photo-browser pattern).
struct ImageViewerView: View {
    let images: [AttachmentFfi]
    let startId: String
    let loader: (AttachmentFfi) async -> Data?

    @Environment(\.dismiss) private var dismiss
    @State private var selection: Int
    /// Vertical offset driven by the swipe-down-to-dismiss gesture.
    @State private var dragOffset: CGFloat = 0

    init(images: [AttachmentFfi], startId: String, loader: @escaping (AttachmentFfi) async -> Data?) {
        self.images = images
        self.startId = startId
        self.loader = loader
        // Seed the paged selection to the tapped image up front. Setting it later
        // (e.g. in `.onAppear`) doesn't reliably move a `.page`-style TabView.
        _selection = State(initialValue: images.firstIndex { $0.id == startId } ?? 0)
    }

    var body: some View {
        ZStack(alignment: .top) {
            Color.black.opacity(backdropOpacity).ignoresSafeArea()

            TabView(selection: $selection) {
                ForEach(Array(images.enumerated()), id: \.offset) { idx, att in
                    ZoomableImagePage(
                        attachment: att,
                        loader: loader,
                        onDismissDrag: { dragOffset = $0 },
                        onDismissEnd: { translation, velocity in endDrag(translation, velocity) }
                    )
                    .tag(idx)
                }
            }
            .tabViewStyle(.page(indexDisplayMode: .never))
            .offset(y: dragOffset)
            .ignoresSafeArea()

            chrome
        }
        .statusBarHidden()
    }

    private var chrome: some View {
        HStack {
            Button { dismiss() } label: {
                Image(systemName: "xmark")
                    .font(.headline.weight(.bold))
                    .foregroundStyle(.white)
                    .frame(width: 40, height: 40)
                    .background(.black.opacity(0.45), in: Circle())
            }
            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.top, 8)
        // Fade the chrome out together with the backdrop during a dismiss drag.
        .opacity(backdropOpacity)
    }

    /// Fade the black backdrop (and chrome) as the user drags down to dismiss.
    private var backdropOpacity: Double { max(0, 1 - Double(abs(dragOffset)) / 500) }

    private func endDrag(_ translation: CGFloat, _ velocity: CGFloat) {
        if abs(translation) > 120 || abs(velocity) > 800 {
            dismiss()
        } else {
            withAnimation(.interactiveSpring) { dragOffset = 0 }
        }
    }
}

/// One page of the viewer: loads + decodes the full-resolution image (falling back
/// to the inline thumbnail as a blurred placeholder) then hands it to the zoomable
/// scroll view.
private struct ZoomableImagePage: View {
    let attachment: AttachmentFfi
    let loader: (AttachmentFfi) async -> Data?
    let onDismissDrag: (CGFloat) -> Void
    let onDismissEnd: (CGFloat, CGFloat) -> Void

    @State private var image: UIImage?
    @State private var thumbImage: UIImage?

    init(
        attachment: AttachmentFfi,
        loader: @escaping (AttachmentFfi) async -> Data?,
        onDismissDrag: @escaping (CGFloat) -> Void,
        onDismissEnd: @escaping (CGFloat, CGFloat) -> Void
    ) {
        self.attachment = attachment
        self.loader = loader
        self.onDismissDrag = onDismissDrag
        self.onDismissEnd = onDismissEnd
        // Seed from the cache so a cached image shows instantly — no spinner,
        // no re-decode — when paging back or reopening.
        _image = State(initialValue: ViewerImageCache.get(attachment.id))
    }

    var body: some View {
        ZStack {
            if let image {
                ZoomableScrollView(
                    image: image,
                    onDismissDrag: onDismissDrag,
                    onDismissEnd: onDismissEnd
                )
            } else {
                if let thumbImage {
                    Image(uiImage: thumbImage).resizable().scaledToFit().blur(radius: 6)
                }
                ProgressView().tint(.white)
            }
        }
        .task(id: attachment.id) { await load() }
    }

    private func load() async {
        // Already have the full image (seeded from cache) — nothing to do.
        guard image == nil else { return }
        if thumbImage == nil, !attachment.thumbnail.isEmpty {
            let thumb = attachment.thumbnail
            thumbImage = await Task.detached(priority: .userInitiated) {
                decodeDownsampledImage(thumb, maxPixel: 960)
            }.value
        }
        guard let data = await loader(attachment) else { return }
        // Decode at a high cap so zoom stays sharp. Outgoing images are capped at
        // 2048px, so this is effectively full-res; the cap bounds memory for
        // larger inbound images.
        let decoded = await Task.detached(priority: .userInitiated) {
            decodeDownsampledImage(data, maxPixel: 4096)
        }.value
        if let decoded { ViewerImageCache.set(attachment.id, decoded) }
        image = decoded
    }
}

/// A single zoomable image backed by `UIScrollView` — pinch + double-tap to zoom,
/// pan when zoomed. Also owns the swipe-down-to-dismiss pan, which only engages
/// when unzoomed and predominantly vertical (so horizontal swipes still page and
/// zoomed pans still pan).
private struct ZoomableScrollView: UIViewRepresentable {
    let image: UIImage
    let onDismissDrag: (CGFloat) -> Void
    let onDismissEnd: (CGFloat, CGFloat) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(onDismissDrag: onDismissDrag, onDismissEnd: onDismissEnd)
    }

    func makeUIView(context: Context) -> UIScrollView {
        let scrollView = LayoutObservingScrollView()
        scrollView.onLayout = { [weak coordinator = context.coordinator] in coordinator?.layoutImage() }
        scrollView.delegate = context.coordinator
        scrollView.showsHorizontalScrollIndicator = false
        scrollView.showsVerticalScrollIndicator = false
        scrollView.backgroundColor = .clear
        scrollView.contentInsetAdjustmentBehavior = .never
        scrollView.decelerationRate = .fast

        let imageView = UIImageView(image: image)
        imageView.contentMode = .scaleAspectFit
        imageView.isUserInteractionEnabled = true
        scrollView.addSubview(imageView)
        context.coordinator.scrollView = scrollView
        context.coordinator.imageView = imageView

        let doubleTap = UITapGestureRecognizer(
            target: context.coordinator, action: #selector(Coordinator.handleDoubleTap(_:)))
        doubleTap.numberOfTapsRequired = 2
        scrollView.addGestureRecognizer(doubleTap)

        let dismissPan = UIPanGestureRecognizer(
            target: context.coordinator, action: #selector(Coordinator.handleDismissPan(_:)))
        dismissPan.delegate = context.coordinator
        scrollView.addGestureRecognizer(dismissPan)

        return scrollView
    }

    func updateUIView(_ scrollView: UIScrollView, context: Context) {
        if context.coordinator.imageView?.image !== image {
            context.coordinator.imageView?.image = image
        }
    }

    /// A `UIScrollView` that reports each layout pass, so the image can be sized +
    /// centered as soon as the view has real bounds (SwiftUI may hand us a
    /// zero-size frame on the first `updateUIView`).
    final class LayoutObservingScrollView: UIScrollView {
        var onLayout: (() -> Void)?
        override func layoutSubviews() {
            super.layoutSubviews()
            onLayout?()
        }
    }

    final class Coordinator: NSObject, UIScrollViewDelegate, UIGestureRecognizerDelegate {
        weak var scrollView: UIScrollView?
        weak var imageView: UIImageView?
        private let onDismissDrag: (CGFloat) -> Void
        private let onDismissEnd: (CGFloat, CGFloat) -> Void
        private var didLayout = false

        init(onDismissDrag: @escaping (CGFloat) -> Void, onDismissEnd: @escaping (CGFloat, CGFloat) -> Void) {
            self.onDismissDrag = onDismissDrag
            self.onDismissEnd = onDismissEnd
        }

        /// Size the image to its native pixels and pick a min zoom scale that fits
        /// the image within the scroll view; `maximumZoomScale` allows 5x beyond
        /// the fitted size.
        func layoutImage() {
            guard let scrollView, let imageView, let image = imageView.image else { return }
            let bounds = scrollView.bounds.size
            guard bounds.width > 0, bounds.height > 0, image.size.width > 0, image.size.height > 0 else { return }
            if didLayout { return }
            didLayout = true

            imageView.frame = CGRect(origin: .zero, size: image.size)
            scrollView.contentSize = image.size
            let minScale = min(bounds.width / image.size.width, bounds.height / image.size.height)
            scrollView.minimumZoomScale = minScale
            scrollView.maximumZoomScale = minScale * 5
            scrollView.zoomScale = minScale
            centerImage()
        }

        /// Keep the image centered when smaller than the viewport (contentInset).
        private func centerImage() {
            guard let scrollView, let imageView else { return }
            let bounds = scrollView.bounds.size
            let content = imageView.frame.size
            let insetX = max(0, (bounds.width - content.width) / 2)
            let insetY = max(0, (bounds.height - content.height) / 2)
            scrollView.contentInset = UIEdgeInsets(top: insetY, left: insetX, bottom: insetY, right: insetX)
        }

        func viewForZooming(in scrollView: UIScrollView) -> UIView? { imageView }
        func scrollViewDidZoom(_ scrollView: UIScrollView) { centerImage() }

        private var isZoomed: Bool {
            guard let scrollView else { return false }
            return scrollView.zoomScale > scrollView.minimumZoomScale + 0.01
        }

        @objc func handleDoubleTap(_ gr: UITapGestureRecognizer) {
            guard let scrollView, let imageView else { return }
            if isZoomed {
                scrollView.setZoomScale(scrollView.minimumZoomScale, animated: true)
            } else {
                let point = gr.location(in: imageView)
                let newScale = scrollView.minimumZoomScale * 3
                let width = scrollView.bounds.width / newScale
                let height = scrollView.bounds.height / newScale
                scrollView.zoom(to: CGRect(x: point.x - width / 2, y: point.y - height / 2,
                                           width: width, height: height), animated: true)
            }
        }

        @objc func handleDismissPan(_ gr: UIPanGestureRecognizer) {
            guard let scrollView else { return }
            let translation = gr.translation(in: scrollView)
            let velocity = gr.velocity(in: scrollView)
            switch gr.state {
            case .began, .changed: onDismissDrag(translation.y)
            case .ended, .cancelled: onDismissEnd(translation.y, velocity.y)
            default: break
            }
        }

        // Only start the dismiss pan when unzoomed and the drag is mostly vertical
        // — so horizontal swipes reach the pager and zoomed pans reach the scroll
        // view.
        func gestureRecognizerShouldBegin(_ gr: UIGestureRecognizer) -> Bool {
            guard let pan = gr as? UIPanGestureRecognizer, let scrollView else { return true }
            guard !isZoomed else { return false }
            let velocity = pan.velocity(in: scrollView)
            return abs(velocity.y) > abs(velocity.x)
        }

        func gestureRecognizer(_ gr: UIGestureRecognizer,
                               shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool { true }
    }
}
