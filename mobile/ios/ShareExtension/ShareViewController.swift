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

        // Copy the image bytes WITHOUT decoding them. Loading the image as a
        // UIImage (and re-rendering it) allocates an uncompressed bitmap — a
        // 24–48 MP iPhone photo is ~96–192 MB — which blows the share extension's
        // ~120 MB memory ceiling and gets the process jetsam-killed before the
        // completion handler even runs (docs/35). `loadDataRepresentation` just
        // reads the already-encoded file bytes (a few MB). The main app decodes,
        // resizes, and strips metadata on staging (`prepareImageForSending`),
        // where there is a real memory budget.
        //
        // Prefer JPEG (Photos exports a compatible JPEG), then other common image
        // encodings; the app re-encodes to JPEG regardless, so the content type is
        // informational. `.image` is the last-resort catch-all.
        let preferredTypes: [UTType] = [.jpeg, .png, .heic, .image]
        let typeId = (preferredTypes.first {
            provider.hasItemConformingToTypeIdentifier($0.identifier)
        } ?? .image).identifier
        let contentType = UTType(typeId)?.preferredMIMEType ?? "image/jpeg"

        log.log("share ext: loading data representation for \(typeId, privacy: .public)")
        provider.loadDataRepresentation(forTypeIdentifier: typeId) { [weak self] data, error in
            guard let self else { return }
            if let error {
                self.log.error("share ext: loadDataRepresentation failed: \(error.localizedDescription, privacy: .public)")
            }
            if let data {
                let ok = AppGroup.writePendingShare(data: data, contentType: contentType)
                self.log.log("share ext: wrote pending share (\(data.count) bytes, \(contentType, privacy: .public)), success=\(ok)")
            } else {
                self.log.error("share ext: no data for shared image")
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
