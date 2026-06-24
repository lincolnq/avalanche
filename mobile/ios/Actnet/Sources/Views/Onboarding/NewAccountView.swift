import SwiftUI

struct NewAccountView: View {
    @EnvironmentObject var appState: AppState
    let inviteToken: InviteToken
    var showRecoverLink: Bool = true

    @State private var displayName = ""
    @State private var showPasskeyExplainer = false
    @State private var showRecovery = false
    /// Operator's privacy policy URL from the server's public `/v1/info`. nil
    /// until loaded, or if the operator hasn't configured one.
    @State private var privacyPolicyURL: URL?

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

            // TODO: Avatar photo picker
            Circle()
                .fill(Color.sand200)
                .frame(width: 100, height: 100)
                .overlay {
                    Image(systemName: "camera")
                        .font(.title2)
                        .foregroundStyle(.secondary)
                }

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

            if let url = privacyPolicyURL {
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
        .task {
            privacyPolicyURL = await PublicServerInfo.privacyPolicyURL(forServer: inviteToken.serverUrl)
        }
        .navigationDestination(isPresented: $showPasskeyExplainer) {
            PasskeyExplainerView(inviteToken: inviteToken, displayName: displayName)
        }
        .navigationDestination(isPresented: $showRecovery) {
            RecoveryExplainerView()
        }
    }
}
