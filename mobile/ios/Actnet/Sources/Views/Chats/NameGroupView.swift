import SwiftUI

/// Signal-style "Name Group" screen, pushed from the composer's New Group
/// button. Collects the group name, the disappearing-messages timer, and (for
/// an empty group) the hosting server, then creates the group and opens its
/// thread. Group photos are intentionally omitted for now — the core has no
/// avatar param and `docs/30-mobile-ux.md` auto-generates a mosaic icon and
/// defers custom icons at creation.
struct NameGroupView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.dismiss) private var dismiss

    let members: [ComposeMessageView.Chip]
    let accountId: String
    /// Servers the active identity belongs to; the first is its home server.
    let servers: [ServerInfo]
    /// Called once the group is created — wired by the composer to navigate to
    /// the new thread and dismiss the whole compose sheet.
    let onCreated: (Conversation) -> Void

    @State private var name: String = ""
    @State private var expirySeconds: UInt32 = 0
    @State private var selectedServerId: String = ""
    @State private var creating: Bool = false
    @State private var errorMessage: String?

    /// Home server = the first in the identity's list (matches the rest of the
    /// app, e.g. `AppState`'s `servers.first` server-url derivation).
    private var homeServer: ServerInfo? { servers.first }

    /// Only an empty group may choose a server; with recipients the founder's
    /// home server is used (and a recipient on another homeserver is reached by
    /// federation, not by moving the group). A picker is also pointless with a
    /// single server.
    private var canChooseServer: Bool { members.isEmpty && servers.count > 1 }

    private var resolvedServer: ServerInfo? {
        servers.first(where: { $0.id == selectedServerId }) ?? homeServer
    }

    /// Group name is required — no auto-name from participants.
    private var trimmedName: String {
        name.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    var body: some View {
        Form {
            Section {
                TextField("Group Name (required)", text: $name)
            }

            Section("Server") {
                if canChooseServer {
                    Picker("Server", selection: $selectedServerId) {
                        ForEach(servers) { server in
                            Text(server.displayHost)
                                .tag(server.id)
                                // Non-home creation isn't wired in the core yet
                                // (`create_group` uses the account's pinned
                                // client server), so gate the alternatives.
                                .disabled(server.id != homeServer?.id)
                        }
                    }
                    Text("Creating on another server isn't supported yet.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else if let server = resolvedServer {
                    HStack {
                        Text(server.displayHost)
                        Spacer()
                        Text("Home").font(.caption).foregroundStyle(.secondary)
                    }
                }
            }

            Section {
                DisappearingMessagesPicker(seconds: $expirySeconds)
            }

            Section("Members (\(members.count))") {
                if members.isEmpty {
                    Text("No members yet — you can add people after creating the group.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(members) { member in
                        HStack(spacing: 10) {
                            ContactAvatar(name: member.label, isBot: isBot(member), size: 32)
                            Text(member.label).lineLimit(1)
                        }
                    }
                }
            }

            if let error = errorMessage {
                Section {
                    Text(error).font(.caption).foregroundStyle(Color.avError)
                }
            }
        }
        .scrollContentBackground(.hidden)
        .background(Color.avPaper)
        .navigationTitle("Name Group")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear {
            if selectedServerId.isEmpty { selectedServerId = homeServer?.id ?? "" }
        }
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Button("Create") { create() }
                    .disabled(creating || trimmedName.isEmpty)
            }
        }
    }

    private func isBot(_ member: ComposeMessageView.Chip) -> Bool {
        appState.isBot(member.did, accountId: accountId)
    }

    private func create() {
        creating = true
        errorMessage = nil
        let title = trimmedName
        let serverUrl = resolvedServer?.id ?? ""
        let recipientDids = members.map(\.did)
        let expiry = expirySeconds
        Task {
            defer { creating = false }
            do {
                let conv = try await appState.createGroupAndOpen(
                    accountId: accountId,
                    serverUrl: serverUrl,
                    title: title,
                    recipientDids: recipientDids,
                    expirySeconds: expiry,
                    firstMessage: nil
                )
                onCreated(conv)
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }
}

#if DEBUG
#Preview("Group with members") {
    let me = Account(
        id: "did:plc:me",
        displayName: "Me",
        avatarData: nil,
        servers: [ServerInfo(
            id: "https://server.example",
            name: "Example",
            url: URL(string: "https://server.example")!
        )]
    )
    return NavigationStack {
        NameGroupView(
            members: [
                ComposeMessageView.Chip(id: "did:plc:alice", did: "did:plc:alice", displayName: "Alice Rivera"),
                ComposeMessageView.Chip(id: "did:plc:bob", did: "did:plc:bob", displayName: "Bob Chena"),
            ],
            accountId: "did:plc:me",
            servers: me.servers,
            onCreated: { _ in }
        )
        .environmentObject(AppState.preview(accounts: [me]))
    }
}

#Preview("Empty group, multi-server") {
    let me = Account(
        id: "did:plc:me",
        displayName: "Me",
        avatarData: nil,
        servers: [
            ServerInfo(id: "https://a.example", name: "Safe Haven", url: URL(string: "https://a.example")!),
            ServerInfo(id: "https://b.example", name: "Backup Server", url: URL(string: "https://b.example")!),
        ]
    )
    return NavigationStack {
        NameGroupView(
            members: [],
            accountId: "did:plc:me",
            servers: me.servers,
            onCreated: { _ in }
        )
        .environmentObject(AppState.preview(accounts: [me]))
    }
}
#endif
