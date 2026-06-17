import SwiftUI

/// Avatar for a contact (someone other than the local user). Shows the
/// contact's photo override if one is set; otherwise initials on a tinted
/// shape, matching `AccountAvatar`.
///
/// `imageData` is reserved for the per-contact `photo_override` field
/// (docs/52-contacts-and-profiles.md) and is `nil` until that feature lands —
/// callers can wire it through with no further change here.
///
/// When `isBot` is set, the avatar renders in a hexagon instead of a circle
/// (docs/54-bot-presentation.md): bots are visually distinct from people at a
/// glance. The frame is client-applied chrome over whatever image the account
/// supplies — branded or default — so the avatar bytes can't forge or undo the
/// signal.
struct ContactAvatar: View {
    let name: String
    var imageData: Data? = nil
    var isBot: Bool = false
    let size: CGFloat

    var body: some View {
        avatarContent
            .frame(width: size, height: size)
    }

    @ViewBuilder
    private var avatarContent: some View {
        let color = Color.avBrand
        if let data = imageData, let uiImage = UIImage(data: data) {
            Image(uiImage: uiImage)
                .resizable()
                .scaledToFill()
                .frame(width: size, height: size)
                .clipShape(frameShape)
        } else {
            frameShape
                .fill(color.opacity(0.2))
                .overlay {
                    Text(initial)
                        .font(.system(size: size * 0.4, weight: .medium))
                        .foregroundColor(color)
                }
        }
    }

    /// Circle for people, hexagon for bots. Erased to `AnyShape` so it can be
    /// used both as the fill for the placeholder and as the clip for an image.
    private var frameShape: AnyShape {
        isBot ? AnyShape(Hexagon()) : AnyShape(Circle())
    }

    private var initial: String {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        return trimmed.isEmpty ? "?" : trimmed.prefix(1).uppercased()
    }
}
