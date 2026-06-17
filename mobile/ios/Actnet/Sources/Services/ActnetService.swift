import Foundation

/// Abstraction over the Rust AppCore for account creation/login.
/// The returned AppCoreProtocol instance handles all subsequent operations
/// (send, receive, etc.) matching the UniFFI-generated interface.
protocol ActnetService: Sendable {
    /// Create a new account. `prfOutput` is the raw 32-byte WebAuthn PRF
    /// output from a passkey ceremony (or the hash of a recovery phrase).
    /// Rust derives both the DID rotation key and the recovery-blob key from
    /// it via HKDF. Pass empty Data to skip recovery setup (random rotation
    /// key, no blob — identity is unrecoverable on device loss).
    /// `displayName` is the user's chosen display name; encrypted under a
    /// freshly generated profile key and uploaded alongside registration.
    func createAccount(serverUrl: String, dbPath: String, dbKey: String, prfOutput: Data, displayName: String, inviteToken: String?) throws -> any AppCoreProtocol
    func login(dbPath: String, dbKey: String) throws -> any AppCoreProtocol

    /// Two-stage account creation for the passkey flow. Stage 1: pass the
    /// passkey PRF output (already obtained from a just-completed WebAuthn
    /// ceremony) so Rust can derive the rotation key, build the genesis +
    /// identity-update PLC ops, and produce the final DID.
    func prepareAccount(serverUrl: String, prfOutput: Data) throws -> any PreparedAccountProtocol

    /// Stage 2: consume the prepared handle, submit the PLC ops, encrypt the
    /// recovery blob with the same passkey-derived key, and register the
    /// account with the homeserver.
    func finalizeAccount(prepared: any PreparedAccountProtocol, dbPath: String, dbKey: String, displayName: String, inviteToken: String?) throws -> any AppCoreProtocol

    /// Recover an account from a passkey-protected recovery blob. Downloads
    /// the blob, decrypts with the PRF-derived key, replaces the old device
    /// on the home server, and returns an `AppCoreProtocol` bound to a fresh
    /// local store.
    func recoverFromBlob(serverUrl: String, did: String, prfOutput: Data, dbPath: String, dbKey: String, displayName: String) throws -> any AppCoreProtocol
}
