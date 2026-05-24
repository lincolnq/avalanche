import Foundation

/// Service that hits a real locally-running homeserver via the Rust AppCore.
struct DevServerActnetService: ActnetService {
    static let defaultServerUrl = "http://localhost:3000"

    func createAccount(serverUrl: String, dbPath: String, dbKey: String, recoveryKey: Data, displayName: String) throws -> any AppCoreProtocol {
        try AppCore.createAccount(serverUrl: serverUrl, dbPath: dbPath, dbKey: dbKey, recoveryKey: recoveryKey, displayName: displayName)
    }

    func login(dbPath: String, dbKey: String) throws -> any AppCoreProtocol {
        try AppCore.login(dbPath: dbPath, dbKey: dbKey)
    }
}
