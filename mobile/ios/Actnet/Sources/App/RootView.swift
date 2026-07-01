import SwiftUI
import UIKit

struct RootView: View {
    @EnvironmentObject var appState: AppState
    @State private var showLogViewer = false

    var body: some View {
        ZStack(alignment: .top) {
            Group {
                if appState.isOnboarding {
                    SplashView()
                } else {
                    MainTabView()
                }
            }
            .background(Color.avPaper)

            OfflineBanner()
        }
        .background(
            // Installs a window-level two-finger triple-tap recognizer.
            TwoFingerTripleTapInstaller { showLogViewer = true }
                .frame(width: 0, height: 0)
        )
        .sheet(isPresented: $showLogViewer) {
            LogViewerView()
        }
        // An image shared in from another app (docs/35): pick a destination chat.
        .sheet(item: $appState.pendingSharedImage) { pending in
            ShareDestinationView(image: pending)
                .environmentObject(appState)
        }
        // "Sign in with Avalanche" consent (docs/25).
        .sheet(item: $appState.pendingLoginRequest) { req in
            ProjectLoginConsentView(request: req)
                .environmentObject(appState)
        }
        .alert(
            "Can’t sign in",
            isPresented: Binding(
                get: { appState.loginError != nil },
                set: { if !$0 { appState.loginError = nil } }
            ),
            presenting: appState.loginError
        ) { _ in
            Button("OK", role: .cancel) { appState.loginError = nil }
        } message: { error in
            switch error {
            case .noAccountOnServer(let server):
                Text("You don’t have an account on \(URL(string: server)?.host ?? server). Join that server, then try signing in again.")
            case .failed(let message):
                Text(message)
            }
        }
    }
}

/// UIKit bridge that attaches a two-finger triple-tap gesture recognizer to
/// the host window so it observes touches anywhere in the app without
/// interfering with normal hit testing.
private struct TwoFingerTripleTapInstaller: UIViewRepresentable {
    let onTrigger: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(onTrigger: onTrigger) }

    func makeUIView(context: Context) -> UIView {
        let view = AttachOnMoveView()
        view.onMove = { [weak view] in
            guard let window = view?.window, context.coordinator.installedOn !== window else { return }
            let gr = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.handle))
            gr.numberOfTapsRequired = 3
            gr.numberOfTouchesRequired = 2
            gr.cancelsTouchesInView = false
            gr.delegate = context.coordinator
            window.addGestureRecognizer(gr)
            context.coordinator.installedOn = window
        }
        return view
    }

    func updateUIView(_ uiView: UIView, context: Context) {}

    final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        let onTrigger: () -> Void
        weak var installedOn: UIWindow?
        init(onTrigger: @escaping () -> Void) { self.onTrigger = onTrigger }
        @objc func handle() { onTrigger() }
        func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool { true }
    }

    final class AttachOnMoveView: UIView {
        var onMove: (() -> Void)?
        override func didMoveToWindow() {
            super.didMoveToWindow()
            onMove?()
        }
    }
}
