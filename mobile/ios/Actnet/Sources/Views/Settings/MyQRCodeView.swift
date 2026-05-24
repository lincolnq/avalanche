import SwiftUI
import CoreImage.CIFilterBuiltins

struct MyQRCodeView: View {
    @EnvironmentObject var appState: AppState

    var body: some View {
        VStack(spacing: 24) {
            if let account = appState.accounts.first,
               let server = account.servers.first {
                let token = makeInviteToken(serverUrl: server.url.absoluteString, inviterDid: account.id)
                let url = "https://go.theavalanche.net/invite/\(token)"

                if let qrImage = generateQRCode(from: url) {
                    Image(uiImage: qrImage)
                        .interpolation(.none)
                        .resizable()
                        .scaledToFit()
                        .frame(width: 250, height: 250)
                        .padding()
                }

                Text(account.displayName)
                    .font(.title2)
                    .fontWeight(.semibold)

                Text("Scan to start a conversation")
                    .foregroundStyle(.secondary)

                ShareLink(item: url) {
                    Label("Share Invite Link", systemImage: "square.and.arrow.up")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)
                .controlSize(.large)
                .padding(.horizontal, 32)
            } else {
                Text("No account")
                    .foregroundStyle(.secondary)
            }

            Spacer()
        }
        .padding(.top, 32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.avPaper)
        .navigationTitle("My QR Code")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func makeInviteToken(serverUrl: String, inviterDid: String) -> String {
        let json = """
        {"server_url":"\(serverUrl)","inviter_did":"\(inviterDid)"}
        """
        return Data(json.utf8)
            .base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    private func generateQRCode(from string: String) -> UIImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(string.utf8)
        filter.correctionLevel = "M"
        guard let ciImage = filter.outputImage else { return nil }
        let falseColor = CIFilter.falseColor()
        falseColor.inputImage = ciImage
        falseColor.color0 = CIColor(color: UIColor(Color.plum800))
        falseColor.color1 = CIColor(color: UIColor(Color.avPaper))
        guard let colored = falseColor.outputImage else { return nil }
        let transform = CGAffineTransform(scaleX: 10, y: 10)
        let scaled = colored.transformed(by: transform)
        let context = CIContext()
        guard let cgImage = context.createCGImage(scaled, from: scaled.extent) else { return nil }
        return UIImage(cgImage: cgImage)
    }
}
