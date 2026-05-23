import SwiftUI

struct NetworkView: View {
    @EnvironmentObject var appState: AppState

    @State private var projectsByServer: [String: [ProjectInfo]] = [:]
    @State private var selectedProject: (project: ProjectInfo, accountId: String)?
    @State private var projectToken: String?
    @State private var showWebView = false
    @State private var isLoading = false

    var body: some View {
        NavigationStack {
            Group {
                if allServers.isEmpty {
                    ContentUnavailableView(
                        "No servers",
                        systemImage: "server.rack",
                        description: Text("Servers and their Projects will appear here.")
                    )
                } else {
                    serverList
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color.avPaper)
            .navigationTitle("Network")
            .task { await loadProjects() }
            .sheet(isPresented: $showWebView) {
                if let sel = selectedProject, let token = projectToken {
                    let urlString = "\(sel.project.url)?token=\(token)"
                    if let url = URL(string: urlString) {
                        ProjectWebView(projectName: sel.project.name, url: url)
                    }
                }
            }
        }
    }

    private var serverList: some View {
        List {

            ForEach(allServers) { server in
                Section(server.name) {
                    let projects = projectsByServer[server.id] ?? []
                    if projects.isEmpty {
                        Text("No Projects")
                            .foregroundStyle(.secondary)
                    } else {
                        ForEach(projects) { project in
                            Button {
                                openProject(project, serverUrl: server.id)
                            } label: {
                                HStack {
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(project.name)
                                            .font(.body)
                                            .foregroundStyle(.primary)
                                        Text(project.description)
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                    }
                                    Spacer()
                                    if isLoading && selectedProject?.project.id == project.id {
                                        ProgressView()
                                    } else {
                                        Image(systemName: "chevron.right")
                                            .foregroundStyle(.secondary)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        .scrollContentBackground(.hidden)
        .background(Color.avPaper)
        .listRowBackground(Color.sand50)
    }

    private var allServers: [ServerInfo] {
        appState.accounts.flatMap(\.servers)
            .reduce(into: [String: ServerInfo]()) { dict, server in
                dict[server.id] = server
            }
            .values
            .sorted { $0.name < $1.name }
    }

    private func loadProjects() async {
        for server in allServers {
            let projects = await appState.fetchProjects(serverUrl: server.id)
            projectsByServer[server.id] = projects
        }
    }

    private func openProject(_ project: ProjectInfo, serverUrl: String) {
        // Find an account on this server to get a token.
        guard let account = appState.accounts.first(where: {
            $0.servers.contains(where: { $0.id == serverUrl })
        }) else { return }

        isLoading = true
        selectedProject = (project: project, accountId: account.id)

        Task {
            do {
                let token = try await appState.requestProjectToken(
                    accountId: account.id,
                    projectUrl: project.url
                )
                projectToken = token
                showWebView = true
            } catch {
                print("Failed to get project token: \(error)")
            }
            isLoading = false
        }
    }
}
