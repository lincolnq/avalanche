import UserNotifications

/// Notification Service Extension (docs/16). The relay sends a content-free
/// alert + `mutable-content` push; iOS invokes this extension, which fetches +
/// decrypts the actual message(s) on-device via app-core and rewrites the banner
/// with the real sender and text. If anything fails or we run out of time, the
/// generic "New message" best-attempt is delivered instead — we degrade to a
/// generic banner, never to silence.
///
/// Targeting is deferred (docs/16): the push carries no account hint, so we fetch
/// every local account. The most recent message rewrites the triggering banner;
/// any others are scheduled as additional local notifications.
///
/// `didReceive` runs on a framework-provided background queue, so the synchronous
/// app-core FFI (which blocks on the Rust runtime) runs inline here — no extra
/// task hop. `serviceExtensionTimeWillExpire` is the escape hatch if the fetch
/// runs past iOS's budget.
class NotificationService: UNNotificationServiceExtension {
    private var contentHandler: ((UNNotificationContent) -> Void)?
    private var bestAttempt: UNMutableNotificationContent?

    override func didReceive(
        _ request: UNNotificationRequest,
        withContentHandler contentHandler: @escaping (UNNotificationContent) -> Void
    ) {
        self.contentHandler = contentHandler
        let best = (request.content.mutableCopy() as? UNMutableNotificationContent)
            ?? UNMutableNotificationContent()
        self.bestAttempt = best

        // Shared secrets/paths (docs/16 deps 1–2). Any miss → generic banner.
        guard let dbKey = try? SecureEnclaveKeyManager.dbPassphrase(),
              let dbDir = AppGroup.dbDir else {
            deliver(items: [])
            return
        }

        let fm = FileManager.default
        var items: [NotifItemFfi] = []
        for account in SharedAccountStore.load() {
            let dbPath = dbDir.appendingPathComponent(account.dbFilename).path
            guard fm.fileExists(atPath: dbPath) else { continue }
            // Best-effort per account: a failure for one must not sink the rest.
            if let fetched = try? fetchNotifications(dbPath: dbPath, dbKey: dbKey) {
                items.append(contentsOf: fetched)
            }
        }
        deliver(items: items)
    }

    /// iOS is about to kill the extension — hand back whatever we have (the
    /// enriched banner if we finished, otherwise the generic best-attempt).
    override func serviceExtensionTimeWillExpire() {
        if let contentHandler, let bestAttempt {
            contentHandler(bestAttempt)
            self.contentHandler = nil
        }
    }

    private func deliver(items: [NotifItemFfi]) {
        guard let contentHandler, let best = bestAttempt else { return }
        self.contentHandler = nil

        // Oldest → newest, so the newest rewrites the shown banner.
        let sorted = items.sorted { $0.sentAtMs < $1.sentAtMs }
        guard let primary = sorted.last else {
            // Nothing fetched — keep the generic "New message" best-attempt.
            contentHandler(best)
            return
        }

        apply(primary, to: best)
        // Surface the rest as their own banners.
        for extra in sorted.dropLast() {
            let content = UNMutableNotificationContent()
            apply(extra, to: content)
            content.sound = .default
            let req = UNNotificationRequest(
                identifier: UUID().uuidString,
                content: content,
                trigger: nil)
            UNUserNotificationCenter.current().add(req)
        }
        contentHandler(best)
    }

    /// Populate a notification's title/body/routing from a fetched item, matching
    /// the in-app `NotificationPresenter` shape (title = sender for DMs, group
    /// name for groups; userInfo carries conversation + account for tap routing).
    private func apply(_ item: NotifItemFfi, to content: UNMutableNotificationContent) {
        if item.isGroup {
            content.title = item.groupTitle ?? "New message"
            content.body = item.senderDisplayName.isEmpty
                ? item.body
                : "\(item.senderDisplayName): \(item.body)"
        } else {
            content.title = item.senderDisplayName.isEmpty ? "New message" : item.senderDisplayName
            content.body = item.body
        }
        content.threadIdentifier = item.conversationId
        content.userInfo = [
            "conversationId": item.conversationId,
            "accountId": item.accountId,
        ]
    }
}
