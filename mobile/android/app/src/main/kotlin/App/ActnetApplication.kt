package net.theavalanche.app

import android.app.Application
import net.theavalanche.app.BuildConfig
import uniffi.app_core.initLogging

/**
 * Application subclass for the Avalanche (actnet) app.
 *
 * Mirrors the initialization that iOS performs in AppDelegate.application(_:didFinishLaunchingWithOptions:):
 *  - Calls initLogging() once (Rust global logger — panics if called twice; safe here
 *    because Application.onCreate runs exactly once per process).
 *  - Does NOT restore accounts — that is driven by AppViewModel.restoreAccounts(), which
 *    is called from the Compose entry point (MainActivity/RootView) after the ViewModel
 *    is attached to the lifecycle. This mirrors the iOS `.task { await appState.restoreAccounts() }`
 *    placed on RootView.
 *
 * iOS background-push handling (didReceiveRemoteNotification) and APNs token forwarding
 * have no 1:1 analog in this Application class; those belong in a FirebaseMessagingService
 * subclass and a notification channel setup routine.
 * TODO(opus): Implement FCM push service for background-message wakeup.
 * TODO(opus): Implement notification channel creation (required on API 26+).
 */
class ActnetApplication : Application() {

    override fun onCreate() {
        super.onCreate()

        // Initialize the Rust logger exactly once, before any FFI call.
        // Mirrors iOS:
        //   #if DEBUG
        //   initLogging(filter: "app_core=debug,net=info,store=info,crypto=info")
        //   #else
        //   initLogging(filter: "info")
        //   #endif
        try {
            if (BuildConfig.DEBUG) {
                initLogging(filter = "app_core=debug,net=info,store=info,crypto=info")
            } else {
                initLogging(filter = "info")
            }
        } catch (e: Exception) {
            // Logging init failed (e.g. called twice in robolectric tests).
            // Swallow silently — the app can still run without structured logging.
            android.util.Log.w("ActnetApplication", "initLogging failed (already initialised?): ${e.message}")
        }

        AppLog.info("ActnetApplication", "Process started, logging initialized")
    }
}
