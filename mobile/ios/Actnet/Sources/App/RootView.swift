import SwiftUI

struct RootView: View {
    @EnvironmentObject var appState: AppState

    var body: some View {
        Group {
            if appState.isOnboarding {
                SplashView()
            } else {
                MainTabView()
            }
        }
        .background(Color.avPaper)
    }
}
