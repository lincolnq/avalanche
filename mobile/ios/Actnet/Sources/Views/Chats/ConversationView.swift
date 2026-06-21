import SwiftUI

struct ConversationView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.dismiss) private var dismiss
    let conversation: Conversation

    /// The live row from `appState` so request/blocked state stays reactive
    /// after an Accept / Block / Report action; falls back to the passed-in
    /// value (e.g. previews) when not in the list.
    private var liveConv: Conversation {
        appState.conversations.first { $0.id == conversation.id } ?? conversation
    }

    @State private var messageText = ""
    @State private var errorMessage: String?
    @State private var scrollPosition = ScrollPosition(idType: Int64.self)
    /// Non-nil while editing an existing message (docs/36); the composer turns
    /// into an edit bar prefilled with its body.
    @State private var editingMessage: Message?
    /// The message whose edit-history sheet is showing, plus its loaded revisions.
    @State private var historyMessage: Message?
    @State private var historyRevisions: [MessageRevisionFfi] = []
    /// Whether we're still a member of this group (docs/53 §Leave). Non-members
    /// keep the readable transcript but lose the composer. Always true for DMs.
    /// Loaded on appear and after leaving.
    @State private var isGroupMember = true

    private var messages: [Message] {
        appState.messagesByConversation[conversation.id] ?? []
    }

    /// Reactions/editing/deletion ride the `ContentMessage` envelope, which now
    /// wraps group content too — so the long-press actions work in both DMs and
    /// groups.
    private var actionsEnabled: Bool { true }

    /// Human edit/delete-for-everyone window (docs/36): 24h from send.
    private static let editWindowMs: Int64 = 24 * 60 * 60 * 1000

    private func canEdit(_ message: Message) -> Bool {
        message.senderAccountId == conversation.accountId
            && !message.isDeleted
            && (Int64(Date().timeIntervalSince1970 * 1000) - message.sentAtMs) <= Self.editWindowMs
    }

    /// Whether an incoming message's sender is a bot, for the octagon-ish
    /// bubble shape (docs/54-bot-presentation.md). Own messages are never bots.
    private func isBotSender(_ message: Message) -> Bool {
        message.senderAccountId != conversation.accountId
            && appState.isBot(message.senderAccountId, accountId: conversation.accountId)
    }

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(messages) { message in
                        if message.isSystemEvent {
                            // Group membership/metadata event (docs/03 §3.6) —
                            // a centered grey line, not a chat bubble.
                            GroupSystemEventRow(
                                text: appState.groupEventText(message, accountId: conversation.accountId)
                            )
                            .id(message.sentAtMs)
                        } else {
                            MessageBubble(
                                message: message,
                                isMe: message.senderAccountId == conversation.accountId,
                                isBot: isBotSender(message),
                                reactions: appState.reactions(for: message),
                                myDid: conversation.accountId,
                                actionsEnabled: actionsEnabled,
                                canEdit: canEdit(message),
                                onToggleReaction: { emoji in
                                    appState.toggleReaction(message: message, emoji: emoji, conversation: conversation)
                                },
                                onEdit: { startEditing(message) },
                                onDelete: { forEveryone in
                                    appState.deleteMessage(message: message, forEveryone: forEveryone, conversation: conversation)
                                },
                                onShowHistory: { showHistory(message) }
                            )
                            .id(message.sentAtMs)
                        }
                    }
                }
                .padding()
            }
            .defaultScrollAnchor(.bottom)
            .scrollPosition($scrollPosition)
            .onScrollTargetVisibilityChange(idType: Int64.self) { visibleIDs in
                guard scenePhase == .active, let threshold = visibleIDs.last else { return }
                appState.markMessagesReadUpTo(
                    sentAtMs: threshold,
                    conversationId: conversation.id,
                    accountId: conversation.accountId
                )
            }
            .onChange(of: messages.count) {
                guard !messages.isEmpty else { return }
                scrollPosition.scrollTo(edge: .bottom)
                appState.markAllMessagesRead(conversationId: conversation.id, accountId: conversation.accountId)
            }

            if let error = errorMessage {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(Color.avError)
                    .padding(.horizontal)
            }

            Divider()

            // Bottom bar: a blocked DM shows an unblock prompt, an un-accepted
            // request shows the Accept/Delete/Report gate (docs/12 §1), and an
            // accepted DM or group shows the normal composer.
            if liveConv.isBlocked, let did = liveConv.recipientDid {
                blockedBar(did: did)
            } else if liveConv.isRequest, let did = liveConv.recipientDid {
                messageRequestGate(did: did)
            } else if conversation.isGroup && !isGroupMember {
                leftGroupBar
            } else {
                composer
            }
        }
        .background(Color.avPaper)
        .navigationTitle(conversation.title)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            // For groups, the centered title + avatar is a tappable link into
            // the group detail screen. (DMs keep the plain navigationTitle.)
            if conversation.isGroup, let groupId = conversation.groupId {
                ToolbarItem(placement: .principal) {
                    NavigationLink {
                        GroupDetailView(groupId: groupId, accountId: conversation.accountId)
                    } label: {
                        HStack(spacing: 8) {
                            ContactAvatar(name: conversation.title, size: 28)
                            Text(conversation.title)
                                .font(.headline)
                                .foregroundStyle(.primary)
                        }
                    }
                }
            }
        }
        .sheet(item: $historyMessage) { msg in
            EditHistorySheet(current: msg, revisions: historyRevisions)
        }
        .onAppear {
            appState.currentConversationId = conversation.id
            appState.loadMessagesFromStore(conversationId: conversation.id, accountId: conversation.accountId)
            appState.loadReactions(conversationId: conversation.id, accountId: conversation.accountId)
            appState.markAllMessagesRead(conversationId: conversation.id, accountId: conversation.accountId)
            // Re-fetch the contact's encrypted profile and update the cached
            // display name if it changed. Primary change-detection path.
            if let recipientDid = conversation.recipientDid {
                appState.refreshContactProfile(did: recipientDid, accountId: conversation.accountId)
            }
            if let groupId = conversation.groupId {
                appState.refreshGroupTitle(groupId: groupId, accountId: conversation.accountId)
                Task {
                    isGroupMember = await appState.isGroupMember(
                        groupId: groupId, accountId: conversation.accountId
                    )
                }
            }
        }
        .onDisappear {
            if appState.currentConversationId == conversation.id {
                appState.currentConversationId = nil
            }
        }
        .task(id: conversation.id) {
            // After messages load, scroll to first unread (or bottom if all read).
            try? await Task.sleep(nanoseconds: 100_000_000)
            let msgs = appState.messagesByConversation[conversation.id] ?? []
            if let firstUnread = msgs.first(where: {
                $0.readAtMs == nil && $0.senderAccountId != conversation.accountId
            }) {
                scrollPosition.scrollTo(id: firstUnread.sentAtMs)
            } else {
                scrollPosition.scrollTo(edge: .bottom)
            }
        }
    }

    /// Shown in place of the composer once you've left the group (docs/53 §Leave).
    /// The transcript stays readable above; you just can't post. The "You left
    /// the group" line is the last entry in the transcript itself.
    @ViewBuilder private var leftGroupBar: some View {
        Text("You left this group")
            .font(.caption)
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity)
            .padding(.horizontal)
            .padding(.vertical, 12)
    }

    /// The normal text composer (with the inline edit bar when editing).
    @ViewBuilder private var composer: some View {
        if editingMessage != nil {
            HStack(spacing: 8) {
                Image(systemName: "pencil")
                    .foregroundStyle(Color.avBrand)
                Text("Editing message")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Button { cancelEdit() } label: {
                    Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary)
                }
            }
            .padding(.horizontal)
            .padding(.top, 6)
        }

        HStack(spacing: 12) {
            TextField(editingMessage == nil ? "Message" : "Edit message", text: $messageText, axis: .vertical)
                .textFieldStyle(.plain)
                .lineLimit(1...5)

            Button {
                if editingMessage != nil { applyEdit() } else { sendMessage() }
            } label: {
                Image(systemName: editingMessage != nil ? "checkmark.circle.fill" : "arrow.up.circle.fill")
                    .font(.title2)
            }
            .disabled(messageText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .onChange(of: messageText) {
            if !messages.isEmpty {
                scrollPosition.scrollTo(edge: .bottom)
            }
        }
    }

    /// The message-request gate (docs/12 §1): a stranger's first contact is
    /// read-only until the user Accepts, Deletes, or Reports & Blocks. Reporting
    /// is exposed only here — not in established conversations.
    @ViewBuilder private func messageRequestGate(did: String) -> some View {
        VStack(spacing: 10) {
            Text("Let \(conversation.title) message you and share your name with them?")
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
            HStack(spacing: 12) {
                Button(role: .destructive) {
                    Task {
                        await appState.reportAndBlock(did: did, accountId: conversation.accountId)
                    }
                } label: {
                    Text("Block").frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)

                Button(role: .destructive) {
                    Task {
                        await appState.deleteRequest(did: did, accountId: conversation.accountId)
                        dismiss()
                    }
                } label: {
                    Text("Delete").frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)

                Button {
                    Task { await appState.acceptRequest(did: did, accountId: conversation.accountId) }
                } label: {
                    Text("Accept").frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 10)
    }

    /// Shown in place of the composer for a blocked DM (docs/12 §2).
    @ViewBuilder private func blockedBar(did: String) -> some View {
        HStack(spacing: 12) {
            Text("You blocked this contact.")
                .font(.caption)
                .foregroundStyle(.secondary)
            Spacer()
            Button("Unblock") {
                Task { await appState.unblockContact(did: did, accountId: conversation.accountId) }
            }
            .buttonStyle(.bordered)
        }
        .padding(.horizontal)
        .padding(.vertical, 10)
    }

    private func startEditing(_ message: Message) {
        editingMessage = message
        messageText = message.body
    }

    private func cancelEdit() {
        editingMessage = nil
        messageText = ""
    }

    private func applyEdit() {
        guard let message = editingMessage else { return }
        appState.editMessage(message: message, newBody: messageText, conversation: conversation)
        editingMessage = nil
        messageText = ""
    }

    private func showHistory(_ message: Message) {
        Task {
            historyRevisions = await appState.loadMessageRevisions(message: message, conversation: conversation)
            historyMessage = message
        }
    }

    private func sendMessage() {
        guard !messageText.isEmpty else { return }
        let text = messageText
        messageText = ""
        errorMessage = nil

        // Optimistically add to UI.
        let messageId = UUID().uuidString
        let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
        let message = Message(
            id: messageId,
            conversationId: conversation.id,
            senderAccountId: conversation.accountId,
            body: text,
            sentAtMs: nowMs,
            readAtMs: nowMs,  // outgoing = immediately read
            deliveryStatus: .sending
        )
        appState.messagesByConversation[conversation.id, default: []].append(message)
        scrollPosition.scrollTo(edge: .bottom)

        // Update conversation metadata for sorting + the chat-list preview.
        // Clear any stale system-event fields so the preview renders this new
        // message, not a prior "X joined" / metadata line.
        if let idx = appState.conversations.firstIndex(where: { $0.id == conversation.id }) {
            appState.conversations[idx].lastMessage = text
            appState.conversations[idx].lastMessageDate = message.sentAt
            appState.conversations[idx].lastMessageSenderDid = conversation.accountId  // "You:"
            appState.conversations[idx].clearLastMessageEvent()
        }

        Task {
            do {
                if conversation.isGroup {
                    try await appState.sendGroupMessage(
                        conversation: conversation,
                        text: text,
                        messageId: messageId,
                        sentAtMs: nowMs
                    )
                } else {
                    guard let recipientDid = conversation.recipientDid else {
                        errorMessage = "Cannot send: no recipient"
                        return
                    }
                    try await appState.sendMessage(
                        conversationId: conversation.id,
                        text: text,
                        recipientDid: recipientDid,
                        senderAccountId: conversation.accountId,
                        messageId: messageId,
                        sentAtMs: nowMs
                    )
                }
            } catch {
                errorMessage = "Failed to send: \(error.localizedDescription)"
            }
        }
    }
}

#if DEBUG
/// Wraps `ConversationView` in a preview-ready environment: one account, a
/// canned contact for name resolution, and pre-seeded messages (which survive
/// `loadMessagesFromStore`, since it only loads when the cache is empty).
@MainActor
private func conversationPreview(_ conversation: Conversation, _ messages: [Message]) -> some View {
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
    let state = AppState.preview(
        accounts: [me],
        contacts: [
            ContactRowFfi(did: "did:plc:bob", displayName: "Bob Chena", isCurated: true, lastInteractionAtMs: 0),
        ]
    )
    state.conversations = [conversation]
    state.messagesByConversation[conversation.id] = messages
    return NavigationStack {
        ConversationView(conversation: conversation)
            .environmentObject(state)
    }
}

#Preview("DM") {
    let conv = Conversation(
        id: "dm-bob",
        title: "Bob Chena",
        accountId: "did:plc:me",
        serverUrl: "https://server.example",
        recipientDid: "did:plc:bob",
        groupId: nil,
        lastMessage: nil,
        lastMessageDate: nil
    )
    return conversationPreview(conv, [
        Message(id: "m1", conversationId: conv.id, senderAccountId: "did:plc:bob",
                body: "Are we still meeting at noon?", sentAtMs: 1_700_000_000_000,
                editedAtMs: nil, readAtMs: 1_700_000_001_000, deliveryStatus: .delivered),
        Message(id: "m2", conversationId: conv.id, senderAccountId: "did:plc:me",
                body: "Yes — I'll be at the front entrance.", sentAtMs: 1_700_000_060_000,
                editedAtMs: nil, readAtMs: 1_700_000_061_000, deliveryStatus: .read),
    ])
}

#Preview("Message Request") {
    let conv = Conversation(
        id: "dm-stranger",
        title: "Jordan Vale",
        accountId: "did:plc:me",
        serverUrl: "https://server.example",
        recipientDid: "did:plc:stranger",
        groupId: nil,
        lastMessage: nil,
        lastMessageDate: nil,
        isRequest: true
    )
    return conversationPreview(conv, [
        Message(id: "m1", conversationId: conv.id, senderAccountId: "did:plc:stranger",
                body: "Hi! I saw you at the rally — want to join our organizing channel?",
                sentAtMs: 1_700_000_000_000,
                editedAtMs: nil, readAtMs: nil, deliveryStatus: .delivered),
    ])
}

