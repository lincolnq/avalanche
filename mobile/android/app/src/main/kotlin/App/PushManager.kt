package net.theavalanche.app

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

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
     * permission). The permission request itself must be launched from the
     * Activity/Composable via ActivityResultContracts.RequestPermission and
     * the result forwarded to [onPermissionResult].
     *
     * TODO(opus): Call this from MainActivity after the permission result
     * callback wires in, or use the Accompanist permissions library from the
     * relevant composable screen.
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
        AppLog.info("PushManager", "FCM registration token: $token")

        // TODO(opus): read RELAY_URL from BuildConfig (set via manifestPlaceholders
        // or buildConfigField in build.gradle.kts, populated from env by the
        // Makefile analogous to iOS RELAY_URL). Hard-coded empty string for now.
        val relayUrl = ""  // TODO(opus): BuildConfig.RELAY_URL
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

        val cores = appViewModel.activeCores()
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
        scope.launch {
            for (core in cores) {
                // registerPushToken is idempotent — safe to call on every launch.
                runCatching {
                    core.registerPushToken(
                        deviceToken = token,
                        platform = platform,
                        relayUrl = relayUrl,
                        environment = environment,
                    )
                    AppLog.info(
                        "PushManager",
                        "registerPushToken ok (relay=$relayUrl, env=$environment)",
                    )
                }.onFailure { error ->
                    AppLog.error(
                        "PushManager",
                        "registerPushToken failed (relay=$relayUrl): ${error.message}",
                    )
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    private fun fetchTokenAndRegister(appViewModel: AppViewModel) {
        // TODO(opus): wire up Firebase Cloud Messaging. Pulling FCM in requires the
        // Firebase BOM + google-services Gradle plugin + a google-services.json,
        // which we deliberately defer — partly to keep the app runnable on
        // de-Googled Android without Google Play Services. Mirrors the iOS push
        // path being a best-effort, env-gated feature. No-op stub for now so the
        // rest of the app builds; restore the body and add the FCM dependency +
        // ActnetFirebaseMessagingService when push is implemented.
        AppLog.info("PushManager", "FCM not wired in yet — skipping push token registration for $appViewModel")
    }
}

// TODO(opus): When FCM is implemented, add an `ActnetFirebaseMessagingService`
// extending `com.google.firebase.messaging.FirebaseMessagingService` here
// (overriding onNewToken / onMessageReceived to forward to PushManager via a
// callback stored on ActnetApplication), add the Firebase BOM + google-services
// plugin to Gradle, and register the <service> in AndroidManifest.xml. Omitted
// for now so the app builds without a Google Play Services dependency.
