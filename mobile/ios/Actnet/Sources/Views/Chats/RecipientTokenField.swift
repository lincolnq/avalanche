import SwiftUI
import UIKit

/// iMessage-style recipient field. Recipients are `NSTextAttachment`s embedded
/// in a single `UITextView`, so a chip *is* a character in the text. That's the
/// key to matching Messages exactly:
///
/// - Selecting a chip is a real text selection — the native selection UI draws
///   over it and the caret disappears, both for free.
/// - Backspace gets the two-stage behavior: pressing it with the caret right
///   after a chip selects the chip; pressing it again (now a selection) deletes
///   it. Tapping a chip selects it the same way.
///
/// The text view owns the content; SwiftUI mirrors it via `chips`/`query`
/// bindings and drives additions imperatively through `RecipientFieldHandle`
/// (autocomplete taps / DID submit). We never rebuild the text from the
/// bindings, so there's no fight with live editing.

/// Lets SwiftUI push a new chip into the text view without owning its content.
@MainActor
final class RecipientFieldHandle: ObservableObject {
    fileprivate weak var textView: RecipientTokenTextView?
    func addChip(_ chip: ComposeMessageView.Chip) { textView?.insertChip(chip) }
}

struct RecipientTokenField: UIViewRepresentable {
    @Binding var chips: [ComposeMessageView.Chip]
    @Binding var query: String
    var prefix: String
    var placeholder: String
    var handle: RecipientFieldHandle
    var onSubmit: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIView(context: Context) -> RecipientTokenTextView {
        let tv = RecipientTokenTextView()
        tv.delegate = context.coordinator
        tv.isScrollEnabled = false
        tv.backgroundColor = .clear
        tv.font = .preferredFont(forTextStyle: .body)
        tv.textColor = .label
        tv.tintColor = UIColor(Color.avBrand)
        tv.autocorrectionType = .no
        tv.autocapitalizationType = .none
        // A chip is an image attachment, which UITextView treats as a drag
        // source — long-pressing one "lifts"/zooms it. Recipients aren't
        // draggable in Messages, so turn the text drag interaction off.
        tv.textDragInteraction?.isEnabled = false
        tv.textContainerInset = UIEdgeInsets(top: 6, left: 0, bottom: 6, right: 0)
        tv.textContainer.lineFragmentPadding = 0
        tv.setContentHuggingPriority(.defaultLow, for: .horizontal)
        tv.prefixText = prefix
        tv.placeholderText = placeholder
        tv.setChips(chips)

        let tap = UITapGestureRecognizer(
            target: context.coordinator,
            action: #selector(Coordinator.handleTap(_:))
        )
        tap.delegate = context.coordinator
        tap.cancelsTouchesInView = false
        tv.addGestureRecognizer(tap)

        handle.textView = tv
        return tv
    }

    func updateUIView(_ tv: RecipientTokenTextView, context: Context) {
        context.coordinator.parent = self
        tv.prefixText = prefix
        tv.placeholderText = placeholder
    }