#Preview("Blocked") {
    let conv = Conversation(
        id: "dm-blocked",
        title: "Jordan Vale",
        accountId: "did:plc:me",
        serverUrl: "https://server.example",
        recipientDid: "did:plc:stranger",
        groupId: nil,
        lastMessage: nil,
        lastMessageDate: nil,
        isBlocked: true
    )
    return conversationPreview(conv, [
        Message(id: "m1", conversationId: conv.id, senderAccountId: "did:plc:stranger",
                body: "Hi! I saw you at the rally — want to join our organizing channel?",
                sentAtMs: 1_700_000_000_000,
                editedAtMs: nil, readAtMs: nil, deliveryStatus: .delivered),
    ])
}

#Preview("Group") {
    let gid = "grp1"
    let conv = Conversation(
        id: groupConversationId(gid),
        title: "March Logistics",
        accountId: "did:plc:me",
        serverUrl: "https://server.example",
        recipientDid: nil,
        groupId: gid,
        lastMessage: nil,
        lastMessageDate: nil,
        isGroup: true
    )
    return conversationPreview(conv, [
        Message(id: "m1", conversationId: conv.id, senderAccountId: "did:plc:bob",
                body: "Crew — check in when you arrive.", sentAtMs: 1_700_000_000_000,
                editedAtMs: nil, readAtMs: 1_700_000_001_000, deliveryStatus: .delivered),
        Message(id: "m2", conversationId: conv.id, senderAccountId: "did:plc:me",
                body: "On site 👍", sentAtMs: 1_700_000_060_000,
                editedAtMs: nil, readAtMs: 1_700_000_061_000, deliveryStatus: .read),
    ])
}
#endif

/// A centered grey system line in the conversation timeline for a group
/// membership/metadata event (docs/03 §3.6) — "Alice added Bob", "Bob left", etc.
struct GroupSystemEventRow: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .multilineTextAlignment(.center)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 4)
            .accessibilityAddTraits(.isStaticText)
    }
}
