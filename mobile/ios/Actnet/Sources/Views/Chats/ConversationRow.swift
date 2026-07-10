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
        // Compose the content decoration (📷/📎/👤) with the body (docs/35): a
        // caption shows "📷 caption", a caption-less content message shows "📷
        // Photo", and plain text shows just the body. Kept in sync between live
        // and post-restart previews via `lastMessagePreview`.
        let rawBody = conversation.lastMessage ?? ""
        let body: String
        if let deco = lastMessagePreviewDecoration(conversation.lastMessagePreview) {
            body = rawBody.isEmpty ? "\(deco.icon) \(deco.noun)" : "\(deco.icon) \(rawBody)"
        } else if !rawBody.isEmpty {
            body = rawBody
        } else {
            return nil
        }
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
                        Text(Self.chatListTimestamp(date))
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

    /// Chat-list timestamp label (shared format across iOS/Android/Desktop). Uses
    /// calendar-day boundaries, not elapsed hours, so a late-yesterday message
    /// reads as a weekday rather than a time:
    ///   <1m -> "now", <1h -> "5m", same day -> "3:43 PM", <1wk -> "Tue",
    ///   older -> locale-ordered short date.
    /// The in-conversation message view uses a different format (out of scope here).
    static func chatListTimestamp(_ date: Date, now: Date = Date()) -> String {
        let elapsed = now.timeIntervalSince(date)
        if elapsed < 60 { return "now" }
        if elapsed < 3600 { return "\(Int(elapsed / 60))m" }

        let cal = Calendar.current
        if cal.isDate(date, inSameDayAs: now) {
            // Locale short time, e.g. "3:43 PM".
            return date.formatted(date: .omitted, time: .shortened)
        }
        // Within the last 7 calendar days -> short weekday ("Tue"). A weekday
        // label is ambiguous at exactly 7 days, so day 7+ falls through to a date.
        let days = cal.dateComponents(
            [.day], from: cal.startOfDay(for: date), to: cal.startOfDay(for: now)
        ).day ?? 0
        if days < 7 {
            return date.formatted(.dateTime.weekday(.abbreviated))
        }
        // Older -> locale short date (locale-ordered day/month).
        return date.formatted(date: .numeric, time: .omitted)
    }
}
