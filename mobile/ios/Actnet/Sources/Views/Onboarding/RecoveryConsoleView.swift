import SwiftUI

struct RecoveryConsoleView: View {
    @EnvironmentObject var appState: AppState

    let recoveryKey: Data
    let did: String

    @State private var logLines: [String] = []
    @State private var serverUrlInput = ""
    @State private var needsServerUrl = false

    var body: some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 2) {
                        ForEach(Array(logLines.enumerated()), id: \.offset) { index, line in
                            Text(line)
                                .font(.system(.caption, design: .monospaced))
                                .foregroundStyle(line.hasPrefix("[!]") ? Color.avError : line.hasPrefix("[ok]") ? Color.avBrand : .primary)
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

            if needsServerUrl {
                VStack(spacing: 12) {
                    Text("Enter your home server URL:")
                        .font(.subheadline)
                    TextField("https://server.example", text: $serverUrlInput)
                        .textFieldStyle(.roundedBorder)
                        .autocapitalization(.none)
                        .keyboardType(.URL)
                    Button("Continue") {
                        needsServerUrl = false
                        Task {
                            await performRecoveryWithServer(serverUrl: serverUrlInput)
                        }
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(serverUrlInput.isEmpty)
                }
                .padding()
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

    private func log(_ line: String) {
        logLines.append(line)
    }

    private func performRecovery() async {
        if did.isEmpty {
            // Phrase-based recovery — we don't have a DID from a passkey.
            // Need the user to provide a server URL.
            log("Recovery phrase entered.")
            log("We need your home server URL to find your recovery blob.")
            needsServerUrl = true
            return
        }

        log("DID: \(did)")
        log("Resolving DID from PLC directory...")
        try? await Task.sleep(nanoseconds: 300_000_000)

        // TODO: Resolve DID via PLC directory to find home server.
        // For now, we don't have PLC directory integration.
        // The recovery blob should be on the home server listed in the DID doc.
        // Until PLC is implemented, we need the user to tell us the server.
        log("[!] PLC directory lookup not yet implemented.")
        log("Please enter your home server URL to continue.")
        needsServerUrl = true
    }

    private func performRecoveryWithServer(serverUrl: String) async {
        log("Connecting to \(serverUrl)...")
        try? await Task.sleep(nanoseconds: 300_000_000)

        let targetDid: String
        if did.isEmpty {
            // Phrase-based: we don't know the DID. We'd need to try to download
            // blobs, but we can't without a DID. For now, show an error.
            log("[!] Recovery phrase flow requires knowing your DID.")
            log("[!] This flow is not yet fully implemented.")
            log("Please use passkey recovery instead, which embeds your DID.")
            return
        } else {
            targetDid = did
        }

        log("Downloading recovery blob for \(targetDid)...")
        do {
            let servers = try await Task.detached {
                try downloadRecoveryBlob(
                    serverUrl: serverUrl,
                    did: targetDid,
                    recoveryKey: self.recoveryKey
                )
            }.value
            log("[ok] Recovery blob decrypted successfully!")
            log("Found \(servers.count) server(s): \(servers.joined(separator: ", "))")

            // TODO: Full recovery flow:
            // 1. Restore identity keypair from blob
            // 2. Generate new device_id
            // 3. Call POST /v1/devices/replace on each server (signed by rotation key)
            // 4. Generate fresh prekeys
            // 5. Create local store with restored identity
            // 6. Navigate to signed-in state
            log("")
            log("[!] Device replacement not yet implemented in client.")
            log("The recovery blob was decrypted — your identity can be restored.")
            log("Full recovery (device replace + re-auth) coming soon.")
        } catch {
            log("[!] Recovery failed: \(error.localizedDescription)")
            log("Check that the server URL and recovery key are correct.")
        }
    }
}
