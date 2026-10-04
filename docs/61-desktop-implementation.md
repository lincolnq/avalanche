# 61 — Desktop implementation notes

> **Status:** Partial — the Tauri desktop app covers messaging, groups, contacts, device
> linking, and multi-account. It has no passkeys (by design), no Project login, and **its
> SQLCipher databases use a constant placeholder key**. Feature parity is tracked in docs/62.
> **Last verified against code:** 2026-10-03

## Summary

The desktop app (`desktop/`) is a Tauri 2 shell: a Solid/TypeScript frontend in the OS
webview, and a thin Rust layer (`desktop/src-tauri`) that links `app-core` directly and
exposes it as Tauri commands. iOS is the reference for behavior. This doc records how the
desktop app is built, why the stack was chosen, and its known divergences. **For which
features exist where, see docs/62** — this doc has no parity table.

## Current design

### Architecture

There is no main/renderer process split. Two layers:

- **Rust backend** (`src-tauri/src/lib.rs`): ~90 hand-written `#[tauri::command]` functions
  over `app-core`, a `Mutex<HashMap<String, Arc<AppCore>>>` keyed by account DID (one core per
  signed-in account), and `tauri-plugin-store` for app metadata. Plugins: store, deep-link,
  notification, single-instance.
- **Solid frontend** (`src/`): calls Rust through the typed `commands.*` in
  `src/bindings.ts`, which tauri-specta generates from the command surface (`make
  desktop-bindings`; CI fails if it drifts).

Call path: `Solid → bindings.ts → invoke() → src-tauri/src/lib.rs → app-core`.

**TS owns the event loop.** The frontend runs one `nextEvents()` loop and one
connection-state loop per signed-in account (`state/createEventLoops.ts`); the Rust side never
spawns its own loop or emits events. See `desktop/CLAUDE.md` for the invariants.

### Source layout

```
desktop/src/
  App.tsx, index.tsx, bindings.ts (generated)
  state/        AppContext.tsx + per-concern factories (createAccounts, createConversations,
                createMessaging, createGroupsAndSafety, createDeviceLink, createEventLoops, …)
  services/     AvalancheService (interface), DevServerAvalancheService, MockAvalancheService
  models/       Account, Conversation, Message, InviteToken, ProjectInfo
  views/        chats/, network/, onboarding/, settings/, common/MainLayout.tsx
  components/   ConversationRow, MessageBubble, ComposeMessageView, GroupDetailView, QRCode, …
  lib/, styles/
desktop/src-tauri/  lib.rs (commands), tauri.conf.json (CSP, capabilities, deep links)
```

### UX adaptation

Desktop replaces iOS's bottom tab bar with a **left sidebar** (`views/common/MainLayout.tsx`),
the standard desktop-messenger pattern. Content is otherwise mapped 1:1 from iOS. Project
pages open in a separate `WebviewWindow` (label `project-*`) that is IPC-isolated by
capability scope (`desktop/CLAUDE.md`, "Security constraints").

### Accounts and recovery: the phrase is the credential (sanctioned divergence)

Desktop has no WebAuthn/PRF authenticator path, so **every desktop signup uses a 12-word
recovery phrase as the PRF stand-in**: the phrase's seed derives the PLC rotation key and the
DID, and restores the account later. Details and load-bearing invariants are in
`desktop/CLAUDE.md` ("Passkey / recovery divergence"). The external-browser passkey design
(docs/56) is the proposed way to add passkeys.

## Known gaps

- **Placeholder database key (security).** Desktop opens SQLCipher with the constant string
  `"dev-placeholder-key"` (`state/createAccounts.ts`, `state/createDeviceLink.ts`). The
  identity DB — including the identity key and the persisted DID rotation key — is
  effectively unencrypted at rest against anyone who can read the app-data directory. Fix:
  generate a random key per install and store it in the OS credential store (macOS Keychain,
  Windows Credential Manager/DPAPI, Linux Secret Service). See docs/09.
