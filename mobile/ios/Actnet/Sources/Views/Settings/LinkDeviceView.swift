import SwiftUI

/// Existing-device side of device linking (docs/04-multi-device.md §4). This
/// device already has the account and authorizes a new one by sealing the
/// identity bundle to it over an ephemeral mailbox.
///
/// Role is independent of gesture: by default this device *shows* a pairing
/// code for the new device to scan, but the user can flip to *scanning* the new
/// device's code instead (useful when the new device can only display one).
struct LinkDeviceView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.dismiss) private var dismiss
    let accountId: String

    enum Mode { case show, scan }
    enum Phase: Equatable { case preparing, waiting, scanning, linking, done, failed(String) }

    @State private var mode: Mode = .show
    @State private var pairingCode: String?
    @State private var phase: Phase = .preparing
    @State private var attempt = 0

    private var taskKey: String { "\(mode == .show ? "show" : "scan")-\(attempt)" }

    var body: some View {
        ScrollView {
            VStack(spacing: 24) {
                Text("Link the other device to this account. Both devices must stay on this screen until linking finishes.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal)
                    .padding(.top, 16)

                switch mode {
                case .show: showSection
                case .scan: scanSection
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

                if phase == .done {
                    Button("Done") { dismiss() }
                        .buttonStyle(.borderedProminent)
                        .controlSize(.large)
                } else if phase != .linking {
                    modeToggle
                }
            }
            .padding(.bottom, 32)
        }
        .background(Color.avPaper)
        .navigationTitle("Link a Device")
        .navigationBarTitleDisplayMode(.inline)
        .task(id: taskKey) { await runFlow() }
    }

    // MARK: - Sections

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

                statusFooter
            } else {
                ProgressView("Preparing…")
                    .frame(height: 220)
            }
        }
        .padding(.horizontal)
    }

    @ViewBuilder
    private var scanSection: some View {
        VStack(spacing: 16) {
            switch phase {
            case .linking:
                ProgressView("Linking…").frame(height: 260)
            case .done:
                statusFooter
            default:
                QRCodeCameraView { code in
                    guard phase == .scanning else { return }
                    phase = .linking
                    Task { await sendAfterScan(code: code) }
                }
                .frame(height: 260)
                .clipShape(RoundedRectangle(cornerRadius: 12))
                Text("Point this camera at the code on the other device.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal)
    }

    @ViewBuilder
    private var statusFooter: some View {
        switch phase {
        case .waiting:
            HStack(spacing: 8) {
                ProgressView()
                Text("Waiting for the other device…")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        case .done:
            Label("Device linked", systemImage: "checkmark.circle.fill")
                .foregroundStyle(.green)
                .font(.headline)
        default:
            EmptyView()
        }
    }

    @ViewBuilder
    private var modeToggle: some View {
        switch mode {
        case .show:
            Button("Scan the other device instead") {
                mode = .scan
                attempt += 1
            }
            .font(.subheadline)
        case .scan:
            Button("Show a code instead") {
                mode = .show
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
            let code = try await appState.linkCreatePairing(accountId: accountId)
            pairingCode = code
            phase = .waiting
            try await appState.linkSendBundle(accountId: accountId)
            phase = .done
        } catch {
            if Task.isCancelled { return }
            phase = .failed(error.localizedDescription)
        }
    }

    private func sendAfterScan(code: String) async {
        do {
            try await appState.linkAcceptPairing(accountId: accountId, code: code)
            try await appState.linkSendBundle(accountId: accountId)
            phase = .done
        } catch {
            phase = .failed(error.localizedDescription)
        }
    }
}
