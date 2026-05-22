import SwiftUI

struct DevSettingsView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
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
                                        .foregroundColor(.accentColor)
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

                Section("Account Preferences") {
                    Toggle("Send read receipts", isOn: Binding(
                        get: { appState.sendReadReceipts },
                        set: { appState.setSendReadReceiptsPref($0) }
                    ))
                }
            }
            .navigationTitle("Dev Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
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
