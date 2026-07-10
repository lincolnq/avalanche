import SwiftUI
import PhotosUI

/// A tappable avatar with an edit affordance (docs/55). Renders `currentImage`
/// (or an initials placeholder) with a small camera badge; tapping offers Take
/// Photo / Choose from Library / Remove, then the circular `AvatarCropView`.
/// Reused for the user's own avatar (Settings, onboarding) and a group avatar.
struct EditableAvatar: View {
    let currentImage: Data?
    let placeholderName: String
    var size: CGFloat = 96
    var onPicked: (Data) -> Void
    var onRemove: (() -> Void)?

    @State private var showDialog = false
    @State private var showLibrary = false
    @State private var showCamera = false
    @State private var photosItem: PhotosPickerItem?
    @State private var cropItem: CropItem?

    var body: some View {
        Button { showDialog = true } label: {
            ContactAvatar(name: placeholderName, imageData: currentImage, size: size)
                .overlay(alignment: .bottomTrailing) { cameraBadge }
        }
        .buttonStyle(.plain)
        .confirmationDialog("Profile photo", isPresented: $showDialog, titleVisibility: .hidden) {
            Button("Take Photo") { showCamera = true }
            Button("Choose from Library") { showLibrary = true }
            if currentImage != nil, let onRemove {
                Button("Remove Photo", role: .destructive) { onRemove() }
            }
            Button("Cancel", role: .cancel) {}
        }
        .photosPicker(isPresented: $showLibrary, selection: $photosItem, matching: .images)
        .onChange(of: photosItem) { _, item in
            guard let item else { return }
            Task {
                let loaded = try? await item.loadTransferable(type: Data.self)
                await MainActor.run {
                    if let data = loaded, let ui = UIImage(data: data) {
                        cropItem = CropItem(image: ui)
                    }
                    photosItem = nil
                }
            }
        }
        .fullScreenCover(isPresented: $showCamera) {
            CameraPicker { ui in cropItem = CropItem(image: ui) }
                .ignoresSafeArea()
        }
        .fullScreenCover(item: $cropItem) { item in
            AvatarCropView(image: item.image, onCancel: { cropItem = nil }) { cropped in
                cropItem = nil
                if let data = AvatarEncoder.encode(cropped) { onPicked(data) }
            }
        }
    }

    private var cameraBadge: some View {
        Image(systemName: "camera.fill")
            .font(.system(size: size * 0.16, weight: .semibold))
            .foregroundStyle(Color.avPaper)
            .frame(width: size * 0.34, height: size * 0.34)
            .background(Circle().fill(Color.avBrand))
            .overlay(Circle().stroke(Color.avPaper, lineWidth: 2))
    }
}

/// `UIImage` isn't `Identifiable`; wrap it so `fullScreenCover(item:)` can drive
/// the cropper and reset cleanly between picks.
private struct CropItem: Identifiable {
    let id = UUID()
    let image: UIImage
}

/// Thin wrapper over `UIImagePickerController` for camera capture — SwiftUI has
/// no native camera control. Falls back to the photo library when no camera is
/// available (e.g. the simulator) so the flow still works there.
private struct CameraPicker: UIViewControllerRepresentable {
    var onImage: (UIImage) -> Void
    @Environment(\.dismiss) private var dismiss

    func makeUIViewController(context: Context) -> UIImagePickerController {
        let picker = UIImagePickerController()
        picker.sourceType = UIImagePickerController.isSourceTypeAvailable(.camera) ? .camera : .photoLibrary
        picker.allowsEditing = false
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ picker: UIImagePickerController, context: Context) {}

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    final class Coordinator: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
        let parent: CameraPicker
        init(_ parent: CameraPicker) { self.parent = parent }

        func imagePickerController(
            _ picker: UIImagePickerController,
            didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]
        ) {
            if let image = info[.originalImage] as? UIImage { parent.onImage(image) }
            parent.dismiss()
        }

        func imagePickerControllerDidCancel(_ picker: UIImagePickerController) {
            parent.dismiss()
        }
    }
}
