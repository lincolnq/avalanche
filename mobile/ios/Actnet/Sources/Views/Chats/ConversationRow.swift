import SwiftUI

struct ConversationRow: View {
    let conversation: Conversation
    let account: Account?

    @EnvironmentObject private var appState: AppState

    private var unreadCount: Int {
        appState.unreadCount(for: conversation)
    }

    var body: some View {
        HStack(spacing: 12) {
            // Group/DM avatar placeholder
            Circle()
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
                    if let lastMessage = conversation.lastMessage {
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
