import SwiftUI
import CoreImage.CIFilterBuiltins

struct IdentityDetailView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.dismiss) private var dismiss
    let account: Account

    @State private var showDeleteConfirmation = false
    @State private var showStubAlert = false
    @State private var stubMessage = ""
    @State private var isDeleting = false
    @State private var deleteError: String?

    private var homeServer: ServerInfo? {
        // No discovery-server bit on the model yet; first server stands in.
        account.servers.first
    }

    private var contactURL: String? {
        guard let server = homeServer else { return nil }
        let token = makeInviteToken(serverUrl: server.url.absoluteString, inviterDid: account.id)
        return "https://go.theavalanche.net/i/\(token)"
    }

    var body: some View {
        ScrollView {
            VStack(spacing: 20) {
                VStack(spacing: 8) {
                    AccountAvatar(account: account, size: 72)
                    Text(account.displayName)
                        .font(.title2)
                        .fontWeight(.semibold)
                }
                .padding(.top, 16)

                if let url = contactURL {
                    VStack(spacing: 12) {
                        if let qr = generateQRCode(from: url) {
                            Image(uiImage: qr)
                                .interpolation(.none)
                                .resizable()
                                .scaledToFit()
                                .frame(width: 220, height: 220)
                        }
                        HStack(spacing: 16) {
                            Button {
                                UIPasteboard.general.string = url
                            } label: {
                                Label("Copy", systemImage: "doc.on.doc")
                            }
                            .buttonStyle(.bordered)

                            ShareLink(item: url) {
                                Label("Share", systemImage: "square.and.arrow.up")
                            }
                            .buttonStyle(.bordered)
                        }
                    }
                }

                VStack(spacing: 12) {
                    DetailRow(label: "DID", value: account.id, mono: true)

                    if let home = homeServer {
                        Button {
                            stubMessage = "Migration / change home server is not implemented yet."
                            showStubAlert = true
                        } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text("Home server")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                    Text(home.name)
                                        .foregroundStyle(.primary)
                                    Text(home.url.absoluteString)
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                }
                                Spacer()
                                Image(systemName: "chevron.right")
                                    .foregroundStyle(.secondary)
                            }
                            .padding(12)
                            .background(Color.sand50, in: RoundedRectangle(cornerRadius: 8))
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal)

                Text("Your home server is listed publicly so people can reach you. Your display name, other server memberships, contacts, and messages are not public. [Learn more]")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal)

                VStack(spacing: 12) {
                    NavigationLink {
                        BlockedContactsView(account: account)
                    } label: {
                        HStack {
                            Label("Blocked Contacts", systemImage: "hand.raised")
                                .foregroundStyle(.primary)
                            Spacer()
                            Image(systemName: "chevron.right")
                                .foregroundStyle(.secondary)
                        }
                        .padding(12)
                        .background(Color.sand50, in: RoundedRectangle(cornerRadius: 8))
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal)
                .padding(.top, 8)

                Button(role: .destructive) {
                    showDeleteConfirmation = true
                } label: {
                    if isDeleting {
                        ProgressView()
                            .frame(maxWidth: .infinity)
                    } else {
                        Text("Delete identity")
                            .frame(maxWidth: .infinity)
                    }
                }
                .buttonStyle(.bordered)
                .controlSize(.large)
                .disabled(isDeleting)
                .padding(.horizontal)
                .padding(.top, 24)
            }
            .padding(.bottom, 32)
        }
        .background(Color.avPaper)
        .navigationTitle("Identity")
        .navigationBarTitleDisplayMode(.inline)
        .confirmationDialog(
            "Delete this identity?",
            isPresented: $showDeleteConfirmation,
            titleVisibility: .visible
        ) {
            Button("Delete", role: .destructive) {
                deleteIdentity()
            }
            Button("Cancel", role: .cancel) { }
        } message: {
            Text("This will delete \(account.displayName) from \(account.servers.count) server\(account.servers.count == 1 ? "" : "s") and mark the identity deleted in the public registry. This cannot be undone. Your other identities on this device will not be affected.")
        }
        .alert("Not implemented", isPresented: $showStubAlert) {
            Button("OK", role: .cancel) { }
        } message: {
            Text(stubMessage)
        }
        .alert("Couldn't delete identity", isPresented: .constant(deleteError != nil)) {
            Button("OK", role: .cancel) { deleteError = nil }
        } message: {
            Text(deleteError ?? "")
        }
    }

    private func deleteIdentity() {
        isDeleting = true
        Task {
            do {
                try await appState.deleteIdentity(account: account)
                // On success the account is gone; pop back to the accounts list
                // (or onboarding, if this was the last identity).
                dismiss()
            } catch {
                deleteError = error.localizedDescription
            }
            isDeleting = false
        }
    }

    private func makeInviteToken(serverUrl: String, inviterDid: String) -> String {
        // Single-char wire keys (s=server_url, d=inviter_did) keep the QR low-density.
        let json = """
        {"s":"\(serverUrl)","d":"\(inviterDid)"}
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

private struct DetailRow: View {
    let label: String
    let value: String
    var mono: Bool = false

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
            Text(value)
                .font(mono ? .system(.footnote, design: .monospaced) : .body)
                .textSelection(.enabled)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(Color.sand50, in: RoundedRectangle(cornerRadius: 8))
    }
}
