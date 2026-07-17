import SwiftUI
import UIKit
import UserNotifications

/// True when the process is hosting a SwiftUI `#Preview`. The preview harness
/// boots the real app (`@main` → `AppDelegate`) and reuses the process across
/// runs, so launch-time side effects must be skipped: `initLogging` is a
/// `try!` FFI call into Rust's global logger, which panics (→ traps) if init'd
/// twice, and `restoreAccounts` touches the keychain / Secure Enclave and spins
/// up cores. Neither is wanted in a preview.
var isRunningInPreview: Bool {
    ProcessInfo.processInfo.environment["XCODE_RUNNING_FOR_PREVIEWS"] == "1"
}

/// Receives APNs callbacks and forwards the device token to PushManager.
/// Also acts as the UNUserNotificationCenter delegate, routing local
/// notification taps back to the right conversation.
@MainActor
final class AppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    var appState: AppState?
    /// A notification tapped during cold launch can fire before RootView wires
    /// up `appState`. Stash it here; RootView flushes it once appState exists
    /// (docs/16).
    var pendingNotificationTarget: (conversationId: String, accountId: String)?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        if isRunningInPreview { return true }
        #if DEBUG
        initLogging(filter: "app_core=debug,net=info,store=info,crypto=info")
        #else
        initLogging(filter: "info")
        #endif
        UNUserNotificationCenter.current().delegate = self
        // Adaptive surface/tint: warm cream in light, deep plum in dark. These
        // mirror Color.avPaper / Color.avBrand (AvalancheColors.swift) but are
        // built as dynamic UIColors so the UIKit-backed chrome (nav/tab bars,
        // the UITableView/UICollectionView behind SwiftUI List) adapts too.
        let paper = UIColor { traits in
            traits.userInterfaceStyle == .dark
                ? UIColor(red: 0.165, green: 0.086, blue: 0.125, alpha: 1.0)  // plum900
                : UIColor(red: 1.000, green: 0.945, blue: 0.914, alpha: 1.0)  // sand100
        }
        let tint = UIColor { traits in
            traits.userInterfaceStyle == .dark
                ? UIColor(red: 0.784, green: 0.706, blue: 0.741, alpha: 1.0)  // plum200
                : UIColor(red: 0.420, green: 0.243, blue: 0.314, alpha: 1.0)  // plum500
        }

        // Global tint
        UIView.appearance().tintColor = tint

        // Navigation bar
        let navAppearance = UINavigationBarAppearance()
        navAppearance.configureWithOpaqueBackground()
        navAppearance.backgroundColor = paper
        UINavigationBar.appearance().standardAppearance = navAppearance
        UINavigationBar.appearance().scrollEdgeAppearance = navAppearance

        // Tab bar
        let tabAppearance = UITabBarAppearance()
        tabAppearance.configureWithOpaqueBackground()
        tabAppearance.backgroundColor = paper
        UITabBar.appearance().standardAppearance = tabAppearance
        UITabBar.appearance().scrollEdgeAppearance = tabAppearance

        // Table/collection views (backs List in SwiftUI)
        UITableView.appearance().backgroundColor = paper
        UICollectionView.appearance().backgroundColor = paper

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

    /// Called when a silent push (content-available: 1) is delivered. iOS
    /// gives us ~30s of background runtime to fetch new messages. We kick
    /// the WebSocket-backed message polling loops (idempotent — no-op if
    /// already running) and give them a short window to drain anything
    /// the relay woke us up for. The event loop dispatches each message
    /// through NotificationPresenter, which schedules the local banner.
    func application(
        _ application: UIApplication,
        didReceiveRemoteNotification userInfo: [AnyHashable: Any],
        fetchCompletionHandler completionHandler: @escaping (UIBackgroundFetchResult) -> Void
    ) {
        print("[PushHandler] silent push received, app state = \(application.applicationState.rawValue)")
        Task { @MainActor in
            guard let appState else {
                completionHandler(.noData)
                return
            }
            if appState.accounts.isEmpty {
                await appState.restoreAccounts()
            } else {
                appState.startMessagePolling()
            }
            try? await Task.sleep(nanoseconds: 8_000_000_000)
            print("[PushHandler] completing background fetch")
            completionHandler(.newData)
        }
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
            guard let conversationId, let accountId else { return }
            if let appState = self.appState {
                appState.openConversationFromNotification(
                    conversationId: conversationId, accountId: accountId)
            } else {
                // Cold launch: the tap can fire before RootView wires appState.
                // RootView flushes this once it's available (docs/16).
                self.pendingNotificationTarget = (conversationId, accountId)
            }
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
            if isRunningInPreview {
                // The preview harness boots the real app to host a `#Preview`.
                // Render nothing here so the launch scene does no work
                // (no RootView, window gestures, banners, or restore) — each
                // `#Preview` supplies its own view and `AppState`.
                Color.clear
            } else {
                RootView()
                    .tint(Color.avBrand)
                    .environmentObject(appState)
                    .task {
                        appDelegate.appState = appState
                        // Replay a notification tapped during cold launch that
                        // arrived before appState existed (docs/16). This stashes
                        // into appState if conversations aren't loaded yet, and
                        // the conversation load replays it from there.
                        if let target = appDelegate.pendingNotificationTarget {
                            appDelegate.pendingNotificationTarget = nil
                            appState.openConversationFromNotification(
                                conversationId: target.conversationId,
                                accountId: target.accountId)
                        }
                        await appState.restoreAccounts()
                        // Cold-launch safety net for shares (docs/35): scenePhase
                        // starts at .active so its onChange won't fire on launch.
                        appState.handleSharedImage()
                    }
                    .onOpenURL { url in
                        appState.handleDeepLink(url)
                    }
                    // Universal links tapped on a real device arrive as a
                    // browsing-web NSUserActivity, which (with an app-delegate
                    // adaptor present) is NOT delivered to onOpenURL.
                    .onContinueUserActivity(NSUserActivityTypeBrowsingWeb) { activity in
                        guard let url = activity.webpageURL else {
                            AppLog.warn("DeepLink", "browsing-web activity with no webpageURL")
                            return
                        }
                        appState.handleDeepLink(url)
                    }
                    .onChange(of: scenePhase) { _, newPhase in
                        let active = (newPhase == .active)
                        appState.isAppActive = active
                        // Gate the WS keepalive (foreground-only) and probe the
                        // connection on resume so a socket that died while the
                        // app was suspended recovers without a restart.
                        appState.setAppActiveAll(active)
                        // Safety net for shared images (docs/35): the extension
                        // opens us via avalanche-share://, but if that open is
                        // missed, picking it up on foreground still surfaces the
                        // picker the moment the user switches to the app.
                        if active { appState.handleSharedImage() }
                    }
            }
        }
    }
}
