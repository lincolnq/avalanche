package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview

// ---------------------------------------------------------------------------
// RootView
//
// Mirrors iOS RootView.swift: switches between SplashView (onboarding) and
// MainTabView (main app) based on AppViewModel.isOnboarding, and overlays
// the OfflineBanner at the top of the screen at all times.
//
// The full NavGraph lives in AppNavGraph (see MainActivity.kt). RootView is
// the outermost layout shell — it wraps AppNavGraph and adds the offline
// banner overlay, mirroring the iOS ZStack that holds Group { ... } and
// OfflineBanner() at alignment: .top.
//
// The iOS two-finger triple-tap gesture that shows LogViewerView is approximated
// on Android as a "log_viewer" route that any screen can push via callback.
// ---------------------------------------------------------------------------

@Composable
fun RootView(appViewModel: AppViewModel) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(AvalancheColors.Paper),
    ) {
        // The NavGraph handles the onboarding <-> main transition.
        AppNavGraph(appViewModel = appViewModel)

        // Floating offline banner — always on top, aligned to the top center.
        // Mirrors iOS ZStack(alignment: .top) with OfflineBanner() at the top.
        Box(modifier = Modifier.align(Alignment.TopCenter)) {
            OfflineBanner(appViewModel = appViewModel)
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun RootViewPreview() {
    AvalancheTheme {
        // Preview cannot construct a real AppViewModel; we rely on the
        // individual composable previews for visual validation.
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(AvalancheColors.Paper),
        )
    }
}
