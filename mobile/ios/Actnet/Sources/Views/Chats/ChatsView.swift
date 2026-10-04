import SwiftUI

struct ChatsView: View {
    @EnvironmentObject var appState: AppState
    @State private var showCompose = false
    @State private var navigationPath = NavigationPath()
    /// Namespace for the selection ring so it animates *between* avatars
    /// (a single moving indicator) rather than fading in/out per tab.
    @Namespace private var tabNamespace

    /// The account (identity) whose conversations are shown, when the user has
    /// more than one account and the tab strip is visible. Selection lives in
    /// AppState (survives navigation); the *effective* value is derived
    /// synchronously — explicit choice if it names a live account, else the
    /// first account — so the first render is already filtered (no unfiltered
    /// flash at cold launch). See `docs/37-chat-organization.md`.
    private var selectedAccountTab: String? {
        guard showsAccountTabs else { return nil }
        if let sel = appState.selectedChatsAccountTab,
           appState.accounts.contains(where: { $0.id == sel }) {
            return sel
        }
        return appState.accounts.first?.id
    }

    /// Whether the per-account tab strip is shown at all. Progressive disclosure:
    /// a single-account user sees a plain unified inbox (Signal baseline, `37`).
    private var showsAccountTabs: Bool {
        appState.accounts.count > 1
    }

    /// Conversations sorted by most recent message first.
    private var sortedConversations: [Conversation] {
        appState.conversations.sorted { a, b in
            (a.lastMessageDate ?? .distantPast) > (b.lastMessageDate ?? .distantPast)
        }
    }

    /// The conversations for the active view: filtered to the selected account
    /// tab when the strip is shown, otherwise the full unified list.
    private var visibleConversations: [Conversation] {
        guard showsAccountTabs, let selected = selectedAccountTab else {
            return sortedConversations
        }
        return sortedConversations.filter { $0.accountId == selected }
    }

    var body: some View {
        NavigationStack(path: $navigationPath) {
            VStack(spacing: 0) {
                header
                if !hasRecoveryKey {
                    RecoveryKeyBanner()
                }
                content
            }
            .background(Color.avPaper.ignoresSafeArea())
            .toolbar(.hidden, for: .navigationBar)
            .hidesTabBarWhenPushed(!navigationPath.isEmpty)
            .navigationDestination(for: Conversation.self) { conversation in
                ConversationView(conversation: conversation)
            }
            .sheet(isPresented: $showCompose) {
                ComposeMessageView()
            }
            .onChange(of: appState.navigateToConversation) {
                guard let conv = appState.navigateToConversation else { return }
                appState.navigateToConversation = nil
                // Replace the whole path in one atomic update: root → conversation.
                // The previous reset-to-empty + `DispatchQueue.main.async` append
                // was two mutations across runloops; on the deep-link path it
                // raced with the tab switch and the still-dismissing ProjectWebView
                // sheet, landing on a blank pushed view. A single assignment lands
                // cleanly at root → conversation with no intermediate empty state.
                navigationPath = NavigationPath([conv])
            }
        }
    }

    // MARK: Header (account tabs + compose)

    /// Custom top header replacing the old nav bar: the per-account tab strip
    /// (only with >1 account) on the leading side, compose on the trailing side.
    /// Compose is deliberately *not* part of the tab strip (`37`).
    private var header: some View {
        HStack(alignment: .center, spacing: 8) {
            if showsAccountTabs {
                accountTabStrip
            } else {
                Spacer()
            }
            composeButton
        }
        .padding(.horizontal, 16)
        .padding(.top, 4)
        .padding(.bottom, 6)
        .background(Color.avPaper)
        .overlay(alignment: .bottom) {
            Rectangle()
                .fill(Color.avInk.opacity(0.08))
                .frame(height: 0.5)
        }
    }

