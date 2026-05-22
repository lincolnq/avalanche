import SwiftUI

@main
struct ActnetApp: App {
    @StateObject private var appState = AppState()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(appState)
                .task {
                    await appState.restoreAccounts()
                }
                .onOpenURL { url in
                    appState.handleDeepLink(url)
                }
        }
    }
}
