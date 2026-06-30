package net.theavalanche.app

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.util.Base64
import androidx.credentials.CreatePublicKeyCredentialRequest
import androidx.credentials.CreatePublicKeyCredentialResponse
import androidx.credentials.CredentialManager
import androidx.credentials.GetCredentialRequest
import androidx.credentials.GetPublicKeyCredentialOption
import androidx.credentials.PublicKeyCredential
import androidx.credentials.exceptions.CreateCredentialCancellationException
import androidx.credentials.exceptions.CreateCredentialException
import androidx.credentials.exceptions.GetCredentialCancellationException
import androidx.credentials.exceptions.GetCredentialException
import androidx.credentials.exceptions.NoCredentialException
import org.json.JSONObject
import java.security.SecureRandom

/**
 * Manages WebAuthn passkey registration and authentication ceremonies via the
 * Android Credential Manager.
 *
 * Mirrors iOS `PasskeyManager.swift` feature-for-feature:
 *  - Registration: creates a new passkey with the signup server URL as the
 *    `user.id` (userHandle), and returns the raw 32-byte PRF output. The Rust
 *    core derives both the DID rotation key and the recovery-blob encryption
 *    key from this output via HKDF — Kotlin just shuttles the bytes.
 *  - Authentication: retrieves an existing passkey and returns the raw PRF
 *    output plus the original signup server URL from the userHandle, which
 *    together let the Rust core recompute the DID without any server lookup.
 *
 * The relying party is `theavalanche.net` — shared across all avalanche servers
 * so passkeys work for recovery regardless of which server the user is on. The
 * app must be associated with that domain via Digital Asset Links published at
 * `https://theavalanche.net/.well-known/assetlinks.json` (the Android analog of
 * iOS Associated Domains `webcredentials:theavalanche.net`).
 *
 * Cross-platform note: both Apple's ASAuthorization PRF and Android's WebAuthn
 * PRF implement the same WebAuthn PRF extension, so the same passkey + the same
 * salt yields bit-identical 32 bytes on either platform. That is what keeps the
 * derived rotation/blob keys — and therefore the DID — consistent across an
 * identity's devices.
 */
object PasskeyManager {

    /** The relying party domain for all avalanche passkeys. */
    const val RELYING_PARTY = "theavalanche.net"

    /**
     * Fixed PRF salt used during both registration and assertion. The
     * authenticator's PRF output is deterministic for `(passkey, salt)`, so the
     * same salt always yields the same 32 bytes. Must match iOS
     * `PasskeyManager.prfSalt`.
     */
    private val PRF_SALT = "actnet-recovery-v1".toByteArray(Charsets.UTF_8)

    /** Base64url flags matching the WebAuthn JSON encoding (no padding, no wrap). */
    private const val B64URL = Base64.URL_SAFE or Base64.NO_PADDING or Base64.NO_WRAP

    /** Result of a passkey registration ceremony. */
    data class RegistrationResult(
        /** Raw 32-byte PRF output. Rust derives rotation key + blob key from this. */
        val prfOutput: ByteArray,
    )

    /** Result of a passkey authentication ceremony. */
    data class AuthenticationResult(
        /** Raw 32-byte PRF output. Rust derives rotation key + blob key from this. */
        val prfOutput: ByteArray,
        /**
         * The signup server URL stored in the credential's userHandle. Used to
         * recompute the genesis op and derive the DID.
         */
        val signupServerUrl: String,
    )

    /**
     * Register a new passkey for a fresh identity.
     *
     * @param context A UI context (the hosting Activity) to anchor the system sheet.
     * @param signupServerUrl The homeserver URL the user is signing up at. Stored
     *   in `user.id` so that recovery can recompute the DID.
     * @param displayName Human-readable label shown in the OS passkey picker
     *   (e.g. "Sam @ safe-haven.org").
     * @return The raw PRF output.
     */
    suspend fun register(
        context: Context,
        signupServerUrl: String,
        displayName: String,
    ): RegistrationResult {
        val challenge = generateChallenge()
        val userHandle = signupServerUrl.toByteArray(Charsets.UTF_8)

        // WebAuthn PublicKeyCredentialCreationOptions as JSON. Request the PRF
        // extension at creation so the authenticator returns the PRF output
        // alongside the new credential. A discoverable (resident) credential is
        // required so recovery can find it without an allowCredentials list.
        val requestJson = JSONObject().apply {
            put("challenge", b64UrlEncode(challenge))
            put(
                "rp",
                JSONObject().apply {
                    put("id", RELYING_PARTY)
                    put("name", "Avalanche")
                },
            )
            put(
                "user",
                JSONObject().apply {
                    put("id", b64UrlEncode(userHandle))
                    put("name", displayName)
                    put("displayName", displayName)
                },
            )
            put(
                "pubKeyCredParams",
                org.json.JSONArray().apply {
                    put(JSONObject().apply { put("type", "public-key"); put("alg", -7) })   // ES256
                    put(JSONObject().apply { put("type", "public-key"); put("alg", -257) }) // RS256
                },
            )
            put(
                "authenticatorSelection",
                JSONObject().apply {
                    put("residentKey", "required")
                    put("requireResidentKey", true)
                    put("userVerification", "required")
                },
            )
            put("extensions", prfEvalExtension())
        }.toString()

        val response = try {
            CredentialManager.create(context).createCredential(
                context = context,
                request = CreatePublicKeyCredentialRequest(requestJson),
            )
        } catch (e: CreateCredentialCancellationException) {
            throw PasskeyException.Cancelled
        } catch (e: CreateCredentialException) {
            throw PasskeyException.Unknown(e.errorMessage?.toString() ?: e.type)
        }

        val responseJson = (response as CreatePublicKeyCredentialResponse).registrationResponseJson
        val prf = extractPrfFirst(responseJson)
            ?: throw PasskeyException.PrfNotSupported
        return RegistrationResult(prfOutput = prf)
    }

