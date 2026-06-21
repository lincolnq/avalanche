import SwiftUI

/// Group detail screen (docs/03-groups.md): member list, admin role changes,
/// disappearing-message timer, and leave-group. Admin-only controls are shown
/// only when the current user is an admin; the server enforces the same.
struct GroupDetailView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.dismiss) private var dismiss

    let groupId: String
    let accountId: String

    @State private var summary: GroupSummaryFfi?
    @State private var loading: Bool = true
    @State private var errorMessage: String?
    /// Bound to the timer picker; seeded from the loaded state so the initial
    /// assignment doesn't fire a spurious change (we guard on the loaded value).
    @State private var expirySeconds: UInt32 = 0
    @State private var showRename = false
    @State private var renameText = ""

    /// True when the current user is an admin of this group — gates the
    /// role/timer controls.
    private var amAdmin: Bool {
        summary?.members.first(where: { $0.did == accountId })?.role == 1
    }

    /// Whether we're still a member — false once we've left (docs/53 §Leave).
    /// Gates the "Leave group" button; the rest of the screen renders read-only
    /// from cached state.
    private var amMember: Bool {
        summary?.members.contains(where: { $0.did == accountId }) ?? false
    }

    var body: some View {
        Group {
            if let s = summary {
                List {
                    Section {
                        VStack(alignment: .leading, spacing: 4) {
                            HStack {
                                Text(s.title.isEmpty ? "Group" : s.title).font(.headline)
                                if amAdmin {
                                    Spacer()
                                    Button("Rename") {
                                        renameText = s.title
                                        showRename = true
                                    }
                                    .font(.subheadline)
                                }
                            }
                            if !s.description.isEmpty {
                                Text(s.description).font(.subheadline).foregroundStyle(.secondary)
                            }
                            Text("Revision \(s.revision)").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    Section("Disappearing messages") {
                        if amAdmin {
                            DisappearingMessagesPicker(seconds: $expirySeconds)
                                .onChange(of: expirySeconds) { _, newValue in
                                    // Ignore the initial seeding assignment; only
                                    // act on a real user change away from the
                                    // loaded value.
                                    if newValue != s.expirySeconds { setExpiry(newValue) }
                                }
                        } else {
                            HStack {
                                Text("Timer")
                                Spacer()
                                Text(DisappearingMessagesPicker.label(for: s.expirySeconds))
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                    Section("Members (\(s.members.count))") {
                        ForEach(orderedMembers(s.members), id: \.encryptedMemberId) { member in
                            HStack(spacing: 10) {
                                ContactAvatar(name: memberName(member), isBot: isBot(member), size: 32)
                                Text(memberName(member))
                                    .lineLimit(1)
                                Spacer()
                                if member.role == 1 {
                                    Text("Admin").font(.caption2)
                                        .padding(.horizontal, 6).padding(.vertical, 2)
                                        .background(Color.accentColor.opacity(0.15))
                                        .clipShape(Capsule())
                                }
                                // Admins get a visible per-member menu to
                                // promote/demote anyone but themselves.
                                if amAdmin && member.did != accountId {
                                    Menu {
                                        if member.role == 1 {
                                            Button {
                                                changeRole(member, toAdmin: false)
                                            } label: {
                                                Label("Remove admin", systemImage: "person.badge.minus")
                                            }
                                        } else {
                                            Button {
                                                changeRole(member, toAdmin: true)
                                            } label: {
                                                Label("Make admin", systemImage: "person.badge.shield.checkmark")
                                            }
                                        }
                                    } label: {
                                        Image(systemName: "ellipsis.circle")
                                            .foregroundStyle(.secondary)
                                    }
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
                    if amMember {
                        Section {
                            Button(role: .destructive) {
                                leaveGroup()
                            } label: {
                                Label("Leave group", systemImage: "rectangle.portrait.and.arrow.right")
                            }
                        }
                    } else {
                        Section {
                            Text("You left this group.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
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
        .alert("Rename group", isPresented: $showRename) {
            TextField("Group name", text: $renameText)
            Button("Cancel", role: .cancel) {}
            Button("Save") { renameGroup(renameText) }
                .disabled(renameText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
        .task { await load() }
    }

    /// Rename the group (admin-only; server-enforced). app-core emits the
    /// "changed the group name to X" timeline entry, so the conversation updates.
    private func renameGroup(_ name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        // Group names must not be empty, and skip a no-op rename.
        guard let core = appState.core(accountId: accountId),
              !trimmed.isEmpty,
              trimmed != (summary?.title ?? "")
        else { return }
        let gid = groupId
        Task {
            do {
                try await Task.detached {
                    try core.setGroupTitle(groupId: gid, newTitle: trimmed)
                }.value
                await load()
            } catch {
                errorMessage = error.localizedDescription
            }
        }
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

    /// Whether a member is a bot, for the hexagon avatar frame
    /// (docs/54-bot-presentation.md). The local user is never a bot.
    private func isBot(_ member: GroupMemberFfi) -> Bool {
        member.did != accountId && appState.isBot(member.did, accountId: accountId)
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
            self.expirySeconds = s.expirySeconds
        } catch {
            // The server fetch is membership-gated, so it 404s for a group we've
            // left (docs/53 §Leave). Fall back to the last-known cached state so
            // the info screen still renders, read-only, instead of erroring.
            let cached = try? await Task.detached(operation: {
                try core.cachedGroupState(groupId: gid)
            }).value
            if let cached {
                self.summary = cached
                self.expirySeconds = cached.expirySeconds
            } else {
                errorMessage = error.localizedDescription
            }
        }
    }

    /// Promote or demote a member (admin-only; server-enforced). The system
    /// timeline entry is emitted by app-core, so the conversation updates too.
    private func changeRole(_ member: GroupMemberFfi, toAdmin: Bool) {
        guard let core = appState.core(accountId: accountId) else { return }
        let gid = groupId
        let emi = member.encryptedMemberId
        let newRole: Int16 = toAdmin ? 1 : 0
        Task {
            do {
                try await Task.detached {
                    try core.changeMemberRole(groupId: gid, encryptedMemberId: emi, newRole: newRole)
                }.value
                await load()
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    /// Change the disappearing-message timer (admin-only; server-enforced).
    private func setExpiry(_ seconds: UInt32) {
        guard let core = appState.core(accountId: accountId) else { return }
        let gid = groupId
        Task {
            do {
                try await Task.detached {
                    try core.setGroupExpiry(groupId: gid, expirySeconds: seconds)
                }.value
                await load()
            } catch {
                errorMessage = error.localizedDescription
                await load()  // revert the picker to the server's value on failure
            }
        }
    }

    private func leaveGroup() {
        guard let core = appState.core(accountId: accountId) else { return }
        let gid = groupId
        Task {
            do {
                // Self-class leave (docs/53): works for any member, not just
                // admins. Tombstone-in-place — the group stays in the inbox,
                // read-only, with a "You left the group" entry as the last
                // message. Reload the timeline so that entry shows, then pop
                // back to the (now read-only) conversation.
                try await Task.detached {
                    try core.leaveGroup(groupId: gid)
                }.value
                appState.reloadGroupTimelineIfLoaded(groupId: gid, accountId: accountId)
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
