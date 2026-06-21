import SwiftUI

struct ServerDetailView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.dismiss) private var dismiss
    let account: Account
    let server: ServerInfo

    @State private var showLeaveConfirmation = false
    @State private var isLeaving = false
    @State private var leaveError: String?

    private var isHome: Bool {
        account.servers.first?.id == server.id
    }

    private var homeServerName: String {
        account.servers.first?.name ?? "your home server"
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(server.name)
                        .font(.title2)
                        .fontWeight(.semibold)
                    Text(server.url.absoluteString)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
                .padding(.horizontal)
                .padding(.top, 16)

                if isHome {
                    VStack(alignment: .leading, spacing: 8) {
                        Label("Home server for \(account.displayName)", systemImage: "house")
                            .font(.subheadline)
                            .foregroundStyle(Color.avBrand)
                        Text("To change your home server or delete this identity, open the identity detail screen.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.sand50, in: RoundedRectangle(cornerRadius: 8))
                    .padding(.horizontal)
                }

                Spacer(minLength: 16)

                if !isHome {
                    Button(role: .destructive) {
                        showLeaveConfirmation = true
                    } label: {
                        if isLeaving {
                            ProgressView()
                                .frame(maxWidth: .infinity)
                        } else {
                            Text("Leave this server")
                                .frame(maxWidth: .infinity)
                        }
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.large)
                    .disabled(isLeaving)
                    .padding(.horizontal)
                }
            }
            .padding(.bottom, 32)
        }
        .background(Color.avPaper)
        .navigationTitle("Server")
        .navigationBarTitleDisplayMode(.inline)
        .confirmationDialog(
            "Leave \(server.name)?",
            isPresented: $showLeaveConfirmation,
            titleVisibility: .visible
        ) {
            Button("Leave", role: .destructive) {
                leaveServer()
            }
            Button("Cancel", role: .cancel) { }
        } message: {
            Text("You'll be removed from any groups and Projects on \(server.name). People you share other servers with will still be able to reach you there. New contacts will reach you at \(homeServerName).")
        }
        .alert("Couldn't leave server", isPresented: .constant(leaveError != nil)) {
            Button("OK", role: .cancel) { leaveError = nil }
        } message: {
            Text(leaveError ?? "")
        }
    }

    private func leaveServer() {
        isLeaving = true
        Task {
            do {
                try await appState.leaveServer(account: account, server: server)
                dismiss()
            } catch {
                leaveError = error.localizedDescription
            }
            isLeaving = false
        }
    }
}
