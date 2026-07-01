# Avalanche Desktop

The Avalanche desktop app — encrypted messaging + organizing tools, built with [Tauri 2.x](https://tauri.app/) and [Solid](https://www.solidjs.com/).

## Prerequisites

All of these must be on your `PATH`:

| Tool | Why |
|------|-----|
| **Node.js >= 26** | Frontend runtime (pinned to 26.3.0 in `node/.node-version`) |
| **Rust** (stable) | Backend (Tauri commands link against `app-core`) |
| **Docker** | Postgres via `docker compose` |
| **Python 3.11+** (as `python3`) | Dev scripts (`dev.py`, `dev-invite.py`) |
| **make** | Build orchestration |
| **fnm** | Node version manager (dev.py invokes it to run testbot/adminbot) |
| **WebView2** | Windows only — included with Windows 11; on 10 install the [evergreen runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) |

**Windows note:** you need `python3` on PATH. If only `python` is installed, copy
`C:\Python314\python.exe` to `C:\Python314\python3.exe`, or put a shim at
`~/.cargo/bin/python3` that does `exec python "$@"`.

## Quick start

```bash
cd desktop
npm ci
make dev-all          # from repo root — starts Postgres, homeserver, testbot, adminbot
npm run tauri dev     # opens the Tauri window
```

Server at `localhost:3000`, testbot at `localhost:3001`, desktop at `localhost:1420`.

## Signing up

```bash
make dev-invite       # prints an invite link — paste it on the onboarding screen
```

## Testbot AI

The testbot echoes by default. To get real AI responses, add credentials to `.env` at the repo root (any Anthropic-compatible endpoint works). Restart `make dev-all` after editing.
