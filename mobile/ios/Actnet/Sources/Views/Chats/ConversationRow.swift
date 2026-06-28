import SwiftUI

struct ConversationRow: View {
    let conversation: Conversation
    let account: Account?

    @EnvironmentObject private var appState: AppState

    private var unreadCount: Int {
        appState.unreadCount(for: conversation)
    }

    /// A DM with a bot renders in the hexagon frame + badge, like every other
    /// bot avatar surface (docs/54-bot-presentation.md). Groups and human DMs
    /// stay circular.
    private var isBotConversation: Bool {
        guard !conversation.isGroup, let did = conversation.recipientDid else { return false }
        return appState.isBot(did, accountId: conversation.accountId)
    }

    /// Preview text for the latest message. For a group system event we render
    /// it reactively (resolving DIDs to names at display time), so it updates
    /// from "You made Unknown an admin" to the real name once profiles resolve —
    /// the same path the conversation view uses. Normal messages show the body.
    private var previewText: String? {
        // System/metadata events render as a full sentence with no sender prefix.
        if conversation.lastMessageKind > 0 {
            let m = Message(
                id: conversation.id,
                conversationId: conversation.id,
                senderAccountId: conversation.lastMessageSenderDid ?? "",
                body: conversation.lastMessage ?? "",
                sentAtMs: 0,
                editedAtMs: nil,
                readAtMs: nil,
                deliveryStatus: .sent,
                kind: conversation.lastMessageKind,
                metadata: conversation.lastMessageMetadata
            )
            return appState.groupEventText(m, accountId: conversation.accountId)
        }
        guard let body = conversation.lastMessage else { return nil }
        // Prefix the sender: "You:" for our own messages, the sender's name in
        // groups. Inbound DMs need no prefix — the conversation title is the
        // sender.
        let sender = conversation.lastMessageSenderDid
        if let sender, sender == conversation.accountId {
            return "You: \(body)"
        }
        if conversation.isGroup, let sender, !sender.isEmpty {
            return "\(appState.resolvedName(for: sender, accountId: conversation.accountId)): \(body)"
        }
        return body
    }

    var body: some View {
        HStack(spacing: 12) {
            // Group/DM avatar placeholder. Hexagon + badge when the DM partner
            // is a bot; circle otherwise.
            let isBot = isBotConversation
            let frame: AnyShape = isBot ? AnyShape(Hexagon()) : AnyShape(Circle())
            frame
                .fill(Color.avCard)
                .frame(width: 48, height: 48)
                .overlay {
                    Image(systemName: conversation.isGroup ? "person.3" : "person")
                        .foregroundStyle(.secondary)
                }

            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(conversation.title)
                        .fontWeight(.medium)
                        .lineLimit(1)

                    Spacer()

                    if let date = conversation.lastMessageDate {
                        Text(date, style: .relative)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }

                HStack {
                    if conversation.isRequest {
                        // First contact from an un-curated DID (docs/12 §1).
                        Text("Message request")
                            .font(.subheadline)
                            .fontWeight(.medium)
                            .foregroundStyle(Color.avBrand)
                            .lineLimit(1)
                    } else if let lastMessage = previewText {
                        Text(lastMessage)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }

                    Spacer()

                    // Multi-account: show which identity this chat belongs to
                    if let account, showAccountIndicator {
                        Text(account.displayName.prefix(1).uppercased())
                            .font(.caption2)
                            .fontWeight(.medium)
                            .foregroundStyle(.white)
                            .frame(width: 18, height: 18)
                            .background(Color.avBrand, in: Circle())
                    }

                    if unreadCount > 0 {
                        Text("\(unreadCount)")
                            .font(.caption2)
                            .fontWeight(.bold)
                            .foregroundStyle(.white)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(Color.avNotification, in: Capsule())
                    }
                }
            }
        }
        .padding(.vertical, 2)
    }

    private var showAccountIndicator: Bool {
        appState.accounts.count > 1
    }
}