- **The Tauri bridge does not compile against current `app-core`.** `next_events` and
  `wait_for_connection_state_change` became native async exports, but
  `src-tauri/src/lib.rs` (`next_events`, `wait_for_connection_state_change` commands) still
  calls them inside `spawn_blocking` as if synchronous (`cargo check` fails with E0599). They
  should simply be awaited.
- **No Project login** ("Sign in with Avalanche", docs/25): desktop can't act as the
  authorizer, though desktop *users* can authorize from their phone.
- **No avatar setting, account tabs, or conversation search** (iOS and Android have them).
- **No QR scanning** — invite entry is paste-a-link only. (QR *display* exists.)
- **Metadata in a plain JSON store.** `tauri-plugin-store` keeps the identity list (own DIDs,
  display names, server URLs, DB filenames) unencrypted in the OS app-data directory, readable
  by any process running as the user. Lower priority than the DB key; same fix (a keychain-
  backed key for a small `manifest.db`).
- `src-tauri/src/lib.rs` (~1,900 lines) is one file; split by domain when it next grows.

## Rationale and rejected alternatives

### Tauri over Electron

- **Security patch ownership.** Electron bundles a specific Chromium; a CVE fix needs an
  Electron release and then a user update. Tauri uses the OS webview (WebView2, WebKit,
  webkit2gtk), patched by the OS vendor independently of us.
- **Architecture consistency.** Tauri's Rust backend links `app-core` directly, matching the
  mobile shells; Electron would add a Node main process, napi bindings, and an IPC layer that
  exist on no other platform.
- **Footprint.** ~30 MB memory and ~5 MB binary versus ~150–200 MB and ~150 MB, which matters
  on donated or cheap hardware (and cheap, replaceable hardware lowers the cost of seizure).
- **Signal Desktop's hardened Electron** (context isolation, no Node in renderer, strict CSP)
  is production-grade and the main argument for Electron; hardened Electron and Tauri are
  comparable on security, but patch ownership still favors Tauri.
- **Project webviews** were the feared blocker and turned out not to be: the iOS
  `ProjectWebView` has no JS bridge, so it maps to a modal `WebviewWindow` with a navigation
  handler.
- **Cross-engine rendering** differences are marginal for a messaging UI. Linux needs
  webkit2gtk-4.1 (Ubuntu 22.04+, Debian 12+); if webkit2gtk stability becomes a problem,
  `tauri-apps/cef-rs` (Chromium on Linux) is the fallback.

### Solid for the shell UI

The shell webview is the **only** privileged webview (it can call Tauri commands; Project
webviews can only fire deep links), so its supply-chain surface matters most.

- **Rejected: Dioxus/Leptos (Rust/WASM).** Best security properties, but Tauri + WASM is
  underdocumented, push-event wiring needs fragile wasm-bindgen interop, and API churn would
  force rewrites of the most privileged layer.
- **Rejected: React/Vue.** Largest npm dependency trees (~600–800+ transitive packages) in the
  highest-privilege context.
- **Rejected: Svelte.** Close second; Svelte 5 Runes were new enough that generated code risked
  mixed Svelte 4/5 patterns, and Solid's fine-grained signals suit real-time UIs.
- **Rejected: Elm, Mithril, Lit** — small community, no security-app precedent, and verbosity
  respectively.
- **Chosen: Solid** — JSX compiled to direct DOM operations with a ~7 KB signals runtime,
  fine-grained reactivity, familiar syntax, and a standard Tauri integration.

Mitigations that close most of the remaining gap to a WASM shell: `npm ci` with a locked
lockfile; a strict production CSP (`default-src 'self'`, no `unsafe-inline`, no `eval`);
`Object.freeze(Object.prototype)` at startup; strict TypeScript; all message content arrives
as typed data from Rust; and a minimal capability surface.
