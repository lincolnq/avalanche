import SwiftUI
import UIKit

struct MessageBubble: View {
    let message: Message
    let isMe: Bool
    /// Sender is a bot: the bubble gets cut (octagon-ish) corners instead of
    /// rounded ones, echoing the hexagon avatar (docs/54-bot-presentation.md).
    var isBot: Bool = false

    /// Sender's display name, shown above the bubble for incoming group
    /// messages (Signal-style). Nil for DMs, own messages, and the 2nd+
    /// message in a consecutive run from the same sender — ConversationView
    /// decides when to pass it.
    var senderName: String? = nil

    /// Whether this is the last message of a consecutive run from the same
    /// sender. The timestamp + delivery indicator only show on the last of a
    /// run (iMessage-style); mid-run bubbles stay clean. ConversationView
    /// computes this. Defaults to true so non-run callers are unaffected.
    var isLastInRun: Bool = true

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
                if let senderName, !isMe {
                    Text(senderName)
                        .font(.caption)
                        .fontWeight(.semibold)
                        .foregroundStyle(senderColor)
                        .padding(.leading, 4)
                }
                bubble
                if !reactionClusters.isEmpty {
                    reactionCluster
                }
            }

            if !isMe { Spacer(minLength: 60) }
        }
    }

    @ViewBuilder
    private var bubble: some View {
        if message.isDeleted {
            contentText
                .foregroundStyle(.secondary)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .overlay(
                    RoundedRectangle(cornerRadius: 16)
                        .strokeBorder(Color.avMuted.opacity(0.4), style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
                )
                .overlay(alignment: .bottomTrailing) { metadataOverlay }
        } else {
            let content = contentText
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(isMe ? Color.avOutgoingBubble : Color.avIncomingBubble)
                .foregroundStyle(isMe ? Color.sand100 : .primary)
                .clipShape(bubbleShape)
                .overlay(alignment: .bottomTrailing) { metadataOverlay }
            if actionsEnabled {
                content.contextMenu { menuItems }
            } else {
                content
            }
        }
    }

    /// The bubble's text with a *clear* copy of the metadata cluster appended
    /// to reserve trailing space on the last line. The visible cluster is then
    /// drawn as a bottom-trailing overlay (`metadataOverlay`): if it fits after
    /// the last line it tucks in there, otherwise the clear copy wraps and
    /// extends the bubble by one line (Signal-style). U+2007 figure spaces give
    /// a non-breaking gap between the body and the timestamp.
    private var contentText: Text {
        let base = message.isDeleted
            ? Text("This message was deleted").italic()
            : Text(message.body)
        guard showMetadata else { return base }
        return base + Text("\u{2007}\u{2007}") + metadataText(reserved: true)
    }

    @ViewBuilder
    private var metadataOverlay: some View {
        if showMetadata {
            metadataText(reserved: false)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
        }
    }

    /// The inline metadata cluster — optional "Edited" marker, the compact
    /// timestamp, and (own messages) the delivery glyph — built as a single
    /// `Text` so the reserved (clear) and visible (colored) copies lay out
    /// identically. Timestamp + delivery only appear on the last message of a
    /// run; "Edited" always shows when applicable.
    private func metadataText(reserved: Bool) -> Text {
        let base: Color = reserved ? .clear : metaColor
        var parts: Text?
        func append(_ piece: Text) { parts = parts.map { $0 + piece } ?? piece }

        if message.isEdited && !message.isDeleted {
            append(Text("Edited").foregroundStyle(base))
        }
        if isLastInRun {
            if parts != nil { append(Text(" ").foregroundStyle(base)) }
            append(Text(shortTimestamp(message.sentAt)).foregroundStyle(base))
            if isMe && !message.isDeleted, let symbol = deliverySymbol {
                let glyph: Color = reserved ? .clear : deliveryColor
                append(Text(" ").foregroundStyle(base)
                    + Text(Image(systemName: symbol)).foregroundStyle(glyph))
            }
        }
        return (parts ?? Text("")).font(.caption2)
    }

    /// Whether any metadata shows at all: the last bubble of a run (timestamp +
    /// delivery) or any edited message ("Edited").
    private var showMetadata: Bool {
        isLastInRun || (message.isEdited && !message.isDeleted)
    }

    /// Compact, Signal-style timestamp: "now" under a minute, "32m" within the
    /// hour, otherwise the locale short time ("5:13 PM"). Computed at render —
    /// it doesn't live-tick between renders.
    private func shortTimestamp(_ date: Date) -> String {
        let secs = Date().timeIntervalSince(date)
        if secs < 60 { return "now" }
        if secs < 3600 { return "\(Int(secs / 60))m" }
        return date.formatted(date: .omitted, time: .shortened)
    }

    private var metaColor: Color {
        isMe ? Color.sand100.opacity(0.8) : .secondary
    }

    /// Inline delivery glyph color. Glyphs only ride the outgoing (plum)
    /// bubble, so they use the same light cluster color as the timestamp — read
    /// is distinguished by the *filled* symbol, not color (avBrand == plum500,
    /// which would be invisible here). Failed stays red; it contrasts fine.
    private var deliveryColor: Color {
        message.deliveryStatus == .failed ? Color.avError : metaColor
    }

    /// Single SF Symbol for the delivery state, drawn inline next to the time.
    private var deliverySymbol: String? {
        switch message.deliveryStatus {
        case .sending: return "clock"
        case .sent: return "checkmark"
        case .delivered: return "checkmark.circle"
        case .read: return "checkmark.circle.fill"
        case .failed: return "exclamationmark.circle"
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

    /// Deterministic per-sender color for the group name label (Signal-style).
    /// Picked from a fixed palette by a stable FNV-style hash of the sender DID
    /// so a given member always gets the same color across launches (String's
    /// built-in hashValue is per-process-randomized, so we roll our own).
    private static let senderPalette: [Color] = [
        .blue, .purple, .pink, .orange, .teal, .indigo, .green, Color.avBrand,
    ]

    private var senderColor: Color {
        var hash: UInt64 = 5381
        for byte in message.senderAccountId.utf8 {
            hash = (hash &* 33) &+ UInt64(byte)
        }
        return Self.senderPalette[Int(hash % UInt64(Self.senderPalette.count))]
    }

    /// Cut-corner (octagon-ish) bubble for bots, rounded for people.
    private var bubbleShape: AnyShape {
        isBot ? AnyShape(CutCornerRectangle(cut: 12)) : AnyShape(RoundedRectangle(cornerRadius: 16))
    }

}
