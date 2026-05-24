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
        webView.load(URLRequest(url: url))
        return webView
    }

    func updateUIView(_ uiView: WKWebView, context: Context) {}

    /// Intercepts navigations to `go.theavalanche.net` and routes them as deep links.
    class Coordinator: NSObject, WKNavigationDelegate {
        let onDeepLink: ((URL) -> Void)?

        init(onDeepLink: ((URL) -> Void)?) {
            self.onDeepLink = onDeepLink
        }

        func webView(
            _ webView: WKWebView,
            decidePolicyFor navigationAction: WKNavigationAction,
            decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
        ) {
            if let url = navigationAction.request.url, AppState.isDeepLink(url) {
                print("[WebView] intercepted deep link: \(url)")
                onDeepLink?(url)
                decisionHandler(.cancel)
                return
            }
            decisionHandler(.allow)
        }
    }
}
