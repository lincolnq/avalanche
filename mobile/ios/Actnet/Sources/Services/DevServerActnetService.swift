import Foundation

/// Service that hits a real locally-running homeserver via the Rust AppCore.
struct DevServerActnetService: ActnetService {
    static let defaultServerUrl = "http://localhost:3000"

    func createAccount(serverUrl: String, dbPath: String, dbKey: String, prfOutput: Data, displayName: String, inviteToken: String?) throws -> any AppCoreProtocol {
        try AppCore.createAccount(serverUrl: serverUrl, dbPath: dbPath, dbKey: dbKey, prfOutput: prfOutput, displayName: displayName, inviteToken: inviteToken)
    }

    func login(dbPath: String, dbKey: String) throws -> any AppCoreProtocol {
        try AppCore.login(dbPath: dbPath, dbKey: dbKey)
    }

    func prepareAccount(serverUrl: String, prfOutput: Data) throws -> any PreparedAccountProtocol {
        try PreparedAccount(serverUrl: serverUrl, prfOutput: prfOutput)
    }

    func finalizeAccount(prepared: any PreparedAccountProtocol, dbPath: String, dbKey: String, displayName: String, inviteToken: String?) throws -> any AppCoreProtocol {
        guard let concrete = prepared as? PreparedAccount else {
            throw ActnetServiceError.preparedAccountTypeMismatch
        }
        return try AppCore.finalizeAccount(prepared: concrete, dbPath: dbPath, dbKey: dbKey, displayName: displayName, inviteToken: inviteToken)
    }

    func recoverFromBlob(serverUrl: String, did: String, prfOutput: Data, dbPath: String, dbKey: String, displayName: String) throws -> any AppCoreProtocol {
        try AppCore.recoverFromBlob(serverUrl: serverUrl, did: did, prfOutput: prfOutput, dbPath: dbPath, dbKey: dbKey, displayName: displayName)
    }

    func makeDeviceLink() -> any DeviceLinkProtocol {
        LiveDeviceLink()
    }
}

/// Live device-link handle wrapping the UniFFI `DeviceLinkNew` object.
final class LiveDeviceLink: DeviceLinkProtocol, @unchecked Sendable {
    private let inner = DeviceLinkNew()

    func createPairing(mailboxServer: String?) throws -> String {
        try inner.createPairing(mailboxServer: mailboxServer)
    }

    func acceptPairing(code: String) throws {
        try inner.acceptPairing(code: code)
    }

    func awaitLinkStep(dbPath: String, dbKey: String) throws -> (any AppCoreProtocol)? {
        try inner.awaitLinkStep(dbPath: dbPath, dbKey: dbKey)
    }
}

enum ActnetServiceError: LocalizedError {
    case preparedAccountTypeMismatch

    var errorDescription: String? {
        switch self {
        case .preparedAccountTypeMismatch:
            return "PreparedAccount instance does not match the active ActnetService."
        }
    }
}
