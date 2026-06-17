import SwiftUI
import UIKit

struct MessageBubble: View {
    let message: Message
    let isMe: Bool
    /// Sender is a bot: the bubble gets cut (octagon-ish) corners instead of
    /// rounded ones, echoing the hexagon avatar (docs/54-bot-presentation.md).
    var isBot: Bool = false

    // Reactions / editing / deletion (docs/33, docs/36). `actionsEnabled` gates
    // the long-press menu to conversations where these ops are supported (DMs).
    var reactions: [ReactionFfi] = []
    var myDid: String = ""
    var actionsEnabled: Bool = false
    var canEdit: Bool = false
    var onToggleReaction: (String) -> Void = { _ in }
    var onEdit: () -> Void = {}
    var onDelete: (Bool) -> Void = { _ in }
    var onShowHistory: () -> Void = {}

    /// Quick-reaction palette shown in the long-press menu.
    private static let quickEmoji = ["👍", "❤️", "😂", "😮", "😢", "🙏"]

    var body: some View {
        HStack {
            if isMe { Spacer(minLength: 60) }

            VStack(alignment: isMe ? .trailing : .leading, spacing: 4) {
                bubble
                if !reactionClusters.isEmpty {
                    reactionCluster
                }

                HStack(spacing: 4) {
                    Text(message.sentAt, style: .time)
                    if message.isEdited && !message.isDeleted {
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
    private var bubble: some View {
        if message.isDeleted {
            Text("This message was deleted")
                .italic()
                .foregroundStyle(.secondary)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .overlay(
                    RoundedRectangle(cornerRadius: 16)
                        .strokeBorder(Color.avMuted.opacity(0.4), style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
                )
        } else {
            let content = Text(message.body)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(isMe ? Color.avOutgoingBubble : Color.avIncomingBubble)
                .foregroundStyle(isMe ? Color.sand100 : .primary)
                .clipShape(bubbleShape)
            if actionsEnabled {
                content.contextMenu { menuItems }
            } else {
                content
            }
        }
    }

    @ViewBuilder
    private var menuItems: some View {
        ControlGroup {
            ForEach(Self.quickEmoji, id: \.self) { emoji in
                Button(emoji) { onToggleReaction(emoji) }
            }
        }
        if canEdit {
            Button { onEdit() } label: { Label("Edit", systemImage: "pencil") }
        }
        if message.editCount > 0 {
            Button { onShowHistory() } label: { Label("Edit History", systemImage: "clock.arrow.circlepath") }
        }
        Button {
            UIPasteboard.general.string = message.body
        } label: { Label("Copy", systemImage: "doc.on.doc") }
        if isMe {
            Button(role: .destructive) { onDelete(true) } label: {
                Label("Delete for Everyone", systemImage: "trash")
            }
        }
        Button(role: .destructive) { onDelete(false) } label: {
            Label("Delete for Me", systemImage: "trash")
        }
    }

    /// Reactions grouped by emoji, ordered by first appearance, with whether
    /// this account is among the reactors (to highlight its own).
    private var reactionClusters: [(emoji: String, count: Int, mine: Bool)] {
        var order: [String] = []
        var counts: [String: Int] = [:]
        var mine: [String: Bool] = [:]
        for r in reactions {
            if counts[r.emoji] == nil { order.append(r.emoji) }
            counts[r.emoji, default: 0] += 1
            if r.reactorDid == myDid { mine[r.emoji] = true }
        }
        return order.map { ($0, counts[$0] ?? 0, mine[$0] ?? false) }
    }

    private var reactionCluster: some View {
        HStack(spacing: 4) {
            ForEach(reactionClusters, id: \.emoji) { cluster in
                Button {
                    onToggleReaction(cluster.emoji)
                } label: {
                    HStack(spacing: 2) {
                        Text(cluster.emoji).font(.caption)
                        if cluster.count > 1 {
                            Text("\(cluster.count)")
                                .font(.caption2)
                                .foregroundStyle(cluster.mine ? Color.avBrand : .secondary)
                        }
                    }
                    .padding(.horizontal, 6)
                    .padding(.vertical, 3)
                    .background(
                        Capsule().fill(cluster.mine ? Color.avBrand.opacity(0.18) : Color.avIncomingBubble)
                    )
                    .overlay(
                        Capsule().strokeBorder(cluster.mine ? Color.avBrand.opacity(0.5) : Color.clear, lineWidth: 1)
                    )
                }
                .buttonStyle(.plain)
            }
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
