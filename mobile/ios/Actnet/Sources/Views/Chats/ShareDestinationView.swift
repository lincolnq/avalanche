import SwiftUI

/// An image shared into the app from another app (docs/35), awaiting a chat.
/// `Identifiable` so it can drive a `.sheet(item:)` presentation.
struct PendingSharedImage: Identifiable {
    let id = UUID()
    let data: Data
    let contentType: String
}

/// Destination picker shown when an image is shared into the app (docs/35).
/// Reuses the conversation list; picking a chat stages the shared image in that
/// conversation's composer for review (caption / remove) before sending — the
/// handoff equivalent of Signal's in-share-sheet chat picker.
struct ShareDestinationView: View {
    @EnvironmentObject var appState: AppState
    let image: PendingSharedImage

    private var sortedConversations: [Conversation] {
        appState.conversations.sorted {
            ($0.lastMessageDate ?? .distantPast) > ($1.lastMessageDate ?? .distantPast)
        }
    }

    var body: some View {
        NavigationStack {
            Group {
                if sortedConversations.isEmpty {
                    ContentUnavailableView(
                        "No conversations",
                        systemImage: "message",
                        description: Text("Start a conversation first, then share to it.")
                    )
                } else {
                    List(sortedConversations) { conversation in
                        Button {
                            appState.routeSharedImage(to: conversation)
                        } label: {
                            ConversationRow(
                                conversation: conversation,
                                account: appState.accounts.first { $0.id == conversation.accountId }
                            )
                        }
                        .buttonStyle(.plain)
                    }
                    .listStyle(.plain)
                    .scrollContentBackground(.hidden)
                    .background(Color.avPaper)
                }
            }
            .navigationTitle("Share to…")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { appState.pendingSharedImage = nil }
                }
            }
        }
    }
}
