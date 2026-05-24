import AuthenticationServices
import CryptoKit
import Foundation

/// Manages WebAuthn passkey registration and authentication ceremonies.
///
/// - Registration: Creates a new passkey and derives a 32-byte symmetric key
///   via the PRF extension. The symmetric key encrypts the recovery blob.
/// - Authentication: Retrieves an existing passkey and re-derives the same
///   symmetric key for recovery blob decryption.
///
/// The relying party is `theavalanche.net` — shared across all actnet servers
/// so passkeys work for recovery regardless of which server the user is on.
@MainActor
final class PasskeyManager: NSObject {

    /// The relying party domain for all actnet passkeys.
    static let relyingParty = "theavalanche.net"

    /// Fixed PRF salt used to derive the recovery symmetric key.
    /// Same salt during registration and authentication produces the same key.
    private static let prfSalt = Data("actnet-recovery-v1".utf8)

    /// Result of a passkey registration ceremony.
    struct RegistrationResult {
        /// 32-byte symmetric key derived from PRF extension output.
        let recoveryKey: Data
        /// The DID stored in the credential's user handle, for recovery lookup.
        let userHandle: Data
    }

    /// Result of a passkey authentication ceremony.
    struct AuthenticationResult {
        /// 32-byte symmetric key derived from PRF extension output.
        let recoveryKey: Data
        /// The DID stored in the credential's user handle.
        let did: String
    }

    private var registrationContinuation: CheckedContinuation<RegistrationResult, Error>?
    private var authenticationContinuation: CheckedContinuation<AuthenticationResult, Error>?

    /// Register a new passkey for an identity.
    ///
    /// - Parameters:
    ///   - did: The DID to store in the passkey's user handle (for recovery lookup).
    ///   - displayName: The display name shown in the passkey manager.
    ///   - anchor: The presentation anchor (window) for the system sheet.
    /// - Returns: The PRF-derived recovery key and credential info.
    func register(
        did: String,
        displayName: String,
        anchor: ASPresentationAnchor
    ) async throws -> RegistrationResult {
        let provider = ASAuthorizationPlatformPublicKeyCredentialProvider(
            relyingPartyIdentifier: Self.relyingParty
        )

        let challenge = Self.generateChallenge()
        let userHandle = Data(did.utf8)

        let request = provider.createCredentialRegistrationRequest(
            challenge: challenge,
            name: displayName,
            userID: userHandle
        )

        // Request PRF extension for symmetric key derivation.
        let inputValues = ASAuthorizationPublicKeyCredentialPRFAssertionInput.InputValues(
            saltInput1: Self.prfSalt
        )
        request.prf = .inputValues(inputValues)

        let controller = ASAuthorizationController(authorizationRequests: [request])
        controller.delegate = self
        controller.presentationContextProvider = WindowAnchorProvider(anchor: anchor)

        return try await withCheckedThrowingContinuation { continuation in
            self.registrationContinuation = continuation
            controller.performRequests()
        }
    }

    /// Authenticate with an existing passkey (for recovery).
    ///
    /// The system presents all passkeys stored for `theavalanche.net`.
    /// The user picks one and confirms with biometrics.
    ///
    /// - Parameter anchor: The presentation anchor for the system sheet.
    /// - Returns: The PRF-derived recovery key and the DID from the user handle.
    func authenticate(
        anchor: ASPresentationAnchor
    ) async throws -> AuthenticationResult {
        let provider = ASAuthorizationPlatformPublicKeyCredentialProvider(
            relyingPartyIdentifier: Self.relyingParty
        )

        let challenge = Self.generateChallenge()
        let request = provider.createCredentialAssertionRequest(challenge: challenge)

        // Request PRF extension to re-derive the same symmetric key.
        let inputValues = ASAuthorizationPublicKeyCredentialPRFAssertionInput.InputValues(
            saltInput1: Self.prfSalt
        )
        request.prf = .inputValues(inputValues)

        let controller = ASAuthorizationController(authorizationRequests: [request])
        controller.delegate = self
        controller.presentationContextProvider = WindowAnchorProvider(anchor: anchor)

        return try await withCheckedThrowingContinuation { continuation in
            self.authenticationContinuation = continuation
            controller.performRequests()
        }
    }

