package net.theavalanche.app

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import uniffi.app_core.ConnectionState

// Offline color — matches iOS Color(red: 0.78, green: 0.42, blue: 0.10)
private val OfflineColor = Color(red = 0.78f, green = 0.42f, blue = 0.10f)

/**
 * Floating pill shown when any account's connection to its homeserver is not in
 * the Connected state. Drives entirely off AppViewModel.connectionStates (via
 * aggregateConnectionState) — the Rust reconnect task is the source of truth.
 *
 * Mirrors iOS OfflineBanner.swift.
 */
@Composable
fun OfflineBanner(appViewModel: AppViewModel) {
    val connectionStates by appViewModel.connectionStates.collectAsState()

    // Recompute aggregate whenever connection states change. Mirrors iOS
    // AppState.aggregateConnectionState computed property.
    val state = appViewModel.aggregateConnectionState

    val visible = shouldShow(state)

    // Tick every second so the countdown refreshes — mirrors iOS TimelineView(.periodic).
    var nowMs by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(visible) {
        while (visible) {
            delay(1_000L)
            nowMs = System.currentTimeMillis()
        }
    }

    AnimatedVisibility(
        visible = visible,
        enter = slideInVertically(initialOffsetY = { -it }) + fadeIn(),
        exit = slideOutVertically(targetOffsetY = { -it }) + fadeOut(),
    ) {
        Surface(
            shape = CircleShape,
            color = OfflineColor,
            shadowElevation = 6.dp,
            modifier = Modifier.padding(top = 4.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.padding(horizontal = 14.dp, vertical = 8.dp),
            ) {
                CircularProgressIndicator(
                    modifier = Modifier.size(14.dp),
                    color = Color.White,
                    strokeWidth = 2.dp,
                )
                Spacer(Modifier.width(8.dp))
                Text(
                    text = statusText(state, nowMs),
                    color = Color.White,
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                )
            }
        }
    }
}

private fun shouldShow(state: ConnectionState): Boolean = state !is ConnectionState.Connected

private fun statusText(state: ConnectionState, nowMs: Long): String = when (state) {
    is ConnectionState.Connected -> ""
    is ConnectionState.Connecting,
    is ConnectionState.Disconnected -> "Reconnecting…"
    is ConnectionState.Reconnecting -> {
        val diffMs = state.nextAttemptAtMs - nowMs
        val secs = maxOf(0L, (diffMs + 999L) / 1000L).toInt()
        if (secs <= 0) "Reconnecting…" else "Offline · retrying in ${secs}s"
    }
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

@Preview(showBackground = true)
@Composable
private fun OfflineBannerPreview() {
    // Preview cannot construct a real AppViewModel (needs Context), so we
    // demonstrate the pill directly without a ViewModel.
    AvalancheTheme {
        Surface(
            shape = CircleShape,
            color = OfflineColor,
            shadowElevation = 6.dp,
            modifier = Modifier.padding(4.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.padding(horizontal = 14.dp, vertical = 8.dp),
            ) {
                CircularProgressIndicator(
                    modifier = Modifier.size(14.dp),
                    color = Color.White,
                    strokeWidth = 2.dp,
                )
                Spacer(Modifier.width(8.dp))
                Text(
                    text = "Offline · retrying in 5s",
                    color = Color.White,
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                )
            }
        }
    }
}
