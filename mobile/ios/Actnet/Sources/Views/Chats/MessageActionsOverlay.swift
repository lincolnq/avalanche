import SwiftUI

/// Signal-style long-press overlay (docs/33): a dimmed backdrop with a floating
/// reaction bar on top, the tapped message rendered as a focal copy, and the
/// action list below — replacing the native `.contextMenu`. Presented by
/// `ConversationView` when a bubble is long-pressed.
///
/// The message *lifts from where it sits* to the vertical center: a hidden
/// in-flow copy establishes the resting layout (and is measured), while a
/// visible copy is positioned absolutely and animated from the source frame to
/// that resting frame. Both copies wrap at the same max width as the timeline
/// bubble (screen − 92: 16pt padding each side + a 60pt gutter), so the bubble
/// is pixel-identical before, during, and after the move.
struct MessageActionsOverlay: View {
    let message: Message
    let isMe: Bool
    let isBot: Bool
    let senderName: String?
    /// Matches the source bubble's timestamp/delivery visibility so the copy is
    /// the same size.
    let isLastInRun: Bool
    /// Global frame of the source bubble content — the animation's start point.
    let sourceFrame: CGRect
    let reactions: [ReactionFfi]
    let myDid: String
    let canEdit: Bool
    var onToggleReaction: (String) -> Void
    var onMore: () -> Void
    var onEdit: () -> Void
    var onDelete: (Bool) -> Void
    var onShowHistory: () -> Void
    var attachmentLoader: (AttachmentFfi) async -> Data?
    var onDismiss: () -> Void

    /// Resting (centered) global frame of the focal bubble, measured from the
    /// hidden in-flow copy. `.zero` until the first layout pass.
    @State private var restingFrame: CGRect = .zero
    /// Drives the lift: false = at `sourceFrame`, true = at `restingFrame`.
    @State private var expanded = false
    /// Backdrop / chrome fade-in.
    @State private var appeared = false

    /// This account's current reaction, highlighted in the bar so a second tap
    /// reads as "remove".
    private var myEmoji: String? {
        reactions.first { $0.reactorDid == myDid }?.emoji
    }

    /// Duration of the in/out morph; also how long we wait before telling the
    /// parent to remove the overlay so the exit animation can play.
    private static let morph = 0.3
    /// Scrim fade duration — fast, and played first on the way in / last on the
    /// way out so the swap between the real bubble and this copy stays hidden.
    private static let scrimFade = 0.1

