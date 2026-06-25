package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
/// Validates the scanned URL and calls [onInviteToken] with a parsed
/// [InviteToken] once validation succeeds. The caller is responsible for
/// navigating onward (typically to IdentityPickerView). On Android this token
/// is reported outward — rather than navigated to internally as on iOS — because
/// the destination lives in the app's single NavHost, not in this composable.
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun QRScannerView(
    onInviteToken: (InviteToken) -> Unit,
    onBack: () -> Unit = {},
) {
    var errorMessage by remember { mutableStateOf<String?>(null) }
    var isValidating by remember { mutableStateOf(false) }

    val scope = rememberCoroutineScope()

    fun handle(value: String) {
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
            TopAppBar(
                title = { Text("Scan QR Code") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
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
