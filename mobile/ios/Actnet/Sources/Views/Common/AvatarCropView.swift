import SwiftUI

/// Full-screen circular-crop editor (docs/55). The user pans and pinch-zooms the
/// picked image inside a square viewport with a circular mask; "Use Photo"
/// snapshots exactly what's framed and hands back a square `UIImage` for
/// `AvatarEncoder` to compress. Presented as a sheet from `EditableAvatar`.
struct AvatarCropView: View {
    let image: UIImage
    var onCancel: () -> Void
    var onUse: (UIImage) -> Void

    @State private var scale: CGFloat = 1
    @GestureState private var pinch: CGFloat = 1
    @State private var offset: CGSize = .zero
    @GestureState private var drag: CGSize = .zero

    var body: some View {
        GeometryReader { geo in
            let side = min(geo.size.width - 32, geo.size.height - 220)
            ZStack {
                Color.black.ignoresSafeArea()
                VStack(spacing: 20) {
                    Spacer(minLength: 0)
                    cropArea(side: side)
                    Text("Drag and pinch to adjust")
                        .font(.footnote)
                        .foregroundStyle(.white.opacity(0.7))
                    Spacer(minLength: 0)
                    buttons(side: side)
                }
                .padding(.vertical, 24)
                .frame(maxWidth: .infinity)
            }
        }
    }

    // The framed square: the transformed image, a circular dimming mask, and a
    // ring indicating the crop boundary.
    private func cropArea(side: CGFloat) -> some View {
        imageLayer(side: side, live: true)
            .overlay(circularDimming(side: side))
            .overlay(
                Circle()
                    .strokeBorder(.white.opacity(0.85), lineWidth: 2)
                    .frame(width: side, height: side)
            )
            .gesture(
                SimultaneousGesture(
                    MagnificationGesture()
                        .updating($pinch) { value, state, _ in state = value }
                        .onEnded { value in scale = max(1, min(scale * value, 6)) },
                    DragGesture()
                        .updating($drag) { value, state, _ in state = value.translation }
                        .onEnded { value in
                            offset = CGSize(width: offset.width + value.translation.width,
                                            height: offset.height + value.translation.height)
                        }
                )
            )
    }

    /// The image aspect-filled into the square with the current zoom/pan applied.
    /// `live` includes the in-flight gesture deltas; the snapshot passes `false`.
    private func imageLayer(side: CGFloat, live: Bool) -> some View {
        let s = live ? scale * pinch : scale
        let o = live
            ? CGSize(width: offset.width + drag.width, height: offset.height + drag.height)
            : offset
        return Image(uiImage: image)
            .resizable()
            .scaledToFill()
            .frame(width: side, height: side)
            .scaleEffect(s)
            .offset(o)
            .frame(width: side, height: side)
            .clipped()
            .contentShape(Rectangle())
    }

    /// Dims everything outside the crop circle.
    private func circularDimming(side: CGFloat) -> some View {
        Color.black.opacity(0.45)
            .mask {
                Rectangle()
                    .overlay(Circle().frame(width: side, height: side).blendMode(.destinationOut))
                    .compositingGroup()
            }
            .allowsHitTesting(false)
    }

    private func buttons(side: CGFloat) -> some View {
        HStack {
            Button("Cancel", role: .cancel) { onCancel() }
                .foregroundStyle(.white)
            Spacer()
            Button {
                if let cropped = snapshot(side: side) { onUse(cropped) }
            } label: {
                Text("Use Photo").fontWeight(.semibold)
            }
            .buttonStyle(.borderedProminent)
        }
        .padding(.horizontal, 24)
    }

    /// Render the framed square to a ~`AvatarEncoder.dimension`px image.
    private func snapshot(side: CGFloat) -> UIImage? {
        let content = imageLayer(side: side, live: false)
        let renderer = ImageRenderer(content: content)
        renderer.scale = max(1, AvatarEncoder.dimension / side)
        renderer.isOpaque = true
        return renderer.uiImage
    }
}