    /// Reverse the morph: first glide the bubble back to its source and fade the
    /// chrome (scrim still up, hiding the copy), *then* fade the scrim out last —
    /// mirroring the entrance — and finally remove. Every dismiss path (backdrop
    /// tap, an emoji, an action) goes through here so the exit always animates.
    private func dismiss() {
        withAnimation(.spring(response: Self.morph, dampingFraction: 0.9)) {
            expanded = false
        }
        // Start the scrim fade so it *finishes* with the glide (its last
        // `scrimFade` overlaps the glide's tail), then remove.
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.morph - Self.scrimFade) {
            withAnimation(.easeOut(duration: Self.scrimFade)) { appeared = false }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.morph) { onDismiss() }
    }

    var body: some View {
        GeometryReader { geo in
            let area = geo.frame(in: .global)
            // Source bubble center, in this reader's local space.
            let sourceLocal = CGPoint(x: sourceFrame.midX - area.minX, y: sourceFrame.midY - area.minY)
            // Resting center = the measured slot (screen center-ish), local.
            let restingLocal = CGPoint(x: restingFrame.midX - area.minX, y: restingFrame.midY - area.minY)
            let atRest = expanded && restingFrame != .zero

            ZStack {
                // Dimmed, blurred backdrop; tap anywhere to dismiss. Fades in
                // fast so the swap from the real bubble to this copy is hidden.
                Rectangle()
                    .fill(.ultraThinMaterial)
                    .ignoresSafeArea()
                    .opacity(appeared ? 1 : 0)
                    .onTapGesture { dismiss() }

                // Layout column: reaction bar, an invisible bubble slot the exact
                // size of the source (spaces the bar/menu and gives the resting
                // center), and the action list — vertically & horizontally
                // centered.
                VStack(spacing: 10) {
                    reactionBar
                        .opacity(expanded ? 1 : 0)
                        .scaleEffect(expanded ? 1 : 0.85, anchor: .bottom)

                    Color.clear
                        .frame(width: sourceFrame.width, height: sourceFrame.height)
                        .onGeometryChange(for: CGRect.self, of: { $0.frame(in: .global) }) { f in
                            guard restingFrame == .zero, f != .zero else { return }
                            restingFrame = f
                            // Next runloop tick: lift from source to center.
                            DispatchQueue.main.async {
                                withAnimation(.spring(response: 0.34, dampingFraction: 0.82)) {
                                    expanded = true
                                }
                            }
                        }

                    actionList
                        .opacity(expanded ? 1 : 0)
                        .scaleEffect(expanded ? 1 : 0.85, anchor: .top)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
                .padding(.horizontal, 16)

                // The visible, animated copy. Sized to the source's *natural*
                // width so it starts exactly overlapping the real bubble (no
                // horizontal snap), then glides to the resting center.
                focalBubble
                    .frame(width: sourceFrame.width)
                    .position(atRest ? restingLocal : sourceLocal)
            }
        }
        .onAppear { withAnimation(.easeOut(duration: Self.scrimFade)) { appeared = true } }
    }

    /// A non-interactive copy of the message. Sized by the caller to the source
    /// bubble's exact width, so its wrapping is identical to the timeline.
    private var focalBubble: some View {
        MessageBubble(
            message: message,
            isMe: isMe,
            isBot: isBot,
            senderName: senderName,
            isLastInRun: isLastInRun,
            reactions: reactions,
            myDid: myDid,
            actionsEnabled: false,
            interactive: false,
            showSideSpacers: false,
            attachmentLoader: attachmentLoader
        )
    }

    /// Quick emoji + the "+" full-picker button, in a floating capsule.
    private var reactionBar: some View {
        HStack(spacing: 6) {
            ForEach(EmojiData.quick, id: \.self) { emoji in
                Button {
                    if myEmoji != emoji { EmojiRecents.record(emoji) }
                    onToggleReaction(emoji)
                    dismiss()
                } label: {
                    Text(emoji)
                        .font(.system(size: 28))
                        .padding(6)
                        .background(
                            Circle().fill(myEmoji == emoji ? Color.avBrand.opacity(0.25) : Color.clear)
                        )
                }
                .buttonStyle(.plain)
            }
            Button {
                onMore()
            } label: {
                Image(systemName: "plus")
                    .font(.system(size: 18, weight: .semibold))
                    .foregroundStyle(Color.avBrand)
                    .frame(width: 40, height: 40)
                    .background(Circle().fill(Color.avCard))
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Capsule().fill(Color.avPaper))
        .shadow(color: .black.opacity(0.15), radius: 8, y: 2)
    }

    private var actionList: some View {
        VStack(spacing: 0) {
            if canEdit {
                actionRow("Edit", "pencil") { onEdit(); dismiss() }
                divider
            }
            if message.editCount > 0 {
                actionRow("Edit History", "clock.arrow.circlepath") { onShowHistory(); dismiss() }
                divider
            }
            actionRow("Copy", "doc.on.doc") {
                UIPasteboard.general.string = message.body
                dismiss()
            }
            if isMe {
                divider
                actionRow("Delete for Everyone", "trash", destructive: true) { onDelete(true); dismiss() }
            }
            divider
            actionRow("Delete for Me", "trash", destructive: true) { onDelete(false); dismiss() }
        }
        .background(RoundedRectangle(cornerRadius: 14).fill(Color.avPaper))
        .shadow(color: .black.opacity(0.15), radius: 8, y: 2)
        .frame(maxWidth: 260)
    }

    private var divider: some View {
        Divider().padding(.leading, 44)
    }

    private func actionRow(_ title: String, _ symbol: String, destructive: Bool = false, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Image(systemName: symbol)
                    .frame(width: 20)
                Text(title)
                Spacer(minLength: 0)
            }
            .foregroundStyle(destructive ? Color.avError : Color.avInk)
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
