import SwiftUI

struct RecoveryExplainerView: View {
    @EnvironmentObject var appState: AppState

    @State private var showRecoveryConsole = false
    @State private var showPhraseEntry = false

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

            Spacer()

            VStack(spacing: 12) {
                Button {
                    // TODO: WebAuthn authentication ceremony
                    // The system will present a sheet showing all passkeys
                    // stored for theavalanche.net. User picks one, confirms
                    // with Face ID. App receives PRF-derived key + DID from
                    // user handle.
                    showRecoveryConsole = true
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
            RecoveryConsoleView()
        }
        .sheet(isPresented: $showPhraseEntry) {
            RecoveryPhraseEntryView(onComplete: {
                showPhraseEntry = false
                showRecoveryConsole = true
            })
        }
    }
}

/// Sheet for entering a written-down recovery phrase.
private struct RecoveryPhraseEntryView: View {
    @Environment(\.dismiss) private var dismiss
    let onComplete: () -> Void

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
                    // TODO: derive symmetric key from phrase, download + decrypt blob
                    onComplete()
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
