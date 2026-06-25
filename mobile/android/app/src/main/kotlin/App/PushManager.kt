package net.theavalanche.app

import com.google.firebase.messaging.FirebaseMessaging

// ---------------------------------------------------------------------------
// PushManager
//
// Android analog of iOS Sources/App/PushManager.swift.
// iOS uses APNs (UNUserNotificationCenter + UIApplication.registerForRemoteNotifications).
// Android uses FCM (Firebase Cloud Messaging).
//
// On Android 13+ (API 33+) POST_NOTIFICATIONS is a runtime permission that must
// be requested before FCM banners are shown. Registration with FCM itself does
// not require a permission on any API level.
//
// The caller (AppViewModel) should call:
//   PushManager.requestPermissionAndRegister(appViewModel)
// after a successful login or account creation, mirroring the iOS call site.
// ---------------------------------------------------------------------------

object PushManager {

    /**
     * Request the POST_NOTIFICATIONS permission (Android 13+) and, once
     * granted (or on older API levels), fetch the FCM registration token and
     * register it with all active cores.
     *
     * Mirrors iOS PushManager.requestPermissionAndRegister(appState:).
     *
     * NOTE: On Android, runtime permission requests must be triggered from an
     * Activity, not from a ViewModel or background service. This method fetches
     * the FCM token unconditionally (FCM registration does not require
     * permission). The POST_NOTIFICATIONS request itself is launched separately
     * by MainActivity (ActivityResultContracts.RequestPermission), which forwards
     * the result to [onPermissionResult]; the permission only gates whether
     * banners are shown, not token registration.
     */
    fun requestPermissionAndRegister(appViewModel: AppViewModel) {
        // Fetch FCM token regardless of notification permission — the token is
        // used for silent (data-only) pushes which don't require a permission.
        fetchTokenAndRegister(appViewModel)
    }

    /**
     * Called when the Android 13+ POST_NOTIFICATIONS permission result
     * arrives. If granted, there is nothing extra to do for FCM (the token is
     * already registered). This is a no-op hook kept for symmetry with the
     * iOS permission callback pattern and in case future logic is needed.
     */
    fun onPermissionResult(granted: Boolean, appViewModel: AppViewModel) {
        if (granted) {
            AppLog.info("PushManager", "POST_NOTIFICATIONS granted")
        } else {
            AppLog.info("PushManager", "POST_NOTIFICATIONS denied — silent pushes still work")
        }
        // Either way, ensure the token is registered (idempotent).
        fetchTokenAndRegister(appViewModel)
    }

    /**
     * Called when FCM issues a new registration token. Registers it with all
     * active cores. Mirrors iOS PushManager.didReceiveToken(_:appState:).
     *
     * Invoke this from [ActnetFirebaseMessagingService.onNewToken] and on
     * fresh app launches (via [requestPermissionAndRegister]).
     */
    fun didReceiveToken(token: String, appViewModel: AppViewModel) {
        // Log only a short prefix — the full FCM token is a capability (anyone
        // holding it can trigger push wakeups to this device) and must not leak
        // into logcat / bug reports.
        val tokenPreview = if (token.length > 8) "${token.take(8)}…" else token
        AppLog.info("PushManager", "FCM registration token: $tokenPreview")

        // RELAY_URL is baked into BuildConfig from a Gradle property / env var /
        // default (see app/build.gradle.kts), mirroring how iOS reads it from Info.plist.
        val relayUrl = BuildConfig.RELAY_URL
        if (relayUrl.isEmpty()) {
            AppLog.warn(
                "PushManager",
                "RELAY_URL is empty — push registration skipped. " +
                    "Set RELAY_URL and rebuild.",
            )
            return
        }

        // FCM tokens are environment-agnostic (unlike APNs sandbox vs production).
        // We pass "fcm" as the platform identifier.
        val platform = "fcm"
        // Android has no sandbox/production split at the token level. Pass
        // "production" to match what the relay server expects for FCM.
        val environment = "production"

        // Hand off to the ViewModel, which runs the per-core registration on its
        // own lifecycle-scoped coroutine. This keeps `cores` access on the main
        // dispatcher (FCM may dispatch this callback off the main thread) and
        // avoids leaking a detached CoroutineScope per token rotation.
        appViewModel.registerPushTokenWithCores(
            token = token,
            platform = platform,
            relayUrl = relayUrl,
            environment = environment,
        )
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    private fun fetchTokenAndRegister(appViewModel: AppViewModel) {
        // Ask FCM for this install's registration token, then register it with the
        // relay. Token fetch is async and does not require notification permission
        // (the wakeups are data-only). onNewToken handles later token rotations.
        FirebaseMessaging.getInstance().token
            .addOnCompleteListener { task ->
                if (!task.isSuccessful) {
                    AppLog.warn(
                        "PushManager",
                        "FCM getToken failed: ${task.exception?.message}",
                    )
                    return@addOnCompleteListener
                }
                val token = task.result
                if (token.isNullOrEmpty()) {
                    AppLog.warn("PushManager", "FCM returned an empty token")
                    return@addOnCompleteListener
                }
                didReceiveToken(token, appViewModel)
            }
    }
}
