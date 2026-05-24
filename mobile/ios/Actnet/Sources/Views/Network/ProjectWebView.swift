import SwiftUI
import WebKit

/// A webview that opens a Project URL with visible chrome (header bar).
/// The user always knows they're in a Project view, not native UI.
struct ProjectWebView: View {
    let projectName: String
    let url: URL
    @EnvironmentObject var appState: AppState
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            WebViewRepresentable(url: url) { deepLinkURL in
                print("[ProjectWebView] onDeepLink called: \(deepLinkURL)")
                dismiss()
                // Delay slightly so the sheet dismissal completes before navigation.
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                    appState.handleDeepLink(deepLinkURL)
                }
            }
            .navigationTitle(projectName)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Close") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .principal) {
                    HStack(spacing: 4) {
                        Image(systemName: "globe")
                            .foregroundStyle(.secondary)
                        Text(projectName)
                            .font(.headline)
                    }
                }
            }
        }
    }
}

struct WebViewRepresentable: UIViewRepresentable {
    let url: URL
    var onDeepLink: ((URL) -> Void)?

    func makeCoordinator() -> Coordinator {
        Coordinator(onDeepLink: onDeepLink)
    }

    func makeUIView(context: Context) -> WKWebView {
        let webView = WKWebView(frame: .zero)
        webView.navigationDelegate = context.coordinator
        webView.uiDelegate = context.coordinator
        webView.load(URLRequest(url: url))
        return webView
    }

    func updateUIView(_ uiView: WKWebView, context: Context) {}

    /// Intercepts navigations to `go.theavalanche.net` and routes them as deep links.
    class Coordinator: NSObject, WKNavigationDelegate, WKUIDelegate {
        let onDeepLink: ((URL) -> Void)?

        init(onDeepLink: ((URL) -> Void)?) {
            self.onDeepLink = onDeepLink
        }

        // iOS 13+ preferred policy method. When both this and the older
        // `decidePolicyFor:decisionHandler:` are implemented, only this one
        // is invoked. The older variant is unreliable as a fallback on
        // recent iOS — implement this one to actually get called.
        @MainActor
        func webView(
            _ webView: WKWebView,
            decidePolicyFor navigationAction: WKNavigationAction,
            preferences: WKWebpagePreferences,
            decisionHandler: @escaping @MainActor (WKNavigationActionPolicy, WKWebpagePreferences) -> Void
        ) {
            let url = navigationAction.request.url
            print("[WebView] decidePolicyFor(prefs): url=\(url?.absoluteString ?? "nil") host=\(url?.host ?? "nil") type=\(navigationAction.navigationType.rawValue) targetFrame=\(navigationAction.targetFrame == nil ? "nil" : "main")")
            if let url, AppState.isDeepLink(url) {
                print("[WebView] intercepted deep link: \(url)")
                onDeepLink?(url)
                decisionHandler(.cancel, preferences)
                return
            }
            decisionHandler(.allow, preferences)
        }

// Some JS-initiated navigations (window.open, target=_blank, certain
        // cross-origin redirects) hit this method instead of decidePolicyFor
        // with a non-nil targetFrame. Log it so we can spot if that's the path.
        func webView(
            _ webView: WKWebView,
            createWebViewWith configuration: WKWebViewConfiguration,
            for navigationAction: WKNavigationAction,
            windowFeatures: WKWindowFeatures
        ) -> WKWebView? {
            let url = navigationAction.request.url
            print("[WebView] createWebViewWith: url=\(url?.absoluteString ?? "nil") host=\(url?.host ?? "nil")")
            if let url, AppState.isDeepLink(url) {
                onDeepLink?(url)
            }
            return nil
        }

        func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
            print("[WebView] didStartProvisionalNavigation: \(webView.url?.absoluteString ?? "nil")")
        }

        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
            print("[WebView] didFailProvisionalNavigation: \(error.localizedDescription)")
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            print("[WebView] didFinish: \(webView.url?.absoluteString ?? "nil")")
        }
    }
}
