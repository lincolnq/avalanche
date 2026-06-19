import Foundation

struct Account: Identifiable, Hashable {
    let id: String  // DID
    var displayName: String
    var avatarData: Data?
    var servers: [ServerInfo]
}

struct ServerInfo: Identifiable, Hashable {
    let id: String  // server URL
    let name: String
    let url: URL

    /// Compact label for the server, e.g. "safe-haven.org" — the form the
    /// compose header and Name Group screen use (docs/30 §Compose). Prefers the
    /// URL host because `name` carries a raw URL on some onboarding paths (the
    /// recovery console passes `serverName: serverUrl`).
    var displayHost: String { url.host ?? name }
}
