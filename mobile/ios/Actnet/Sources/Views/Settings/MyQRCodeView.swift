import SwiftUI
import CoreImage.CIFilterBuiltins

struct MyQRCodeView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.dismiss) private var dismiss
    @State private var showScanner = false
    @State private var pastedInvite: String = ""

    private var pastedInviteURL: URL? {
        let trimmed = pastedInvite.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let url = URL(string: trimmed), AppState.isDeepLink(url) else {
            return nil
        }
        let components = url.pathComponents.filter { $0 != "/" }
        guard components.first == "invite", components.count >= 2 else { return nil }
        return url
    }

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

                HStack(spacing: 16) {
                    Button {
                        UIPasteboard.general.string = url
                    } label: {
                        Image(systemName: "doc.on.doc")
                    }
                    .buttonStyle(.bordered)

                    ShareLink(item: url) {
                        Image(systemName: "square.and.arrow.up")
                    }
                    .buttonStyle(.bordered)
                }

            } else {
                Text("No account")
                    .foregroundStyle(.secondary)
            }

            Spacer()

            Button {
                showScanner = true
            } label: {
                Label("Scan", systemImage: "camera")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered)
            .controlSize(.large)
            .padding(.horizontal, 32)

            VStack(spacing: 8) {
                HStack(spacing: 8) {
                    TextField("Paste invite link", text: $pastedInvite)
                        .textFieldStyle(.roundedBorder)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)

                    Button {
                        if let s = UIPasteboard.general.string {
                            pastedInvite = s
                        }
                    } label: {
                        Image(systemName: "doc.on.clipboard")
                    }
                    .buttonStyle(.bordered)
                }

                Button {
                    if let url = pastedInviteURL {
                        appState.handleDeepLink(url)
                        dismiss()
                    }
                } label: {
                    Label("Send Message", systemImage: "paperplane")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(pastedInviteURL == nil)
            }
            .padding(.horizontal, 32)
            .padding(.bottom, 24)
        }
        .padding(.top, 32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.avPaper)
        .navigationTitle("My QR Code")
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(isPresented: $showScanner) {
            QRScannerView { value in
                guard let url = URL(string: value), AppState.isDeepLink(url) else { return }
                appState.handleDeepLink(url)
                dismiss()
            }
        }
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
