package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

/// QR scanner screen that mirrors iOS QRScannerView.swift.
///
/// When [onScanned] is non-null, decoded values are forwarded to the caller
/// instead of starting the onboarding identity-picker flow.
/// When [onScanned] is null, the composable validates the scanned URL and
/// calls [onInviteToken] with a parsed [InviteToken] once validation succeeds.
///
/// Navigation: pass [onInviteToken] to move to IdentityPickerView after a
/// successful scan; pass [onScanned] to intercept the raw string elsewhere.
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun QRScannerView(
    onScanned: ((String) -> Unit)? = null,
    onInviteToken: (InviteToken) -> Unit = {},
) {
    var errorMessage by remember { mutableStateOf<String?>(null) }
    var isValidating by remember { mutableStateOf(false) }

    val scope = rememberCoroutineScope()

    fun handle(value: String) {
        if (onScanned != null) {
            onScanned(value)
            return
        }
        // Validate that it looks like a go.theavalanche.net deep link.
        // Mirrors iOS: guard let url = URL(string: value), AppState.isDeepLink(url)
        val isDeepLink = value.contains("go.theavalanche.net")
        if (!isDeepLink) {
            errorMessage = "Not an Avalanche invite QR code"
            return
        }
        isValidating = true
        scope.launch {
            try {
                val token = InviteToken.fromUrl(value)
                isValidating = false
                onInviteToken(token)
            } catch (e: Exception) {
                errorMessage = "Invite validation failed: ${e.message}"
                isValidating = false
            }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(title = { Text("Scan QR Code") })
        },
    ) { innerPadding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding),
        ) {
            // Live camera view fills the whole frame (ignores safe-area edges on iOS).
            QRCodeCameraView(
                onScanned = ::handle,
                modifier = Modifier.fillMaxSize(),
            )

            // Hint / error label pinned to the bottom — mirrors the VStack{Spacer; Text} overlay.
            Column(
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .padding(horizontal = 24.dp)
                    .padding(bottom = 48.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    text = errorMessage ?: "Point your camera at an Avalanche invite QR code",
                    color = Color.White,
                    textAlign = TextAlign.Center,
                    modifier = Modifier
                        .background(
                            color = Color.Black.copy(alpha = 0.5f),
                            shape = RoundedCornerShape(12.dp),
                        )
                        .padding(12.dp),
                )
            }

            // Spinner shown while the invite token is being validated.
            if (isValidating) {
                CircularProgressIndicator(
                    modifier = Modifier.align(Alignment.Center),
                    color = Color.White,
                    strokeWidth = 3.dp,
                )
            }
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun QRScannerViewPreview() {
    AvalancheTheme {
        // QRCodeCameraView requires a real camera; the preview just renders the scaffold.
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color.Black),
        ) {
            Column(
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .padding(horizontal = 24.dp)
                    .padding(bottom = 48.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    text = "Point your camera at an Avalanche invite QR code",
                    color = Color.White,
                    textAlign = TextAlign.Center,
                    modifier = Modifier
                        .background(
                            color = Color.Black.copy(alpha = 0.5f),
                            shape = RoundedCornerShape(12.dp),
                        )
                        .padding(12.dp),
                )
            }
        }
    }
}
