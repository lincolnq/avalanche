# Desktop CLAUDE.md

## Platform Parity Rule

**Any feature added or changed on iOS must be implemented on Desktop (Tauri)
in the same session. Any feature added or changed on Desktop must be implemented
on iOS.**

iOS is the reference implementation. The Desktop app must match it
feature-for-feature. When in doubt about behavior, check the iOS source.

The same rule applies across all three platforms — see `mobile/CLAUDE.md` for
iOS/Android and the root `CLAUDE.md` for the overall parity rule.

Use `docs/61-desktop-implementation.md` as the parity tracking document — update
the `[ ]` / `[x]` checkboxes as each component is completed.

---

## Repository layout

```
desktop/                         # Tauri app (this directory)
├── src/                         # Solid frontend
└── src-tauri/                   # Rust backend (Tauri commands, app config)
```

The Tauri Rust backend (`src-tauri/`) is the Desktop equivalent of UniFFI
bindings in `core/crates/app-core/` — a bridge layer, not an app. It links
directly against the `app-core` crate and exposes its methods as Tauri commands.

---

## Tauri Architecture

There is no main/renderer process split. The app has two layers:

**Rust backend** (`src-tauri/src/lib.rs`):
- Links against `app-core` directly — no Node.js intermediary
- Exposes methods as Tauri commands (`#[tauri::command]`)
- Manages metadata persistence via `tauri-plugin-store`

**Solid frontend** (`src/`, runs in a WebView sandbox):
- Runs Solid — no direct native access
- Calls Rust via `invoke('command_name', args)`
- Polls for decrypted events via Tauri commands (TS-owned loop, no background thread)

All Rust core calls flow: `Solid → invoke() → src-tauri/src/lib.rs → app-core → Rust`.

### Architectural invariant: TS owns the event loop

`startEventLoop()` in `AppContext.tsx` polls `nextEvents()` in a loop. The
Tauri command blocks on the Rust side (`ffi_runtime().block_on`) and returns
via IPC when events arrive — the JS event loop stays responsive. Each poll
parks one blocking-pool thread for the duration of the call (the event loop and
the connection-state loop run concurrently, so steady state is two), but each
loop serializes its own calls — no unbounded fan-out. Events queue in
app-core's MPSC channel between polls. No registration race — the consumer
initiates every fetch.

- ❌ Never spawn a thread/task in `lib.rs` to call `next_events()`
- ❌ Never `app_handle.emit(…)` events or `listen(…)` in the frontend
- ❌ Never add generation counters or cancellation tokens to `AppState`

---

## Desktop Workflow

```bash
cd desktop && npm run tauri dev    # dev mode with hot reload (or: make dev-desktop)
cd desktop && npm run tauri build  # package for current platform
```

The Tauri CLI is the npm-local `@tauri-apps/cli` (a devDependency), invoked via
the `tauri` package script. `cargo tauri ...` only works if you've separately
`cargo install tauri-cli`, which this repo does not assume.

FFI constraints:
- Tauri commands are async-capable — use `async fn` for Rust calls that block
- Event polling runs in the TS frontend via a loop calling `nextEvents()` — the
  Tauri command blocks on the Rust side but returns via IPC when events arrive,
  so the JS event loop stays responsive

---

## UX Adaptation: Tabs → Sidebar

iOS uses a bottom tab bar (Calls / Chats / Network). Desktop uses a **left
sidebar** — the standard for desktop messaging apps (Signal Desktop, Slack,
Discord). The three sections are identical in content; only the navigation
chrome differs. This is the one intentional UX divergence from iOS.

---

## Visual Reference: Screenshots

`docs/screenshots/` contains iOS simulator screenshots organized by screen name
(e.g. `splash.png`, `chats-list.png`, `conversation.png`). When implementing a
screen on Desktop, use the matching screenshot as a visual reference if it
exists. If it doesn't exist, derive the layout from the iOS source alone —
screenshots are optional, not required.

Screenshots are only capturable on macOS with the iOS simulator. Contributors on
Windows or Linux skip this step entirely.

---

## Adding a New Screen (checklist)

Before closing any branch that adds or changes Desktop UI:

- [ ] iOS SwiftUI view created/updated in `mobile/ios/`
- [ ] Desktop Solid component created/updated in `desktop/src/views/`
- [ ] Tauri command added to `desktop/src-tauri/src/lib.rs` if new Rust calls needed
- [ ] AppContext updated to match AppState changes
- [ ] New model fields added to both `.swift` and `.ts` types
- [ ] `docs/61-desktop-implementation.md` parity table updated
- [ ] *(macOS only)* Screenshot taken and saved to `docs/screenshots/<screen-name>.png`

## Adding a New FFI Method (checklist)