    /**
     * Authenticate with an existing passkey (for recovery).
     *
     * The system presents all passkeys stored for `theavalanche.net`. The user
     * picks one and confirms with biometrics.
     *
     * @param context A UI context (the hosting Activity) to anchor the system sheet.
     * @return The PRF-derived recovery key and the signup server URL from the userHandle.
     */
    suspend fun authenticate(context: Context): AuthenticationResult {
        val challenge = generateChallenge()

        // WebAuthn PublicKeyCredentialRequestOptions as JSON. No allowCredentials
        // (discoverable mode) so the OS shows every avalanche passkey. Request the
        // PRF extension with the same salt to re-derive the same symmetric key.
        val requestJson = JSONObject().apply {
            put("challenge", b64UrlEncode(challenge))
            put("rpId", RELYING_PARTY)
            put("userVerification", "required")
            put("extensions", prfEvalExtension())
        }.toString()

        val result = try {
            CredentialManager.create(context).getCredential(
                context = context,
                request = GetCredentialRequest(listOf(GetPublicKeyCredentialOption(requestJson))),
            )
        } catch (e: GetCredentialCancellationException) {
            throw PasskeyException.Cancelled
        } catch (e: NoCredentialException) {
            throw PasskeyException.NoCredential
        } catch (e: GetCredentialException) {
            throw PasskeyException.Unknown(e.errorMessage?.toString() ?: e.type)
        }

        val credential = result.credential as? PublicKeyCredential
            ?: throw PasskeyException.Unknown("Unexpected credential type")
        val responseJson = credential.authenticationResponseJson

        val prf = extractPrfFirst(responseJson)
            ?: throw PasskeyException.PrfNotSupported
        val signupServerUrl = extractUserHandle(responseJson)
            ?: throw PasskeyException.Unknown("Passkey is missing its server URL (userHandle)")

        return AuthenticationResult(prfOutput = prf, signupServerUrl = signupServerUrl)
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /** Build the `{ prf: { eval: { first: <b64url salt> } } }` extensions object. */
    private fun prfEvalExtension(): JSONObject =
        JSONObject().apply {
            put(
                "prf",
                JSONObject().apply {
                    put(
                        "eval",
                        JSONObject().apply { put("first", b64UrlEncode(PRF_SALT)) },
                    )
                },
            )
        }

    /**
     * Pull `clientExtensionResults.prf.results.first` (base64url) out of a
     * WebAuthn registration or authentication response JSON, or null if the
     * provider did not return a PRF result.
     */
    private fun extractPrfFirst(responseJson: String): ByteArray? {
        val results = JSONObject(responseJson)
            .optJSONObject("clientExtensionResults")
            ?.optJSONObject("prf")
            ?.optJSONObject("results")
            ?: return null
        val first = results.optString("first", "").takeIf { it.isNotEmpty() } ?: return null
        return b64UrlDecode(first)
    }

    /** Pull `response.userHandle` (base64url) out of an authentication response JSON. */
    private fun extractUserHandle(responseJson: String): String? {
        val handle = JSONObject(responseJson)
            .optJSONObject("response")
            ?.optString("userHandle", "")
            ?.takeIf { it.isNotEmpty() }
            ?: return null
        return String(b64UrlDecode(handle), Charsets.UTF_8)
    }

    private fun generateChallenge(): ByteArray {
        val bytes = ByteArray(32)
        SecureRandom().nextBytes(bytes)
        return bytes
    }

    private fun b64UrlEncode(bytes: ByteArray): String = Base64.encodeToString(bytes, B64URL)

    /** Tolerant of input with or without padding (URL_SAFE accepts both). */
    private fun b64UrlDecode(s: String): ByteArray = Base64.decode(s, Base64.URL_SAFE)
}

/**
 * Errors surfaced by [PasskeyManager]. Mirrors iOS `PasskeyError`.
 */
sealed class PasskeyException(message: String) : Exception(message) {
    /** The provider doesn't support the PRF extension needed for recovery. */
    object PrfNotSupported : PasskeyException(
        "Your password manager doesn't support the PRF extension needed for recovery. " +
            "Try a different passkey provider.",
    )

    /** The user cancelled the ceremony. Callers should silently re-enable UI. */
    object Cancelled : PasskeyException("Passkey operation was cancelled.")

    /** No passkey was available to satisfy a recovery assertion. */
    object NoCredential : PasskeyException(
        "No passkey was found for this device. Try a different recovery method.",
    )

    /** Any other failure, carrying the provider's message. */
    class Unknown(detail: String) : PasskeyException(detail)
}

/**
 * Unwrap a (possibly themed/wrapped) [Context] to the hosting [Activity], which
 * the Credential Manager needs to anchor its system UI. `LocalContext.current`
 * is the Activity in a normal setContent host, but unwrapping is robust against
 * ContextThemeWrapper and preview contexts.
 */
internal fun Context.findActivity(): Activity {
    var ctx: Context = this
    while (ctx is ContextWrapper) {
        if (ctx is Activity) return ctx
        ctx = ctx.baseContext
    }
    throw PasskeyException.Unknown("No Activity available to present the passkey UI")
}
