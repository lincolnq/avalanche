import UIKit
import UserNotifications

/// Schedules local notifications for inbound messages.
///
/// Suppression rules (adapted from Signal-iOS):
/// - App active + viewing this conversation → no banner (badge still updates).
/// - App active + viewing a different conversation → banner + sound.
/// - App backgrounded/inactive → banner + sound.
///
/// Receipts and other non-Text envelope variants are filtered out in Rust
/// (`AppCoreInner::receive_messages_ws_async`) before they reach Swift, so
/// every call here corresponds to a real user-visible message.
@MainActor
enum NotificationPresenter {
    /// Per-conversation timestamp of the last notification sound. Used to
    /// throttle rapid-fire sounds when many messages arrive in a burst.
    private static var lastSoundAt: [String: Date] = [:]
    private static let soundThrottleSeconds: TimeInterval = 3

    /// Schedule a local notification for an inbound message and refresh the
    /// app badge. No-op for outgoing messages — callers must only invoke this
    /// for messages where `senderDid != accountId`.
    static func present(
        message: Message,
        conversation: Conversation,
        senderDisplayName: String,
        appState: AppState
    ) {
        updateBadge(appState: appState)

        // Empty body — nothing useful to show. (Defensive: shouldn't happen
        // for Body::Text, but guards against future control-message variants
        // accidentally surfacing as DecryptedMessage.)
        let body = message.body.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !body.isEmpty else { return }

        // Suppress when the user is already reading this conversation.
        if appState.isAppActive && appState.currentConversationId == conversation.id {
            return
        }

        let content = UNMutableNotificationContent()
        content.title = senderDisplayName
        content.body = body
        content.threadIdentifier = conversation.id
        content.userInfo = [
            "conversationId": conversation.id,
            "accountId": conversation.accountId,
        ]

        if shouldPlaySound(for: conversation.id) {
            content.sound = .default
        }

        let request = UNNotificationRequest(
            identifier: message.id,
            content: content,
            trigger: nil
        )
        UNUserNotificationCenter.current().add(request) { error in
            if let error {
                print("[NotificationPresenter] failed to schedule: \(error)")
            }
        }
    }

    /// Recompute the app icon badge from in-memory unread counts.
    static func updateBadge(appState: AppState) {
        let total = appState.conversations.reduce(0) { sum, conv in
            sum + appState.unreadCount(for: conv)
        }
        UNUserNotificationCenter.current().setBadgeCount(total) { error in
            if let error {
                print("[NotificationPresenter] failed to set badge: \(error)")
            }
        }
    }

    private static func shouldPlaySound(for conversationId: String) -> Bool {
        let now = Date()
        if let last = lastSoundAt[conversationId], now.timeIntervalSince(last) < soundThrottleSeconds {
            return false
        }
        lastSoundAt[conversationId] = now
        return true
    }
}
