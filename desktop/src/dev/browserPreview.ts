// Dev-only browser preview: when the dev frontend (vite, localhost:1420) is
// opened in a plain browser instead of the Tauri webview, run it against the
// MockAvalancheService with a canned signed-in account, so the UI can be
// rendered and screenshotted headlessly (`scripts/preview-shot.mjs`) without a
// running app, server, or visible window. Never true in production builds or
// inside Tauri.
export const isBrowserPreview: boolean =
  import.meta.env.DEV && typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);

const KEY = "avalanche-preview-store";

// Stand-in for the plugin-store file (avalanche.json) in browser preview.
// Starts with one mock account so the app opens straight into Chats.
// `?accounts=N` seeds N mock accounts (multi-account UI, e.g. account tabs).
export function previewStoreGet<T>(key: string): T | undefined {
  const n = Number(new URLSearchParams(location.search).get("accounts") ?? "1") || 1;
  const names = ["Preview User", "Second Identity", "Third Identity"];
  const all = JSON.parse(localStorage.getItem(KEY) ?? "null") ?? {
    accounts: names.slice(0, n).map((displayName, i) => ({
      did: "",
      displayName,
      dbPath: `preview-${i}.db`,
      servers: [{ id: "https://mock.avalancheapp.net", name: "Mock Server", url: "https://mock.avalancheapp.net" }],
    })),
  };
  return all[key] as T | undefined;
}

export function previewStoreSet(key: string, value: unknown): void {
  const all = JSON.parse(localStorage.getItem(KEY) ?? "{}");
  all[key] = value;
  localStorage.setItem(KEY, JSON.stringify(all));
}
