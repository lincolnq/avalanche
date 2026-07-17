import Foundation
import Security

enum SecureEnclaveKeyManager {
    private static let service = "actnet"
    private static let account = "db.encryption.key"

    static func dbPassphrase() throws -> String {
        // Resolve the keychain access groups for this build. If we can't (should
        // never happen on a normally-signed build), fall back to unscoped storage
        // so the app still works — the NSE just won't be able to read the key.
        guard let groups = accessGroups() else {
            return try dbPassphrase(accessGroup: nil)
        }
        // Normal path: the key is already in the shared group.
        if let existing = try loadFromKeychain(accessGroup: groups.shared) {
            return existing
        }
        // One-time migration: a key written by a build that predated the shared
        // access group sits in the app's default group, unreadable by the NSE.
        // Re-add it under the shared group and drop the legacy copy. Idempotent —
        // once moved, the shared-group load above short-circuits this.
        if let legacy = try loadFromKeychain(accessGroup: groups.legacyDefault) {
            try saveToKeychain(legacy, accessGroup: groups.shared)
            deleteFromKeychain(accessGroup: groups.legacyDefault)
            return legacy
        }
        // Fresh install: generate straight into the shared group.
        let newKey = try generatePassphrase()
        try saveToKeychain(newKey, accessGroup: groups.shared)
        return newKey
    }

    /// The passphrase without an explicit access group (fallback only).
    private static func dbPassphrase(accessGroup: String?) throws -> String {
        if let existing = try loadFromKeychain(accessGroup: accessGroup) { return existing }
        let newKey = try generatePassphrase()
        try saveToKeychain(newKey, accessGroup: accessGroup)
        return newKey
    }

    /// The shared keychain group the NSE reads (docs/16 dep 1) and the app's
    /// legacy default group (where a pre-docs/16 key sits, for migration). Both
    /// are `<teamPrefix>.…`; `teamPrefix` is discovered at runtime so it always
    /// matches whatever `$(AppIdentifierPrefix)` in the entitlement resolved to
    /// for this build (Team ID on a signed build). Returns nil if it can't be
    /// resolved.
    private static func accessGroups() -> (shared: String, legacyDefault: String)? {
        guard let prefix = teamPrefix() else { return nil }
        return (
            shared: "\(prefix).net.theavalanche.app.shared",
            legacyDefault: "\(prefix).net.theavalanche.app"
        )
    }

    /// Discover the access-group team prefix (the value `$(AppIdentifierPrefix)`
    /// resolved to) by reading back the group iOS assigns to a probe item written
    /// with no explicit group: it lands in one of the app's own groups, whose
    /// leading dot-segment is the prefix. This is the standard "bundle seed ID"
    /// technique and is stable across launches (the probe item persists).
    private static func teamPrefix() -> String? {
        let probeAccount = "keychain-access-group-probe"
        let base: [CFString: Any] = [
            kSecClass:       kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: probeAccount,
        ]
        var query = base
        query[kSecReturnAttributes] = true
        query[kSecMatchLimit] = kSecMatchLimitOne

        var result: AnyObject?
        var status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound {
            var add = base
            add[kSecReturnAttributes] = true
            add[kSecValueData] = Data()
            status = SecItemAdd(add as CFDictionary, &result)
        }
        guard status == errSecSuccess,
              let attrs = result as? [CFString: Any],
              let group = attrs[kSecAttrAccessGroup] as? String,
              let prefix = group.components(separatedBy: ".").first,
              !prefix.isEmpty else {
            return nil
        }
        return prefix
    }

    private static func generatePassphrase() throws -> String {
        var bytes = [UInt8](repeating: 0, count: 32)
        let status = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        guard status == errSecSuccess else {
            throw KeyManagerError.randomGenerationFailed(status)
        }
        return bytes.map { String(format: "%02x", $0) }.joined()
    }

    private static func loadFromKeychain(accessGroup: String?) throws -> String? {
        var query: [CFString: Any] = [
            kSecClass:       kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: account,
            kSecReturnData:  true,
            kSecMatchLimit:  kSecMatchLimitOne,
        ]
        if let accessGroup { query[kSecAttrAccessGroup] = accessGroup }
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data,
              let str = String(data: data, encoding: .utf8) else {
            throw KeyManagerError.keychainReadFailed(status)
        }
        return str
    }

    private static func saveToKeychain(_ passphrase: String, accessGroup: String?) throws {
        guard let data = passphrase.data(using: .utf8) else { return }
        var query: [CFString: Any] = [
            kSecClass:          kSecClassGenericPassword,
            kSecAttrService:    service,
            kSecAttrAccount:    account,
            kSecValueData:      data,
            kSecAttrAccessible: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        if let accessGroup { query[kSecAttrAccessGroup] = accessGroup }
        let status = SecItemAdd(query as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw KeyManagerError.keychainWriteFailed(status)
        }
    }

    private static func deleteFromKeychain(accessGroup: String?) {
        var query: [CFString: Any] = [
            kSecClass:       kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: account,
        ]
        if let accessGroup { query[kSecAttrAccessGroup] = accessGroup }
        SecItemDelete(query as CFDictionary)
    }

    enum KeyManagerError: Error {
        case randomGenerationFailed(OSStatus)
        case keychainReadFailed(OSStatus)
        case keychainWriteFailed(OSStatus)
    }
}
