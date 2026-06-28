import SwiftUI
import AuthenticationServices

struct PasskeyExplainerView: View {
    @EnvironmentObject var appState: AppState
    let inviteToken: InviteToken
    let displayName: String

    @State private var isRegistering = false
    @State private var errorMessage: String?
    @State private var showRecoveryPhraseSetup = false

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
                    .fill(Color.avCard)
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
                    registerWithPasskey()
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
                    showRecoveryPhraseSetup = true
                } label: {
                    Text("Use a recovery phrase instead")
                        .font(.subheadline)
                }
                .disabled(isRegistering)

                Button {
                    register(prfOutput: Data())
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
        .navigationDestination(isPresented: $showRecoveryPhraseSetup) {
            RecoveryPhraseSetupView(inviteToken: inviteToken, displayName: displayName)
        }
    }

    private func registerWithPasskey() {
        isRegistering = true
        errorMessage = nil
        Task {
            do {
                // Get the window for the ASAuthorization sheet.
                guard let window = UIApplication.shared.connectedScenes
                    .compactMap({ $0 as? UIWindowScene })
                    .flatMap(\.windows)
                    .first(where: \.isKeyWindow) else {
                    errorMessage = "Could not find app window"
                    isRegistering = false
                    return
                }

                // Stage 1: run the passkey ceremony first. The credential's
                // `user.id` is set to the signup server URL — that's what lets
                // recovery recompute the DID later without prompting the user.
                let labelForPicker = "\(displayName) @ \(inviteToken.serverName)"
                let passkeyManager = PasskeyManager()
                let result = try await passkeyManager.register(
                    signupServerUrl: inviteToken.serverUrl.absoluteString,
                    displayName: labelForPicker,
                    anchor: window
                )

                // Stage 2: derive the rotation key from the PRF output and
                // build both PLC ops. The DID drops out of this.
                let prepared = try await appState.prepareAccount(
                    serverUrl: inviteToken.serverUrl.absoluteString,
                    prfOutput: result.prfOutput
                )

                // Stage 3: submit the PLC ops, encrypt the recovery blob with
                // the PRF-derived key, and register with the homeserver.
                try await appState.finalizePreparedAccount(
                    prepared: prepared,
                    serverUrl: inviteToken.serverUrl.absoluteString,
                    serverName: inviteToken.serverName,
                    displayName: displayName,
                    inviteToken: inviteToken.token
                )

                if let redirect = inviteToken.postOnboardingRedirect,
                   let url = URL(string: redirect) {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                        appState.handleDeepLink(url)
                    }
                }
            } catch let error as ASAuthorizationError where error.code == .canceled {
                // User cancelled — don't show error, just re-enable buttons.
                isRegistering = false
            } catch {
                errorMessage = error.localizedDescription
                isRegistering = false
            }
        }
    }

    private func register(prfOutput: Data) {
        isRegistering = true
        errorMessage = nil
        Task {
            do {
                try await appState.createAccount(
                    serverUrl: inviteToken.serverUrl.absoluteString,
                    serverName: inviteToken.serverName,
                    displayName: displayName,
                    inviteToken: inviteToken.token,
                    prfOutput: prfOutput
                )
                // createAccount sets isOnboarding = false, which navigates to MainTabView.
                // If the invite has a post-onboarding redirect, follow it.
                if let redirect = inviteToken.postOnboardingRedirect,
                   let url = URL(string: redirect) {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                        appState.handleDeepLink(url)
                    }
                }
            } catch {
                errorMessage = error.localizedDescription
                isRegistering = false
            }
        }
    }
}
