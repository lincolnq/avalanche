import SwiftUI

struct RecoveryKeyBanner: View {
    @State private var dismissed = true

    var body: some View {
        if !dismissed {
            HStack {
                Image(systemName: "exclamationmark.shield")
                Text("Secure your account")
                    .font(.subheadline)
                Spacer()
                Button("Set up") {
                    // TODO: Navigate to recovery key setup
                }
                .font(.subheadline.bold())
                Button {
                    dismissed = true
                } label: {
                    Image(systemName: "xmark")
                        .font(.caption)
                }
            }
            .padding(.horizontal)
            .padding(.vertical, 10)
            .background(Color.avWarning.opacity(0.15))
        }
    }
}
