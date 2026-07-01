import SwiftUI

/// Consent screen for "Sign in with Avalanche" (docs/25-project-login.md).
///
/// This is the user's act of *choosing to sign in* to a Project as this
/// identity — not a per-scope approval (scopes are admin-granted). The granted
/// capabilities are shown for legibility. For the cross-device (device-grant)
/// front-end it adds the "signing in on another device" phishing warning.
struct ProjectLoginConsentView: View {
    @EnvironmentObject var appState: AppState
    let request: ProjectLoginRequest

    private var serverHost: String {
        URL(string: request.serverUrl)?.host ?? request.serverUrl
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 24) {
                Spacer(minLength: 8)

                Image(systemName: "person.badge.key.fill")
                    .font(.system(size: 44))
                    .foregroundStyle(.tint)

                VStack(spacing: 6) {
                    Text("Sign in with Avalanche")
                        .font(.title2.bold())

                    HStack(spacing: 6) {
                        Text(request.displayLabel)
                            .font(.headline)
                        if request.official {
                            Image(systemName: "checkmark.seal.fill")
                                .foregroundStyle(.tint)
                                .accessibilityLabel("Verified")
                        }
                    }
                    if let projectUrl = request.projectUrl, let host = URL(string: projectUrl)?.host {
                        Text(host)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                }

                Text("You'll sign in with your account on **\(serverHost)**.")
                    .font(.callout)
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)

                // Cross-device phishing warning (docs/25).
                if request.isCrossDevice {
                    Label {
                        Text("You're signing in on another device. Only continue if you started this on your other device.")
                    } icon: {
                        Image(systemName: "exclamationmark.triangle.fill")
                    }
                    .font(.footnote)
                    .foregroundStyle(.orange)
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.orange.opacity(0.12), in: RoundedRectangle(cornerRadius: 10))
                }

                // Capability legibility (not a per-scope prompt).
                Label("This Project can message you and add you to groups.",
                      systemImage: "bubble.left.and.bubble.right")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)

                Spacer()

                VStack(spacing: 12) {
                    Button {
                        appState.approveLogin(request)
                    } label: {
                        Text("Sign In")
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)

                    Button(role: .cancel) {
                        appState.cancelLogin()
                    } label: {
                        Text("Cancel")
                            .frame(maxWidth: .infinity)
                    }
                    .controlSize(.large)
                }
            }
            .padding(24)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { appState.cancelLogin() }
                }
            }
        }
    }
}
