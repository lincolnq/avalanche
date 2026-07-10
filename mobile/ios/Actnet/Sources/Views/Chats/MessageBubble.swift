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
    // the long-press overlay to conversations where these ops are supported.
    var reactions: [ReactionFfi] = []
    var myDid: String = ""
    var actionsEnabled: Bool = false
    /// Tapping an existing reaction cluster toggles this account's reaction.
    var onToggleReaction: (String) -> Void = { _ in }
    /// Long-press on the bubble — ConversationView raises the Signal-style
    /// actions overlay (docs/33). Gated by `actionsEnabled` and `interactive`.
    /// The `CGRect` is this bubble's content frame in global coords, so the
    /// overlay can animate the message from where it sits into the center.
    var onLongPress: (CGRect) -> Void = { _ in }
    /// When false the bubble is a static copy (e.g. inside the actions overlay):
    /// no long-press gesture is attached.
    var interactive: Bool = true
    /// When false the surrounding side `Spacer`s are dropped so the bubble sizes
    /// to its content — used by the actions overlay, which positions the copy
    /// itself. The content still honors `maxWidth` from the caller for identical
    /// wrapping.
    var showSideSpacers: Bool = true
    /// Loads decrypted bytes for an attachment (docs/35); injected by the
    /// conversation view so the bubble stays free of app-core access.
    var attachmentLoader: (AttachmentFfi) async -> Data? = { _ in nil }
    /// Tapping an image attachment opens the fullscreen viewer (docs/35).
    /// ConversationView supplies this; defaults to no-op for the static overlay
    /// copy.
    var onImageTap: (AttachmentFfi) -> Void = { _ in }
    /// Tapping "Save contact" on a shared contact card (docs/35); ConversationView
    /// wires this to `AppState.saveSharedContact`. No-op by default.
    var onSaveContact: (SharedContactFfi) -> Void = { _ in }
    /// Contact-card context-menu actions (docs/35): open a DM with the person,
    /// and copy the card to re-share it. ConversationView wires these.
    var onMessageContact: (SharedContactFfi) -> Void = { _ in }
    var onCopyContact: (SharedContactFfi) -> Void = { _ in }
    /// DIDs already curated in the contact book, so a shared contact card shows
    /// "Saved" rather than an active Save button (docs/35).
    var savedContactDids: Set<String> = []

    /// This bubble's content frame in global coords, tracked so a long-press can
    /// hand the overlay a start position for the lift-to-center animation.
    @State private var contentFrame: CGRect = .zero

    var body: some View {
        if showSideSpacers {
            HStack {
                if isMe { Spacer(minLength: 60) }
                contentStack
                if !isMe { Spacer(minLength: 60) }
            }
        } else {
            contentStack
        }
    }

    /// The message content (sender name, attachments, bubble, previews,
    /// reactions) without the surrounding side spacers. Shared by the timeline
    /// (wrapped in the HStack) and the actions overlay copy.
    private var contentStack: some View {
        VStack(alignment: isMe ? .trailing : .leading, spacing: 4) {
            if let senderName, !isMe {
                Text(senderName)
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundStyle(senderColor)
                    .padding(.leading, 4)
            }
            if !message.attachments.isEmpty {
                ForEach(Array(message.attachments.enumerated()), id: \.offset) { _, att in
                    AttachmentView(
                        attachment: att,
                        loader: attachmentLoader,
                        onTap: att.contentType.hasPrefix("image/") ? { onImageTap(att) } : nil
                    )
                }
            }
            // The text bubble is omitted for an attachment- or contact-only
            // message (empty body), so it doesn't get an empty grey bubble.
            if !message.body.isEmpty
                || (message.attachments.isEmpty && message.contacts.isEmpty)
                || message.isDeleted {
                bubble
            }
            // Link-preview cards (docs/35) below the text bubble.
            ForEach(Array(message.previews.enumerated()), id: \.offset) { _, preview in
                LinkPreviewCard(preview: preview, isMe: isMe, loader: attachmentLoader)
            }
            // Shared contact cards (docs/35) below the text bubble.
            ForEach(Array(message.contacts.enumerated()), id: \.offset) { _, contact in
                SharedContactCard(
                    contact: contact,
                    isMe: isMe,
                    alreadySaved: savedContactDids.contains(contact.did),
                    onSave: { onSaveContact(contact) },
                    onMessage: { onMessageContact(contact) },
                    onCopy: { onCopyContact(contact) }
                )
            }
            if !reactionClusters.isEmpty {
                reactionCluster
            }
        }
        .onGeometryChange(for: CGRect.self, of: { $0.frame(in: .global) }) { contentFrame = $0 }
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
                .foregroundStyle(isMe ? Color.sand100 : Color.avInk)
                .clipShape(bubbleShape)
                .overlay(alignment: .bottomTrailing) { metadataOverlay }
            if actionsEnabled && interactive {
                content.onLongPressGesture(minimumDuration: 0.35) {
                    UIImpactFeedbackGenerator(style: .medium).impactOccurred()
                    onLongPress(contentFrame)
                }
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
            : Text(Self.linkified(message.body))
        guard showMetadata else { return base }
        return base + Text("\u{2007}\u{2007}") + metadataText(reserved: true)
    }

    /// Turn URLs in `body` into tappable links. SwiftUI renders an
    /// `AttributedString` carrying `.link` attributes as live links (a tap opens
    /// them via the environment's `openURL`); the `Text` still concatenates into
    /// the metadata-flow trick above. Detection uses `NSDataDetector`, the same
    /// engine the system uses elsewhere.
    static func linkified(_ body: String) -> AttributedString {
        var attr = AttributedString(body)
        guard !body.isEmpty,
              let detector = try? NSDataDetector(types: NSTextCheckingResult.CheckingType.link.rawValue)
        else { return attr }
        let full = NSRange(location: 0, length: (body as NSString).length)
        detector.enumerateMatches(in: body, range: full) { match, _, _ in
            guard let match, let url = match.url,
                  let r = Range(match.range, in: body),
                  let lo = AttributedString.Index(r.lowerBound, within: attr),
                  let hi = AttributedString.Index(r.upperBound, within: attr)
            else { return }
            attr[lo..<hi].link = url
            attr[lo..<hi].underlineStyle = .single
        }
        return attr
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
        .blue, .purple, .pink, .orange, .teal, .indigo, .green, Color.plum400,
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
