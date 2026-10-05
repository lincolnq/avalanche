// Desktop auto-update (docs/63). Checks shortly after launch and every few
// hours, downloads a newer signed release in the background, and waits for the
// user to choose "Restart to update" — the app never restarts on its own.
// Signature verification happens in the updater plugin; a bad signature is an
// error here and the package is discarded.
//
// Release builds only: dev builds don't register the updater plugin (lib.rs),
// and the browser preview has no Tauri at all.
import { createRoot, createSignal } from "solid-js";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { isBrowserPreview } from "../dev/browserPreview";

export type UpdateStatus =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate" }
  | { kind: "downloading"; version: string; percent: number | null }
  | { kind: "ready"; version: string }
  | { kind: "installing"; version: string }
  | { kind: "error"; message: string };

const FIRST_CHECK_MS = 10_000;
const CHECK_INTERVAL_MS = 4 * 60 * 60 * 1000;

export const updatesEnabled = !import.meta.env.DEV && !isBrowserPreview;

const { status, setStatus } = createRoot(() => {
  const [status, setStatus] = createSignal<UpdateStatus>({ kind: "idle" });
  return { status, setStatus };
});
export { status as updateStatus };

/** Dev-only: force a status to see the UI states (window.__av.setUpdateStatus). */
export const setUpdateStatusForDev = (s: UpdateStatus) => {
  if (import.meta.env.DEV) setStatus(s);
};

let pending: Update | null = null;
let started = false;

/** Check now; download in the background if there's a newer version. */
export async function checkForUpdates(): Promise<void> {
  if (!updatesEnabled) {
    setStatus({ kind: "error", message: "Updates are off in development builds." });
    return;
  }
  const s = status().kind;
  if (s === "checking" || s === "downloading" || s === "ready" || s === "installing") return;
  setStatus({ kind: "checking" });
  try {
    const update = await check();
    if (!update) {
      setStatus({ kind: "upToDate" });
      return;
    }
    let total = 0;
    let received = 0;
    setStatus({ kind: "downloading", version: update.version, percent: null });
    await update.download((ev) => {
      if (ev.event === "Started") total = ev.data.contentLength ?? 0;
      else if (ev.event === "Progress") {
        received += ev.data.chunkLength;
        setStatus({
          kind: "downloading",
          version: update.version,
          percent: total ? Math.min(100, Math.round((received / total) * 100)) : null,
        });
      }
    });
    pending = update;
    setStatus({ kind: "ready", version: update.version });
  } catch (e) {
    // Quiet: logged and shown in Settings → About, retried at the next interval.
    console.warn("update check failed:", e);
    setStatus({ kind: "error", message: e instanceof Error ? e.message : String(e) });
  }
}

/** Install the downloaded update and relaunch (the user's explicit choice). */
export async function installAndRestart(): Promise<void> {
  if (!pending) return;
  setStatus({ kind: "installing", version: pending.version });
  try {
    await pending.install();
    await relaunch();
  } catch (e) {
    console.warn("update install failed:", e);
    setStatus({ kind: "error", message: e instanceof Error ? e.message : String(e) });
  }
}

/** Start the background schedule once (call at app start). No-op in dev. */
export function startAutoUpdates(): void {
  if (started || !updatesEnabled) return;
  started = true;
  setTimeout(() => void checkForUpdates(), FIRST_CHECK_MS);
  setInterval(() => void checkForUpdates(), CHECK_INTERVAL_MS);
}