    private var accountTabStrip: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(appState.accounts) { account in
                    accountTab(account)
                }
            }
            // A single ring that follows the selected avatar's frame. Because
            // the avatars are the geometry *sources* and this is the sole
            // non-source follower, the slide is one continuous move — so the
            // duration below is the real speed knob (no insert/remove snap).
            .overlay {
                if let selected = selectedAccountTab {
                    Circle()
                        .strokeBorder(Color.avBrand, lineWidth: 2)
                        .frame(width: 36, height: 36)
                        .matchedGeometryEffect(id: selected, in: tabNamespace, isSource: false)
                        .allowsHitTesting(false)
                }
            }
            .animation(.easeOut(duration: 0.13), value: appState.selectedChatsAccountTab)
        }
        // The unread badge rides above the avatar's frame (offset y: -4) and the
        // tab content has only 2pt of headroom, so the ScrollView's default
        // clip shears the badge's top. Let the overlay draw outside the clip.
        .scrollClipDisabled()
    }

    private func accountTab(_ account: Account) -> some View {
        let selected = selectedAccountTab == account.id
        return Button {
            appState.selectedChatsAccountTab = account.id
        } label: {
            VStack(spacing: 5) {
                accountIcon(account, selected: selected)
                Text(accountLabel(account))
                    .font(.caption)
                    .fontWeight(selected ? .semibold : .regular)
                    .foregroundStyle(selected ? Color.avInk : Color.avMuted)
                    .lineLimit(1)
            }
            .frame(width: 68)
            .padding(.vertical, 2)
        }
        .buttonStyle(AccountTabButtonStyle())
    }

    /// The tab's icon: the identity's own avatar, falling back to a monogram
    /// circle. Selection is shown iOS-style — an avBrand ring that slides
    /// between avatars (via `matchedGeometryEffect`), selected avatar full and
    /// slightly enlarged, others dimmed (no underline). Auto-derived so a
    /// per-account tab is meaningful with zero config; user-created/topic tabs
    /// will carry a chosen symbol instead (tab organizer, deferred — `docs/37`).
    private func accountIcon(_ account: Account, selected: Bool) -> some View {
        avatarThumbnail(account)
            .frame(width: 30, height: 30)
            .clipShape(Circle())
            .opacity(selected ? 1 : 0.45)
            .scaleEffect(selected ? 1 : 0.9)
            .padding(3)
            // Publish this avatar's (36×36 padded) frame as a ring geometry
            // source; the single follower ring in the strip overlay matches
            // whichever one is selected.
            .matchedGeometryEffect(id: account.id, in: tabNamespace, isSource: true)
            // Unread badge — sum across the tab's conversations, same
            // notification/white style as the row badge, riding the avatar's
            // top-trailing corner.
            .overlay(alignment: .topTrailing) {
                let unread = unreadCount(for: account)
                if unread > 0 {
                    Text("\(unread)")
                        .font(.system(size: 9, weight: .bold))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 4)
                        .padding(.vertical, 1)
                        .background(Color.avNotification, in: Capsule())
                        .offset(x: 9, y: -4)
                        .allowsHitTesting(false)
                }
            }
    }

    /// Total unread messages across this account's conversations — rendered
    /// as a notification badge on the tab avatar (matches the row badge style).
    private func unreadCount(for account: Account) -> Int {
        appState.conversations
            .filter { $0.accountId == account.id }
            .reduce(0) { $0 + appState.unreadCount(for: $1) }
    }

    @ViewBuilder
    private func avatarThumbnail(_ account: Account) -> some View {
        if let data = account.avatarData, let image = UIImage(data: data) {
            Image(uiImage: image)
                .resizable()
                .scaledToFill()
        } else {
            Circle()
                .fill(Color.avCard)
                .overlay {
                    Text(accountLabel(account).prefix(1).uppercased())
                        .font(.subheadline)
                        .fontWeight(.semibold)
                        .foregroundStyle(Color.avBrand)
                }
        }
    }

    private var composeButton: some View {
        Button {
            showCompose = true
        } label: {
            Image(systemName: "square.and.pencil")
                .font(.title3)
                .foregroundStyle(Color.avBrand)
                .frame(width: 44, height: 44)
        }
        .accessibilityLabel("New message")
    }

    // MARK: Conversation list

    @ViewBuilder
    private var content: some View {
        if appState.conversations.isEmpty && !appState.conversationsLoaded {
            // Initial load still in flight — show a spinner instead of the
            // empty state, so "No conversations yet" doesn't flash on launch.
            ProgressView()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Color.avCard)
        } else if visibleConversations.isEmpty {
            ContentUnavailableView(
                "No conversations yet",
                systemImage: "message",
                description: Text("Messages from all your servers will appear here.")
            )
            .background(Color.avCard)
        } else {
            conversationList
        }
    }

    private var conversationList: some View {
        List {
            ForEach(visibleConversations) { conversation in
                Button {
                    navigationPath.append(conversation)
                } label: {
                    ConversationRow(conversation: conversation, account: accountFor(conversation))
                }
                .buttonStyle(ChatRowButtonStyle())
                .listRowInsets(EdgeInsets())
                .listRowBackground(Color.avCard)
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(Color.avCard)
    }

    // MARK: Helpers

    /// A short label for an account tab: the identity's display name, falling
    /// back to its first server's compact host when the name is unset.
    private func accountLabel(_ account: Account) -> String {
        if !account.displayName.isEmpty { return account.displayName }
        return account.servers.first?.displayHost ?? "Account"
    }

    private var hasRecoveryKey: Bool {
        // TODO: Check via Rust core
        false
    }

    private func accountFor(_ conversation: Conversation) -> Account? {
        appState.accounts.first { $0.id == conversation.accountId }
    }
}

/// Press feedback for a chat row: a full-bleed backing highlight while the
/// finger is down (over the row's `avCard`), released on lift. Rows are Buttons
/// (not NavigationLinks) so this press state is controllable — and so they get
/// no auto-disclosure chevron, matching a Signal/Messages-style chat list.
struct ChatRowButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(configuration.isPressed ? Color.avInk.opacity(0.06) : Color.clear)
            .contentShape(Rectangle())
    }
}

