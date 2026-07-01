package net.theavalanche.app

/// A Project available on a homeserver.
data class ProjectInfo(
    val name: String,
    val url: String,
    val description: String,
    /// OAuth login client id (docs/25), if this Project supports "Sign in with
    /// Avalanche".
    val clientId: String? = null,
    /// Server-vouched official flag (docs/54), shown as the verified badge.
    val official: Boolean = false,
) {
    // Mirrors Swift's `var id: String { url }` from Identifiable conformance.
    val id: String get() = url
}