1. Add Rust method to `core/crates/app-core/src/lib.rs` (`#[uniffi::export]`, sync)
2. `make bindings` — regenerates Swift + Kotlin UniFFI glue
3. Add to `AppCoreProtocol` in `ActnetService.swift` and stub in `MockActnetService.swift`
4. Add to `ActnetService` interface in Kotlin and stub in `MockActnetService.kt`
5. Call from `AppState.swift` via `Task.detached`
6. Call from `AppViewModel.kt` via `withContext(Dispatchers.IO)`
7. Add Tauri command in `desktop/src-tauri/src/lib.rs` with `#[specta::specta]`, then **run `make desktop-bindings`** to regenerate `desktop/src/bindings.ts` (it is checked in, not generated at build time — a plain `cargo tauri dev` does **not** regenerate it; codegen only runs behind the `codegen` feature). Commit the regenerated `bindings.ts` with your change — CI fails if it drifts from the command surface. Update `AvalancheService` interface + `MockAvalancheService` + `DevServerAvalancheService` if the signature changed. No manual typed wrapper needed; the generated `commands.*` in `bindings.ts` handle the invoke call.

---

## Passkey / recovery divergence (sanctioned)

Desktop has **no WebAuthn passkey / PRF authenticator path**, so it uses iOS's
**recovery-phrase account mode** for every signup (iOS offers passkey *or*
phrase; desktop offers phrase only):

- **The recovery phrase is the signup credential.** New-account creation routes
  through `RecoveryPhraseSetupView` (in `views/onboarding/`): generate a 12-word
  BIP39 phrase, have the user write it down and confirm three words, then derive
  the 32-byte seed (`recoveryPhraseToSeed`) and pass it to `createAccount` as the
  **PRF output**. The seed therefore derives the PLC rotation key and the DID,
  and `AppCore::create_account` uploads the recovery blob keyed to that DID — so
  the phrase alone can reconstruct the account.
  - This is **load-bearing**: an empty PRF would make `prepare` generate a
    *random* rotation key (`app-core/src/lib.rs` `prepare`), so the DID would be
    unreproducible and `recover_from_phrase` could never locate or decrypt the
    blob. Never pass `[]` to `createAccount` on the human signup path.
- **Restore.** `RecoveryExplainerView` → `RecoveryConsoleView` take the phrase +
  home-server URL, recompute the DID from the seed (`deriveDidFromPasskey`), and
  call `recoverFromPhrase` (→ `recover_from_blob`). DID + rotation key match the
  originals because both come from the same phrase seed.
- **No two-stage handle, no passkey views, no post-hoc setup.** iOS's
  `PreparedAccount` (`prepareAccount`/`finalizeAccount`), `PasskeyExplainerView`,
  and any "secure your account later" banner are intentionally omitted — recovery
  is established once, at signup. `RecoveryKeyBanner` is an inert stub (as on iOS).

If a desktop WebAuthn/PRF path is ever added, revisit all of the above.

## Security constraints

The shell is the only WebView with Tauri command access. Keep these invariants:

- `npm ci` only — never `npm install` in production or CI
- Tauri CSP in `tauri.conf.json`: the production `csp` must stay strict — `default-src 'self'`, **no `unsafe-inline`**, no `eval`. Only `devCsp` may relax this (it keeps `'unsafe-inline'` because Vite injects HMR styles inline in dev). Never add `'unsafe-inline'` to the production `csp`.
- `Object.freeze(Object.prototype)` at app startup in `src/index.tsx`
- Strict TypeScript (`strict: true` in `tsconfig.json`, no `any`)
- All message content received as typed data from Tauri commands — never parse raw bytes in the frontend
- Minimal Tauri command surface: only declare commands the shell legitimately needs in `tauri.conf.json` capabilities
- **Project webviews are IPC-isolated by capability scope, and that scope is the only thing isolating them.** A project page (`new WebviewWindow("project-*", …)`) loads untrusted remote content. It can't reach app-core IPC because (a) the `default` capability scopes `allow-all`/core to `windows: ["main"]` + `local: true`, so a `project-*` label and a remote URL are both denied by the ACL, and (b) `withGlobalTauri: false` means no `__TAURI__` bridge is injected. Never broaden that capability's window scope to a glob, add a `remote` block, or set `withGlobalTauri: true` without review. After any change to capabilities or webview creation, **verify isolation empirically rather than from the config read**: open a project window and confirm `invoke('ping')` from its console is rejected (`ping not allowed on window project-..., allowed on: [windows: main, URL: local]`).

---

## Conventions & common pitfalls (learned from review)

**General directive:** the Day-1 scaffold was generated fast and shipped a layer of
latent state-management bugs (missing context methods, divergent onboarding paths,
module-global mock state, non-reactive caches, wrong enum-ordering comparisons). When
adding or generating substantial frontend code — especially anything touching
`AppContext` state, the service layer, or onboarding flow — **review and verify it as
if hand-written**: trace every code path to completion, run `tsc` + `npm run build`,
and prefer generalizing a shared mechanism over copy-pasting a path with subtle drift.
Generated code is a draft, not a deliverable. The specific patterns below are the ones
that have already bitten us — check new code against each.

These caused real bugs in the Day-1 scaffold. Follow them when adding/changing UI.

