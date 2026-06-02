import SwiftUI

/// New-message composer. Single flow for DMs (1 recipient) and groups (2+
/// recipients), per `docs/30-mobile-ux.md` §Compose. This is the "core" slice
/// from that doc: chip field, autocomplete from the local contact table,
/// direct DID entry. The From pill, server-pinning, and yellow/red chips for
/// cross-server / unreachable recipients are deferred.
struct ComposeMessageView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.dismiss) private var dismiss

    @State private var chips: [Chip] = []
    @State private var query: String = ""
    @State private var messageText: String = ""
    @State private var selectedAccountId: String?
    @State private var allContacts: [ContactRowFfi] = []
    @State private var sending: Bool = false
    @State private var errorMessage: String?

    /// A confirmed recipient. `displayName` may be empty when the user typed
    /// a raw DID we haven't seen before; the chip falls back to a truncated
    /// DID in that case.
    struct Chip: Identifiable, Hashable {
        let id: String  // == did
        let did: String
        let displayName: String
    }

    private var accounts: [Account] { appState.accounts }
    private var activeAccountId: String? { selectedAccountId ?? accounts.first?.id }

    private var serverUrlForActiveAccount: String {
        guard let id = activeAccountId,
              let account = accounts.first(where: { $0.id == id }),
              let server = account.servers.first else { return "" }
        return server.id
    }

    private var trimmedQuery: String {
        query.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var queryLooksLikeDid: Bool {
        trimmedQuery.hasPrefix("did:")
    }

    /// People = curated contacts (docs/35). Filtered by query if present.
    private var peopleResults: [ContactRowFfi] {
        let q = trimmedQuery.lowercased()
        return allContacts.filter { c in
            guard c.isCurated, !chips.contains(where: { $0.did == c.did }) else { return false }
            guard !q.isEmpty else { return true }
            return c.displayName.lowercased().contains(q) || c.did.lowercased().contains(q)
        }
    }

    /// Other = every other contact row. Behaves like a discovery surface.
    private var otherResults: [ContactRowFfi] {
        let q = trimmedQuery.lowercased()
        return allContacts.filter { c in
            guard !c.isCurated, !chips.contains(where: { $0.did == c.did }) else { return false }
            guard !q.isEmpty else { return true }
            return c.displayName.lowercased().contains(q) || c.did.lowercased().contains(q)
        }
    }

    private var groupNameAutoDefault: String {
        let names = chips.map { $0.displayName.isEmpty ? shortDid($0.did) : $0.displayName }
        switch names.count {
        case 0: return ""
        case 1: return names[0]
        case 2: return "\(names[0]), \(names[1])"
        case 3: return "\(names[0]), \(names[1]), \(names[2])"
        default:
            let prefix = names.prefix(2).joined(separator: ", ")
            return "\(prefix) & \(names.count - 2) others"
        }
    }

    private var canSend: Bool {
        !chips.isEmpty && !messageText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !sending
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                if accounts.count > 1 {
                    accountPicker
                    Divider()
                }
                recipientField
                Divider()
                if chips.isEmpty || !trimmedQuery.isEmpty {
                    autocompleteList
                } else {
                    Spacer()
                }
                composerBar
            }
            .background(Color.avPaper)
            .navigationTitle(chips.count >= 2 ? "New Group" : "New Message")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
        .task { await loadContacts() }
    }

    private var accountPicker: some View {
        HStack {
            Text("From").foregroundStyle(.secondary)
            Picker("Account", selection: $selectedAccountId) {
                ForEach(accounts) { account in
                    Text(account.displayName).tag(Optional(account.id))
                }
            }
            .pickerStyle(.menu)
            Spacer()
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
    }

    private var recipientField: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("To:").font(.caption).foregroundStyle(.secondary)
            FlowLayout(spacing: 6) {
                ForEach(chips) { chip in
                    HStack(spacing: 4) {
                        Text(chip.displayName.isEmpty ? shortDid(chip.did) : chip.displayName)
                            .lineLimit(1)
                        Button {
                            chips.removeAll { $0.id == chip.id }
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                                .foregroundStyle(.secondary)
                        }
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(Color.accentColor.opacity(0.15))
                    .clipShape(Capsule())
                }
                TextField(chips.isEmpty ? "Type a name or DID" : "", text: $query)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                    .frame(minWidth: 120)
                    .onSubmit { commitQueryAsChip() }
            }
            if chips.count >= 2 {
                HStack {
                    Text("Group:").font(.caption).foregroundStyle(.secondary)
                    Text(groupNameAutoDefault).font(.caption).lineLimit(1)
                }
                .padding(.top, 2)
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 10)
    }

    private var autocompleteList: some View {
        List {
            if queryLooksLikeDid && !trimmedQuery.isEmpty {
                Button {
                    addChip(did: trimmedQuery, displayName: "")
                } label: {
                    HStack {
                        Image(systemName: "person.crop.circle.badge.plus")
                        Text("Add \(trimmedQuery)").lineLimit(1)
                    }
                }
            }
            if !peopleResults.isEmpty {
                Section("People") {
                    ForEach(peopleResults, id: \.did) { c in
                        contactRow(c)
                    }
                }
            }
            if !otherResults.isEmpty {
                Section("Other") {
                    ForEach(otherResults, id: \.did) { c in
                        contactRow(c)
                    }
                }
            }
            if peopleResults.isEmpty && otherResults.isEmpty && !queryLooksLikeDid {
                Text("Type a DID, or wait — anyone you message will appear here.")
                    .foregroundStyle(.secondary)
                    .font(.footnote)
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(Color.avPaper)
    }

    private func contactRow(_ c: ContactRowFfi) -> some View {
        Button {
            let name = c.displayName.isEmpty ? shortDid(c.did) : c.displayName
            addChip(did: c.did, displayName: name)
        } label: {
            VStack(alignment: .leading) {
                Text(c.displayName.isEmpty ? shortDid(c.did) : c.displayName)
                    .foregroundStyle(.primary)
                if !c.displayName.isEmpty {
                    Text(c.did).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
            }
        }
    }

    private var composerBar: some View {
        VStack(spacing: 0) {
            if let error = errorMessage {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(Color.avError)
                    .padding(.horizontal)
            }
            Divider()
            HStack(spacing: 12) {
                TextField("Message", text: $messageText, axis: .vertical)
                    .textFieldStyle(.plain)
                    .lineLimit(1...5)
                Button {
                    sendTapped()
                } label: {
                    Image(systemName: "arrow.up.circle.fill")
                        .font(.title2)
                }
                .disabled(!canSend)
            }
            .padding(.horizontal)
            .padding(.vertical, 8)
        }
    }

    private func loadContacts() async {
        guard let id = activeAccountId else { return }
        let rows = await appState.listContacts(accountId: id)
        await MainActor.run {
            self.allContacts = rows
        }
    }

    private func addChip(did: String, displayName: String) {
        guard !chips.contains(where: { $0.did == did }) else {
            query = ""
            return
        }
        chips.append(Chip(id: did, did: did, displayName: displayName))
        query = ""
    }

    private func commitQueryAsChip() {
        if queryLooksLikeDid {
            addChip(did: trimmedQuery, displayName: "")
        } else if let first = peopleResults.first ?? otherResults.first {
            let name = first.displayName.isEmpty ? shortDid(first.did) : first.displayName
            addChip(did: first.did, displayName: name)
        }
    }

    private func sendTapped() {
        guard let accountId = activeAccountId else { return }
        let body = messageText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !body.isEmpty, !chips.isEmpty else { return }
        sending = true
        errorMessage = nil

        Task {
            defer { sending = false }
            do {
                if chips.count == 1 {
                    // DM: reuse the existing thread (or create one) and tail-append.
                    let conv = appState.findOrCreateDMConversation(
                        recipientDid: chips[0].did,
                        accountId: accountId
                    )
                    let messageId = UUID().uuidString
                    let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
                    // Append optimistically so the thread renders mid-flight.
                    let optimistic = Message(
                        id: messageId,
                        conversationId: conv.id,
                        senderAccountId: accountId,
                        body: body,
                        sentAtMs: nowMs,
                        readAtMs: nowMs,
                        deliveryStatus: .sending
                    )
                    appState.messagesByConversation[conv.id, default: []].append(optimistic)
                    if let idx = appState.conversations.firstIndex(where: { $0.id == conv.id }) {
                        appState.conversations[idx].lastMessage = body
                        appState.conversations[idx].lastMessageDate = Date()
                    }
                    try await appState.sendMessage(
                        conversationId: conv.id,
                        text: body,
                        recipientDid: chips[0].did,
                        senderAccountId: accountId,
                        messageId: messageId,
                        sentAtMs: nowMs
                    )
                    appState.navigateToConversation = conv
                } else {
                    let conv = try await appState.createGroupAndOpen(
                        accountId: accountId,
                        serverUrl: serverUrlForActiveAccount,
                        title: groupNameAutoDefault,
                        recipientDids: chips.map { $0.did },
                        firstMessage: body
                    )
                    appState.navigateToConversation = conv
                }
                dismiss()
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    private func shortDid(_ did: String) -> String {
        if did.count > 18 { return String(did.prefix(12)) + "…" + String(did.suffix(4)) }
        return did
    }
}

/// Minimal iOS-16+ flow layout (rows of chips). SwiftUI doesn't ship one
/// out of the box; this is a tiny custom Layout that wraps children onto
/// new lines as horizontal space runs out.
private struct FlowLayout: Layout {
    var spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout Void) -> CGSize {
        let width = proposal.width ?? .infinity
        return layout(subviews: subviews, in: width).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout Void) {
        let placements = layout(subviews: subviews, in: bounds.width).placements
        for (idx, p) in placements.enumerated() {
            subviews[idx].place(
                at: CGPoint(x: bounds.minX + p.x, y: bounds.minY + p.y),
                proposal: ProposedViewSize(width: p.w, height: p.h)
            )
        }
    }

    private func layout(subviews: Subviews, in width: CGFloat) -> (size: CGSize, placements: [(x: CGFloat, y: CGFloat, w: CGFloat, h: CGFloat)]) {
        var x: CGFloat = 0
        var y: CGFloat = 0
        var lineHeight: CGFloat = 0
        var maxX: CGFloat = 0
        var placements: [(x: CGFloat, y: CGFloat, w: CGFloat, h: CGFloat)] = []
        for sv in subviews {
            let size = sv.sizeThatFits(.unspecified)
            if x + size.width > width && x > 0 {
                x = 0
                y += lineHeight + spacing
                lineHeight = 0
            }
            placements.append((x: x, y: y, w: size.width, h: size.height))
            x += size.width + spacing
            maxX = max(maxX, x - spacing)
            lineHeight = max(lineHeight, size.height)
        }
        return (CGSize(width: maxX, height: y + lineHeight), placements)
    }
}
