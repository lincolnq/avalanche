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
- Manages WebSocket loops, metadata persistence via `tauri-plugin-store`

**Solid frontend** (`src/`, runs in a WebView sandbox):
- Runs Solid — no direct native access
- Calls Rust via `invoke('command_name', args)`
- Receives push events via Tauri event system

All Rust core calls flow: `Solid → invoke() → src-tauri/src/lib.rs → app-core → Rust`.

---

## Desktop Workflow

```bash
cd desktop && cargo tauri dev    # dev mode with hot reload
cd desktop && cargo tauri build  # package for current platform
```

FFI constraints:
- Tauri commands are async-capable — use `async fn` for Rust calls that block
- WebSocket loops run in the Rust backend via Tauri's async runtime
- Push events to the frontend via `app_handle.emit('event-name', payload)`

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
7. Add Tauri command in `desktop/src-tauri/src/lib.rs`, typed wrapper in `DevServerActnetService.ts`, stub in `MockActnetService.ts`

---

## Security constraints

The shell is the only WebView with Tauri command access. Keep these invariants:

- `npm ci` only — never `npm install` in production or CI
- Tauri CSP in `tauri.conf.json`: the production `csp` must stay strict — `default-src 'self'`, **no `unsafe-inline`**, no `eval`. Only `devCsp` may relax this (it keeps `'unsafe-inline'` because Vite injects HMR styles inline in dev). Never add `'unsafe-inline'` to the production `csp`.
- `Object.freeze(Object.prototype)` at app startup in `src/index.tsx`
- Strict TypeScript (`strict: true` in `tsconfig.json`, no `any`)
- All message content received as typed data from Tauri commands — never parse raw bytes in the frontend
- Minimal Tauri command surface: only declare commands the shell legitimately needs in `tauri.conf.json` capabilities

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
`startEventLoop`/`startConnectionLoop` (and `startPolling`) must guard against
re-entry (`if (running) return;`) — they're called from more than one place
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

### Optimistic updates need an id-reconciliation story
Optimistic messages use a client-generated id. Before DevServer/real-backend mode is
wired, the incoming-event handler must reconcile/de-duplicate the server-delivered copy
of an outgoing message against its optimistic entry (client id vs server id) — otherwise
it appears twice. Leave a `// TODO:` at the append site until that path exists.

### Enum comparisons must encode the real ordering
Don't `>`-compare enum members whose numeric values aren't in semantic order. E.g.
`DeliveryStatus` is `sending0/sent1/delivered2/read3/failed4` — `failed` is terminal,
not "more advanced than read." Use an explicit rank/compare for progression, and treat
terminal states (`failed`) separately rather than by numeric magnitude.

### Distinguish "no connection" from "connected"
Aggregate/derived connection state must not default to `connected` when there are zero
connections (e.g. no account yet, or just after logout) — return a non-connected state
so the UI doesn't show a green indicator before any connection exists.
