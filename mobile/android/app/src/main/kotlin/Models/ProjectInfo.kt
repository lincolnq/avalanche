package net.theavalanche.app

/// A Project available on a homeserver.
data class ProjectInfo(
    val name: String,
    val url: String,
    val description: String,
) {
    // Mirrors Swift's `var id: String { url }` from Identifiable conformance.
    val id: String get() = url
}
