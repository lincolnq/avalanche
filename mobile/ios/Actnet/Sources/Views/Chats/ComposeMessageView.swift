import SwiftUI
import UIKit

/// New-conversation composer. A consistent layout — To-field with recipient
/// pills, an always-browsable (and typing-filtered) contact list, and two
/// persistent actions: **DM** (enabled at exactly one recipient; opens the
/// existing or a fresh thread) and **New Group** (always available; routes to
/// the Name Group screen). See `docs/30-mobile-ux.md` §Compose.
///
/// Sending identity comes from the From picker; the header shows that
/// identity's server. True per-contact identity routing (`preferred_identity`)
/// and cross-server founding are deferred — see `docs/02-todos-deferred.md`.
struct ComposeMessageView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.dismiss) private var dismiss

    @State private var chips: [Chip] = []
    @State private var query: String = ""
    @State private var selectedAccountId: String?
    @State private var allContacts: [ContactRowFfi] = []
    @State private var sending: Bool = false
    @State private var errorMessage: String?
    @State private var showingContactPicker = false
    /// Drives the push to the Name Group screen from the New Group button.
    @State private var showNameGroup = false
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

    /// Servers the active identity belongs to (home server first).
    private var activeAccountServers: [ServerInfo] {
        guard let id = activeAccountId,
              let account = accounts.first(where: { $0.id == id }) else { return [] }
        return account.servers
    }

    /// The server a conversation founded right now would live on — the active
    /// identity's home server. Shown in the header and on the Name Group screen.
    private var activeServer: ServerInfo? { activeAccountServers.first }

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

    /// New Group button label: "New Empty Group" with no recipients, otherwise
    /// "New Group (N)" filled with the recipient count.
    private var newGroupTitle: String {
        chips.isEmpty ? "New Empty Group" : "New Group (\(chips.count))"
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
                // Contacts are always browsable; typing filters them in place.
                autocompleteList
                actionBar
            }
            .background(Color.avPaper)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .principal) {
                    VStack(spacing: 0) {
                        Text("New Conversation").font(.headline)
                        if let server = activeServer {
                            Text("at \(server.displayHost)")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        dismiss()
                    } label: {
                        Image(systemName: "xmark")
                    }
                    .accessibilityLabel("Cancel")
                }
            }
            .navigationDestination(isPresented: $showNameGroup) {
                NameGroupView(
                    members: chips,
                    accountId: activeAccountId ?? "",
                    servers: activeAccountServers,
                    onCreated: { conv in
                        appState.navigateToConversation = conv
                        dismiss()
                    }
                )
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
                Text("No more contacts to add.")
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

    /// Two persistent actions. DM is enabled only at exactly one recipient and
    /// is primary in that case; New Group is always available and becomes
    /// primary once there are 2+ recipients (per the redesign).
    private var actionBar: some View {
        VStack(spacing: 0) {
            if let error = errorMessage {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(Color.avError)
                    .padding(.horizontal)
            }
            Divider()
            HStack(spacing: 12) {
                styledButton(prominent: chips.count == 1) {
                    dmTapped()
                } label: {
                    Text("DM").frame(maxWidth: .infinity)
                }
                .disabled(chips.count != 1 || sending)

                styledButton(prominent: chips.count >= 2) {
                    showNameGroup = true
                } label: {
                    Text(newGroupTitle).frame(maxWidth: .infinity)
                }
                .disabled(sending)
            }
            .controlSize(.large)
            .tint(Color.avBrand)
            .padding(.horizontal)
            .padding(.vertical, 8)
        }
    }

    /// Picks `.borderedProminent` vs `.bordered` — the two are distinct types,
    /// so the choice can't be made inline on one `Button`.
    @ViewBuilder
    private func styledButton<L: View>(
        prominent: Bool,
        action: @escaping () -> Void,
        @ViewBuilder label: () -> L
    ) -> some View {
        if prominent {
            Button(action: action, label: label).buttonStyle(.borderedProminent)
        } else {
            Button(action: action, label: label).buttonStyle(.bordered)
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
    ///   `https://go.theavalanche.net/i/<base64url {"d":…}>`  (d = inviter_did)
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
        case "i", "invite":
            guard let data = Data(base64URLEncoded: parts[1]),
                  let payload = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let did = payload["d"] as? String,
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

    /// DM action: jump straight to the thread with the single recipient,
    /// reusing the existing conversation or starting a fresh one. No first
    /// message is sent — the composer just lands the user in the thread.
    private func dmTapped() {
        guard chips.count == 1, let accountId = activeAccountId else { return }
        let conv = appState.findOrCreateDMConversation(
            recipientDid: chips[0].did,
            accountId: accountId
        )
        appState.navigateToConversation = conv
        dismiss()
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

#Preview("Multiple recipients") {
    ComposeMessageView(initialChips: [
        ComposeMessageView.Chip(id: "did:plc:alice", did: "did:plc:alice", displayName: "Alice Rivera"),
        ComposeMessageView.Chip(id: "did:plc:alice2", did: "did:plc:alice", displayName: "Alice Rivera Two"),
        ComposeMessageView.Chip(id: "did:plc:alice3", did: "did:plc:alice", displayName: "Alice Rivera Three"),
        ComposeMessageView.Chip(id: "did:plc:alice4", did: "did:plc:alice", displayName: "Alice Rivera Four")


    ])
    .environmentObject(composePreviewState())
}
#endif
