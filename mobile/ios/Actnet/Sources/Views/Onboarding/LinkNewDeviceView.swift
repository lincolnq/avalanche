import SwiftUI

/// New-device (joining) side of device linking (docs/04-multi-device.md §4).
/// This device has no account yet; it receives the identity bundle from an
/// already-signed-in device over an ephemeral mailbox and registers itself as
/// an additional device.
///
/// By default this device *scans* the existing device's pairing code, but the
/// user can flip to *showing* its own code instead. When showing, the mailbox
/// defaults to the built-in host, so the new device needs no server URL up
/// front. On success, `AppState` exits onboarding and the root view swaps to
/// the main app — no explicit navigation here.
struct LinkNewDeviceView: View {
    @EnvironmentObject var appState: AppState

    enum Mode { case scan, show }
    enum Phase: Equatable { case scanning, preparing, showing, linking, failed(String) }

    @State private var mode: Mode = .scan
    @State private var pairingCode: String?
    @State private var phase: Phase = .scanning
    @State private var attempt = 0

    private var taskKey: String { "\(mode == .scan ? "scan" : "show")-\(attempt)" }

    var body: some View {
        ScrollView {
            VStack(spacing: 24) {
                Text("Link this device to an account you're already signed in to on another device. Keep both devices on this screen until linking finishes.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal)
                    .padding(.top, 16)

                switch mode {
                case .scan: scanSection
                case .show: showSection
                }

                if case .failed(let message) = phase {
                    VStack(spacing: 12) {
                        Text(message)
                            .font(.footnote)
                            .foregroundStyle(.red)
                            .multilineTextAlignment(.center)
                        Button("Try Again") { attempt += 1 }
                            .buttonStyle(.bordered)
                    }
                    .padding(.horizontal)
                }

                if phase != .linking {
                    modeToggle
                }
            }
            .padding(.bottom, 32)
        }
        .background(Color.avPaper)
        .navigationTitle("Link to a Device")
        .navigationBarTitleDisplayMode(.inline)
        .task(id: taskKey) { await runFlow() }
    }

    // MARK: - Sections

    @ViewBuilder
    private var scanSection: some View {
        VStack(spacing: 16) {
            if phase == .linking {
                ProgressView("Linking…").frame(height: 260)
            } else {
                QRCodeCameraView { code in
                    guard phase == .scanning else { return }
                    phase = .linking
                    Task { await completeAfterScan(code: code) }
                }
                .frame(height: 260)
                .clipShape(RoundedRectangle(cornerRadius: 12))
                Text("Point this camera at the code shown on the other device.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal)
    }

    @ViewBuilder
    private var showSection: some View {
        VStack(spacing: 16) {
            if let code = pairingCode {
                QRCodeView(text: code)
                Text(code)
                    .font(.system(.footnote, design: .monospaced))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .padding(.horizontal)
                Button {
                    UIPasteboard.general.string = code
                } label: {
                    Label("Copy code", systemImage: "doc.on.doc")
                }
                .buttonStyle(.bordered)

                HStack(spacing: 8) {
                    ProgressView()
                    Text("Waiting for the other device…")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            } else {
                ProgressView("Preparing…").frame(height: 220)
            }
        }
        .padding(.horizontal)
    }

    @ViewBuilder
    private var modeToggle: some View {
        switch mode {
        case .scan:
            Button("Show a code instead") {
                mode = .show
                attempt += 1
            }
            .font(.subheadline)
        case .show:
            Button("Scan the other device instead") {
                mode = .scan
                attempt += 1
            }
            .font(.subheadline)
        }
    }

    // MARK: - Flow

    private func runFlow() async {
        pairingCode = nil
        guard mode == .show else {
            phase = .scanning
            return
        }
        phase = .preparing
        do {
            let link = appState.makeDeviceLink()
            let code = try await Task.detached { try link.createPairing(mailboxServer: nil) }.value
            pairingCode = code
            phase = .showing
            try await appState.completeDeviceLink(link)
            // Success flips `isOnboarding`; this view is torn down with the stack.
        } catch {
            if Task.isCancelled { return }
            phase = .failed(error.localizedDescription)
        }
    }

    private func completeAfterScan(code: String) async {
        do {
            let link = appState.makeDeviceLink()
            try await Task.detached { try link.acceptPairing(code: code) }.value
            try await appState.completeDeviceLink(link)
        } catch {
            phase = .failed(error.localizedDescription)
        }
    }
}
