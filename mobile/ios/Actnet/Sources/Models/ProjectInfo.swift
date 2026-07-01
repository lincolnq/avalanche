import Foundation

/// A Project available on a homeserver.
struct ProjectInfo: Identifiable, Hashable {
    var id: String { url }
    let name: String
    let url: String
    let description: String
    /// OAuth login client id (docs/25), if this Project supports "Sign in with
    /// Avalanche".
    var clientId: String? = nil
    /// Server-vouched official flag (docs/54), shown as the verified badge.
    var official: Bool = false
}
