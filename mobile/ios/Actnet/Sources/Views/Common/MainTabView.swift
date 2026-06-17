import SwiftUI

struct MainTabView: View {
    @EnvironmentObject var appState: AppState

    var body: some View {
        TabView(selection: $appState.selectedTab) {
            ChatsView()
                .tabItem {
                    Label("Chats", systemImage: "message")
                }
                .tag(AppState.Tab.chats)

            NetworkView()
                .tabItem {
                    Label("Network", systemImage: "server.rack")
                }
                .tag(AppState.Tab.network)
        }
    }
}
