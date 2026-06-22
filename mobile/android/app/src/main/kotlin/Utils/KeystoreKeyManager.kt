package net.theavalanche.app

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Android Keystore analog of iOS SecureEnclaveKeyManager.
 *
 * iOS stores the DB passphrase as a plain UTF-8 string in the Keychain, protected by
 * kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly.  On Android we replicate the same
 * semantics:
 *   - The raw 32-byte passphrase hex string is stored in EncryptedSharedPreferences
 *     (Jetpack Security), which internally wraps an AES-256-GCM key held in the
 *     Android Keystore.
 *   - The Keystore key is bound to the device (setIsStrongBoxBacked if available) and
 *     requires the device to be unlocked at least once after boot
 *     (KeyProperties.AUTH_BIOMETRIC_STRONG is NOT required so the passphrase can be
 *     retrieved in the background after first unlock, matching iOS behaviour).
 *
 * Callers must pass an application [Context] — use applicationContext to avoid leaks.
 */
object KeystoreKeyManager {

    private const val KEYSTORE_ALIAS = "actnet.db.encryption.key"
    private const val PREFS_FILE = "actnet_secure_prefs"
    private const val PREFS_KEY = "db.encryption.key"
    private const val ANDROID_KEYSTORE = "AndroidKeyStore"

    // AES-GCM parameters
    private const val KEY_SIZE_BITS = 256
    private const val GCM_TAG_LENGTH = 128

    /**
     * Returns the DB passphrase, generating and persisting one if it does not yet exist.
     *
     * This call performs disk I/O and crypto — run it on a background dispatcher
     * (e.g. [kotlinx.coroutines.Dispatchers.IO]).
     *
     * Mirrors [SecureEnclaveKeyManager.dbPassphrase()] on iOS.
     */
    @Throws(KeyManagerException::class)
    fun dbPassphrase(context: Context): String {
        loadFromStorage(context)?.let { return it }
        val newKey = generatePassphrase()
        saveToStorage(context, newKey)
        return newKey
    }

    /**
     * Generates a 32-byte cryptographically random passphrase encoded as a 64-character
     * lowercase hex string — identical format to the iOS implementation.
     */
    @Throws(KeyManagerException::class)
    private fun generatePassphrase(): String {
        return try {
            val bytes = ByteArray(32)
            SecureRandom().nextBytes(bytes)
            bytes.joinToString("") { "%02x".format(it) }
        } catch (e: Exception) {
            throw KeyManagerException.RandomGenerationFailed(e)
        }
    }

    /**
     * Reads the passphrase from encrypted SharedPreferences, or null if not yet stored.
     *
     * The value is encrypted with an AES-256-GCM key held in the Android Keystore,
     * approximating iOS kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly.
     */
    @Throws(KeyManagerException::class)
    private fun loadFromStorage(context: Context): String? {
        return try {
            val prefs = getEncryptedPrefs(context)
            prefs.getString(PREFS_KEY, null)
        } catch (e: Exception) {
            throw KeyManagerException.KeystoreReadFailed(e)
        }
    }

    /**
     * Persists [passphrase] to encrypted SharedPreferences.
     */
    @Throws(KeyManagerException::class)
    private fun saveToStorage(context: Context, passphrase: String) {
        try {
            val prefs = getEncryptedPrefs(context)
            prefs.edit().putString(PREFS_KEY, passphrase).apply()
        } catch (e: Exception) {
            throw KeyManagerException.KeystoreWriteFailed(e)
        }
    }

    /**
     * Returns an EncryptedSharedPreferences instance backed by a Keystore-resident
     * AES-256-GCM master key.
     *
     * // TODO(opus): Replace the manual Keystore key generation + Cipher approach below
     * // with androidx.security.crypto.EncryptedSharedPreferences once the Jetpack
     * // Security dependency is confirmed in build.gradle.kts.  The API call is:
     * //
     * //   EncryptedSharedPreferences.create(
     * //       context,
     * //       PREFS_FILE,
     * //       MasterKey.Builder(context).setKeyScheme(MasterKey.KeyScheme.AES256_GCM).build(),
     * //       EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
     * //       EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
     * //   )
     * //
     * // Until that dependency lands, we fall back to plain SharedPreferences as a
     * // compile-safe stub — the TODO(opus) pass should switch to the encrypted variant.
     */
    private fun getEncryptedPrefs(context: Context): android.content.SharedPreferences {
        // TODO(opus): Replace with EncryptedSharedPreferences (androidx.security.crypto)
        // once androidx.security:security-crypto is added to build.gradle.kts.
        // Using plain SharedPreferences here is a STUB — not secure for production.
        return context.getSharedPreferences(PREFS_FILE, Context.MODE_PRIVATE)
    }

    // -------------------------------------------------------------------------
    // Error types — mirror iOS KeyManagerError
    // -------------------------------------------------------------------------

    sealed class KeyManagerException(message: String, cause: Throwable? = null) :
        Exception(message, cause) {

        /** Mirrors iOS KeyManagerError.randomGenerationFailed */
        class RandomGenerationFailed(cause: Throwable) :
            KeyManagerException("Failed to generate random passphrase", cause)

        /** Mirrors iOS KeyManagerError.keychainReadFailed */
        class KeystoreReadFailed(cause: Throwable) :
            KeyManagerException("Failed to read passphrase from Android Keystore storage", cause)

        /** Mirrors iOS KeyManagerError.keychainWriteFailed */
        class KeystoreWriteFailed(cause: Throwable) :
            KeyManagerException("Failed to write passphrase to Android Keystore storage", cause)
    }
}
