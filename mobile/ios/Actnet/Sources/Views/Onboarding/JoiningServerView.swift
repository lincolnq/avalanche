import SwiftUI

/// Registers an existing DID with a new server.
struct JoiningServerView: View {
    @EnvironmentObject var appState: AppState
    let inviteToken: InviteToken
    let existingAccount: Account

    @State private var isJoining = false
    @State private var errorMessage: String?

    var body: some View {
        VStack(spacing: 24) {
            AccountAvatar(account: existingAccount, size: 80)

            Text("Join \(inviteToken.serverName) as \(existingAccount.displayName)?")
                .font(.headline)
                .multilineTextAlignment(.center)

            if let url = inviteToken.privacyPolicyURL {
                Link("View \(inviteToken.serverName)'s privacy policy", destination: url)
                    .font(.callout)
            }

            if let error = errorMessage {
                Text(error)
                    .foregroundStyle(Color.avError)
                    .font(.callout)
            }

            Button {
                joinServer()
            } label: {
                if isJoining {
                    ProgressView()
                        .frame(maxWidth: .infinity)
                } else {
                    Text("Join")
                        .frame(maxWidth: .infinity)
                }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .padding(.horizontal, 32)
            .disabled(isJoining)

            Spacer()
        }
        .padding(.top, 48)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.avPaper)
        .navigationTitle("Join Server")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func joinServer() {
        isJoining = true
        errorMessage = nil
        Task {
            do {
                try await appState.joinServer(
                    serverUrl: inviteToken.serverUrl.absoluteString,
                    serverName: inviteToken.serverName,
                    existingAccountId: existingAccount.id
                )
                // Joining from Settings runs with isOnboarding already false, so
                // nothing tears this view down — reset the spinner explicitly
                // (the Accounts sheet handles dismissal via addAccountCompletedTick).
                isJoining = false
            } catch {
                errorMessage = error.localizedDescription
                isJoining = false
            }
        }
    }
}
