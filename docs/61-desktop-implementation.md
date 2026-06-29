# Desktop Implementation Plan

iOS is the reference implementation. This document tracks what needs to be built
to reach functional parity on Desktop (Windows, macOS, Linux) and how to maintain
it going forward.

See `desktop/CLAUDE.md` for the parity rule and Tauri workflow.

---

## Tech Stack Decision

Tauri was chosen over Electron. The decision was not obvious — Electron has real advantages — so the full reasoning is recorded here.

### Why Tauri

**Security patch ownership.** Electron bundles a specific Chromium version. When a Chromium CVE drops, protection requires: (1) Electron releases an update with the patched Chromium, and (2) users install it. Tauri uses the OS system webview — WebView2 on Windows, WebKit on macOS, webkit2gtk on Linux — which is patched automatically by the OS vendor independent of the app. For a security-sensitive app targeting activist groups who may not keep software updated, offloading the rendering engine patch cycle to the OS is a meaningful reduction in ongoing maintenance burden and a better security posture in practice.

**Architecture consistency.** Tauri's Rust backend with no Node.js intermediary matches the iOS/Android shell pattern exactly: the same `app-core` crate, bridged by a thin platform layer, with the frontend calling into it. Electron would require a separate napi-rs bindings crate, a Node.js main process, and an IPC layer between the renderer and native code — an extra moving part that doesn't exist on any other platform.

**Resource footprint.** Tauri's memory baseline is ~30MB vs ~150–200MB for Electron; binary size is ~5MB vs ~150MB. This matters for two reasons. First, activist groups realistically run on donated or cheap hardware — old laptops, Raspberry Pis. Second, cheap replaceable hardware is itself a security property: a $35 device you can destroy or abandon lowers the cost of physical seizure. (Note: the homeserver's Postgres requirement is the harder constraint for Pi-class hardware; SQLite homeserver would need to land before the desktop shell choice becomes the binding constraint.)

### Why not Electron

The main argument for Electron is that Signal Desktop uses it — a directly copyable reference implementation with a well-hardened configuration (context isolation on, Node integration off in renderer, strict CSP, sandboxed webview). That configuration closes most of Electron's structural security gaps and is genuinely production-grade.

However, hardened Electron and Tauri are comparable in practice on security — the advantage Tauri has is structural (deny-by-default capability system, no Node intermediary) while hardened Electron achieves similar results through discipline. The security patch ownership difference remains regardless of hardening.

### ProjectWebView

The main concern raised about Tauri was `ProjectWebView` — embedding a project's web UI inside the app shell, which requires auth token injection and navigation interception. This turned out to be a non-issue for two reasons.

First, the iOS reference (`mobile/ios/Actnet/Sources/Views/Network/ProjectWebView.swift`) is a plain modal sheet with no JavaScript bridge to native code — no `WKScriptMessageHandler`, no injected scripts, no auth token injection. It loads a project URL in a sandboxed `WKWebView` and intercepts navigations to `go.theavalanche.net` as deep links. This maps directly to a Tauri `WebviewWindow` modal with a `navigation_handler`. No embedded-webview concern applies.

