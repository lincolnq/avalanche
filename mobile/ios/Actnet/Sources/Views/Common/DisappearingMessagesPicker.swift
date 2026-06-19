import SwiftUI

/// Reusable disappearing-messages timer picker. Binds to a duration in seconds
/// (`0` = off), matching the `expiry_seconds` the Rust core stores for groups
/// (`create_group`) and DMs (`set_conversation_timer`). The option set mirrors
/// Signal's standard durations so the choice is familiar.
///
/// Rendered as a `Menu`-style `Picker` so it slots into a `Form`/`List` row.
/// A DM-level timer can reuse this verbatim once it's wired into the
/// conversation detail screen.
struct DisappearingMessagesPicker: View {
    @Binding var seconds: UInt32

    /// (label, seconds). `0` is the "Off" sentinel.
    static let options: [(label: String, seconds: UInt32)] = [
        ("Off", 0),
        ("30 seconds", 30),
        ("5 minutes", 5 * 60),
        ("1 hour", 60 * 60),
        ("8 hours", 8 * 60 * 60),
        ("1 day", 24 * 60 * 60),
        ("1 week", 7 * 24 * 60 * 60),
        ("4 weeks", 4 * 7 * 24 * 60 * 60),
    ]

    /// Human label for an arbitrary stored value; falls back to the raw second
    /// count if it isn't one of the canonical options (e.g. a value set by a
    /// future custom-duration UI).
    static func label(for seconds: UInt32) -> String {
        options.first(where: { $0.seconds == seconds })?.label ?? "\(seconds)s"
    }

    var body: some View {
        Picker("Disappearing messages", selection: $seconds) {
            ForEach(Self.options, id: \.seconds) { option in
                Text(option.label).tag(option.seconds)
            }
        }
    }
}

#if DEBUG
private struct DisappearingMessagesPickerPreview: View {
    @State private var seconds: UInt32 = 0
    var body: some View {
        Form {
            DisappearingMessagesPicker(seconds: $seconds)
            Text("Selected: \(DisappearingMessagesPicker.label(for: seconds))")
                .foregroundStyle(.secondary)
        }
    }
}

#Preview {
    DisappearingMessagesPickerPreview()
}
#endif