    /// Derive a 32-byte symmetric key from PRF output using HKDF.
    private static func deriveRecoveryKey(from prfOutput: Data) -> Data {
        let inputKey = SymmetricKey(data: prfOutput)
        let derivedKey = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: inputKey,
            salt: Data("actnet-recovery-blob".utf8),
            info: Data("AES-256-GCM".utf8),
            outputByteCount: 32
        )
        return derivedKey.withUnsafeBytes { Data($0) }
    }

    /// Generate a random challenge for the WebAuthn ceremony.
    private static func generateChallenge() -> Data {
        var bytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        return Data(bytes)
    }
}

// MARK: - ASAuthorizationControllerDelegate

extension PasskeyManager: ASAuthorizationControllerDelegate {

    nonisolated func authorizationController(
        controller: ASAuthorizationController,
        didCompleteWithAuthorization authorization: ASAuthorization
    ) {
        Task { @MainActor in
            if let credential = authorization.credential as? ASAuthorizationPlatformPublicKeyCredentialRegistration {
                handleRegistration(credential)
            } else if let credential = authorization.credential as? ASAuthorizationPlatformPublicKeyCredentialAssertion {
                handleAssertion(credential)
            }
        }
    }

    nonisolated func authorizationController(
        controller: ASAuthorizationController,
        didCompleteWithError error: Error
    ) {
        Task { @MainActor in
            registrationContinuation?.resume(throwing: error)
            registrationContinuation = nil
            authenticationContinuation?.resume(throwing: error)
            authenticationContinuation = nil
        }
    }

    @MainActor
    private func handleRegistration(_ credential: ASAuthorizationPlatformPublicKeyCredentialRegistration) {
        guard let prfOutput = credential.prf, let prfKey = prfOutput.first else {
            registrationContinuation?.resume(
                throwing: PasskeyError.prfNotSupported
            )
            registrationContinuation = nil
            return
        }

        let prfData = prfKey.withUnsafeBytes { Data($0) }
        let recoveryKey = Self.deriveRecoveryKey(from: prfData)
        let result = RegistrationResult(
            recoveryKey: recoveryKey,
            userHandle: credential.rawClientDataJSON
        )
        registrationContinuation?.resume(returning: result)
        registrationContinuation = nil
    }

    @MainActor
    private func handleAssertion(_ credential: ASAuthorizationPlatformPublicKeyCredentialAssertion) {
        guard let prfOutput = credential.prf else {
            authenticationContinuation?.resume(
                throwing: PasskeyError.prfNotSupported
            )
            authenticationContinuation = nil
            return
        }

        let prfData = prfOutput.first.withUnsafeBytes { Data($0) }
        let recoveryKey = Self.deriveRecoveryKey(from: prfData)
        let did = String(data: credential.userID, encoding: .utf8) ?? ""
        let result = AuthenticationResult(
            recoveryKey: recoveryKey,
            did: did
        )
        authenticationContinuation?.resume(returning: result)
        authenticationContinuation = nil
    }
}

// MARK: - Supporting types

enum PasskeyError: LocalizedError {
    case prfNotSupported
    case cancelled
    case unknown(String)

    var errorDescription: String? {
        switch self {
        case .prfNotSupported:
            return "Your password manager doesn't support the PRF extension needed for recovery. Try a different passkey provider."
        case .cancelled:
            return "Passkey operation was cancelled."
        case .unknown(let msg):
            return msg
        }
    }
}

/// Provides the presentation anchor for the ASAuthorizationController sheet.
private class WindowAnchorProvider: NSObject, ASAuthorizationControllerPresentationContextProviding {
    let anchor: ASPresentationAnchor

    init(anchor: ASPresentationAnchor) {
        self.anchor = anchor
    }

    func presentationAnchor(for controller: ASAuthorizationController) -> ASPresentationAnchor {
        anchor
    }
}
