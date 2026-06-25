package net.theavalanche.app

import android.Manifest
import android.content.pm.PackageManager
import android.os.VibrationEffect
import android.os.Vibrator
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.compose.ui.tooling.preview.Preview as ComposePreview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import com.google.zxing.BinaryBitmap
import com.google.zxing.MultiFormatReader
import com.google.zxing.NotFoundException
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.common.HybridBinarizer
import java.util.concurrent.Executors

/// Live camera view that scans QR codes and reports the decoded string.
/// Calls [onScanned] once per appearance, then ignores further detections
/// until the composable leaves and re-enters the composition.
/// Mirrors mobile/ios/Actnet/Sources/Views/Common/QRCodeCameraView.swift.
@Composable
fun QRCodeCameraView(
    onScanned: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current

    // Track whether we've already fired onScanned this appearance — reset on
    // recomposition/re-entry, matching the iOS "once per appearance" contract.
    var didScan by remember { mutableStateOf(false) }
    var errorMessage by remember { mutableStateOf<String?>(null) }

    // CAMERA is a runtime ("dangerous") permission on Android — unlike iOS,
    // there is no auto-prompt on first camera access, so we must request it
    // explicitly before CameraX will bind. We gate here (rather than in
    // MainActivity) so every caller of QRCodeCameraView gets the prompt.
    var hasCameraPermission by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED,
        )
    }
    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted -> hasCameraPermission = granted }
    LaunchedEffect(Unit) {
        if (!hasCameraPermission) {
            permissionLauncher.launch(Manifest.permission.CAMERA)
        }
    }

    val cameraExecutor = remember { Executors.newSingleThreadExecutor() }
    DisposableEffect(Unit) {
        onDispose {
            cameraExecutor.shutdown()
        }
    }

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(Color.Black),
    ) {
        if (!hasCameraPermission) {
            Column(
                modifier = Modifier
                    .align(Alignment.Center)
                    .padding(horizontal = 24.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                Text(
                    text = "Camera access is needed to scan QR codes.",
                    color = Color.White,
                    textAlign = TextAlign.Center,
                )
                Button(onClick = { permissionLauncher.launch(Manifest.permission.CAMERA) }) {
                    Text("Allow camera access")
                }
            }
            return@Box
        }

        errorMessage?.let { msg ->
            Text(
                text = msg,
                color = Color.White,
                modifier = Modifier
                    .align(Alignment.Center)
                    .padding(horizontal = 24.dp),
            )
        } ?: AndroidView(
            factory = { ctx ->
                PreviewView(ctx).apply {
                    scaleType = PreviewView.ScaleType.FILL_CENTER
                }
            },
            modifier = Modifier.fillMaxSize(),
            update = { previewView ->
                val cameraProviderFuture = ProcessCameraProvider.getInstance(context)
                cameraProviderFuture.addListener({
                    val cameraProvider = cameraProviderFuture.get()

                    val preview = Preview.Builder().build().also {
                        it.surfaceProvider = previewView.surfaceProvider
                    }

                    val imageAnalysis = ImageAnalysis.Builder()
                        .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                        .build()

                    val reader = MultiFormatReader()

                    imageAnalysis.setAnalyzer(cameraExecutor) { imageProxy ->
                        if (!didScan) {
                            val result = tryDecodeQr(imageProxy, reader)
                            if (result != null) {
                                didScan = true
                                vibrateOnce(context)
                                // The analyzer runs on a background executor, but
                                // onScanned typically drives navigation / Compose
                                // state, which must touch the main thread. Hop back.
                                ContextCompat.getMainExecutor(context).execute {
                                    onScanned(result)
                                }
                            }
                        }
                        imageProxy.close()
                    }

                    try {
                        cameraProvider.unbindAll()
                        cameraProvider.bindToLifecycle(
                            lifecycleOwner,
                            CameraSelector.DEFAULT_BACK_CAMERA,
                            preview,
                            imageAnalysis,
                        )
                    } catch (e: Exception) {
                        errorMessage = "Camera not available"
                    }
                }, ContextCompat.getMainExecutor(context))
            },
        )
    }
}

/// Attempt to decode a QR code from a CameraX ImageProxy using ZXing.
/// Returns the decoded string, or null if no QR code was found.
private fun tryDecodeQr(
    imageProxy: androidx.camera.core.ImageProxy,
    reader: MultiFormatReader,
): String? {
    val plane = imageProxy.planes[0]
    val bytes = ByteArray(plane.buffer.remaining())
    plane.buffer.get(bytes)
    val source = PlanarYUVLuminanceSource(
        bytes,
        plane.rowStride,
        plane.rowStride,
        0,
        0,
        imageProxy.width,
        imageProxy.height,
        false,
    )
    val bitmap = BinaryBitmap(HybridBinarizer(source))
    return try {
        reader.decodeWithState(bitmap).text
    } catch (_: NotFoundException) {
        null
    } finally {
        reader.reset()
    }
}

/// Trigger a single short vibration — mirrors
/// AudioServicesPlaySystemSound(kSystemSoundID_Vibrate) on iOS.
@Suppress("DEPRECATION")
private fun vibrateOnce(context: android.content.Context) {
    val vibrator = context.getSystemService(android.content.Context.VIBRATOR_SERVICE) as? Vibrator
        ?: return
    if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.O) {
        vibrator.vibrate(VibrationEffect.createOneShot(100L, VibrationEffect.DEFAULT_AMPLITUDE))
    } else {
        vibrator.vibrate(100L)
    }
}

@ComposePreview(showBackground = true)
@Composable
private fun QRCodeCameraViewPreview() {
    AvalancheTheme {
        // Camera won't run in the preview host; show the placeholder layout.
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color.Black),
            contentAlignment = Alignment.Center,
        ) {
            Text("Camera preview unavailable in IDE", color = Color.White)
        }
    }
}
