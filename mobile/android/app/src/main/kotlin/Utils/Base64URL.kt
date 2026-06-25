package net.theavalanche.app

import android.util.Base64

/**
 * Decodes a base64url-encoded string (no padding) into a ByteArray.
 * Mirrors Data.init?(base64URLEncoded:) in Utils/Base64URL.swift.
 *
 * Returns null if the string is not valid base64url.
 */
fun decodeBase64URL(string: String): ByteArray? {
    // Strictly reject anything outside the URL-safe alphabet (RFC 4648 §5, no
    // padding). After the substitution below, Base64.DEFAULT would otherwise
    // also accept standard '+'/'/' (and '='), so two different-looking tokens
    // could decode to the same bytes — and this is an invite-token / deep-link
    // parsing path. Must match iOS Data(base64URLEncoded:).
    if (string.any { it !in 'A'..'Z' && it !in 'a'..'z' && it !in '0'..'9' && it != '-' && it != '_' }) {
        return null
    }
    var base64 = string
        .replace('-', '+')
        .replace('_', '/')
    // Add padding if needed.
    val remainder = base64.length % 4
    if (remainder > 0) {
        base64 += "=".repeat(4 - remainder)
    }
    return try {
        Base64.decode(base64, Base64.DEFAULT)
    } catch (e: IllegalArgumentException) {
        null
    }
}
