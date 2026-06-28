import UIKit
import UniformTypeIdentifiers
import os

/// Minimal share extension (docs/35): captures one shared image, re-encodes it to
/// JPEG, hands it to the main app via the App Group container, then opens the app
/// so the user picks a destination chat there. It runs NO app-core — no crypto,
/// DB, or network in the extension sandbox.
///
/// Launching the containing app from a share extension is officially unsupported
/// by Apple (only Today widgets may, via NSExtensionContext). The responder-chain
/// `openURL` technique below is the widely-shipped workaround (Bluesky, etc.). Two
/// hard-won details: (1) it must use the NON-deprecated
/// `openURL:options:completionHandler:` selector — iOS 18 force-fails the old
/// `openURL:` — and (2) it works on a real device but is a no-op on the Simulator.
/// The App Group write is the reliable part; the foreground check in AppState is a
/// safety net if the open doesn't land.
class ShareViewController: UIViewController {

    private let log = Logger(subsystem: "net.theavalanche.app", category: "share")

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .clear
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        log.log("share ext: viewDidAppear, handling share")
        handleShare()
    }

    private func handleShare() {
        guard let item = (extensionContext?.inputItems as? [NSExtensionItem])?.first,
              let provider = item.attachments?.first(where: {
                  $0.hasItemConformingToTypeIdentifier(UTType.image.identifier)
              }) else {
            log.error("share ext: no image attachment found in input items")
            finish()
            return
        }

        log.log("share ext: found image provider, loading UIImage")
        // Load as a UIImage and re-encode to JPEG so the bytes are always a format
        // the recipient can render (incoming shares may be HEIC/PNG/etc.) — matches
        // the photo-picker / paste paths, which the app uploads as image/jpeg.
        provider.loadObject(ofClass: UIImage.self) { [weak self] object, error in
            guard let self else { return }
            if let error {
                self.log.error("share ext: loadObject failed: \(error.localizedDescription, privacy: .public)")
            }
            if let image = object as? UIImage,
               let data = image.preparedForSending() {
                let ok = AppGroup.writePendingShare(data: data, contentType: "image/jpeg")
                self.log.log("share ext: wrote pending share (\(data.count) bytes), success=\(ok)")
            } else {
                self.log.error("share ext: could not derive JPEG data from shared image")
            }
            DispatchQueue.main.async { self.openHostApp() }
        }
    }

    /// Foreground the containing app via its custom URL scheme. Walks the responder
    /// chain for the `UIApplication` and invokes the non-deprecated
    /// `openURL:options:completionHandler:` through the Obj-C runtime (the typed
    /// `UIApplication.open(_:options:completionHandler:)` is compile-blocked in an
    /// app extension, so we call it via its IMP).
    private func openHostApp() {
        let url = URL(string: "\(AppGroup.shareURLScheme)://shared")! as NSURL
        let selector = NSSelectorFromString("openURL:options:completionHandler:")
        var responder: UIResponder? = self
        var invoked = false
        while let current = responder {
            if let application = current as? UIApplication, application.responds(to: selector) {
                typealias OpenURLFn = @convention(c) (NSObject, Selector, NSURL, NSDictionary, Any?) -> Void
                let imp = application.method(for: selector)
                let callable = unsafeBitCast(imp, to: OpenURLFn.self)
                callable(application, selector, url, NSDictionary(), nil)
                invoked = true
                self.log.log("share ext: invoked openURL:options:completionHandler: on UIApplication")
                break
            }
            responder = current.next
        }
        if !invoked {
            self.log.error("share ext: no UIApplication in responder chain (expected on Simulator)")
        }
        // Defer completeRequest so the open has a chance to take effect before the
        // extension tears down.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { [weak self] in
            self?.finish()
        }
    }

    private func finish() {
        log.log("share ext: completing request")
        extensionContext?.completeRequest(returningItems: nil, completionHandler: nil)
    }
}
