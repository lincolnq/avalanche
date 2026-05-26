import SwiftUI

struct RecoveryConsoleView: View {
    @EnvironmentObject var appState: AppState

    let recoveryKey: Data
    let did: String

    @State private var logLines: [String] = []
    @State private var serverUrlInput: String = {
        #if DEBUG
        return "http://localhost:3000"
        #else
        return ""
        #endif
    }()
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
        if line.hasPrefix("[!]") {
            AppLog.error("recovery", String(line.dropFirst(3).trimmingCharacters(in: .whitespaces)))
        } else if line.hasPrefix("[ok]") {
            AppLog.ok("recovery", String(line.dropFirst(4).trimmingCharacters(in: .whitespaces)))
        } else {
            AppLog.info("recovery", line)
        }
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

        // did:local:* (bot/test accounts) have no PLC entry — fall straight
        // through to the manual URL prompt.
        guard did.hasPrefix("did:plc:") else {
            log("DID is not a did:plc:* — manual home server URL required.")
            needsServerUrl = true
            return
        }

        log("Resolving DID from PLC directory...")
        let resolved: String
        do {
            resolved = try await Task.detached { try resolveHomeserverFromPlc(did: did) }.value
        } catch {
            log("[!] PLC lookup failed: \(error.localizedDescription)")
            log("Please enter your home server URL to continue.")
            needsServerUrl = true
            return
        }
        log("[ok] Home server: \(resolved)")
        await performRecoveryWithServer(serverUrl: resolved)
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
            try await appState.recoverAccount(
                serverUrl: serverUrl,
                serverName: serverUrl,
                did: targetDid,
                recoveryKey: recoveryKey,
                displayName: ""
            )
            log("[ok] Identity restored. Replacing device on home server...")
            log("[ok] Signed in!")
            // `recoverAccount` flips `appState.isOnboarding = false`, which
            // swaps the root view to MainTabView. No explicit navigation needed.
        } catch {
            log("[!] Recovery failed: \(error.localizedDescription)")
            log("Check that the server URL and recovery key are correct.")
        }
    }
}
