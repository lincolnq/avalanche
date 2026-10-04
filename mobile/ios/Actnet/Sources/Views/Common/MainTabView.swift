import SwiftUI

/// The app's root tab surface. A native `TabView` so it renders as a real
/// iOS 26 Liquid Glass tab bar (and the `.search` role becomes the detached
/// floating search capsule) — see `docs/37-chat-organization.md`. On iOS 18–25
/// it degrades to the standard opaque tab bar and the search role shows as a
/// normal tab, the same graceful-fallback approach as `composerPillBackground`.
struct MainTabView: View {
    @EnvironmentObject var appState: AppState

    var body: some View {
        if #available(iOS 26.0, *) {
            tabs.tabBarMinimizeBehavior(.onScrollDown)
        } else {
            tabs
        }
    }

    @ViewBuilder
    private var tabs: some View {
        TabView(selection: $appState.selectedTab) {
            // Brand tab glyphs — design/Chats Bubble.svg and design/Network
            // Globe.svg, imported as template SVGs so the tab tint applies.
            Tab("Chats", image: "TabChats", value: AppState.Tab.chats) {
                ChatsView()
            }

            Tab("Network", image: "TabNetwork", value: AppState.Tab.network) {
                NetworkView()
            }

            Tab("Settings", systemImage: "gearshape", value: AppState.Tab.settings) {
                AccountsView()
            }

            Tab("Search", systemImage: "magnifyingglass", value: AppState.Tab.search, role: .search) {
                ConversationSearchView()
            }
        }
    }
}

/// Hides the tab bar while anything is pushed on a tab's NavigationStack (you
/// don't tab-navigate from inside a thread, so the composer gets the bottom
/// edge). Apply to the stack's *root* view. Driving it from the path rather
/// than `.toolbar(.hidden)` on the pushed view lets it animate with the push
/// and reappear as the pop starts, instead of popping back in only after the
/// pushed view is torn down.
private struct HidesTabBarWhenPushed: ViewModifier {
    let isPushed: Bool
    @State private var hidden = false

    func body(content: Content) -> some View {
        content
            .toolbar(hidden ? .hidden : .visible, for: .tabBar)
            .onAppear { hidden = isPushed }
            .onChange(of: isPushed) { _, pushed in
                withAnimation(.easeInOut(duration: 0.25)) { hidden = pushed }
            }
    }
}

extension View {
    func hidesTabBarWhenPushed(_ isPushed: Bool) -> some View {
        modifier(HidesTabBarWhenPushed(isPushed: isPushed))
    }
}
