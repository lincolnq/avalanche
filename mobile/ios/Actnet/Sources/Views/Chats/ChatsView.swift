import SwiftUI

struct ChatsView: View {
    @EnvironmentObject var appState: AppState
    @State private var showAccounts = false
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
                        showAccounts = true
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
            .sheet(isPresented: $showAccounts) {
                AccountsView()
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

    private var conversationList: some View {
        List(sortedConversations) { conversation in
            NavigationLink(value: conversation) {
                ConversationRow(conversation: conversation, account: accountFor(conversation))
            }
        }
        .listStyle(.plain)
        .listRowBackground(Color.avPaper)
        .scrollContentBackground(.hidden)
        .background(Color.avPaper)
    }

    private var hasRecoveryKey: Bool {
        // TODO: Check via Rust core
        false
    }

    private func accountFor(_ conversation: Conversation) -> Account? {
        appState.accounts.first { $0.id == conversation.accountId }
    }
}
