import SwiftUI
import UIKit

/// Receives APNs callbacks and forwards the device token to PushManager.
final class AppDelegate: NSObject, UIApplicationDelegate {
    var appState: AppState?

    func application(
        _ application: UIApplication,
        didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data
    ) {
        guard let appState else { return }
        PushManager.didReceiveToken(deviceToken, appState: appState)
    }

    func application(
        _ application: UIApplication,
        didFailToRegisterForRemoteNotificationsWithError error: Error
    ) {
        print("[PushManager] failed to register for remote notifications: \(error)")
    }
}

@main
struct ActnetApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @StateObject private var appState = AppState()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(appState)
                .task {
                    appDelegate.appState = appState
                    await appState.restoreAccounts()
                }
                .onOpenURL { url in
                    appState.handleDeepLink(url)
                }
        }
    }
}
