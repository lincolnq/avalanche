import SwiftUI

struct ConversationView: View {
    @EnvironmentObject var appState: AppState
    let conversation: Conversation

    @State private var messageText = ""
    @State private var errorMessage: String?
    @State private var scrollTarget: String?
    @State private var scrollToken = UUID()
    @State private var initialLoadDone = false

    private var messages: [Message] {
        appState.messagesByConversation[conversation.id] ?? []
    }

    var body: some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(messages) { message in
                            MessageBubble(
                                message: message,
                                isMe: message.senderAccountId == conversation.accountId
                            )
                            .id(message.id)
                        }
                        Color.clear.frame(height: 1).id("bottom")
                    }
                    .padding()
                }
                .defaultScrollAnchor(.bottom)
                .onChange(of: scrollToken) {
                    if let scrollTarget {
                        proxy.scrollTo(scrollTarget, anchor: .bottom)
                    }
                }
            }

            if let error = errorMessage {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
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
                    scrollTo("bottom")
                }
            }
        }
        .navigationTitle(conversation.title)
        .navigationBarTitleDisplayMode(.inline)
        .onAppear {
            appState.loadMessagesFromStore(conversationId: conversation.id, accountId: conversation.accountId)
            DispatchQueue.main.async {
                initialScroll()
            }
            appState.markAllMessagesRead(conversationId: conversation.id, accountId: conversation.accountId)
        }
        .onChange(of: messages.count) {
            guard initialLoadDone else { return }
            // Scroll to and mark new messages as read while the conversation is visible.
            if !messages.isEmpty {
                scrollTo("bottom")
            }
            appState.markAllMessagesRead(conversationId: conversation.id, accountId: conversation.accountId)
        }
    }

    private func initialScroll() {
        let msgs = messages
        if let firstUnread = msgs.first(where: { $0.readAtMs == nil && $0.senderAccountId != conversation.accountId }) {
            scrollTo(firstUnread.id)
        } else if !msgs.isEmpty {
            scrollTo("bottom")
        }
        initialLoadDone = true
    }

    private func scrollTo(_ id: String) {
        scrollTarget = id
        scrollToken = UUID()
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
        scrollTo("bottom")

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
