import SwiftUI
import UIKit

/// Receives APNs callbacks and forwards the device token to PushManager.
@MainActor
final class AppDelegate: NSObject, UIApplicationDelegate {
    var appState: AppState?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
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
}

@main
struct ActnetApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @StateObject private var appState = AppState()

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
        }
    }
}
