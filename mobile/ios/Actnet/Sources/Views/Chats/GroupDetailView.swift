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
                        ForEach(s.members, id: \.encryptedMemberId) { member in
                            HStack {
                                VStack(alignment: .leading) {
                                    Text(displayName(for: member.did))
                                    Text(member.did).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                                }
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

    private func displayName(for did: String) -> String {
        let cached = appState.displayName(for: did, accountId: accountId)
        if !cached.isEmpty, cached != did { return cached }
        return shortDid(did)
    }

    private func shortDid(_ did: String) -> String {
        if did.count > 18 { return String(did.prefix(12)) + "…" + String(did.suffix(4)) }
        return did
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
