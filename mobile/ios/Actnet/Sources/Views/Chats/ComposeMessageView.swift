import SwiftUI

/// Sheet for starting a new DM conversation by entering a recipient DID.
struct ComposeMessageView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.dismiss) private var dismiss

    @State private var recipientDid = ""
    @State private var selectedAccountId: String?

    private var accounts: [Account] { appState.accounts }

    private var activeAccountId: String? {
        selectedAccountId ?? accounts.first?.id
    }

    var body: some View {
        NavigationStack {
            Form {
                if accounts.count > 1 {
                    Section("From") {
                        Picker("Account", selection: $selectedAccountId) {
                            ForEach(accounts) { account in
                                Text(account.displayName)
                                    .tag(Optional(account.id))
                            }
                        }
                    }
                }

                Section("Recipient") {
                    TextField("did:plc:...", text: $recipientDid)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                }
            }
            .navigationTitle("New Message")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Start") { startConversation() }
                        .disabled(recipientDid.isEmpty || activeAccountId == nil)
                }
            }
        }
    }

    private func startConversation() {
        guard let accountId = activeAccountId else { return }
        let did = recipientDid.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !did.isEmpty else { return }
        _ = appState.findOrCreateDMConversation(recipientDid: did, accountId: accountId)
        dismiss()
    }
}
