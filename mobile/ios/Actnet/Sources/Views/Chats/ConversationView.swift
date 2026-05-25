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
        .onAppear {
            appState.currentConversationId = conversation.id
            appState.loadMessagesFromStore(conversationId: conversation.id, accountId: conversation.accountId)
            appState.markAllMessagesRead(conversationId: conversation.id, accountId: conversation.accountId)
            // Re-fetch the contact's encrypted profile and update the cached
            // display name if it changed. Primary change-detection path.
            if let recipientDid = conversation.recipientDid {
                appState.refreshContactProfile(did: recipientDid, accountId: conversation.accountId)
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
            deliveryStatus: .sent
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
            } catch {
                errorMessage = "Failed to send: \(error.localizedDescription)"
            }
        }
    }
}