    func sizeThatFits(_ proposal: ProposedViewSize, uiView: RecipientTokenTextView, context: Context) -> CGSize? {
        let width = proposal.width ?? UIView.layoutFittingExpandedSize.width
        let fitted = uiView.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude))
        return CGSize(width: width, height: fitted.height)
    }

    final class Coordinator: NSObject, UITextViewDelegate, UIGestureRecognizerDelegate {
        var parent: RecipientTokenField
        init(_ parent: RecipientTokenField) { self.parent = parent }

        func textViewDidChange(_ textView: UITextView) {
            guard let tv = textView as? RecipientTokenTextView else { return }
            let newChips = tv.currentChips
            let newQuery = tv.currentQuery
            if parent.chips.map(\.id) != newChips.map(\.id) { parent.chips = newChips }
            if parent.query != newQuery { parent.query = newQuery }
        }

        func textViewDidChangeSelection(_ textView: UITextView) {
            (textView as? RecipientTokenTextView)?.refreshChipAppearance()
        }

        // A chip is an image attachment, so UITextView treats it as a shareable
        // text item and offers a Copy / Share / Save Image menu on long-press.
        // Suppress both the menu and the default tap action — we handle chip
        // taps ourselves. (These only affect text items, not the normal text
        // selection edit menu.)
        func textView(
            _ textView: UITextView,
            menuConfigurationFor textItem: UITextItem,
            defaultMenu: UIMenu
        ) -> UITextItem.MenuConfiguration? {
            nil
        }

        func textView(
            _ textView: UITextView,
            primaryActionFor textItem: UITextItem,
            defaultAction: UIAction
        ) -> UIAction? {
            nil
        }

        func textView(_ textView: UITextView, shouldChangeTextIn range: NSRange, replacementText text: String) -> Bool {
            // Return commits the typed query rather than inserting a newline.
            if text == "\n" {
                parent.onSubmit()
                return false
            }
            return true
        }

        @objc func handleTap(_ gr: UITapGestureRecognizer) {
            guard let tv = gr.view as? RecipientTokenTextView else { return }
            if !tv.isFirstResponder { tv.becomeFirstResponder() }
            // If the tap landed on a chip, select it. Defer so it wins over the
            // caret the text view places from the same tap.
            if let idx = tv.attachmentIndex(at: gr.location(in: tv)) {
                DispatchQueue.main.async {
                    tv.selectedRange = NSRange(location: idx, length: 1)
                }
            }
        }

        func gestureRecognizer(
            _ g: UIGestureRecognizer,
            shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer
        ) -> Bool { true }
    }
}

/// `UITextView` whose recipients are `ChipAttachment`s. Chips live at the front
/// of the text; whatever the user types trails after the last chip and is the
/// "query".
final class RecipientTokenTextView: UITextView {
    private let placeholderLabel = UILabel()
    private let prefixLabel = UILabel()
    /// Width currently carved out of the first line for the prefix; cached so we
    /// only rewrite `exclusionPaths` (which forces a relayout) when it changes.
    private var appliedPrefixWidth: CGFloat = -1
    private let prefixGap: CGFloat = 4

    var placeholderText: String = "" {
        didSet {
            placeholderLabel.text = placeholderText
            refreshPlaceholderVisibility()
        }
    }

    /// Inline label that prefixes the field (e.g. "To:"). The first line of
    /// recipients flows after it; wrapped lines run flush to the left edge.
    var prefixText: String = "" {
        didSet {
            prefixLabel.text = prefixText
            prefixLabel.isHidden = prefixText.isEmpty
            appliedPrefixWidth = -1
            setNeedsLayout()
        }
    }

