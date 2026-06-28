import Foundation

/// Shared App Group container used to hand an image off from the share extension
/// to the main app (docs/35). Compiled into BOTH the `Actnet` app target and the
/// `ShareExtension` target — the extension writes a pending share, the app reads
/// and clears it on launch/foreground.
///
/// This is deliberately a thin file handoff, not a shared database: the extension
/// runs no app-core (no crypto/DB/network), so there is no multi-process access
/// to the encrypted store. The app does the picking, uploading, and sending.
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
