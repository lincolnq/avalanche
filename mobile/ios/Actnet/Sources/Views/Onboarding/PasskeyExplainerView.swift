import SwiftUI

struct PasskeyExplainerView: View {
    @EnvironmentObject var appState: AppState
    let inviteToken: InviteToken
    let displayName: String

    @State private var isRegistering = false
    @State private var errorMessage: String?

    var body: some View {
        VStack(spacing: 24) {
            Spacer()

            Text("Create a passkey to protect this identity")
                .font(.title2)
                .fontWeight(.semibold)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)

            // Profile preview
            VStack(spacing: 8) {
                Circle()
                    .fill(Color.sand200)
                    .frame(width: 64, height: 64)
                    .overlay {
                        Text(String(displayName.prefix(1)).uppercased())
                            .font(.title)
                            .foregroundStyle(.secondary)
                    }
                Text(displayName)
                    .font(.headline)
            }

            Text("Passkeys are stored securely in your password manager or iCloud, and synced across all your devices. You'll use it to sign back into this identity if you lose this device.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)

            if let error = errorMessage {
                Text(error)
                    .foregroundStyle(Color.avError)
                    .font(.callout)
            }

            Spacer()

            VStack(spacing: 12) {
                Button {
                    // TODO: WebAuthn registration ceremony
                    // For now, proceed directly to signup
                    register()
                } label: {
                    if isRegistering {
                        ProgressView()
                            .frame(maxWidth: .infinity)
                    } else {
                        Label("Create Passkey", systemImage: "person.badge.key")
                            .frame(maxWidth: .infinity)
                    }
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(isRegistering)

                Button {
                    // TODO: Recovery phrase generation flow
                    register()
                } label: {
                    Text("Use a recovery phrase instead")
                        .font(.subheadline)
                }
                .disabled(isRegistering)

                Button {
                    register()
                } label: {
                    Text("Skip recovery setup")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .disabled(isRegistering)
            }
            .padding(.horizontal, 32)
            .padding(.bottom, 48)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.avPaper)
        .navigationTitle("Recovery")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func register() {
        isRegistering = true
        errorMessage = nil
        Task {
            do {
                try await appState.createAccount(
                    serverUrl: inviteToken.serverUrl.absoluteString,
                    serverName: inviteToken.serverName,
                    displayName: displayName
                )
                // createAccount sets isOnboarding = false, which navigates away
            } catch {
                errorMessage = error.localizedDescription
                isRegistering = false
            }
        }
    }
}