    override init(frame: CGRect, textContainer: NSTextContainer?) {
        super.init(frame: frame, textContainer: textContainer)
        placeholderLabel.textColor = .placeholderText
        placeholderLabel.font = font
        placeholderLabel.numberOfLines = 1
        addSubview(placeholderLabel)

        prefixLabel.textColor = .secondaryLabel
        prefixLabel.font = font
        prefixLabel.numberOfLines = 1
        prefixLabel.isHidden = true
        addSubview(prefixLabel)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    override var font: UIFont? {
        didSet {
            placeholderLabel.font = font
            prefixLabel.font = font
            appliedPrefixWidth = -1
        }
    }

    private var hasAutoFocused = false

    /// Focus the field the first time it appears, like Messages' "To:" field.
    override func didMoveToWindow() {
        super.didMoveToWindow()
        guard !hasAutoFocused, window != nil else { return }
        hasAutoFocused = true
        becomeFirstResponder()
    }

    override func layoutSubviews() {
        super.layoutSubviews()

        let prefixSize = prefixLabel.intrinsicContentSize
        let prefixWidth = prefixText.isEmpty ? 0 : prefixSize.width + prefixGap
        let lineHeight = font?.lineHeight ?? 20

        // Carve a first-line-only column out of the left for the prefix. Height
        // is one (minimum) line, so it intersects only the first line fragment;
        // wrapped lines below it use the full width.
        if prefixWidth != appliedPrefixWidth {
            appliedPrefixWidth = prefixWidth
            textContainer.exclusionPaths = prefixWidth > 0
                ? [UIBezierPath(rect: CGRect(x: 0, y: 0, width: prefixWidth, height: lineHeight))]
                : []
        }

        // Vertically center the prefix within the (possibly taller) first line.
        let firstLineHeight = textStorage.length > 0
            ? layoutManager.lineFragmentRect(forGlyphAt: 0, effectiveRange: nil).height
            : lineHeight
        prefixLabel.frame = CGRect(
            x: 0,
            y: textContainerInset.top + max(0, (firstLineHeight - prefixSize.height) / 2),
            width: prefixSize.width,
            height: prefixSize.height
        )

        // Placeholder sits after the prefix on the first line.
        let plHeight = placeholderLabel.intrinsicContentSize.height
        placeholderLabel.frame = CGRect(
            x: prefixWidth,
            y: textContainerInset.top + max(0, (firstLineHeight - plHeight) / 2),
            width: max(0, bounds.width - prefixWidth - textContainerInset.right),
            height: plHeight
        )
    }

    func refreshPlaceholderVisibility() {
        placeholderLabel.isHidden = textStorage.length > 0
    }

    // MARK: Two-stage backspace

    override func deleteBackward() {
        let r = selectedRange
        // Caret (no selection) sitting right after a chip → select it instead of
        // deleting. A second backspace (now a selection) falls through to super.
        if r.length == 0, r.location > 0, isChip(at: r.location - 1) {
            selectedRange = NSRange(location: r.location - 1, length: 1)
            return
        }
        super.deleteBackward()
    }

    // MARK: Reading state back out

    /// Force chips to re-pick their normal/selected image (call when the
    /// selection changes — the layout manager caches attachment images).
    func refreshChipAppearance() {
        guard textStorage.length > 0 else { return }
        layoutManager.invalidateDisplay(forCharacterRange: NSRange(location: 0, length: textStorage.length))
    }

    func isChip(at index: Int) -> Bool {
        guard index >= 0, index < textStorage.length else { return false }
        return textStorage.attribute(.attachment, at: index, effectiveRange: nil) is ChipAttachment
    }

    var currentChips: [ComposeMessageView.Chip] {
        var out: [ComposeMessageView.Chip] = []
        let full = NSRange(location: 0, length: textStorage.length)
        textStorage.enumerateAttribute(.attachment, in: full) { value, _, _ in
            if let chipAttachment = value as? ChipAttachment { out.append(chipAttachment.chip) }
        }
        return out
    }

    /// Text after the last chip — what the user is currently typing.
    var currentQuery: String {
        (textStorage.string as NSString).substring(with: trailingTextRange)
    }

    func attachmentIndex(at point: CGPoint) -> Int? {
        let glyphPoint = CGPoint(
            x: point.x - textContainerInset.left,
            y: point.y - textContainerInset.top
        )
        var fraction: CGFloat = 0
        let idx = layoutManager.characterIndex(
            for: glyphPoint,
            in: textContainer,
            fractionOfDistanceBetweenInsertionPoints: &fraction
        )
        return isChip(at: idx) ? idx : nil
    }

    // MARK: Mutation

    /// Replace all content with the given chips (used once, to seed the field).
    func setChips(_ chips: [ComposeMessageView.Chip]) {
        let ms = NSMutableAttributedString()
        for chip in chips { ms.append(attributedChip(chip)) }
        textStorage.setAttributedString(ms)
        selectedRange = NSRange(location: textStorage.length, length: 0)
        typingAttributes = defaultTypingAttributes
        refreshPlaceholderVisibility()
    }

    /// Append a chip, replacing any in-progress typed query. No-op (beyond
    /// clearing the query) if the recipient is already present.
    func insertChip(_ chip: ComposeMessageView.Chip) {
        let alreadyPresent = currentChips.contains { $0.did == chip.did }
        let replacement = alreadyPresent ? NSAttributedString() : attributedChip(chip)
        textStorage.replaceCharacters(in: trailingTextRange, with: replacement)
        selectedRange = NSRange(location: textStorage.length, length: 0)
        typingAttributes = defaultTypingAttributes
        refreshPlaceholderVisibility()
        // Programmatic edits don't fire the delegate; sync SwiftUI manually.
        delegate?.textViewDidChange?(self)
    }

    private var trailingTextRange: NSRange {
        var end = 0
        let full = NSRange(location: 0, length: textStorage.length)
        textStorage.enumerateAttribute(.attachment, in: full) { value, range, _ in
            if value is ChipAttachment { end = max(end, range.location + range.length) }
        }
        return NSRange(location: end, length: textStorage.length - end)
    }

    private var defaultTypingAttributes: [NSAttributedString.Key: Any] {
        [.font: font ?? .preferredFont(forTextStyle: .body),
         .foregroundColor: textColor ?? .label]
    }

    private func attributedChip(_ chip: ComposeMessageView.Chip) -> NSAttributedString {
        let f = font ?? .preferredFont(forTextStyle: .body)
        let attachment = ChipAttachment(chip: chip, label: chip.label, font: f)
        attachment.textView = self
        let image = attachment.image ?? UIImage()
        // Center the pill on the line (its bottom sits below the baseline).
        let yOffset = ((f.capHeight - image.size.height) / 2).rounded()
        attachment.bounds = CGRect(origin: CGPoint(x: 0, y: yOffset), size: image.size)
        let s = NSMutableAttributedString(attachment: attachment)
        s.addAttribute(.font, value: f, range: NSRange(location: 0, length: s.length))
        return s
    }
}

/// A recipient rendered as a rounded pill image, carrying the `Chip` it stands
/// for so the field can read its recipients back out of the text. Renders a
/// filled "selected" variant when its character is within the text view's
/// selection — UITextView doesn't draw a selection highlight over an attachment
/// glyph, so we draw the selected state ourselves.
final class ChipAttachment: NSTextAttachment {
    let chip: ComposeMessageView.Chip
    private let normalImage: UIImage
    private let selectedImage: UIImage
    /// The hosting field, so we can consult its current selection at draw time.
    weak var textView: UITextView?

