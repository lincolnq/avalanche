import SwiftUI
import UIKit

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
    @State private var showingContactPicker = false
    /// Lets the autocomplete / DID-submit paths push a chip into the
    /// `UITextView`-backed recipient field, which owns the chip content.
    @StateObject private var fieldHandle = RecipientFieldHandle()

    /// A confirmed recipient. `displayName` may be empty when the user typed
    /// a raw DID we haven't seen before; `label` falls back to a truncated
    /// DID in that case.
    struct Chip: Identifiable, Hashable {
        let id: String  // == did
        let did: String
        let displayName: String

        /// User-visible text for the chip. Never a raw full DID.
        var label: String { displayName.isEmpty ? shortenDid(did) : displayName }
    }

    init(initialChips: [Chip] = []) {
        _chips = State(initialValue: initialChips)
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
        let names = chips.map(\.label)
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
        // Recognize a contact link pasted into the recipient field (the link
        // another user's app generates) and turn it into a chip.
        .onChange(of: query) { _, newValue in
            let trimmed = newValue.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else { return }
            _ = handleContactLink(trimmed)
        }
        .sheet(isPresented: $showingContactPicker) {
            ContactPickerSheet(
                contacts: allContacts,
                excludedDids: Set(chips.map(\.did)),
                nameFor: contactName,
                isBotFor: isBot,
                onSelect: { c in addChip(did: c.did, displayName: contactName(c)) },
                onScanLink: handleContactLink
            )
        }
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
            HStack(alignment: .top, spacing: 8) {
                RecipientTokenField(
                    chips: $chips,
                    query: $query,
                    prefix: "To:",
                    placeholder: "Type a name",
                    handle: fieldHandle,
                    onSubmit: commitQueryAsChip
                )
                .frame(maxWidth: .infinity, alignment: .leading)

                Button {
                    showingContactPicker = true
                } label: {
                    Image(systemName: "plus.circle")
                        .font(.title2)
                        .foregroundStyle(Color.avBrand)
                }
                .accessibilityLabel("Add recipient")
            }

            if chips.count >= 2 {
                HStack {
                    Text("Group:").font(.caption).foregroundStyle(.secondary)
                    Text(groupNameAutoDefault).font(.caption).lineLimit(1)
                }
                .padding(.top, 2)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
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
            addChip(did: c.did, displayName: contactName(c))
        } label: {
            HStack(spacing: 10) {
                ContactAvatar(name: contactName(c), isBot: isBot(c), size: 32)
                Text(contactName(c))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
            }
        }
    }

    /// Whether a contact is a bot, for the hexagon avatar frame
    /// (docs/54-bot-presentation.md). Resolves through the same shared
    /// `AppState` path as the name.
    private func isBot(_ c: ContactRowFfi) -> Bool {
        guard let id = activeAccountId else { return false }
        return appState.isBot(c.did, accountId: id)
    }

    /// The name to show for a contact. Resolves through the shared
    /// `AppState.resolvedName` path so humans (cached profile) and bots
    /// (server record) render identically; the contact-list rows' own
    /// `displayName` is seeded into that cache in `loadContacts`. Never a DID.
    /// (User-set overrides — nickname/photo, docs/35 — slot in here once
    /// stored.)
    private func contactName(_ c: ContactRowFfi) -> String {
        guard let id = activeAccountId else {
            return c.displayName.isEmpty ? "Unknown" : c.displayName
        }
        return appState.resolvedName(for: c.did, accountId: id)
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
            // Feed the names we already have into the shared resolver so it
            // doesn't re-fetch them; bots (no cached profile name) fall through
            // to the server lookup via `resolvedName`.
            for c in rows {
                appState.cacheDisplayName(c.displayName, for: c.did)
            }
        }
    }

    /// Add a recipient. The token field owns the chip content (dedup, clearing
    /// the typed query, selection), then mirrors the result back into `chips` /
    /// `query`.
    private func addChip(did: String, displayName: String) {
        fieldHandle.addChip(Chip(id: did, did: did, displayName: displayName))
    }

    /// If `raw` is an Avalanche contact link (QR payload or pasted URL), add the
    /// recipient it points at as a chip and report success. Both link shapes
    /// another user's app can produce carry a DID:
    ///   `https://go.theavalanche.net/conversation/<did>`
    ///   `https://go.theavalanche.net/invite/<base64url {"inviter_did":…}>`
    @discardableResult
    private func handleContactLink(_ raw: String) -> Bool {
        guard let did = Self.recipientDid(fromContactLink: raw) else { return false }
        guard !chips.contains(where: { $0.did == did }) else { return true }
        addChip(did: did, displayName: "")
        return true
    }

    /// Extract a recipient DID from a contact link, or `nil` if it isn't one.
    /// Decodes the invite token locally — we only need the DID to make a chip,
    /// not full server validation.
    static func recipientDid(fromContactLink raw: String) -> String? {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URL(string: trimmed), AppState.isDeepLink(url) else { return nil }
        let parts = url.pathComponents.filter { $0 != "/" }
        guard parts.count >= 2 else { return nil }
        switch parts[0] {
        case "conversation":
            return parts[1].hasPrefix("did:") ? parts[1] : nil
        case "invite":
            guard let data = Data(base64URLEncoded: parts[1]),
                  let payload = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let did = payload["inviter_did"] as? String,
                  did.hasPrefix("did:") else { return nil }
            return did
        default:
            return nil
        }
    }

    private func commitQueryAsChip() {
        if queryLooksLikeDid {
            addChip(did: trimmedQuery, displayName: "")
        } else if let first = peopleResults.first ?? otherResults.first {
            addChip(did: first.did, displayName: contactName(first))
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

}

/// Full-list contact picker presented from the recipient field's "+" button.
/// Mirrors the inline autocomplete's People / Other split, but always shows the
/// whole curated/known set (filterable) rather than reacting to the typed query.
/// Selecting a contact adds it as a chip and dismisses.
private struct ContactPickerSheet: View {
    let contacts: [ContactRowFfi]
    let excludedDids: Set<String>
    let nameFor: (ContactRowFfi) -> String
    let isBotFor: (ContactRowFfi) -> Bool
    let onSelect: (ContactRowFfi) -> Void
    /// Adds the recipient encoded in a scanned/pasted contact link; returns
    /// false if the payload isn't a recognizable Avalanche contact link.
    let onScanLink: (String) -> Bool

    @Environment(\.dismiss) private var dismiss
    @State private var search = ""
    @State private var showingScanner = false
    @State private var scanError: String?

    private var filtered: [ContactRowFfi] {
        let q = search.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return contacts.filter { c in
            guard !excludedDids.contains(c.did) else { return false }
            guard !q.isEmpty else { return true }
            return nameFor(c).lowercased().contains(q) || c.did.lowercased().contains(q)
        }
    }

    private var people: [ContactRowFfi] { filtered.filter(\.isCurated) }
    private var other: [ContactRowFfi] { filtered.filter { !$0.isCurated } }

    var body: some View {
        NavigationStack {
            List {
                Button {
                    scanError = nil
                    showingScanner = true
                } label: {
                    HStack(spacing: 10) {
                        Image(systemName: "qrcode.viewfinder")
                            .font(.title3)
                            .frame(width: 32, height: 32)
                            .foregroundStyle(Color.avBrand)
                        Text("Scan QR Code")
                            .foregroundStyle(Color.avBrand)
                    }
                }
                if !people.isEmpty {
                    Section("People") {
                        ForEach(people, id: \.did, content: row)
                    }
                }
                if !other.isEmpty {
                    Section("Other") {
                        ForEach(other, id: \.did, content: row)
                    }
                }
                if filtered.isEmpty {
                    Text("No contacts to add.")
                        .foregroundStyle(.secondary)
                        .font(.footnote)
                }
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .background(Color.avPaper)
            .searchable(text: $search, prompt: "Search contacts")
            .navigationTitle("Add Recipient")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        dismiss()
                    } label: {
                        Image(systemName: "xmark")
                    }
                    .accessibilityLabel("Close")
                }
            }
            .sheet(isPresented: $showingScanner) {
                NavigationStack {
                    QRScannerView { value in
                        showingScanner = false
                        if onScanLink(value) {
                            dismiss()
                        } else {
                            scanError = "That QR code isn't an Avalanche contact link."
                        }
                    }
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) {
                            Button("Cancel") { showingScanner = false }
                        }
                    }
                }
            }
            .alert("Couldn't add contact", isPresented: .constant(scanError != nil)) {
                Button("OK") { scanError = nil }
            } message: {
                Text(scanError ?? "")
            }
        }
    }

    private func row(_ c: ContactRowFfi) -> some View {
        Button {
            onSelect(c)
            dismiss()
        } label: {
            HStack(spacing: 10) {
                ContactAvatar(name: nameFor(c), isBot: isBotFor(c), size: 32)
                Text(nameFor(c))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
            }
        }
    }
}

