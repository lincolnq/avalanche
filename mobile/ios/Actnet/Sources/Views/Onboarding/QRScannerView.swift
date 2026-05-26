import SwiftUI

struct QRScannerView: View {
    /// Optional override. When set, the scanned string is handed off to the
    /// caller instead of routing to the onboarding identity picker.
    var onScanned: ((String) -> Void)? = nil

    @State private var scannedToken: InviteToken?
    @State private var errorMessage: String?
    @State private var isValidating = false

    var body: some View {
        ZStack {
            QRCodeCameraView { value in
                handle(value)
            }
            .ignoresSafeArea()

            VStack {
                Spacer()
                Text(errorMessage ?? "Point your camera at an Avalanche invite QR code")
                    .font(.callout)
                    .foregroundStyle(.white)
                    .multilineTextAlignment(.center)
                    .padding()
                    .background(.black.opacity(0.5), in: RoundedRectangle(cornerRadius: 12))
                    .padding(.horizontal, 24)
                    .padding(.bottom, 48)
            }

            if isValidating {
                ProgressView()
                    .tint(.white)
                    .scaleEffect(1.5)
            }
        }
        .navigationTitle("Scan QR Code")
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(item: $scannedToken) { token in
            IdentityPickerView(inviteToken: token)
        }
    }

    private func handle(_ value: String) {
        if let onScanned {
            onScanned(value)
            return
        }
        guard let url = URL(string: value), AppState.isDeepLink(url) else {
            errorMessage = "Not an Avalanche invite QR code"
            return
        }
        isValidating = true
        Task {
            do {
                scannedToken = try await InviteToken.from(url: url)
            } catch {
                errorMessage = "Invite validation failed: \(error.localizedDescription)"
            }
            isValidating = false
        }
    }
}
