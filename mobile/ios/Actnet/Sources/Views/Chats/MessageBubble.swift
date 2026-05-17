import SwiftUI

struct MessageBubble: View {
    let message: Message
    let isMe: Bool

    var body: some View {
        HStack {
            if isMe { Spacer(minLength: 60) }

            VStack(alignment: isMe ? .trailing : .leading, spacing: 4) {
                Text(message.body)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(isMe ? Color.accentColor : Color(.systemGray5))
                    .foregroundStyle(isMe ? .white : .primary)
                    .clipShape(RoundedRectangle(cornerRadius: 16))

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
                .foregroundStyle(.blue)
                .overlay(
                    Image(systemName: "checkmark")
                        .foregroundStyle(.blue)
                        .offset(x: 4)
                )
        }
    }
}
