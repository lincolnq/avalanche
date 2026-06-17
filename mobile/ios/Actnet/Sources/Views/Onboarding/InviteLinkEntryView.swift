import SwiftUI

struct InviteLinkEntryView: View {
    @State private var linkText = ""
    @State private var inviteToken: InviteToken?
    @State private var errorMessage: String?
    @State private var isValidating = false

    var body: some View {
        VStack(spacing: 24) {
            TextField("Paste invite link", text: $linkText)
                .textFieldStyle(.roundedBorder)
                .textContentType(.URL)
                .autocapitalization(.none)
                .padding(.horizontal, 32)

            if let error = errorMessage {
                Text(error)
                    .foregroundStyle(Color.avError)
                    .font(.callout)
            }

            Button {
                validateLink()
            } label: {
                if isValidating {
                    ProgressView()
                } else {
                    Text("Continue")
                }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(linkText.isEmpty || isValidating)

            #if DEBUG
            Button("Use localhost:3000 (dev)") {
                linkText = Self.localhostDevToken
                validateLink()
            }
            .font(.callout)
            .disabled(isValidating)
            #endif

            Spacer()
        }
        .padding(.top, 32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.avPaper)
        .navigationTitle("Enter Invite Link")
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(item: $inviteToken) { token in
            IdentityPickerView(inviteToken: token)
        }
    }

    /// Base64url-encoded bootstrap token for the local dev server, with
    /// single-char wire keys (s=server_url, k=bootstrap_secret). The dev server
    /// runs closed registration with the `CHANGEME` secret (see dev.py / `make
    /// dev`), so the token carries it. Debug-only convenience.
    private static let localhostDevToken: String = {
        let json = #"{"s":"http://localhost:3000","k":"CHANGEME"}"#
        let data = Data(json.utf8)
        // base64url, no padding
        return data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }()

    private func validateLink() {
        errorMessage = nil
        isValidating = true

        Task {
            do {
                let token: InviteToken
                if let url = URL(string: linkText), url.host == "go.theavalanche.net" {
                    // Full URL: https://go.theavalanche.net/i/<token>
                    token = try await InviteToken.from(url: url)
                } else {
                    // Bare token string
                    token = try await InviteToken.from(token: linkText.trimmingCharacters(in: .whitespacesAndNewlines))
                }
                inviteToken = token
            } catch {
                errorMessage = error.localizedDescription
            }
            isValidating = false
        }
    }
}
