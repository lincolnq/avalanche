import UIKit
import UserNotifications

/// Manages APNs push notification registration and device token lifecycle.
///
/// Call `requestPermissionAndRegister` after a successful login or account
/// creation. Wire `didReceiveToken` from the AppDelegate's
/// `application(_:didRegisterForRemoteNotificationsWithDeviceToken:)`.
enum PushManager {
    /// Request push permission and, if granted, register with APNs.
    static func requestPermissionAndRegister(appState: AppState) async {
        let center = UNUserNotificationCenter.current()
        let granted = (try? await center.requestAuthorization(options: [.alert, .sound, .badge])) ?? false
        guard granted else { return }
        await MainActor.run {
            UIApplication.shared.registerForRemoteNotifications()
        }
    }

    /// Called when APNs issues a device token. Registers it with all active cores.
    @MainActor
    static func didReceiveToken(_ tokenData: Data, appState: AppState) {
        let tokenString = tokenData.map { String(format: "%02x", $0) }.joined()
        print("[PushManager] APNs device token: \(tokenString)")

        let relayUrl = (Bundle.main.object(forInfoDictionaryKey: "RELAY_URL") as? String) ?? ""
        guard !relayUrl.isEmpty else {
            print("[PushManager] RELAY_URL is empty — push registration skipped. Set RELAY_URL env var and re-run `make xcode`.")
            return
        }
        // Debug builds get sandbox APNs tokens; release builds get production
        // tokens. Sending one to the wrong endpoint returns BadDeviceToken.
        #if DEBUG
        let environment = "sandbox"
        #else
        let environment = "production"
        #endif

        let cores = appState.activeCores()
        Task {
            for core in cores {
                // registerPushToken is idempotent and self-rotates when the
                // existing pseudonym is older than the rotation threshold —
                // safe to call on every launch.
                do {
                    try await Task.detached {
                        try core.registerPushToken(
                            deviceToken: tokenString,
                            platform: "apns",
                            relayUrl: relayUrl,
                            environment: environment
                        )
                    }.value
                    print("[PushManager] registerPushToken ok (relay=\(relayUrl), env=\(environment))")
                } catch {
                    print("[PushManager] registerPushToken failed (relay=\(relayUrl)): \(error)")
                }
            }
        }
    }
}
