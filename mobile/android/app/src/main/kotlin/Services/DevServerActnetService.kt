package net.theavalanche.app

import uniffi.app_core.AppErrorFfi
import uniffi.app_core.PreparedAccount

/**
 * Live [ActnetService] backed by a real locally-running homeserver via the
 * Rust AppCore UniFFI bindings.
 *
 * Mirrors iOS `DevServerActnetService` — a concrete implementation of the
 * `ActnetService` protocol that forwards all account-lifecycle calls to the
 * UniFFI-generated [AppCore] static constructors.
 *
 * All methods are synchronous and BLOCKING. Callers (ViewModel layer) must
 * dispatch onto [kotlinx.coroutines.Dispatchers.IO] before invoking.
 */
class DevServerActnetService : ActnetService {

    companion object {
        const val DEFAULT_SERVER_URL = "http://localhost:3000"
    }

    @Throws(AppErrorFfi::class)
    override fun createAccount(
        serverUrl: String,
        dbPath: String,
        dbKey: String,
        prfOutput: ByteArray,
        displayName: String,
        inviteToken: String?,
    ): AppCoreProtocol =
        LiveActnetService.createAccount(serverUrl, dbPath, dbKey, prfOutput, displayName, inviteToken)

    @Throws(AppErrorFfi::class)
    override fun login(dbPath: String, dbKey: String): AppCoreProtocol =
        LiveActnetService.login(dbPath, dbKey)

    @Throws(AppErrorFfi::class)
    override fun prepareAccount(serverUrl: String, prfOutput: ByteArray): PreparedAccount =
        LiveActnetService.prepareAccount(serverUrl, prfOutput)

    @Throws(AppErrorFfi::class)
    override fun finalizeAccount(
        prepared: PreparedAccount,
        dbPath: String,
        dbKey: String,
        displayName: String,
        inviteToken: String?,
    ): AppCoreProtocol =
        LiveActnetService.finalizeAccount(prepared, dbPath, dbKey, displayName, inviteToken)

    @Throws(AppErrorFfi::class)
    override fun recoverFromBlob(
        serverUrl: String,
        did: String,
        prfOutput: ByteArray,
        dbPath: String,
        dbKey: String,
        displayName: String,
    ): AppCoreProtocol =
        LiveActnetService.recoverFromBlob(serverUrl, did, prfOutput, dbPath, dbKey, displayName)

    override fun makeDeviceLink(): DeviceLink = LiveActnetService.makeDeviceLink()
}

/**
 * Mirrors iOS `ActnetServiceError` — errors that can arise from the
 * [ActnetService] layer itself (distinct from [AppErrorFfi] thrown by the Rust core).
 */
sealed class ActnetServiceError(message: String) : Exception(message) {
    /** Thrown by [DevServerActnetService.finalizeAccount] when the [PreparedAccount] argument
     *  was not produced by the same service implementation. */
    object PreparedAccountTypeMismatch : ActnetServiceError(
        "PreparedAccount instance does not match the active ActnetService."
    )
}
