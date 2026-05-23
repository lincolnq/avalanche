import SwiftUI

/// Shown when the user has scanned/entered an invite.
/// If accounts exist, lets them pick an existing identity or create a new one.
/// If no accounts exist, goes straight to the new account flow.
struct IdentityPickerView: View {
    @EnvironmentObject var appState: AppState
    let inviteToken: InviteToken

    @State private var showNewAccount = false

    var body: some View {
        if appState.accounts.isEmpty {
            NewAccountView(inviteToken: inviteToken, showRecoverLink: true)
        } else {
            existingAccountPicker
        }
    }

    private var existingAccountPicker: some View {
        List {
            Section("Join \(inviteToken.serverName) as...") {
                ForEach(appState.accounts) { account in
                    NavigationLink {
                        JoiningServerView(inviteToken: inviteToken, existingAccount: account)
                    } label: {
                        HStack {
                            AccountAvatar(account: account, size: 40)
                            VStack(alignment: .leading) {
                                Text(account.displayName)
                                    .fontWeight(.medium)
                                Text(account.servers.map(\.name).joined(separator: ", "))
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                }
            }

            Section {
                NavigationLink {
                    NewAccountView(inviteToken: inviteToken, showRecoverLink: false)
                } label: {
                    Label("Create a new identity", systemImage: "plus.circle")
                        .foregroundStyle(Color.avBrand)
                }

                NavigationLink {
                    RecoveryExplainerView()
                } label: {
                    Label("Recover an identity", systemImage: "person.badge.key")
                        .foregroundStyle(.orange)
                }
            }
        }
        .navigationTitle("Choose Identity")
        .navigationBarTitleDisplayMode(.inline)
    }
}
