import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// Clipboard codec for the "Copy contact → paste into a message" flow (docs/35).
///
/// A contact card is copied to the system pasteboard under a private type
/// carrying `{did,name}` JSON (so paste reconstructs it losslessly in-app),
/// plus a plain-text fallback (the name) so pasting into any other app is
/// still meaningful. The composer's paste affordance reads the private type.
enum ContactPasteboard {
    static let typeIdentifier = "net.theavalanche.contact"

    /// Copy a contact card to the system pasteboard.
    static func write(did: String, name: String) {
        let payload: [String: String] = ["did": did, "name": name]
        guard let json = try? JSONSerialization.data(withJSONObject: payload) else { return }
        UIPasteboard.general.items = [[
            typeIdentifier: json,
            UTType.utf8PlainText.identifier: Data(name.utf8),
        ]]
    }

    /// True when the clipboard holds a contact card (drives the paste button).
    static var hasContact: Bool {
        UIPasteboard.general.contains(pasteboardTypes: [typeIdentifier])
    }

    /// Read a contact card off the clipboard, or nil if none / malformed.
    static func read() -> SharedContactFfi? {
        guard let data = UIPasteboard.general.data(forPasteboardType: typeIdentifier),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: String],
              let did = obj["did"], !did.isEmpty
        else { return nil }
        return SharedContactFfi(did: did, name: obj["name"] ?? "")
    }
}

/// A shared contact card rendered inside a message bubble (docs/35). Shows the
/// name the sender knows the person by, plus a "Save contact" action that adds
/// them to the recipient's contact book. Received cards only — the sender's own
/// copy shows the same card without re-saving (they already know the contact).
struct SharedContactCard: View {
    let contact: SharedContactFfi
    let isMe: Bool
    /// True when this DID is already a curated contact — the card then shows a
    /// non-interactive "Saved" state instead of an active Save button. Driven by
    /// the real contact book (docs/52), not ephemeral tap state.
    var alreadySaved: Bool = false
    /// Invoked when the user taps "Save contact". No-op for the sender's copy.
    var onSave: () -> Void = {}
    /// Long-press context-menu actions (docs/35): open a DM with this person,
    /// and copy the card to the clipboard to re-share it elsewhere.
    var onMessage: () -> Void = {}
    var onCopy: () -> Void = {}

    private var displayName: String {
        contact.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            ? String(contact.did.suffix(8))
            : contact.name
    }

    var body: some View {
        HStack(spacing: 10) {
            // On the plum sent card the brand-plum avatar would vanish; use a
            // light tint there. Received cards keep the default brand tint.
            ContactAvatar(name: displayName, isBot: false, size: 40, tint: isMe ? .sand100 : .avBrand)
            VStack(alignment: .leading, spacing: 2) {
                Text(displayName)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(isMe ? Color.sand100 : Color.avInk)
                    .lineLimit(1)
                Text("Contact")
                    .font(.caption)
                    .foregroundStyle(isMe ? Color.sand100.opacity(0.8) : Color.secondary)
            }
            Spacer(minLength: 8)
            // The sender already has this contact; only recipients get Save.
            // A recipient who already has the DID curated sees a "Saved" state;
            // otherwise an active Save button (docs/35).
            if !isMe {
                if alreadySaved {
                    Label("Saved", systemImage: "checkmark")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                        .labelStyle(.titleAndIcon)
                } else {
                    Button {
                        onSave()
                    } label: {
                        Text("Save")
                            .font(.caption.weight(.semibold))
                            // `.borderedProminent` fills with the `avBrand` tint but
                            // defaults its title to white — unreadable on the light
                            // `plum200` the tint becomes in dark mode. `avPaper` is
                            // the luminance-inverse of `avBrand`, so it contrasts in
                            // both modes (sand on plum / plum on light-plum).
                            .foregroundStyle(Color.avPaper)
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
                }
            }
        }
        .padding(10)
        .frame(maxWidth: 280, alignment: .leading)
        // Mirror the message bubble (MessageBubble `bubble`): full outgoing plum
        // for your own card, incoming tone for a received one — paired with the
        // text colors above (sand on plum, ink on incoming) for legible contrast
        // in both light and dark. (LinkPreviewCard uses a 0.6 plum, but that lands
        // as a low-contrast mid-tone under black text in light mode.)
        .background(isMe ? Color.avOutgoingBubble : Color.avIncomingBubble)
        .clipShape(RoundedRectangle(cornerRadius: 14))
        // Border tint follows the side, like the text/avatar: `avMuted` reads on
        // the incoming (sand/plum) card, but is a mid-tone that disappears on the
        // plum sent card in light mode — use a light stroke there instead.
        .overlay(
            RoundedRectangle(cornerRadius: 14)
                .strokeBorder(
                    isMe ? Color.sand100.opacity(0.5) : Color.avMuted.opacity(0.25),
                    lineWidth: 1
                )
        )
        // Long-press → native menu (docs/35): message the person, or copy the
        // card to re-share it into another conversation.
        .contextMenu {
            Button {
                onMessage()
            } label: {
                Label("Message \(displayName)", systemImage: "bubble.left")
            }
            Button {
                onCopy()
            } label: {
                Label("Copy contact", systemImage: "doc.on.doc")
            }
        }
    }
}

#if DEBUG
private let previewContact = SharedContactFfi(did: "did:plc:carol", name: "Carol (canvass lead)")

/// Shared preview scaffold: the card on the chat-page background, with the
/// app-root tint applied (`.tint(.avBrand)` — otherwise the `.borderedProminent`
/// Save button falls back to system blue, since previews don't run through
/// `ActnetApp`). `isMe` picks the sent (plum) vs received (incoming) styling.
@ViewBuilder
private func previewCard(isMe: Bool) -> some View {
    SharedContactCard(contact: previewContact, isMe: isMe, alreadySaved: false)
        .padding()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.avPaper)
        .tint(Color.avBrand)
}

#Preview("Received")        { previewCard(isMe: false) }
#Preview("Received (Dark)") { previewCard(isMe: false).preferredColorScheme(.dark) }
#Preview("Sent")            { previewCard(isMe: true) }
#Preview("Sent (Dark)")     { previewCard(isMe: true).preferredColorScheme(.dark) }
#endif