    init(chip: ComposeMessageView.Chip, label: String, font: UIFont) {
        self.chip = chip
        normalImage = ChipAttachment.render(label: label, font: font, selected: false)
        selectedImage = ChipAttachment.render(label: label, font: font, selected: true)
        super.init(data: nil, ofType: nil)
        image = normalImage  // drives `bounds` sizing
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    override func image(forBounds bounds: CGRect, textContainer: NSTextContainer?, characterIndex: Int) -> UIImage? {
        if let tv = textView, tv.selectedRange.length > 0,
           NSLocationInRange(characterIndex, tv.selectedRange) {
            return selectedImage
        }
        return normalImage
    }

    private static func render(label: String, font: UIFont, selected: Bool) -> UIImage {
        let hPad: CGFloat = 9
        let vPad: CGFloat = 3
        let trailingGap: CGFloat = 6  // breathing room before the next chip/text
        let textColor: UIColor = selected ? .white : .label
        let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: textColor]
        let textSize = (label as NSString).size(withAttributes: attrs)
        let pillWidth = textSize.width.rounded(.up) + hPad * 2
        let pillHeight = textSize.height.rounded(.up) + vPad * 2
        let size = CGSize(width: pillWidth + trailingGap, height: pillHeight)

        let renderer = UIGraphicsImageRenderer(size: size)
        return renderer.image { _ in
            let pill = CGRect(x: 0, y: 0, width: pillWidth, height: pillHeight)
            let path = UIBezierPath(roundedRect: pill, cornerRadius: pillHeight / 2)
            let brand = UIColor(Color.avBrand)
            (selected ? brand : brand.withAlphaComponent(0.15)).setFill()
            path.fill()
            (label as NSString).draw(at: CGPoint(x: hPad, y: vPad), withAttributes: attrs)
        }
    }
}

/// Short, non-DID-leaking label for a recipient we have no name for yet.
func shortenDid(_ did: String) -> String {
    if did.count > 18 { return String(did.prefix(12)) + "…" + String(did.suffix(4)) }
    return did
}
