import Foundation

/// A pending "Sign in with Avalanche" request parsed from an `authorize` deep
/// link (docs/25-project-login.md). Drives the consent sheet.
struct ProjectLoginRequest: Identifiable {
    /// Which OAuth front-end initiated this login.
    enum Flow {
        /// Same-device authorization-code + PKCE: on approval we mint a code and
        /// redirect the browser back to `redirectUri?code=...&state=...`.
        case authorizationCode(redirectUri: String, codeChallenge: String, codeChallengeMethod: String, state: String?)
        /// Cross-device device grant: on approval we approve the `userCode` a
        /// Project started on another device (desktop browser).
        case device(userCode: String)
    }

    let id = UUID()
    let clientId: String
    let serverUrl: String
    /// The local account (on `serverUrl`) that will authorize this login.
    let accountId: String
    let scope: String?
    let flow: Flow

    /// Resolved from the homeserver's Projects list (trustworthy); `nil` until
    /// resolved or if the client isn't in the registry.
    var projectName: String?
    var projectUrl: String?
    var official: Bool = false

    /// True for the cross-device (QR/desktop) front-end — drives the extra
    /// "signing in on another device" consent warning.
    var isCrossDevice: Bool {
        if case .device = flow { return true }
        return false
    }

    /// A user-facing label: the verified Project name if resolved, else the
    /// client id.
    var displayLabel: String { projectName ?? clientId }
}

/// A structured login failure the app surfaces so it (or a Project) can hook an
/// invitation/onboarding flow. `onboarding` itself is out of scope (docs/25).
enum ProjectLoginError: Identifiable {
    /// The user has no account on the homeserver the login targets.
    case noAccountOnServer(serverUrl: String)
    /// The approval/mint call failed.
    case failed(String)

    var id: String {
        switch self {
        case .noAccountOnServer(let s): return "noaccount:\(s)"
        case .failed(let m): return "failed:\(m)"
        }
    }
}