#if DEBUG
/// Shared preview environment: one account plus a spread of contact cases —
/// curated humans, a human whose profile hasn't resolved yet (→ "Unknown"), and
/// a bot whose name lives server-side and resolves through the same path as
/// humans (the normalization this view relies on).
@MainActor
private func composePreviewState() -> AppState {
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
        ContactRowFfi(did: "did:plc:alice", displayName: "Alice Rivera", isCurated: true, lastInteractionAtMs: 0),
        ContactRowFfi(did: "did:plc:bob", displayName: "Bob Chena", isCurated: true, lastInteractionAtMs: 0),
        ContactRowFfi(did: "did:plc:carol", displayName: "Carol X", isCurated: false, lastInteractionAtMs: 0),
        // Empty local name: a bot resolves its name server-side (via
        // `getAccountInfo`), which is also where `isBot` comes from — so this
        // row exercises the bot avatar chrome (docs/54-bot-presentation.md).
        ContactRowFfi(did: "did:local:adminbot", displayName: "", isCurated: false, lastInteractionAtMs: 0),
    ]
    return AppState.preview(
        accounts: [me],
        contacts: contacts,
        botNames: ["did:local:adminbot": "Adminbot"]
    )
}

#Preview("Empty") {
    ComposeMessageView()
        .environmentObject(composePreviewState())
}

#Preview("One recipient") {
    ComposeMessageView(initialChips: [
        ComposeMessageView.Chip(id: "did:plc:alice", did: "did:plc:alice", displayName: "Alice Rivera"),
        ComposeMessageView.Chip(id: "did:plc:alice2", did: "did:plc:alice", displayName: "Alice Rivera Two"),
        ComposeMessageView.Chip(id: "did:plc:alice3", did: "did:plc:alice", displayName: "Alice Rivera Three"),
        ComposeMessageView.Chip(id: "did:plc:alice4", did: "did:plc:alice", displayName: "Alice Rivera Four")


    ])
    .environmentObject(composePreviewState())
}
#endif
