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

    func prepareAccount(serverUrl: String) throws -> any PreparedAccountProtocol {
        try PreparedAccount(serverUrl: serverUrl)
    }

    func finalizeAccount(prepared: any PreparedAccountProtocol, dbPath: String, dbKey: String, recoveryKey: Data, displayName: String) throws -> any AppCoreProtocol {
        guard let concrete = prepared as? PreparedAccount else {
            throw ActnetServiceError.preparedAccountTypeMismatch
        }
        return try AppCore.finalizeAccount(prepared: concrete, dbPath: dbPath, dbKey: dbKey, recoveryKey: recoveryKey, displayName: displayName)
    }

    func recoverFromBlob(serverUrl: String, did: String, recoveryKey: Data, dbPath: String, dbKey: String, displayName: String) throws -> any AppCoreProtocol {
        try AppCore.recoverFromBlob(serverUrl: serverUrl, did: did, recoveryKey: recoveryKey, dbPath: dbPath, dbKey: dbKey, displayName: displayName)
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
