import Foundation

/// A Project available on a homeserver.
struct ProjectInfo: Identifiable, Hashable {
    var id: String { url }
    let name: String
    let url: String
    let description: String
}
