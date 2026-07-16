import { commands } from "../../bindings";
import type { ProjectInfoFfi } from "../../services/AvalancheService";

/**
 * Whether a project URL is safe to load in a webview window (T67 hardening).
 * This is a *scheme* check only: it guards against a malicious/compromised
 * project-directory entry opening a `javascript:`, `file:`, or `data:` window
 * in a Tauri webview (a project entry is server-supplied). `https:` and `http:`
 * are both allowed, with no host restriction.
 *
 * We deliberately don't restrict the host: iOS/Android impose no allowlist and
 * load whatever `project.url` is, and a loopback-only `http:` rule broke
 * legitimate local-dev transports (e.g. a laptop's Tailscale URL). Desktop keeps
 * the scheme gate that mobile gets for free from WKWebView/WebView sandboxing —
 * on Tauri a `file:` URL could read the local filesystem. Allowing `http:` to
 * any host does mean the `?token=` can traverse plaintext to a remote host; that
 * trade-off is tracked in docs/02-todos-deferred.md.
 */
export function isAllowedProjectUrl(raw: string): boolean {
  let parsed: URL;
  try {
    parsed = new URL(raw);
  } catch {
    return false;
  }
  return parsed.protocol === "https:" || parsed.protocol === "http:";
}

/**
 * Open a project URL with an auth token in an isolated Tauri webview window.
 * Returns true if the window was created successfully.
 *
 * The window is created on the Rust side (`commands.openProjectWindow`) rather
 * than here with `new WebviewWindow`, because it installs a Rust `on_navigation`
 * handler that intercepts in-webview clicks on our deep links
 * (`avalanche://…` / `go.theavalanche.net/{conversation,i,invite}/…`) and routes
 * them to the main app's `handleDeepLink` (parity with iOS/Android). A
 * navigation handler is a Rust closure JS cannot supply. The token is appended
 * as a query param there. See src-tauri/src/lib.rs `open_project_window`.
 *
 * Security: the Rust side gives the window a non-`main` label so the capability
 * ACL denies it app-core IPC (isolation invariant, desktop/CLAUDE.md).
 */
export async function openProjectWindow(
  project: ProjectInfoFfi,
  token: string
): Promise<boolean> {
  // Front-line scheme gate (Rust re-checks). A project entry is server-supplied,
  // so a hostile/buggy one must not open a `javascript:`/`file:`/`data:` webview.
  if (!isAllowedProjectUrl(project.url)) {
    console.error("Refusing to open project with unsafe URL:", project.url);
    return false;
  }

  try {
    const res = await commands.openProjectWindow(project.url, token, project.name);
    if (res.status === "error") {
      console.error("Failed to open project window:", res.error);
      return false;
    }
    return true;
  } catch (e) {
    console.error("Failed to open project window:", e);
    return false;
  }
}
