import SwiftUI

struct ConversationView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.scenePhase) private var scenePhase
    let conversation: Conversation

    @State private var messageText = ""
    @State private var errorMessage: String?
    @State private var scrollPosition = ScrollPosition(idType: Int64.self)

    private var messages: [Message] {
        appState.messagesByConversation[conversation.id] ?? []
    }

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(messages) { message in
                        MessageBubble(
                            message: message,
                            isMe: message.senderAccountId == conversation.accountId
                        )
                        .id(message.sentAtMs)
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

            HStack(spacing: 12) {
                TextField("Message", text: $messageText, axis: .vertical)
                    .textFieldStyle(.plain)
                    .lineLimit(1...5)

                Button {
                    sendMessage()
                } label: {
                    Image(systemName: "arrow.up.circle.fill")
                        .font(.title2)
                }
                .disabled(messageText.isEmpty)
            }
            .padding(.horizontal)
            .padding(.vertical, 8)
            .onChange(of: messageText) {
                if !messages.isEmpty {
                    scrollPosition.scrollTo(edge: .bottom)
                }
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
        .onAppear {
            appState.currentConversationId = conversation.id
            appState.loadMessagesFromStore(conversationId: conversation.id, accountId: conversation.accountId)
            appState.markAllMessagesRead(conversationId: conversation.id, accountId: conversation.accountId)
            // Re-fetch the contact's encrypted profile and update the cached
            // display name if it changed. Primary change-detection path.
            if let recipientDid = conversation.recipientDid {
                appState.refreshContactProfile(did: recipientDid, accountId: conversation.accountId)
            }
            if let groupId = conversation.groupId {
                appState.refreshGroupTitle(groupId: groupId, accountId: conversation.accountId)
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

        // Update conversation metadata for sorting.
        if let idx = appState.conversations.firstIndex(where: { $0.id == conversation.id }) {
            appState.conversations[idx].lastMessage = text
            appState.conversations[idx].lastMessageDate = message.sentAt
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
