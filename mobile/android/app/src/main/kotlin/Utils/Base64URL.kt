package net.theavalanche.app

import android.util.Base64

/**
 * Decodes a base64url-encoded string (no padding) into a ByteArray.
 * Mirrors Data.init?(base64URLEncoded:) in Utils/Base64URL.swift.
 *
 * Returns null if the string is not valid base64url.
 */
fun decodeBase64URL(string: String): ByteArray? {
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
