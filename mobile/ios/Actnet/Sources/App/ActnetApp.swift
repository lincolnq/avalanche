import SwiftUI
import UIKit
import UserNotifications

/// Receives APNs callbacks and forwards the device token to PushManager.
/// Also acts as the UNUserNotificationCenter delegate, routing local
/// notification taps back to the right conversation.
@MainActor
final class AppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    var appState: AppState?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        UNUserNotificationCenter.current().delegate = self
        let sand100 = UIColor(red: 1.0, green: 0.945, blue: 0.914, alpha: 1.0)
        let plum500 = UIColor(red: 0.420, green: 0.243, blue: 0.314, alpha: 1.0)

        // Global tint
        UIView.appearance().tintColor = plum500

        // Navigation bar
        let navAppearance = UINavigationBarAppearance()
        navAppearance.configureWithOpaqueBackground()
        navAppearance.backgroundColor = sand100
        UINavigationBar.appearance().standardAppearance = navAppearance
        UINavigationBar.appearance().scrollEdgeAppearance = navAppearance

        // Tab bar
        let tabAppearance = UITabBarAppearance()
        tabAppearance.configureWithOpaqueBackground()
        tabAppearance.backgroundColor = sand100
        UITabBar.appearance().standardAppearance = tabAppearance
        UITabBar.appearance().scrollEdgeAppearance = tabAppearance

        // Table/collection views (backs List in SwiftUI)
        UITableView.appearance().backgroundColor = sand100
        UICollectionView.appearance().backgroundColor = sand100

        return true
    }

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

    // MARK: - UNUserNotificationCenterDelegate

    /// Called when a notification is delivered while the app is in the
    /// foreground. We've already filtered out the "user is reading this chat"
    /// case at schedule time, so anything that arrives here should still
    /// surface as a banner.
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .list, .sound, .badge])
    }

    /// Called when the user taps a notification (foreground or background).
    /// Routes to the relevant conversation.
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let userInfo = response.notification.request.content.userInfo
        let conversationId = userInfo["conversationId"] as? String
        let accountId = userInfo["accountId"] as? String
        // Tell iOS we're done immediately — the navigation hop below happens
        // independently on the main actor.
        completionHandler()
        Task { @MainActor in
            guard let conversationId, let accountId, let appState = self.appState else { return }
            let conv = appState.conversations.first(where: { $0.id == conversationId && $0.accountId == accountId })
            guard let conv else { return }
            appState.selectedTab = .chats
            appState.navigateToConversation = conv
        }
    }
}

@main
struct ActnetApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @StateObject private var appState = AppState()
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            RootView()
                .tint(Color.avBrand)
                .environmentObject(appState)
                .task {
                    appDelegate.appState = appState
                    await appState.restoreAccounts()
                }
                .onOpenURL { url in
                    appState.handleDeepLink(url)
                }
                .onChange(of: scenePhase) { _, newPhase in
                    appState.isAppActive = (newPhase == .active)
                }
        }
    }
}
