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

/// Intercepts `actnet://` URLs so WKWebView recognizes the scheme.
/// Without this, WKWebView silently drops navigation to unknown schemes.
class DeepLinkSchemeHandler: NSObject, WKURLSchemeHandler {
    let onDeepLink: ((URL) -> Void)?

    init(onDeepLink: ((URL) -> Void)?) {
        self.onDeepLink = onDeepLink
    }

    func webView(_ webView: WKWebView, start urlSchemeTask: any WKURLSchemeTask) {
        let url = urlSchemeTask.request.url!
        print("[SchemeHandler] intercepted: \(url)")
        onDeepLink?(url)
        // Fail the load — we don't want to actually render anything.
        urlSchemeTask.didFailWithError(URLError(.cancelled))
    }

    func webView(_ webView: WKWebView, stop urlSchemeTask: any WKURLSchemeTask) {}
}

struct WebViewRepresentable: UIViewRepresentable {
    let url: URL
    var onDeepLink: ((URL) -> Void)?

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        let handler = DeepLinkSchemeHandler(onDeepLink: onDeepLink)
        // Keep a strong reference so it isn't deallocated.
        context.coordinator.schemeHandler = handler
        config.setURLSchemeHandler(handler, forURLScheme: "actnet")

        let webView = WKWebView(frame: .zero, configuration: config)
        webView.load(URLRequest(url: url))
        return webView
    }

    func updateUIView(_ uiView: WKWebView, context: Context) {}

    class Coordinator: NSObject {
        var schemeHandler: DeepLinkSchemeHandler?
    }
}
