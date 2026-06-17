import SwiftUI

struct AccountsView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.dismiss) private var dismiss
    @State private var showScanner = false
    @State private var showAddAccount = false

    private var sortedAccounts: [Account] {
        // Doc: identity groups sorted by creation order (oldest first).
        // No created-at stored yet; preserve current order as a stand-in.
        appState.accounts
    }

    /// Marketing version + build number from the app bundle, e.g.
    /// "Avalanche 1.0 (42)". Shown in the About footer.
    private var appVersion: String {
        let info = Bundle.main.infoDictionary
        let version = info?["CFBundleShortVersionString"] as? String ?? "—"
        let build = info?["CFBundleVersion"] as? String ?? "—"
        return "Avalanche \(version) (\(build))"
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Button {
                        showScanner = true
                    } label: {
                        Label("Scan Invite", systemImage: "qrcode.viewfinder")
                    }
                }

                ForEach(sortedAccounts) { account in
                    Section {
                        NavigationLink {
                            IdentityDetailView(account: account)
                        } label: {
                            HStack(spacing: 12) {
                                AccountAvatar(account: account, size: 32)
                                Text(account.displayName)
                                    .font(.headline)
                            }
                        }

                        ForEach(sortedServers(for: account)) { server in
                            NavigationLink {
                                ServerDetailView(account: account, server: server)
                            } label: {
                                ServerRow(server: server, isHome: isHome(server, of: account))
                            }
                        }
                    }
                }

                Section {
                    Button {
                        showAddAccount = true
                    } label: {
                        Label("Add an account", systemImage: "plus")
                    }
                }

                Section {
                    Link(destination: URL(string: "https://github.com/lincolnq/avalanche/issues")!) {
                        Label("Get Help", systemImage: "questionmark.circle")
                    }
                } header: {
                    Text("About")
                } footer: {
                    VStack(spacing: 4) {
                        Text(appVersion)
                        Link("Open Source License", destination: URL(string: "https://github.com/lincolnq/avalanche/blob/main/LICENSE")!)
                            .font(.caption)
                    }
                    .frame(maxWidth: .infinity, alignment: .center)
                }
            }
            
            .scrollContentBackground(.hidden)
            .background(Color.avPaper)
            .navigationTitle("Accounts")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .navigationDestination(isPresented: $showScanner) {
                QRScannerView { value in
                    guard let url = URL(string: value), AppState.isDeepLink(url) else { return }
                    appState.handleDeepLink(url)
                    dismiss()
                }
            }
            .navigationDestination(isPresented: $showAddAccount) {
                AddAccountView()
            }
        }
    }

    private func sortedServers(for account: Account) -> [ServerInfo] {
        // Doc: servers sorted by activity count (highest first). No activity
        // data yet; stable order by name as a placeholder.
        account.servers.sorted { $0.name < $1.name }
    }

    private func isHome(_ server: ServerInfo, of account: Account) -> Bool {
        // Doc: each identity has exactly one discovery (home) server. We don't
        // model this distinction on Account yet; treat the first server in the
        // identity's list as home.
        account.servers.first?.id == server.id
    }
}

private struct ServerRow: View {
    let server: ServerInfo
    let isHome: Bool

    var body: some View {
        HStack(spacing: 8) {
            Text(server.name)
                .font(.body)
            if isHome {
                Text("home")
                    .font(.caption2)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.avBrand.opacity(0.15), in: Capsule())
                    .foregroundStyle(Color.avBrand)
            }
            Spacer()
        }
    }
}
