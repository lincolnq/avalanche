import SwiftUI
import WebKit

/// A webview that opens a Project URL with visible chrome (header bar).
/// The user always knows they're in a Project view, not native UI.
struct ProjectWebView: View {
    let projectName: String
    let url: URL
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            WebViewRepresentable(url: url)
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

    func makeUIView(context: Context) -> WKWebView {
        let webView = WKWebView()
        webView.load(URLRequest(url: url))
        return webView
    }

    func updateUIView(_ uiView: WKWebView, context: Context) {}
}
