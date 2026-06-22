# Avalanche Desktop — Dev Setup

This is the Tauri desktop app (SolidJS frontend + Rust backend). It currently
runs in **Mock mode** — no real server, no real crypto. All data is seeded
in-memory so you can demo the UI without a running homeserver.

---

## Prerequisites

### All platforms

| Tool | Version | Install |
|------|---------|---------|
| **Rust** | stable (1.75+) | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| **Node.js** | 18+ | [nodejs.org](https://nodejs.org) or `nvm install --lts` |
| **npm** | 9+ | bundled with Node.js |

### Windows (additional)

- **Visual Studio Build Tools 2022** with the "Desktop development with C++"
  workload — needed by Rust's MSVC toolchain and `tauri-winres` (embeds the
  app icon into the `.exe`).
- **WebView2** — pre-installed on Windows 10 (Nov 2020+) and Windows 11.
  If missing: [download from Microsoft](https://developer.microsoft.com/en-us/microsoft-edge/webview2/).

### macOS (additional)

- **Xcode Command Line Tools**: `xcode-select --install`
- No extra system libraries needed; WebKit ships with macOS.

### Linux (additional, Ubuntu/Debian)

```bash
sudo apt update && sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  libssl-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf
```

For other distros see the [Tauri Linux prerequisites](https://tauri.app/start/prerequisites/#linux).

---

## Running the dev build

```bash
# From the repo root
cd desktop
npm install        # first time only
npm run tauri dev  # starts Vite + Rust dev server, opens the window
```

The first `npm run tauri dev` compiles the full Rust workspace (~5–15 min
depending on your machine). Subsequent runs are incremental and start in
seconds.

---

## What you'll see

The app opens in **Mock mode** — no server required:

1. **Splash screen** — click "Enter Invite Link" and paste any string (it's
   ignored; the mock creates a local account automatically).
2. **Chat list** — three seeded conversations: General, Announcements (group
   chats), and a DM with Jamie (Organizer).
3. **Messaging** — type a message and press Enter. The other participant
   echoes it back after ~1 second so you can see the full send/receive flow.
4. **Sign out** — button at the bottom of the sidebar returns to the splash
   screen.

---

## Platform notes

| Platform | Status | Notes |
|----------|--------|-------|
| **Windows** | ✅ Tested | Requires MSVC + WebView2 (see above) |
| **macOS** | ✅ Should work | Requires Xcode Command Line Tools |
| **Linux** | ✅ Should work | Install WebKit/GTK deps above |

Icons for all platforms (`.ico`, `.icns`, `.png`) are committed in
`src-tauri/icons/` and were generated from `design/app-icon-1024.png` via
`npx tauri icon`. Regenerate them if the source image changes.

---

## Switching to real-server mode

Mock mode is the default. To point at a real homeserver, open
`src/state/AppContext.tsx` and change both `ServiceMode.Mock` occurrences to
`ServiceMode.DevServer`, then restart. The Tauri commands in
`src-tauri/src/lib.rs` are currently stubs and will need implementing first.
