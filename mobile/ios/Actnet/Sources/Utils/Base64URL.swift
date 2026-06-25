import Foundation

extension Data {
    /// Decode a base64url-encoded string (no padding) into Data.
    init?(base64URLEncoded string: String) {
        // Strictly reject anything outside the URL-safe alphabet (RFC 4648 §5,
        // no padding). After the substitution below, base64Encoded would
        // otherwise also accept standard '+'/'/' (and '='), so two
        // different-looking tokens could decode to the same bytes — and this is
        // an invite-token / deep-link parsing path. Must match Android
        // decodeBase64URL.
        let allowed = CharacterSet(charactersIn:
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_")
        guard string.unicodeScalars.allSatisfy({ allowed.contains($0) }) else {
            return nil
        }
        var base64 = string
            .replacingOccurrences(of: "-", with: "+")
            .replacingOccurrences(of: "_", with: "/")
        // Add padding if needed.
        let remainder = base64.count % 4
        if remainder > 0 {
            base64.append(String(repeating: "=", count: 4 - remainder))
        }
        self.init(base64Encoded: base64)
    }
}
