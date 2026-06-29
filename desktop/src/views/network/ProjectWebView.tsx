import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { ProjectInfoFfi } from "../../services/AvalancheService";

/**
 * Whether a project URL is safe to load in a webview window (T67 hardening).
 * Guards against a malicious/compromised project-directory entry opening a
 * `javascript:`, `file:`, or `data:` window, or plain-http remote content.
 * Requires `https:`, except `http:` is allowed for loopback hosts because local
 * dev projects run on `http://localhost:<port>` (see dev.py `project_host`).
 * iOS imposes no host allowlist — it loads whatever `project.url` is — so we
 * deliberately don't invent one here; this is purely a scheme/loopback check.
 */
export function isAllowedProjectUrl(raw: string): boolean {
  let parsed: URL;
  try {
    parsed = new URL(raw);
  } catch {
    return false;
  }
  if (parsed.protocol === "https:") return true;
  if (parsed.protocol === "http:") {
    const h = parsed.hostname;
    return h === "localhost" || h === "127.0.0.1" || h === "::1" || h === "[::1]";
  }
  return false;
}

/**
 * Open a project URL with an auth token in a Tauri WebviewWindow modal.
 * Returns true if the window was created successfully.
 */
export async function openProjectWindow(
  project: ProjectInfoFfi,
  token: string
): Promise<boolean> {
  // T67: reject anything that isn't https (or http-to-loopback for dev) before
  // creating a window — a project entry is server-supplied, so a hostile/buggy
  // one must not be able to open a `javascript:`/`file:`/`data:` webview.
  if (!isAllowedProjectUrl(project.url)) {
    console.error("Refusing to open project with unsafe URL:", project.url);
    return false;
  }

  const label = `project-${crypto.randomUUID().slice(0, 8)}`;
  // Pass the token as a query parameter, matching the iOS reference
  // (mobile/ios/.../NetworkView.swift) and the project interface contract: a
  // project that renders server-side needs the token in the request the server
  // sees, which a URL hash fragment is not.
  // TODO(security, cross-platform): a query-string token is written to the
  // project server's access logs, can leak via Referer, and persists in
  // history. A hash fragment avoids that but is invisible to server-rendered
  // projects, so changing it is a protocol-wide decision (all platforms + the
  // project contract) to make with the project owner — not a desktop-only
  // change. See docs/02-todos-deferred.md.
  const url = `${project.url}?token=${encodeURIComponent(token)}`;

  // TODO(Day 4): intercept navigation to go.theavalanche.net and close the
  // modal / emit a deeplink event.
  // Security: this ephemeral `project-*` window is IPC-isolated. The `allow-all`
  // commands are granted only to `windows: ["main"]` + local content
  // (src-tauri/capabilities/default.json); this window's label doesn't match and
  // its content is remote, so app-core IPC is denied. Verified at runtime:
  // `invoke('ping')` from a project window rejects with "ping not allowed on
  // window project-..., allowed on: [windows: main, URL: local]". The thing to
  // guard is that scope: broadening `allow-all` to a window glob or adding a
  // `remote` block would hand the native surface to remote content (see
  // src-tauri/permissions/avalanche.toml).

  return new Promise((resolve) => {
    try {
      const webview = new WebviewWindow(label, {
        url,
        title: project.name,
        width: 900,
        height: 700,
        resizable: true,
      });

      webview.once("tauri://created", () => resolve(true));
      webview.once("tauri://error", (e) => {
        console.error("Project webview creation error:", e);
        resolve(false);
      });
    } catch (e) {
      console.error("Failed to open project webview:", e);
      resolve(false);
    }
  });
}