### Styling: co-located CSS, never inline
The production CSP forbids `'unsafe-inline'`, which blocks **both** inline `<style>`
elements and inline `style="…"`/`style={{}}` attributes (CSP can't allowlist
attribute styles via nonce/hash — they must not exist). So:
- Each view's styles live in a co-located `.css` file imported at the top of the
  component (e.g. `import "./ChatsView.css"`). No `const styles = \`…\`` +
  `<style>{styles}</style>`, and no inline `style={{}}` objects/`style="…"` attributes.
- Shared colors, the font stack, and reusable component classes (`.btn-primary`,
  `.text-input`, `.back-btn`, `.spinner`, …) live once in `src/styles/theme.css`;
  view CSS references them via `var(--…)` tokens. Don't hardcode hex colors or the
  font stack in a view.
- Vite emits these as linked external stylesheets (served from `'self'`), so the
  strict prod CSP is satisfied without any inline styles.

### AppContext surface must be complete
A view that destructures a method from `useApp()` will get `undefined` at runtime if
that method isn't on the context — **TypeScript does not catch a missing key pulled
from an object**, so it surfaces only as a runtime `"x is not a function"` crash.
When a view calls `useApp().foo()`, `foo` must be declared in the `AppContextValue`
interface **and** present in the `ctx` object literal. (This crashed the entire
invite/QR onboarding path once.)

### Background loops: idempotent start, tear down before swapping state
`startPolling` (which starts the event + connection loops) must guard against
re-entry (`if (running) return;`) — it's called from more than one place
(`restoreAccounts` and `createAccount`), and a double start runs duplicate loops that
process every incoming event twice. When switching accounts/service, call
`stopPolling`/`resetSession` **before** swapping the `service()` signal, or the
in-flight loop will run against the new, uninitialized service.

### Onboarding navigation uses a back-stack, not hardcoded `onBack` targets
`OnboardingFlow` drives screens through a back-stack (`navigate()` pushes, `goBack()`
pops). Wire every screen's `onBack` to `goBack()`. Never hardcode a screen's back
target to a specific other screen — screens are reachable via multiple paths
(splash → link entry → identity picker, deep-link → link entry, etc.) and a fixed
target dead-ends on the paths it didn't anticipate.

### Solid store writes
All store updates go through `setStore(...)`/`produce(...)`. Never assign to a store
field directly (`store.x = …`) — it silently fails to update the UI.

### All account-entry paths converge on one "enter app" step
`createAccount`, `restoreAccounts`, and `joinServer` (existing-identity) must finish
through the **same** completion helper (`enterApp()`: reset the conversations guard,
`loadConversationsFromStore()`, `startPolling()`, clear `isOnboarding`). Don't give one
path its own ad-hoc completion — `joinServer` once skipped polling + conversation load
entirely, so that user landed with a dead event loop. Add a new entry path only by
calling the shared helper.

### Mock/dev services hold per-instance state, never module globals
Session state in a service (e.g. the mock's current DID) must be an instance field, not
a module-level `let`. `logout`/`switchMode` construct a fresh service instance to drop
session state; a module global survives that and bleeds the previous identity into the
new session (stale DID → mismatched conversation keys).

### Caches read during render must be reactive; getters must not side-effect
A context method that looks like a getter (`displayName(did)`) must not fire IPC/network
as a side effect and return a placeholder — and any cache it reads in a tracking scope
must be a Solid store/signal, not a plain `Map`. A non-reactive cache never propagates
the resolved value to the UI and re-fires the fetch every render. Back such caches with
reactive state and guard against duplicate in-flight fetches per key.

### Distinguish "no connection" from "connected"
Aggregate/derived connection state must not default to `connected` when there are zero
connections (e.g. no account yet, or just after logout) — return a non-connected state
so the UI doesn't show a green indicator before any connection exists.

### Debugging best practices (from the Day 3 trenches)

#### Event polling: TS-owned loop, not a Rust background thread

`nextEvents()` blocks on the Rust side until decrypted events arrive, then
returns via IPC. The TS side calls it in a loop, which parks one blocking-pool
thread per in-flight call (the connection-state loop adds a second). Each loop
serializes its own calls. Do not reintroduce a Rust background thread or
`app_handle.emit(…)` — the loop is the single unified path for both DevServer
and Mock mode.

#### Unicode in prompts breaks JSON

Em dashes (`—`), curly quotes (`‘` `’`), and other non-ASCII
characters in system prompts silently break `JSON.stringify()`. Some providers
reject these with "invalid unicode code point". Use ASCII-safe alternatives
(`--`, `'`, `"`).

#### Model response format varies by provider

Don't assume `content[0].text`. DeepSeek v4 models return a `"thinking"` block
first. Find the first block with `type: "text"`; fall back to `thinking` text
if no text block is present. Defensive parsing over positional indexing — the
same fix applies to any LLM backend swap.

#### `.env` reload won't override existing env vars

`dev.py` uses `os.environ.setdefault()` which skips variables already in the
environment. Changing a value in `.env` requires a fresh shell.
