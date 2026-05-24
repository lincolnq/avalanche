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

    private func validateLink() {
        errorMessage = nil
        isValidating = true

        Task {
            do {
                let token: InviteToken
                if let url = URL(string: linkText), url.host == "go.theavalanche.net" {
                    // Full URL: https://go.theavalanche.net/invite/<token>
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
