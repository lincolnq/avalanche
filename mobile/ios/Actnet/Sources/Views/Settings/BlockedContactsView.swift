import SwiftUI

/// Settings → (Identity) → Blocked Contacts (docs/12 §7). Lists the DIDs this
/// identity has blocked and lets the user unblock them. The block list is
/// identity-scoped and synced across the identity's devices.
struct BlockedContactsView: View {
    @EnvironmentObject var appState: AppState
    let account: Account

    @State private var blocked: [ContactRowFfi] = []
    @State private var loaded = false

    var body: some View {
        List {
            if blocked.isEmpty {
                Section {
                    Text(loaded ? "You haven't blocked anyone." : "Loading…")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            } else {
                Section {
                    ForEach(blocked, id: \.did) { row in
                        HStack(spacing: 12) {
                            ContactAvatar(name: displayName(row), size: 36)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(displayName(row))
                                    .fontWeight(.medium)
                                    .lineLimit(1)
                                Text(row.did)
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                            Spacer()
                            Button("Unblock") {
                                Task {
                                    await appState.unblockContact(did: row.did, accountId: account.id)
                                    await reload()
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                        }
                    }
                } footer: {
                    Text("Blocked contacts can't message you, and you can't message them. Unblocking reverses both.")
                }
            }
        }
        .scrollContentBackground(.hidden)
        .background(Color.avPaper)
        .navigationTitle("Blocked Contacts")
        .navigationBarTitleDisplayMode(.inline)
        .task { await reload() }
    }

    private func displayName(_ row: ContactRowFfi) -> String {
        row.displayName.isEmpty ? row.did : row.displayName
    }

    private func reload() async {
        blocked = await appState.listBlocked(accountId: account.id)
        loaded = true
    }
}
