import SwiftUI

struct ChatsView: View {
    @EnvironmentObject var appState: AppState
    @State private var showDevSettings = false
    @State private var showCompose = false
    @State private var navigationPath = NavigationPath()

    /// Conversations sorted by most recent message first.
    private var sortedConversations: [Conversation] {
        appState.conversations.sorted { a, b in
            (a.lastMessageDate ?? .distantPast) > (b.lastMessageDate ?? .distantPast)
        }
    }

    var body: some View {
        NavigationStack(path: $navigationPath) {
            Group {
                if appState.conversations.isEmpty {
                    ContentUnavailableView(
                        "No conversations yet",
                        systemImage: "message",
                        description: Text("Messages from all your servers will appear here.")
                    )
                } else {
                    conversationList
                }
            }
            .navigationTitle("Chats")
            .navigationDestination(for: Conversation.self) { conversation in
                ConversationView(conversation: conversation)
            }
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button {
                        showDevSettings = true
                    } label: {
                        Image(systemName: "gearshape")
                            .font(.subheadline)
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        showCompose = true
                    } label: {
                        Image(systemName: "square.and.pencil")
                    }
                }
            }
            .overlay(alignment: .top) {
                if !hasRecoveryKey {
                    RecoveryKeyBanner()
                }
            }
            .sheet(isPresented: $showDevSettings) {
                DevSettingsView()
            }
            .sheet(isPresented: $showCompose) {
                ComposeMessageView()
            }
            .onChange(of: appState.navigateToConversation) {
                if let conv = appState.navigateToConversation {
                    appState.navigateToConversation = nil
                    navigationPath = NavigationPath()
                    // Push after clearing so we land cleanly at root → conversation.
                    DispatchQueue.main.async {
                        navigationPath.append(conv)
                    }
                }
            }
        }
    }

    private var conversationList: some View {
        List(sortedConversations) { conversation in
            NavigationLink(value: conversation) {
                ConversationRow(conversation: conversation, account: accountFor(conversation))
            }
        }
        .listStyle(.plain)
    }

    private var hasRecoveryKey: Bool {
        // TODO: Check via Rust core
        false
    }

    private func accountFor(_ conversation: Conversation) -> Account? {
        appState.accounts.first { $0.id == conversation.accountId }
    }
}
