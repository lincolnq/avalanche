import SwiftUI

struct AccountAvatar: View {
    let account: Account
    let size: CGFloat

    var body: some View {
        if let data = account.avatarData, let uiImage = UIImage(data: data) {
            Image(uiImage: uiImage)
                .resizable()
                .scaledToFill()
                .frame(width: size, height: size)
                .clipShape(Circle())
        } else {
            let color = Color.avBrand
            Circle()
                .fill(color.opacity(0.2))
                .frame(width: size, height: size)
                .overlay {
                    Text(account.displayName.prefix(1).uppercased())
                        .font(.system(size: size * 0.4, weight: .medium))
                        .foregroundColor(color)
                }
        }
    }
}
