import Foundation

/// Shared App Group container. Two uses:
///   1. Hand an image off from the share extension to the main app (docs/35) — a
///      thin file handoff (the ShareExtension runs no app-core).
///   2. Host the per-account SQLCipher databases and the account list so the
///      Notification Service Extension (docs/16) can open the same encrypted
///      store the app does. Unlike (1), this *is* shared mutable DB state; the
///      cross-process coordination is handled in app-core (docs/16 Stage 4).
///
/// Compiled into the `Actnet` app target and the `ShareExtension` target (and,
/// once it lands, the NSE target).
enum AppGroup {
    /// Must match the `com.apple.security.application-groups` entitlement on both
    /// targets in `project.yml`.
    static let identifier = "group.net.theavalanche.app"

    /// Custom URL scheme the extension opens to foreground the app after staging
    /// a share. Registered in the app's Info.{Debug,Release}.plist CFBundleURLTypes.
    static let shareURLScheme = "avalanche-share"

    private static let contentTypeKey = "pendingShareContentType"

    private static var containerURL: URL? {
        FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: identifier)
    }

    /// Directory inside the App Group container holding the per-account SQLCipher
    /// databases (docs/16 dep 2). `nil` if the container can't be resolved
    /// (entitlement/provisioning issue) — callers fall back to app-private storage.
    static var dbDir: URL? {
        containerURL?.appendingPathComponent("actnet", isDirectory: true)
    }

    /// Shared `UserDefaults` suite, where the account list lives so the NSE can
    /// enumerate accounts (docs/16 dep 3). `nil` if the suite can't be opened.
    static var sharedDefaults: UserDefaults? {
        UserDefaults(suiteName: identifier)
    }

    private static var pendingShareURL: URL? {
        containerURL?.appendingPathComponent("pending-share.bin")
    }

    private static var defaults: UserDefaults? {
        UserDefaults(suiteName: identifier)
    }

    /// Persist an incoming shared image for the main app to pick up. Called from
    /// the share extension. A single pending slot — last share wins (we accept one
    /// image at a time). Returns false if the App Group container couldn't be
    /// resolved (entitlement/provisioning issue) or the write failed.
    @discardableResult
    static func writePendingShare(data: Data, contentType: String) -> Bool {
        guard let url = pendingShareURL else { return false }
        do {
            try data.write(to: url, options: .atomic)
        } catch {
            return false
        }
        defaults?.set(contentType, forKey: contentTypeKey)
        return true
    }

    /// Read and clear the pending shared image, if any. Called from the main app
    /// when it handles the `avalanche-share://` open (or on foreground).
    static func takePendingShare() -> (data: Data, contentType: String)? {
        guard let url = pendingShareURL,
              let data = try? Data(contentsOf: url) else { return nil }
        let contentType = defaults?.string(forKey: contentTypeKey) ?? "image/jpeg"
        try? FileManager.default.removeItem(at: url)
        defaults?.removeObject(forKey: contentTypeKey)
        return (data, contentType)
    }
}
