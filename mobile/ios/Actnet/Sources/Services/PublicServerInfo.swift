import Foundation

/// Client-side helper for a homeserver's public, unauthenticated `GET /v1/info`
/// endpoint (server name + optional privacy policy URL). Used by the onboarding
/// screens to surface the operator's privacy policy before a user joins or
/// creates an account on that server.
enum PublicServerInfo {
    private struct Response: Decodable {
        let server_name: String
        let privacy_policy_url: String?
    }

    /// Best-effort fetch of the operator's privacy policy URL for `serverUrl`.
    /// Returns nil if the server is unreachable, the endpoint errors, or no
    /// policy is configured — callers just hide the link in that case.
    static func privacyPolicyURL(forServer serverUrl: URL) async -> URL? {
        let infoURL = serverUrl.appendingPathComponent("v1/info")
        guard let (data, response) = try? await URLSession.shared.data(from: infoURL),
              (response as? HTTPURLResponse)?.statusCode == 200,
              let info = try? JSONDecoder().decode(Response.self, from: data),
              let urlString = info.privacy_policy_url,
              let url = URL(string: urlString)
        else { return nil }
        return url
    }
}
