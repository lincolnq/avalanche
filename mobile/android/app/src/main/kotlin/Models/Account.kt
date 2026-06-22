package net.theavalanche.app

import android.net.Uri

/**
 * Represents a local account (identity). The [id] field is the account's DID.
 * Mirrors iOS Sources/Models/Account.swift — keep in sync.
 */
data class Account(
    val id: String,              // DID
    var displayName: String,
    var avatarData: ByteArray? = null,
    var servers: List<ServerInfo> = emptyList()
) {
    // ByteArray equality by content so data class semantics are correct.
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is Account) return false
        return id == other.id &&
            displayName == other.displayName &&
            (avatarData === other.avatarData ||
                (avatarData != null && other.avatarData != null &&
                    avatarData.contentEquals(other.avatarData))) &&
            servers == other.servers
    }

    override fun hashCode(): Int {
        var result = id.hashCode()
        result = 31 * result + displayName.hashCode()
        result = 31 * result + (avatarData?.contentHashCode() ?: 0)
        result = 31 * result + servers.hashCode()
        return result
    }
}

/**
 * A homeserver the account is registered on.
 *
 * [id] is the server URL string (matches iOS `id: String // server URL`).
 *
 * [displayHost] prefers the URI host component over [name] because on some
 * onboarding paths [name] carries a raw URL string (the recovery console
 * passes `serverName: serverUrl`). Mirrors the Swift computed var of the
 * same name (docs/30 §Compose).
 */
data class ServerInfo(
    val id: String,   // server URL string, used as stable identity key
    val name: String,
    val url: Uri
) {
    val displayHost: String
        get() = url.host?.takeIf { it.isNotEmpty() } ?: name
}
