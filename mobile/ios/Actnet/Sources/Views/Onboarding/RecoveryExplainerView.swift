import SwiftUI
import AuthenticationServices
import CryptoKit

struct RecoveryExplainerView: View {
    @EnvironmentObject var appState: AppState

    @State private var showRecoveryConsole = false
    @State private var showPhraseEntry = false
    @State private var errorMessage: String?
    @State private var recoveryKey: Data?
    @State private var recoveryDid: String?

    var body: some View {
        VStack(spacing: 24) {
            Spacer()

            Image(systemName: "person.badge.key")
                .font(.system(size: 48))
                .foregroundStyle(Color.avBrand)

            Text("Recover an identity")
                .font(.title2)
                .fontWeight(.semibold)

            Text("Use a passkey or recovery phrase to restore an identity you created on another device.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)

            if let error = errorMessage {
                Text(error)
                    .foregroundStyle(Color.avError)
                    .font(.callout)
                    .padding(.horizontal, 32)
            }

            Spacer()

            VStack(spacing: 12) {
                Button {
                    recoverWithPasskey()
                } label: {
                    Label("Recover using Passkey", systemImage: "person.badge.key")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)

                Button {
                    showPhraseEntry = true
                } label: {
                    Text("Enter your recovery phrase instead")
                        .font(.subheadline)
                }
            }
            .padding(.horizontal, 32)
            .padding(.bottom, 48)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.avPaper)
        .navigationTitle("Recovery")
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(isPresented: $showRecoveryConsole) {
            RecoveryConsoleView(recoveryKey: recoveryKey ?? Data(), did: recoveryDid ?? "")
        }
        .sheet(isPresented: $showPhraseEntry) {
            RecoveryPhraseEntryView(onComplete: { phrase in
                showPhraseEntry = false
                // Derive key from phrase using SHA-256 (simple derivation for now).
                // A proper KDF (Argon2, scrypt) would be better but this matches
                // the current design's simplicity.
                let phraseData = Data(phrase.utf8)
                let hash = SHA256Hash.hash(data: phraseData)
                recoveryKey = Data(hash)
                // For phrase-based recovery, we don't have the DID from a passkey.
                // The user will need to enter a server URL manually.
                // For now, show the console which will prompt for it.
                recoveryDid = ""
                showRecoveryConsole = true
            })
        }
    }

    private func recoverWithPasskey() {
        errorMessage = nil
        Task {
            do {
                guard let window = UIApplication.shared.connectedScenes
                    .compactMap({ $0 as? UIWindowScene })
                    .flatMap(\.windows)
                    .first(where: \.isKeyWindow) else {
                    errorMessage = "Could not find app window"
                    return
                }

                let passkeyManager = PasskeyManager()
                let result = try await passkeyManager.authenticate(anchor: window)

                // Check if this identity is already signed in on this device.
                if appState.accounts.contains(where: { $0.id == result.did }) {
                    errorMessage = "This identity is already signed in on this device."
                    return
                }

                recoveryKey = result.recoveryKey
                recoveryDid = result.did
                showRecoveryConsole = true
            } catch let error as ASAuthorizationError where error.code == .canceled {
                // User cancelled — no error message needed.
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }
}

/// Simple SHA-256 wrapper.
private enum SHA256Hash {
    static func hash(data: Data) -> [UInt8] {
        let digest = CryptoKit.SHA256.hash(data: data)
        return Array(digest)
    }
}

/// Sheet for entering a written-down recovery phrase.
private struct RecoveryPhraseEntryView: View {
    @Environment(\.dismiss) private var dismiss
    let onComplete: (String) -> Void

    @State private var phrase = ""

    var body: some View {
        NavigationStack {
            VStack(spacing: 24) {
                Text("Enter your recovery phrase")
                    .font(.headline)

                TextField("Recovery phrase", text: $phrase, axis: .vertical)
                    .textFieldStyle(.roundedBorder)
                    .padding(.horizontal, 32)
                    .lineLimit(3...6)

                Button("Recover") {
                    onComplete(phrase)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(phrase.isEmpty)

                Spacer()
            }
            .padding(.top, 32)
            .navigationTitle("Recovery Phrase")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
    }
}
