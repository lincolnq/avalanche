import Foundation

/// Minimal account info persisted so the app can restore on launch and the
/// Notification Service Extension (docs/16) can enumerate accounts to fetch.
///
/// Stored as JSON under `SharedAccountStore.accountsKey` in the shared App Group
/// `UserDefaults` suite (docs/16 dep 3). `dbFilename` is just the filename (e.g.
/// "account-34B35698.db"), resolved against `AppGroup.dbDir` at runtime — this
/// avoids breakage when the container UUID changes between launches.
///
/// This type is the single source of truth for the persisted shape: `AppState`
/// (app target) reads/writes it, and the NSE reads it via `SharedAccountStore`.
struct PersistedAccount: Codable {
    let did: String
    let displayName: String
    let dbFilename: String
    let servers: [PersistedServer]
}

struct PersistedServer: Codable {
    let id: String
    let name: String
    let url: String
}

/// Read-only accessor for the persisted account list, used by the Notification
/// Service Extension (which has no `AppState`). The app writes this list via
/// `AppState`'s persistence helpers; both sides key on the same suite + key +
/// JSON shape.
enum SharedAccountStore {
    /// Must match `AppState.accountsKey`.
    static let accountsKey = "persistedAccounts"

    static func load() -> [PersistedAccount] {
        guard let data = AppGroup.sharedDefaults?.data(forKey: accountsKey),
              let list = try? JSONDecoder().decode([PersistedAccount].self, from: data)
        else { return [] }
        return list
    }
}