/// Press feedback for an account tab: a subtle backing highlight + slight
/// shrink while the finger is down, released on lift. Immediate (short ease),
/// independent of the springy selection animation.
private struct AccountTabButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background {
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .fill(Color.avInk.opacity(configuration.isPressed ? 0.08 : 0))
            }
            .scaleEffect(configuration.isPressed ? 0.93 : 1)
            .animation(.easeOut(duration: 0.12), value: configuration.isPressed)
    }
}

#if DEBUG
@MainActor
private func previewChatsState(accounts: [Account]) -> AppState {
    let state = AppState.preview(accounts: accounts)
    state.conversationsLoaded = true
    let now = Date()
    func conv(_ id: String, _ title: String, _ account: Account, _ body: String,
              _ ago: TimeInterval, group: Bool = false) -> Conversation {
        Conversation(
            id: id, title: title, accountId: account.id,
            serverUrl: account.servers.first?.url.absoluteString ?? "",
            lastMessage: body, lastMessageDate: now.addingTimeInterval(-ago), isGroup: group
        )
    }
    let real = accounts[0]
    var convos = [
        conv("c1", "Canvass NW", real, "See you at 5", 120, group: true),
        conv("c2", "Alice", real, "sounds good", 900),
        conv("c3", "Phone Bank", real, "who's in tonight?", 3600, group: true),
    ]
    if accounts.count > 1 {
        let persona = accounts[1]
        convos += [
            conv("c4", "Bob", persona, "hey there", 7200),
            conv("c5", "Reading Group", persona, "chapter 4 thoughts?", 90_000, group: true),
        ]
    }
    state.conversations = convos
    state.unreadCounts = ["c1": 3, "c3": 1, "c4": 2]
    return state
}

private func previewAccount(_ id: String, _ name: String, _ host: String) -> Account {
    Account(
        id: id, displayName: name, avatarData: nil,
        servers: [ServerInfo(id: host, name: host, url: URL(string: "https://\(host)")!)]
    )
}

#Preview("Chats – multi-account") {
    let accounts = [
        previewAccount("did:plc:real", "Real", "safe-haven.org"),
        previewAccount("did:plc:persona", "Persona", "pseudo.example"),
    ]
    return ChatsView().environmentObject(previewChatsState(accounts: accounts))
}

#Preview("Chats – multi-account (dark)") {
    let accounts = [
        previewAccount("did:plc:real", "Real", "safe-haven.org"),
        previewAccount("did:plc:persona", "Persona", "pseudo.example"),
    ]
    return ChatsView()
        .environmentObject(previewChatsState(accounts: accounts))
        .preferredColorScheme(.dark)
}

#Preview("Chats – single account") {
    let accounts = [previewAccount("did:plc:real", "Real", "safe-haven.org")]
    return ChatsView().environmentObject(previewChatsState(accounts: accounts))
}
#endif
