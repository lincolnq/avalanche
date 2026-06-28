package net.theavalanche.app

import android.net.Uri
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Language
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * A WebView that opens a Project URL with visible chrome (header bar).
 * The user always knows they're in a Project view, not native UI.
 *
 * Mirrors iOS ProjectWebView.swift.
 *
 * Navigation: the caller passes [onDismiss] (close button / back) and
 * [onDeepLink] (intercepted go.theavalanche.net URL). NavGraph wires them.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ProjectWebView(
    projectName: String,
    url: String,
    onDismiss: () -> Unit = {},
    onDeepLink: (Uri) -> Unit = {},
    appViewModel: AppViewModel = viewModel(),
) {
    val scope = rememberCoroutineScope()
    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    // Principal: globe icon + project name — mirrors iOS .principal ToolbarItem
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(
                            imageVector = Icons.Filled.Language,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.6f),
                        )
                        Spacer(Modifier.width(4.dp))
                        Text(
                            text = projectName,
                            style = MaterialTheme.typography.titleMedium,
                        )
                    }
                },
                navigationIcon = {
                    // Leading "Close" button — mirrors iOS .topBarLeading ToolbarItem
                    TextButton(onClick = onDismiss) {
                        Text("Close")
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = LocalAvalancheColors.current.paper,
                    titleContentColor = LocalAvalancheColors.current.ink,
                ),
            )
        },
        containerColor = LocalAvalancheColors.current.paper,
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding),
        ) {
            WebViewRepresentable(
                url = url,
                modifier = Modifier.fillMaxSize(),
                onDeepLink = { deepLinkUri ->
                    android.util.Log.d("ProjectWebView", "onDeepLink called: $deepLinkUri")
                    onDismiss()
                    // Delay slightly so the sheet dismissal completes before navigation.
                    // The coroutine runs on the composition scope (main dispatcher) and is
                    // cancelled automatically if this composable leaves the tree.
                    scope.launch {
                        delay(300L)
                        onDeepLink(deepLinkUri)
                    }
                },
            )
        }
    }
}

/**
 * AndroidView wrapper around [android.webkit.WebView] that intercepts navigations to
 * `go.theavalanche.net` and routes them as deep links.
 *
 * Mirrors iOS WebViewRepresentable + Coordinator.
 */
@Composable
fun WebViewRepresentable(
    url: String,
    modifier: Modifier = Modifier,
    onDeepLink: ((Uri) -> Unit)? = null,
) {
    val context = LocalContext.current

    // Capture the callback in a remembered lambda so the WebViewClient doesn't
    // capture a stale closure reference if recomposition changes it.
    val deepLinkCallback = remember(onDeepLink) { onDeepLink }

    AndroidView(
        factory = { ctx ->
            WebView(ctx).apply {
                android.util.Log.d("WebView", "loading $url")
                // JavaScript + DOM storage are required by the Project web app.
                // Mixed (http-in-https) content is intentionally left at its secure
                // default (NEVER_ALLOW) — Projects are served over https.
                settings.javaScriptEnabled = true
                settings.domStorageEnabled = true

                webViewClient = object : WebViewClient() {

                    override fun shouldOverrideUrlLoading(
                        view: WebView,
                        request: WebResourceRequest,
                    ): Boolean {
                        val requestUrl = request.url
                        android.util.Log.d(
                            "WebView",
                            "shouldOverrideUrlLoading: url=${requestUrl} host=${requestUrl?.host}"
                        )
                        if (requestUrl != null && isDeepLink(requestUrl)) {
                            android.util.Log.d("WebView", "intercepted deep link: $requestUrl")
                            deepLinkCallback?.invoke(requestUrl)
                            return true // cancel the WebView navigation
                        }
                        return false // allow
                    }

                    override fun onPageStarted(
                        view: WebView,
                        pageUrl: String?,
                        favicon: android.graphics.Bitmap?,
                    ) {
                        android.util.Log.d("WebView", "onPageStarted: $pageUrl")
                    }

                    override fun onPageFinished(view: WebView, pageUrl: String?) {
                        android.util.Log.d("WebView", "onPageFinished: $pageUrl")
                    }

                    override fun onReceivedError(
                        view: WebView,
                        request: WebResourceRequest,
                        error: android.webkit.WebResourceError,
                    ) {
                        android.util.Log.d(
                            "WebView",
                            "onReceivedError: ${error.description} for ${request.url}"
                        )
                    }
                }

                loadUrl(url)
            }
        },
        modifier = modifier,
    )
}

/**
 * Returns true if [uri] is a deep link for this app (`go.theavalanche.net`).
 * Mirrors iOS AppState.isDeepLink(_:).
 */
private fun isDeepLink(uri: Uri): Boolean = uri.host == "go.theavalanche.net"

@Preview(showBackground = true)
@Composable
private fun ProjectWebViewPreview() {
    AvalancheTheme {
        ProjectWebView(
            projectName = "Example Project",
            url = "https://example.com",
            onDismiss = {},
            onDeepLink = {},
        )
    }
}
