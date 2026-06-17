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

    var body: some View {
        HStack(spacing: 12) {
            // Group/DM avatar placeholder. Hexagon + badge when the DM partner
            // is a bot; circle otherwise.
            let isBot = isBotConversation
            let frame: AnyShape = isBot ? AnyShape(Hexagon()) : AnyShape(Circle())
            frame
                .fill(Color.sand200)
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
                    } else if let lastMessage = conversation.lastMessage {
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
