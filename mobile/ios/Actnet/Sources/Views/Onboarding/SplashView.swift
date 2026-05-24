import SwiftUI

struct SplashView: View {
    @EnvironmentObject var appState: AppState
    @State private var showScanner = false
    @State private var showLinkEntry = false
    @State private var showRecovery = false
    @State private var showDevSettings = false
    @State private var pendingInvite: InviteToken?
    @State private var isValidatingDeepLink = false

    var body: some View {
        NavigationStack {
            ZStack(alignment: .top) {
                VStack(spacing: 12) {
                    Image("Wordmark")
                        .resizable()
                        .scaledToFit()
                        .frame(maxWidth: 280)

                    Text("Encrypted organizing")
                        .font(.title3)
                        .foregroundStyle(.secondary)
                }
                .padding(.top, 200)

                VStack {
                    Spacer()

                    VStack(spacing: 16) {
                        Button {
                            showScanner = true
                        } label: {
                            Label("Scan Invite QR Code", systemImage: "qrcode.viewfinder")
                                .frame(maxWidth: .infinity)
                        }
                        .buttonStyle(.borderedProminent)
                        .controlSize(.large)

                        Button {
                            showLinkEntry = true
                        } label: {
                            Label("Enter Invite Link", systemImage: "link")
                                .frame(maxWidth: .infinity)
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.large)

                        Button {
                            showRecovery = true
                        } label: {
                            Text("Recover account")
                                .font(.subheadline)
                        }
                    }
                    .padding(.horizontal, 32)
                    .padding(.bottom, 48)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .ignoresSafeArea()
            .background(Color.avPaper)
            .overlay(alignment: .topTrailing) {
                Button {
                    showDevSettings = true
                } label: {
                    Image(systemName: "gearshape")
                        .font(.subheadline)
                        .padding()
                }
            }
            .navigationDestination(isPresented: $showScanner) {
                QRScannerView()
            }
            .navigationDestination(isPresented: $showLinkEntry) {
                InviteLinkEntryView()
            }
            .navigationDestination(isPresented: $showRecovery) {
                RecoveryExplainerView()
            }
            .sheet(isPresented: $showDevSettings) {
                DevSettingsView()
            }
            .toolbar(.hidden, for: .navigationBar)
            .navigationDestination(item: $pendingInvite) { token in
                IdentityPickerView(inviteToken: token)
            }
            .onChange(of: appState.pendingInviteToken) {
                if let token = appState.pendingInviteToken {
                    appState.pendingInviteToken = nil
                    isValidatingDeepLink = true
                    Task {
                        do {
                            pendingInvite = try await InviteToken.from(token: token)
                        } catch {
                            print("[SplashView] invite validation failed: \(error)")
                        }
                        isValidatingDeepLink = false
                    }
                }
            }
        }
        .background(Color.avPaper)
    }
}
