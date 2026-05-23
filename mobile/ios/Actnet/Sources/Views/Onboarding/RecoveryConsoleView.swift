import SwiftUI

struct RecoveryConsoleView: View {
    @EnvironmentObject var appState: AppState

    @State private var logLines: [String] = []

    var body: some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 2) {
                        ForEach(Array(logLines.enumerated()), id: \.offset) { index, line in
                            Text(line)
                                .font(.system(.caption, design: .monospaced))
                                .foregroundStyle(line.hasPrefix("[!]") ? Color.avError : .primary)
                                .id(index)
                        }
                    }
                    .padding()
                }
                .onChange(of: logLines.count) { _, newCount in
                    if newCount > 0 {
                        withAnimation {
                            proxy.scrollTo(newCount - 1, anchor: .bottom)
                        }
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.avPaper)
        .navigationTitle("Recovering...")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            await performRecovery()
        }
    }

    private func performRecovery() async {
        logLines.append("Resolving DID from PLC directory...")
        try? await Task.sleep(nanoseconds: 500_000_000)

        logLines.append("[!] Recovery not yet implemented.")
        logLines.append("Passkey + recovery blob infrastructure needed.")
        logLines.append("See docs/33-identity-auth-recovery.md for details.")
    }
}
