import Foundation
import Security

enum SecureEnclaveKeyManager {
    private static let service = "actnet"
    private static let account = "db.encryption.key"

    static func dbPassphrase() throws -> String {
        if let existing = try loadFromKeychain() { return existing }
        let newKey = try generatePassphrase()
        try saveToKeychain(newKey)
        return newKey
    }

    private static func generatePassphrase() throws -> String {
        var bytes = [UInt8](repeating: 0, count: 32)
        let status = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        guard status == errSecSuccess else {
            throw KeyManagerError.randomGenerationFailed(status)
        }
        return bytes.map { String(format: "%02x", $0) }.joined()
    }

    private static func loadFromKeychain() throws -> String? {
        let query: [CFString: Any] = [
            kSecClass:            kSecClassGenericPassword,
            kSecAttrService:      service,
            kSecAttrAccount:      account,
            kSecReturnData:       true,
            kSecMatchLimit:       kSecMatchLimitOne,
        ]
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data,
              let str = String(data: data, encoding: .utf8) else {
            throw KeyManagerError.keychainReadFailed(status)
        }
        return str
    }

    private static func saveToKeychain(_ passphrase: String) throws {
        guard let data = passphrase.data(using: .utf8) else { return }
        let query: [CFString: Any] = [
            kSecClass:                   kSecClassGenericPassword,
            kSecAttrService:             service,
            kSecAttrAccount:             account,
            kSecValueData:               data,
            kSecAttrAccessible:          kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        let status = SecItemAdd(query as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw KeyManagerError.keychainWriteFailed(status)
        }
    }

    enum KeyManagerError: Error {
        case randomGenerationFailed(OSStatus)
        case keychainReadFailed(OSStatus)
        case keychainWriteFailed(OSStatus)
    }
}
