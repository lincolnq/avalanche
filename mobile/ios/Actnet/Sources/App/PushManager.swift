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
    static func didReceiveToken(_ tokenData: Data, appState: AppState) {
        let tokenString = tokenData.map { String(format: "%02x", $0) }.joined()
        let cores = appState.activeCores()
        Task {
            for core in cores {
                try? await Task.detached {
                    try core.registerPushToken(deviceToken: tokenString, platform: "apns")
                }.value
            }
        }
    }
}
