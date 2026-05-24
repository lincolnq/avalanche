import Foundation

struct InviteToken: Identifiable, Hashable {
    var id: String { token }
    let token: String
    let serverUrl: URL
    let serverName: String
    let inviterDid: String?
    let postOnboardingRedirect: String?

    /// Parse a go.theavalanche.net invite URL and validate the token with the server.
    static func from(url: URL) async throws -> InviteToken {
        // Extract token from path: /invite/<token>
        let pathComponents = url.pathComponents.filter { $0 != "/" }
        guard pathComponents.first == "invite", pathComponents.count >= 2 else {
            throw InviteError.invalidURL
        }
        let token = pathComponents[1]
        return try await from(token: token)
    }

    /// Validate a raw base64url token string with the server.
    static func from(token: String) async throws -> InviteToken {
        let info = try validateInvite(token: token)
        guard let serverUrl = URL(string: info.serverUrl) else {
            throw InviteError.invalidServerUrl
        }
        return InviteToken(
            token: token,
            serverUrl: serverUrl,
            serverName: info.serverName,
            inviterDid: info.inviterDid,
            postOnboardingRedirect: info.postOnboardingRedirect
        )
    }
}

enum InviteError: LocalizedError {
    case invalidURL
    case invalidServerUrl

    var errorDescription: String? {
        switch self {
        case .invalidURL: "Invalid invite link"
        case .invalidServerUrl: "Invalid server URL in invite"
        }
    }
}
