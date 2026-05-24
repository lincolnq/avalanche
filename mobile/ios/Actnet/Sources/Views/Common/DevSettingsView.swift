import SwiftUI

struct DevSettingsView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.dismiss) private var dismiss
    @State private var showMyQRCode = false
    @State private var showLogoutConfirmation = false

    private var isLoggedIn: Bool { !appState.accounts.isEmpty }

    var body: some View {
        NavigationStack {
            List {
                if isLoggedIn {
                    Section {
                        Button {
                            showMyQRCode = true
                        } label: {
                            Label("My QR Code", systemImage: "qrcode")
                        }
                    }

                    Section {
                        Button(role: .destructive) {
                            showLogoutConfirmation = true
                        } label: {
                            Label("Log Out", systemImage: "rectangle.portrait.and.arrow.right")
                        }
                    }
                }

                if !isLoggedIn {
                    Section("Service Mode") {
                        ForEach(ServiceMode.allCases, id: \.rawValue) { mode in
                            Button {
                                appState.switchMode(mode)
                                dismiss()
                            } label: {
                                HStack {
                                    VStack(alignment: .leading) {
                                        Text(mode.rawValue)
                                        Text(subtitle(for: mode))
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                    }
                                    Spacer()
                                    if appState.serviceMode == mode {
                                        Image(systemName: "checkmark")
                                            .foregroundColor(.avBrand)
                                    }
                                }
                            }
                            .foregroundStyle(.primary)
                        }
                    }

                    if appState.serviceMode == .devServer {
                        Section("Dev Server") {
                            HStack {
                                Text("URL")
                                Spacer()
                                Text(DevServerActnetService.defaultServerUrl)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                }

                Section("State") {
                    HStack {
                        Text("Accounts")
                        Spacer()
                        Text("\(appState.accounts.count)")
                            .foregroundStyle(.secondary)
                    }
                    HStack {
                        Text("Conversations")
                        Spacer()
                        Text("\(appState.conversations.count)")
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .background(Color.avPaper)
            .navigationTitle("Dev Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .navigationDestination(isPresented: $showMyQRCode) {
                MyQRCodeView()
            }
            .confirmationDialog("Log out?", isPresented: $showLogoutConfirmation, titleVisibility: .visible) {
                Button("Log Out", role: .destructive) {
                    appState.logout()
                    dismiss()
                }
            } message: {
                Text("This will remove all accounts and messages from this device.")
            }
        }
    }

    private func subtitle(for mode: ServiceMode) -> String {
        switch mode {
        case .mock: return "In-memory, no server needed"
        case .devServer: return "Connects to \(DevServerActnetService.defaultServerUrl)"
        }
    }
}
