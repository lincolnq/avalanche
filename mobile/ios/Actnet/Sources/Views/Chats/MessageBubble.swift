import SwiftUI

struct MessageBubble: View {
    let message: Message
    let isMe: Bool
    /// Sender is a bot: the bubble gets cut (octagon-ish) corners instead of
    /// rounded ones, echoing the hexagon avatar (docs/54-bot-presentation.md).
    var isBot: Bool = false

    var body: some View {
        HStack {
            if isMe { Spacer(minLength: 60) }

            VStack(alignment: isMe ? .trailing : .leading, spacing: 4) {
                Text(message.body)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(isMe ? Color.avOutgoingBubble : Color.avIncomingBubble)
                    .foregroundStyle(isMe ? Color.sand100 : .primary)
                    .clipShape(bubbleShape)

                HStack(spacing: 4) {
                    Text(message.sentAt, style: .time)
                    if message.isEdited {
                        Text("· Edited")
                    }
                    if isMe {
                        deliveryIndicator
                    }
                }
                .font(.caption2)
                .foregroundStyle(.secondary)
            }

            if !isMe { Spacer(minLength: 60) }
        }
    }

    /// Cut-corner (octagon-ish) bubble for bots, rounded for people.
    private var bubbleShape: AnyShape {
        isBot ? AnyShape(CutCornerRectangle(cut: 12)) : AnyShape(RoundedRectangle(cornerRadius: 16))
    }

    @ViewBuilder
    private var deliveryIndicator: some View {
        switch message.deliveryStatus {
        case .sending:
            Image(systemName: "clock")
                .foregroundStyle(.secondary)
        case .sent:
            Image(systemName: "checkmark")
                .foregroundStyle(.secondary)
        case .delivered:
            Image(systemName: "checkmark")
                .foregroundStyle(.secondary)
                .overlay(
                    Image(systemName: "checkmark")
                        .foregroundStyle(.secondary)
                        .offset(x: 4)
                )
        case .read:
            Image(systemName: "checkmark")
                .foregroundStyle(Color.avBrand)
                .overlay(
                    Image(systemName: "checkmark")
                        .foregroundStyle(Color.avBrand)
                        .offset(x: 4)
                )
        case .failed:
            Image(systemName: "exclamationmark.circle.fill")
                .foregroundStyle(Color.avError)
        }
    }
}
