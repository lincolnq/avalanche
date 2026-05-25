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

    /// Two-stage account creation for the passkey flow. Stage 1: generate
    /// identity + rotation keys and derive the DID locally so the passkey
    /// ceremony can write the real DID into the credential's user handle.
    func prepareAccount(serverUrl: String) throws -> any PreparedAccountProtocol

    /// Stage 2: consume the prepared handle, submit the PLC genesis op, and
    /// register the account with the homeserver.
    func finalizeAccount(prepared: any PreparedAccountProtocol, dbPath: String, dbKey: String, recoveryKey: Data, displayName: String) throws -> any AppCoreProtocol

    /// Recover an account from a passkey-protected recovery blob. Downloads
    /// the blob, decrypts with `recoveryKey`, replaces the old device on the
    /// home server, and returns an `AppCoreProtocol` bound to a fresh local store.
    func recoverFromBlob(serverUrl: String, did: String, recoveryKey: Data, dbPath: String, dbKey: String, displayName: String) throws -> any AppCoreProtocol
}
