import SwiftUI

/// Content of the native `.search` role tab (`docs/37-chat-organization.md`).
/// A `.searchable` conversation list filtered by title, across all accounts —
/// search is a cross-cutting lens, not scoped to the selected account tab.
/// Client-side only for v1: filters the already-loaded conversation list, no
/// server-side search.
struct ConversationSearchView: View {
    @EnvironmentObject var appState: AppState
    @State private var query = ""
    @State private var navigationPath = NavigationPath()

    private var results: [Conversation] {
        let sorted = appState.conversations.sorted { a, b in
            (a.lastMessageDate ?? .distantPast) > (b.lastMessageDate ?? .distantPast)
        }
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return sorted }
        return sorted.filter { $0.title.localizedCaseInsensitiveContains(trimmed) }
    }

    var body: some View {
        NavigationStack(path: $navigationPath) {
            VStack(spacing: 0) {
                header
                List {
                    ForEach(results) { conversation in
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
                .overlay {
                    if results.isEmpty && !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        ContentUnavailableView.search(text: query)
                    }
                }
            }
            .background(Color.avPaper.ignoresSafeArea())
            .toolbar(.hidden, for: .navigationBar)
            .hidesTabBarWhenPushed(!navigationPath.isEmpty)
            .navigationDestination(for: Conversation.self) { conversation in
                ConversationView(conversation: conversation)
            }
        }
        .searchable(text: $query, prompt: "Search conversations")
    }

    /// Custom header with the SAME geometry as ChatsView's (which replaces
    /// the nav bar with the account tab strip) — otherwise the two tabs' top
    /// bars differ in height and everything below jumps on tab switch. An
    /// invisible prototype of one account tab provides the height, so the
    /// match survives dynamic type. Keep the metrics in sync with
    /// `ChatsView.accountTab` / `ChatsView.header`.
    private var header: some View {
        HStack(alignment: .center, spacing: 8) {
            ZStack(alignment: .leading) {
                if appState.accounts.count > 1 {
                    // Prototype of ChatsView.accountTab's vertical structure.
                    VStack(spacing: 5) {
                        Circle().frame(width: 30, height: 30).padding(3)
                        Text("X").font(.caption).padding(.vertical, 2)
                    }
                    .hidden()
                } else {
                    // Single account: ChatsView's header is just the 44pt
                    // compose button.
                    Color.clear.frame(width: 1, height: 44)
                }
                Text("Search")
                    .font(.headline)
            }
            Spacer()
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

    private func accountFor(_ conversation: Conversation) -> Account? {
        appState.accounts.first { $0.id == conversation.accountId }
    }
}
