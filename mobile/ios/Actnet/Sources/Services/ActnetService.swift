import Foundation

/// Abstraction over the Rust AppCore for account creation/login.
/// The returned AppCoreProtocol instance handles all subsequent operations
/// (send, receive, etc.) matching the UniFFI-generated interface.
protocol ActnetService: Sendable {
    /// Create a new account. `recoveryKey` is a 32-byte symmetric key from
    /// passkey PRF or recovery phrase. Pass empty Data to skip recovery setup.
    /// `displayName` is the user's chosen display name; encrypted under a
    /// freshly generated profile key and uploaded alongside registration.
    func createAccount(serverUrl: String, dbPath: String, dbKey: String, recoveryKey: Data, displayName: String) throws -> any AppCoreProtocol
    func login(dbPath: String, dbKey: String) throws -> any AppCoreProtocol
}

/// Uses the real UniFFI-generated AppCore bindings.
/// Uncomment when the Rust XCFramework is linked.
// struct RealActnetService: ActnetService {
//     func createAccount(serverUrl: String, dbPath: String, dbKey: String, recoveryKey: Data) throws -> any AppCoreProtocol {
//         try AppCore.createAccount(serverUrl: serverUrl, dbPath: dbPath, dbKey: dbKey, recoveryKey: recoveryKey)
//     }
//     func login(dbPath: String, dbKey: String) throws -> any AppCoreProtocol {
//         try AppCore.login(dbPath: dbPath, dbKey: dbKey)
//     }
// }
