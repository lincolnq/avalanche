import SwiftUI

struct NewAccountView: View {
    @EnvironmentObject var appState: AppState
    let inviteToken: InviteToken

    @State private var displayName = ""
    @State private var isRegistering = false
    @State private var errorMessage: String?

    var body: some View {
        VStack(spacing: 24) {
            Text("Joining \(inviteToken.serverName)")
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

            if let error = errorMessage {
                Text(error)
                    .foregroundStyle(Color.avError)
                    .font(.callout)
            }

            Button {
                register()
            } label: {
                if isRegistering {
                    ProgressView()
                        .frame(maxWidth: .infinity)
                } else {
                    Text("Continue")
                        .frame(maxWidth: .infinity)
                }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .padding(.horizontal, 32)
            .disabled(displayName.isEmpty || isRegistering)

            Spacer()
        }
        .padding(.top, 32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.avPaper)
        .navigationTitle("Create Account")
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
            } catch {
                errorMessage = error.localizedDescription
                isRegistering = false
            }
        }
    }
}
