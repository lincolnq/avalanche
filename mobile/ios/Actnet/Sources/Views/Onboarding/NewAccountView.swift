import SwiftUI

struct NewAccountView: View {
    @EnvironmentObject var appState: AppState
    let inviteToken: InviteToken
    var showRecoverLink: Bool = true

    @State private var displayName = ""
    @State private var showPasskeyExplainer = false
    @State private var showRecovery = false

    var body: some View {
        VStack(spacing: 24) {
            Text("Create a new identity")
                .font(.headline)
                .foregroundStyle(.secondary)

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
