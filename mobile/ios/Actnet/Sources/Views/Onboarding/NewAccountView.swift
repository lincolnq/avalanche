import SwiftUI

struct NewAccountView: View {
    @EnvironmentObject var appState: AppState
    let inviteToken: InviteToken
    var showRecoverLink: Bool = true

    @State private var displayName = ""
    @State private var avatarData: Data?
    @State private var showPasskeyExplainer = false
    @State private var showRecovery = false

    var body: some View {
        VStack(spacing: 24) {
            VStack(spacing: 4) {
                Text("Create a new identity")
                    .font(.headline)
                    .foregroundStyle(.secondary)

                Text("on \(inviteToken.serverName)")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }

            EditableAvatar(
                currentImage: avatarData,
                placeholderName: displayName,
                size: 100,
                onPicked: { data in
                    avatarData = data
                    // Stash for application once the account/core exists (docs/55).
                    appState.pendingOnboardingAvatar = data
                },
                onRemove: avatarData == nil ? nil : {
                    avatarData = nil
                    appState.pendingOnboardingAvatar = nil
                }
            )

            TextField("Your name", text: $displayName)
                .textFieldStyle(.roundedBorder)
                .padding(.horizontal, 32)
                .multilineTextAlignment(.center)

            Button {
                showPasskeyExplainer = true
            } label: {
                Text("Next")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .padding(.horizontal, 32)
            .disabled(displayName.isEmpty)

            if let url = inviteToken.privacyPolicyURL {
                Link("View \(inviteToken.serverName)'s privacy policy", destination: url)
                    .font(.caption)
            }

            Spacer()

            if showRecoverLink {
                Button {
                    showRecovery = true
                } label: {
                    Text("Recover an existing identity instead")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .padding(.bottom, 16)
            }
        }
        .padding(.top, 32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.avPaper)
        .navigationTitle("New Identity")
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(isPresented: $showPasskeyExplainer) {
            PasskeyExplainerView(inviteToken: inviteToken, displayName: displayName)
        }
        .navigationDestination(isPresented: $showRecovery) {
            RecoveryExplainerView()
        }
    }
}
