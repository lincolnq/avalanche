import SwiftUI

/// Minimal group detail screen (docs/03-groups.md step 8): member list and
/// leave-group action. Roles, expiry timer, invite-link, and admin actions
/// are deferred per the "Create + thread + member list" scope.
struct GroupDetailView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.dismiss) private var dismiss

    let groupId: String
    let accountId: String

    @State private var summary: GroupSummaryFfi?
    @State private var loading: Bool = true
    @State private var errorMessage: String?

    var body: some View {
        Group {
            if let s = summary {
                List {
                    Section {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(s.title.isEmpty ? "Group" : s.title).font(.headline)
                            if !s.description.isEmpty {
                                Text(s.description).font(.subheadline).foregroundStyle(.secondary)
                            }
                            Text("Revision \(s.revision)").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    Section("Members (\(s.members.count))") {
                        ForEach(orderedMembers(s.members), id: \.encryptedMemberId) { member in
                            HStack(spacing: 10) {
                                ContactAvatar(name: memberName(member), size: 32)
                                Text(memberName(member))
                                    .lineLimit(1)
                                Spacer()
                                if member.role == 1 {
                                    Text("Admin").font(.caption2)
                                        .padding(.horizontal, 6).padding(.vertical, 2)
                                        .background(Color.accentColor.opacity(0.15))
                                        .clipShape(Capsule())
                                }
                            }
                        }
                    }
                    if !s.pendingInvites.isEmpty {
                        Section("Pending invites (\(s.pendingInvites.count))") {
                            ForEach(s.pendingInvites, id: \.encryptedMemberId) { p in
                                Text(p.encryptedMemberId).font(.caption).lineLimit(1)
                            }
                        }
                    }
                    Section {
                        Button(role: .destructive) {
                            leaveGroup()
                        } label: {
                            Label("Leave group", systemImage: "rectangle.portrait.and.arrow.right")
                        }
                    }
                }
                .scrollContentBackground(.hidden)
                .background(Color.avPaper)
            } else if loading {
                ProgressView().padding()
            } else if let error = errorMessage {
                ContentUnavailableView(
                    "Couldn't load group",
                    systemImage: "exclamationmark.triangle",
                    description: Text(error)
                )
            }
        }
        .background(Color.avPaper)
        .navigationTitle("Group info")
        .navigationBarTitleDisplayMode(.inline)
        .task { await load() }
    }

    /// The current user sorts first; everyone else keeps server order.
    private func orderedMembers(_ members: [GroupMemberFfi]) -> [GroupMemberFfi] {
        members.sorted { a, _ in a.did == accountId }
    }

    /// Member's display name, or "You" for the current user.
    private func memberName(_ member: GroupMemberFfi) -> String {
        member.did == accountId
            ? "You"
            : appState.resolvedName(for: member.did, accountId: accountId)
    }

    private func load() async {
        loading = true
        defer { loading = false }
        guard let core = appState.core(accountId: accountId) else {
            errorMessage = "No core for account"
            return
        }
        let gid = groupId
        do {
            let s = try await Task.detached {
                try core.fetchGroupState(groupId: gid)
            }.value
            self.summary = s
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func leaveGroup() {
        guard let s = summary,
              let me = s.members.first(where: { $0.did == accountId })
        else { return }
        guard let core = appState.core(accountId: accountId) else { return }
        let emi = me.encryptedMemberId
        let gid = groupId
        Task {
            do {
                try await Task.detached {
                    try core.removeMember(groupId: gid, encryptedMemberId: emi)
                }.value
                dismiss()
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }
}

#if DEBUG
#Preview {
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
    let contacts: [ContactRowFfi] = [
        ContactRowFfi(did: "did:plc:bob", displayName: "Bob Chena", isCurated: true, lastInteractionAtMs: 0),
        ContactRowFfi(did: "did:plc:carol", displayName: "Carol X", isCurated: true, lastInteractionAtMs: 0),
    ]
    let summary = GroupSummaryFfi(
        groupId: "grp1",
        masterKey: Data(count: 32),
        revision: 3,
        title: "March Logistics",
        description: "Planning crew for the day-of action.",
        expirySeconds: 0,
        // Self listed last on the wire — the view sorts "You" to the top.
        members: [
            GroupMemberFfi(did: "did:plc:bob", encryptedMemberId: "emi-bob", role: 0, joinedAtMs: 0),
            GroupMemberFfi(did: "did:plc:carol", encryptedMemberId: "emi-carol", role: 1, joinedAtMs: 0),
            GroupMemberFfi(did: "did:plc:me", encryptedMemberId: "emi-me", role: 1, joinedAtMs: 0),
        ],
        pendingInvites: [
            GroupPendingFfi(encryptedMemberId: "emi-dave", timestampMs: 0),
        ],
        pendingApprovals: []
    )
    return NavigationStack {
        GroupDetailView(groupId: "grp1", accountId: "did:plc:me")
            .environmentObject(AppState.preview(
                accounts: [me],
                contacts: contacts,
                groups: ["grp1": summary]
            ))
    }
}
#endif