Second, the Tauri APIs that were blockers at the time of initial evaluation have since shipped: `Webview::set_cookie()` landed in Tauri 2.8.0 (mid-2025, cross-platform), and navigation interception via `navigation_handler` was already available. The concern about embedding a webview as an inline positioned element within the Solid layout (tauri #13311, closed "not planned") is irrelevant because our implementation is a modal window, not an inline element.

### WebKit inconsistency

Tauri uses three different rendering engines across platforms, which raises a cross-platform consistency concern. In practice, for a messaging app UI — flexbox layouts, scrolling message lists, input fields, CSS animations — the engines are consistent enough that the QA burden is minor. Inconsistencies are at the margins (Wayland compositing, transparency, iframe event routing) and are unlikely to affect core messaging UI.

The more concrete Linux concern is webkit2gtk stability: a Tauri maintainer has flagged quality issues in webkit2gtk (discussion #8524). The escape hatch is `tauri-apps/cef-rs` — an experimental Chromium renderer for Linux that is actively maintained — which would restore uniform rendering on Linux at the cost of a larger binary. Linux requires webkit2gtk-4.1 (Ubuntu 22.04+, Debian 12+, Pi OS Bookworm); older distros are unsupported.

---

## UI Framework Decision

Solid was chosen over React, Vue, Svelte, Dioxus, and several other options. The decision involved a detailed evaluation of the security properties of each option relative to the threat model (activist users, AI-accelerated exploitation accessible to non-state actors with commodity hardware), the role of the Project architecture in shaping what the shell actually needs to protect, and practical considerations around integration reliability and tooling. The full reasoning is recorded here.

### The shell is the only privileged WebView

The Project architecture creates a meaningful asymmetry. Projects load in sandboxed, bridgeless WebViews on all three platforms — WKWebView on iOS, Android WebView on Android, `Tauri WebviewWindow` on desktop. These WebViews have no IPC access to native code; they can only fire deeplinks. The shell WebView, by contrast, has full Tauri command access to `app-core`.

This means the shell is the only WebView context through which Tauri commands can be exploited. A supply chain attack landing in a Project's WebView can fire deeplinks; a supply chain attack landing in the shell's WebView can call any command the shell exposes — send messages, read state, interact with the crypto layer. Hardening the shell specifically is therefore not symmetric with hardening Projects: it protects the highest-privilege surface in the app.

### Why not Dioxus or Leptos

Dioxus and Leptos (Rust/WASM) are the options with the strongest security properties: zero npm supply chain, Rust memory safety, and a WASM sandbox layer within the WebView. If the shell framework choice were purely a security decision, one of these would be the answer.

However, both carry integration risks that outweigh the remaining security benefit:

**Tauri + WASM is underspecified.** The well-documented Dioxus path is Dioxus Desktop, which bypasses Tauri and uses WRY directly. Running Dioxus as a WASM frontend inside a Tauri WebView — the path that preserves Tauri's capability system — has almost no documentation or production examples. The integration gaps would be discovered mid-implementation.

**The event system is on the critical path.** A messaging app's frontend is event-driven: new messages, delivery status, typing indicators, connection state all push from the Rust backend to the frontend. In a JS framework this is `listen('event', handler)` — two lines. In Dioxus WASM, receiving push events requires wasm-bindgen interop that is non-trivial to set up and fragile to maintain. Getting this wrong means messages don't display reliably, which is a worse failure mode than a slightly larger theoretical attack surface.

**API instability on security-critical code.** Dioxus has made breaking changes across every major version (0.1–0.6). A forced shell rewrite to track a Dioxus version introduces regression risk at the layer that sits in front of Tauri command access — exactly where you don't want disruptive changes.

A mitigated JS framework closes roughly 90% of the gap to Dioxus. The remaining 10% — the WASM sandbox boundary and the elimination of JS execution context entirely — is real but does not justify blocking integration risks on the critical path of a messaging app.

### Why not React or Vue

React's ~800+ transitive npm dependencies represent the largest supply chain surface of any option evaluated. The shell WebView is the highest-privilege surface in the app; having the most explorable dependency tree in it is a direct conflict with the threat model. Vue's profile is similar (~600–800 deps, full virtual DOM runtime). Neither offers a meaningful advantage over Solid for this use case.

Signal Desktop uses React inside hardened Electron. That is a valid architecture for Signal's context. However, Signal's React choice predates AI-accelerated supply chain exploitation as a realistic threat for non-state actors with commodity hardware; the cost-benefit calculation has shifted. Hardening at the configuration layer (CSP, locked deps) is the right response, and applying those mitigations to a minimal-runtime framework is a strictly better posture than applying them to a heavy one.

### Why not Svelte

Svelte was the closest alternative. Both Svelte and Solid compile to direct DOM operations with no virtual DOM; both have minimal runtime footprint after compilation; both close the supply chain gap to an acceptable level with the mitigations below. The decision between them came down to three factors:

**Svelte 5 Runes training data.** Svelte 5's reactivity model (Runes: `$state()`, `$derived()`, `$effect()`) is a significant departure from Svelte 4 and shipped late 2024. Claude Code's training data for Svelte 5 specifically is thinner, and there is a meaningful risk of generating Svelte 4/5 mixed patterns that compile incorrectly. For a project where Claude Code is a primary implementation tool, this is a practical reliability concern.

**Solid's compiler closes the security gap.** Solid's Vite/Babel plugin compiles JSX to direct DOM operations at build time — not to virtual DOM calls. The ~7kb signals runtime is the only code that ships. The security difference from Svelte's "compiles away" property is negligible in practice.

**Fine-grained reactivity for first-party Projects.** The shell is a thin, stable surface where the reactive model rarely matters. First-party Projects (Channel Directory, Collab Docs, Engagement Tracking, etc.) are full web apps where real-time state management is central. Solid's signals — where each signal subscription updates exactly the DOM nodes it affects — are a better architectural fit for those UIs than Svelte's compiler-based component-level reactivity.

### Why not Elm, Mithril, or Lit

**Elm** has zero npm supply chain (its own package registry) and strong type-safety properties. It was eliminated on response capacity: a found vulnerability depends on a small volunteer community with no corporate backing.

**Mithril** was initially evaluated as a security-app precedent (Bitwarden). This turned out to be incorrect — Bitwarden uses Angular. Without that precedent, Mithril is a technically sound but niche choice with a slow development pace and no notable security-app validation.

**Lit** (Google Web Components) has strong backing (Google, W3C browser standards) and minimal runtime. It was eliminated on ergonomics: Lit's verbosity is manageable for a thin shell but becomes a real cost when building the complex UIs that first-party Projects require.

### Why Solid

- **Compiler + no virtual DOM.** JSX is compiled to direct DOM operations. Only the ~7kb signals runtime ships — no framework overhead at runtime, no virtual DOM diffing.
- **Fine-grained reactivity.** Each signal subscription updates exactly the DOM nodes wired to it. This is the right model for both the shell (message delivery indicators, connection state) and first-party Projects (complex real-time UIs).
- **JSX familiarity.** Solid's syntax is React-like. Claude Code generates correct, verifiable Solid reliably. React developers can read it immediately.
- **Consistent framework for first-party Projects.** The same framework, tooling, and component library serve both the desktop shell and first-party Projects — shared design system, consistent UX, one set of conventions.
- **Well-understood Tauri integration.** Solid runs as a standard web app in the Tauri WebView. Official Tauri template exists. `invoke()` and `listen()` work identically to any JS framework — no wasm-bindgen bridge, no integration unknowns.
- **Netlify-backed, stable API.** Ryan Carniato (creator) works at Netlify. The API has been stable since 1.0.

### Security mitigations applied

These mitigations close the remaining gap between Solid and a Rust/WASM solution:

- `npm ci` with locked `package-lock.json` — reproducible builds, no silent dependency updates
- `npm audit` + supply chain scanning (e.g. `socket.dev`) in CI
- Tauri CSP: `default-src 'self'`, no inline scripts, no `eval`
- `Object.freeze(Object.prototype)` at app startup — closes prototype pollution
- All message content flows from Rust via typed Tauri commands — the shell never parses raw bytes from the network
- Strict TypeScript (`strict: true`, no `any`) — eliminates a class of type confusion bugs at compile time
- Minimal Tauri command surface — only commands the shell legitimately needs are declared in `tauri.conf.json`

---

## Tech Stack

| Concern | Desktop | iOS equivalent |
|---|---|---|
| Language | TypeScript | Swift |
| UI framework | Solid + Tauri | SwiftUI |
| State management | Solid signals + stores | ObservableObject + @Published |
| Navigation | `@solidjs/router` | NavigationStack |
| Async | async/await + Promises | async/await + Task |
| Camera (QR) | Tauri plugin or native dialog | AVFoundation + VisionKit |
| WebView | Tauri `WebviewWindow` (modal) | WKWebView |
| Rust bridge | Tauri commands (Rust backend in `src-tauri/`) | UniFFI Swift bindings |
| Persistence (metadata) | `tauri-plugin-store` (JSON file) | UserDefaults (JSON) |
| Local crypto DB | SQLCipher via Tauri Rust core | SQLCipher via UniFFI Rust core |

---

## Project Structure

```
desktop/
├── src/                             # Solid frontend
│   ├── index.tsx
│   ├── App.tsx
│   ├── models/
│   │   ├── Account.ts
│   │   ├── Conversation.ts
│   │   ├── Message.ts
│   │   ├── InviteToken.ts
│   │   └── ProjectInfo.ts
│   ├── state/
│   │   └── AppContext.tsx           # mirrors iOS AppState
│   ├── services/
│   │   ├── ActnetService.ts
│   │   ├── MockActnetService.ts
│   │   └── DevServerActnetService.ts
│   └── views/
│       ├── onboarding/
│       │   ├── SplashView.tsx
│       │   ├── QRScannerView.tsx
│       │   ├── InviteLinkEntryView.tsx
│       │   ├── IdentityPickerView.tsx
│       │   ├── JoiningServerView.tsx
│       │   └── NewAccountView.tsx
│       ├── chats/
│       │   ├── ChatsView.tsx
│       │   ├── ConversationRow.tsx
│       │   ├── ConversationView.tsx
│       │   ├── MessageBubble.tsx
│       │   ├── ComposeMessageView.tsx
│       │   └── RecoveryKeyBanner.tsx
│       ├── calls/
│       │   └── CallsView.tsx
│       ├── network/
│       │   ├── NetworkView.tsx
│       │   └── ProjectWebView.tsx
│       └── common/
│           ├── AccountAvatar.tsx
│           ├── DevSettingsView.tsx
│           └── MainLayout.tsx       # sidebar nav (desktop adapts tabs → sidebar)
├── src-tauri/
│   ├── src/
│   │   └── lib.rs                   # Tauri commands — bridge between Solid and Rust core
│   ├── Cargo.toml
│   └── tauri.conf.json              # app config, window setup, capability declarations
├── package.json
├── tsconfig.json
└── vite.config.ts                   # bundler for frontend
```

### Tauri architecture

There is no main/renderer process split. The app has two layers:

- **Rust backend** (`src-tauri/src/lib.rs`): links against `app-core` directly — no Node.js intermediary. Exposes methods as Tauri commands (`#[tauri::command]`). Manages WebSocket loops and metadata persistence.
- **Solid frontend** (`src/`): runs in a WebView sandbox. Calls Rust via `invoke('command_name', args)`. Receives push events via the Tauri event system.

All Rust core calls go: `Solid component → invoke() → src-tauri/src/lib.rs → app-core → Rust`.

---

## Desktop UX Adaptation

The iOS app uses a bottom tab bar (Calls / Chats / Network). On desktop, tabs become a **left sidebar** — the standard pattern for desktop messaging apps (Signal Desktop, Slack, Discord all do this). The three sections are identical; only the navigation chrome differs.

Everything else — conversation list, message bubbles, delivery indicators, network view, project webview — maps 1:1 from iOS with no conceptual change.

---

## Parity Map

Status: `[x]` implemented · `[~]` deliberate divergence (see note) · `[ ]` not yet.
File names below are the current ones; some iOS-era names in this doc's history
(`ActnetService`, `renderer/App.tsx`) were renamed during implementation.

### App Shell

| iOS | Desktop | Status |
|---|---|---|
| `ActnetApp.swift` | `index.tsx` + `App.tsx` | `[x]` |
| `RootView.swift` | Root router in `App.tsx` | `[x]` |
| `AppState.swift` | `state/AppContext.tsx` | `[x]` |

### Models

| iOS | Desktop | Status |
|---|---|---|
| `Account.swift` | `models/Account.ts` | `[x]` |
| `Conversation.swift` | `models/Conversation.ts` | `[x]` |
| `Message.swift` | `models/Message.ts` | `[x]` |
| `InviteToken.swift` | `models/InviteToken.ts` | `[x]` |
| `ProjectInfo.swift` | `models/ProjectInfo.ts` | `[x]` |

### Services

| iOS | Desktop | Status |
|---|---|---|
| `ActnetService.swift` protocol | `services/AvalancheService.ts` interface | `[x]` |
| `MockActnetService.swift` | `services/MockAvalancheService.ts` | `[x]` |
| `DevServerActnetService.swift` | `services/DevServerAvalancheService.ts` | `[x]` |
| UniFFI `AppCore` | Tauri commands (`src-tauri/src/lib.rs`) via `invoke()` | `[x]` |

### Onboarding

| iOS | Desktop | Status |
|---|---|---|
| `SplashView.swift` | `onboarding/SplashView.tsx` | `[x]` |
| `QRScannerView.swift` | (paste-link only; camera/QR not built) | `[~]` |
| `InviteLinkEntryView.swift` | `onboarding/InviteLinkEntryView.tsx` | `[x]` |
| `IdentityPickerView.swift` | `onboarding/IdentityPickerView.tsx` | `[x]` |
| `JoiningServerView.swift` | `onboarding/JoiningServerView.tsx` | `[x]` |
| `NewAccountView.swift` | `onboarding/NewAccountView.tsx` | `[x]` |
| — (back-stack driver) | `onboarding/OnboardingFlow.tsx` | `[x]` |
| `RecoveryPhrase…` (signup credential) | `onboarding/RecoveryPhraseSetupView.tsx` | `[x]` |
| `RecoveryExplainerView.swift` | `onboarding/RecoveryExplainerView.tsx` | `[x]` |
| `RecoveryConsoleView.swift` | `onboarding/RecoveryConsoleView.tsx` | `[x]` |
| `LinkNewDeviceView.swift` (T71, new-device side) | `onboarding/LinkNewDeviceView.tsx` | `[x]` |

### Navigation

| iOS | Desktop | Status |
|---|---|---|
| `MainTabView.swift` (bottom tabs) | `common/MainLayout.tsx` (left sidebar) | `[x]` |

### Chats

| iOS | Desktop | Status |
|---|---|---|
| `ChatsView.swift` | `chats/ChatsView.tsx` | `[x]` |
| `ConversationRow.swift` | `components/ConversationRow.tsx` | `[x]` |
| `ConversationView.swift` | `chats/ConversationView.tsx` | `[x]` |
| `MessageBubble.swift` | `components/MessageBubble.tsx` | `[x]` |
| `ComposeMessageView.swift` | `components/ComposeMessageView.tsx` | `[x]` |
| `RecoveryKeyBanner.swift` | `components/RecoveryKeyBanner.tsx` (inert stub, as iOS) | `[x]` |
| `AttachmentViews.swift` | `components/AttachmentView.tsx` + `components/LinkPreviewCard.tsx` | `[x]` |
| `EditHistorySheet.swift` | `components/EditHistorySheet.tsx` | `[x]` |
| `DisappearingMessagesPicker.swift` | `components/DisappearingMessagesPicker.tsx` | `[x]` |
| group create / detail | `components/NameGroupView.tsx` + `components/GroupDetailView.tsx` | `[x]` |
| new conversation | `components/NewConversationView.tsx` + `components/RecipientTokenField.tsx` | `[x]` |

### Calls

| iOS | Desktop | Status |
|---|---|---|
| `CallsView.swift` | (no Calls section on desktop sidebar) | `[~]` |

### Network

| iOS | Desktop | Status |
|---|---|---|
| `NetworkView.swift` | `network/NetworkView.tsx` | `[x]` |
| `ProjectWebView.swift` | `network/ProjectWebView.tsx` | `[x]` |

### Common / Settings

| iOS | Desktop | Status |
|---|---|---|
| `AccountAvatar.swift` | `components/AccountAvatar.tsx` | `[x]` |
| `ContactAvatar.swift` / `Hexagon.swift` | `components/ContactAvatar.tsx` | `[x]` |
| `DevSettingsView.swift` | `settings/DevSettingsView.tsx` | `[x]` |
| settings hub | `settings/SettingsView.tsx` | `[x]` |
| `AccountsView.swift` | `settings/AccountsView.tsx` | `[x]` |
| identity / server detail | `settings/IdentityDetailView.tsx` + `settings/ServerDetailView.tsx` | `[x]` |
| blocked contacts | `settings/BlockedContactsView.tsx` | `[x]` |
| `LinkDeviceView.swift` (T71, existing-device side) | `settings/LinkDeviceView.tsx` | `[x]` |
| `OfflineBanner` + reconnect | `components/OfflineBanner.tsx` | `[x]` |

### State Behaviors (AppContext mirrors AppState)

| Behavior | Status |
|---|---|
| Account restoration on launch | `[x]` |
| `createAccount(...)` (recovery-phrase credential) | `[x]` |
| `joinServer(...)` | `[x]` |
| `switchMode(mode)` (mock/devserver) | `[~]` (mock is test-only, not a runtime mode) |
| `sendMessage(...)` / `sendMessageWithAttachments(...)` — optimistic + core via IPC | `[x]` |
| `markAllMessagesRead(conversationId, accountId)` | `[x]` |
| `loadMessagesFromStore(conversationId, accountId)` | `[x]` |
| `findOrCreateDMConversation(recipientDid, accountId)` | `[x]` |
| TS-owned event loop (`nextEvents()` poll; reconnect in core) | `[x]` |
| `handleIncomingMessage()` / `handleIncomingEvents()` | `[x]` |
| `applyDeliveryStatusUpdates()` | `[x]` |
| Multi-device sync events (`conversationUpdated`, foreground `setAppActive`) | `[x]` |
| Device linking (`completeDeviceLink` / `linkSendBundle`, T71) | `[x]` |
| Warm display-name cache on load (`cachedDisplayNames`, T78) | `[x]` |
| `fetchProjects()` / `requestProjectToken()` | `[x]` |
| Conversation list derived from SQLCipher store | `[x]` |
| `unreadCount(for:)` (authoritative seed from core) | `[x]` |
| Multi-account (one shared inbox over all identities) | `[ ]` (single-account today — see Deferred) |

---

## Implementation Phases

### Phase 1 — Tauri project + Rust bridge

- Create `desktop/` with Tauri + Vite + Solid + TypeScript
- Wire `app-core` crate into `src-tauri/src/lib.rs` as Tauri commands
- `tauri.conf.json` config for Win/Mac/Linux targets
- `make desktop` Makefile target

**Done when:** Tauri window opens; can call a Rust command from the frontend via `invoke()` and get a result.

### Phase 2 — Models + AppContext

- TypeScript interfaces for all models
- `AppContext.tsx` with all state and methods
- `MockActnetService.ts` with seeded conversations
- `DevServerActnetService.ts` calling Rust via IPC

**Done when:** mock mode works end-to-end in the renderer with no Rust calls.

### Phase 3 — Navigation skeleton

- `@solidjs/router` setup
- `MainLayout.tsx`: left sidebar with Calls / Chats / Network links
- Root routing: onboarding flow vs. main layout

### Phase 4 — Onboarding screens

- `SplashView.tsx`: logo, scan QR (file input fallback on desktop), enter link, dev settings
- `InviteLinkEntryView.tsx`: text field, parse `actnet://` or `https://…/invite/…`
- `IdentityPickerView.tsx`, `JoiningServerView.tsx`, `NewAccountView.tsx`
- QR scanner note: desktop doesn't have a camera in the same sense; accept pasted link or image file upload as the primary path, with optional webcam capture as secondary

### Phase 5 — Chats

- Full chats tab matching iOS: conversation list, conversation view, message bubbles, compose, delivery indicators, unread counts, mark read

### Phase 6 — Network + Calls tabs

- `NetworkView.tsx`: server/project list
- `ProjectWebView.tsx`: opens project URL in a Tauri `WebviewWindow` modal; intercepts deep link navigations back to the app (mirrors `ProjectWebView.swift`)
- `CallsView.tsx`: placeholder

### Phase 7 — Dev settings + polish

- `DevSettingsView.tsx`: mode selector, server URL, counts
- System tray integration (optional: keep app running in background)
- Native notifications via `tauri-plugin-notification`

### Phase 8 — Multi-account (day 6, required before Desktop is "complete")

Desktop currently runs a **single account**: `src-tauri` holds one
`Mutex<Option<Arc<AppCore>>>` and every Tauri command resolves that one core, so
the frontend's `getActiveAccountId()` (`AppContext.tsx`) is just a mirror of it.
iOS and Android already run **multiple accounts concurrently** — a `cores` map
keyed by accountId, per-account event/connection loops, all merged into one
shared inbox (there is no "currently active" identity to switch between). See
`docs/53-multi-account-ux.md` for the UX, and `restoreAccounts`/`cores` in
`mobile/ios/.../App/AppState.swift` + `mobile/android/.../App/AppViewModel.kt`
for the reference implementation.

Port that model to Desktop:
- **Backend:** `AppState.app: Mutex<Option<Arc<AppCore>>>` becomes a map keyed by
  accountId; `get_app` resolves per account; every command takes an `account_id`;
  `next_events` / `wait_for_connection_state_change` run per account; account
  lifecycle commands add/remove map entries instead of replacing the one slot.
- **Frontend:** account-aware service (`serviceFor(accountId)` or pass the id);
  one event + connection loop per account (loop over `store.accounts`); remove
  `getActiveAccountId` (its 2 call sites take the accountId from their own
  per-account loop); add a "sign in another account" onboarding path that spins
  up a second core without tearing down the first.
- **Already half-built:** `connectionStates` is keyed by accountId and
  `aggregateConnectionState` already merges across accounts; conversation IDs
  already embed the account (`dm-${accountId}-${senderDid}`). One gap to resolve:
  `group-${groupId}` is not account-scoped.

Spec-first (per the repo "spec before code" rule) against `docs/53` + the mobile
implementations. Best done as a fresh branch off `main` after day-3/4/5 land, so
it doesn't disrupt the in-review stack.

**Done when:** two accounts can be signed in at once, both receive in real time,
and their conversations show up in one merged list, matching iOS/Android.

---

## Open Questions

1. **Tauri commands structure.** `src-tauri/src/lib.rs` will grow as commands are added — may want to split into submodules by domain (auth, messages, projects) once it reaches meaningful size.
2. **QR scanning on desktop.** Primary path is paste-a-link. Webcam QR scanning is a nice-to-have.
3. **WebSocket in Rust backend.** The WS loop runs in `src-tauri` and pushes events to the frontend via `app_handle.emit()` — same pattern as native mobile running it off the main thread.
4. **webkit2gtk on constrained Linux.** Requires webkit2gtk-4.1 (Ubuntu 22.04+, Debian 12+, Pi OS Bookworm). Older distros unsupported. If webkit2gtk stability becomes a blocker, `tauri-apps/cef-rs` (experimental Chromium renderer for Linux) is the fallback.

## Deferred / Known Limitations

- **Single-account only (until day 6).** Desktop holds one `AppCore` and assumes one account throughout (`getActiveAccountId`, `accounts[0]`). Multi-account (one shared inbox over all identities, like iOS/Android) is required for parity and is tracked as **Phase 8 / day 6** above. Desktop is not "complete" until this lands.
- **tauri-plugin-store metadata exposure.** The identity list (own DIDs, display names, server URLs, DB filename) is stored as a plain JSON file in the OS app-data directory (`%APPDATA%\actnet\` on Windows, `~/Library/Application Support/actnet/` on macOS, `~/.local/share/actnet/` on Linux). It is protected only by OS-level filesystem permissions — not encrypted, not keyed from hardware-backed crypto. Desktop sandboxing is weaker than mobile: any process running as the same user has straightforward read access. An attacker (or malware at user privilege) gets enough to link the device to specific orgs. Message content and the contact graph are not exposed — they live inside the per-identity SQLCipher DBs. The fix would be a small `manifest.db` encrypted with a key stored in the platform credential store (Windows DPAPI / Credential Manager, macOS Keychain, Linux Secret Service). Deferred because the sensitivity of the leaked metadata is low relative to the cross-platform implementation complexity. See the analogous iOS note in `docs/02-todos-deferred.md`.
