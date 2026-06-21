# Desktop Implementation Plan

Stacked branches: `main` → `desktop/day-1` → `desktop/day-2` → `desktop/day-3` → `desktop/day-4` → `desktop/day-5`

Each day's loop works on its own branch and opens a PR into the previous day's branch (not main).

---

## Loop agent context (read at the start of EVERY iteration)

1. Read `desktop/CLAUDE.md` — architecture, workflow, security constraints, checklists.
2. Reference implementation: `mobile/ios/Actnet/Sources/` — iOS is the source of truth for behavior.
3. iOS has **2 tabs only** (Chats + Network — Calls tab was removed, see `39a5cf3`).
4. Conventions from `desktop/CLAUDE.md` apply throughout:
   - `npm ci` only, never `npm install`
   - Strict CSP in `tauri.conf.json`, `Object.freeze(Object.prototype)` in `src/index.tsx`
   - `strict: true` TypeScript, no `any`
   - All message content via typed Tauri commands — never parse raw bytes in frontend

Available skills (use when applicable):
- `/new-ffi-method <name>` — if adding a brand-new FFI method not yet in `app-core/src/lib.rs`
- `/new-swift-model <Name>` — if adding a new Swift model (iOS parity)
- `/new-view <Name>` — if adding a new SwiftUI view (iOS parity)
- `/done` — run post-implementation review before opening a PR

Parity rule: desktop implementation MIRRORS existing iOS views — no new iOS views needed. Do not run `/new-view` for tasks in this plan.

---

## Day 1 — Scaffold · Models · Services · AppContext · Navigation

**Branch:** `desktop/day-1` (off `main`)
**Done when:** `cargo tauri dev` opens a Tauri window; mock mode shows sidebar + seeded conversation list with no Rust calls.

### Phase 1 — Tauri Scaffold

- [ ] **T01** — Frontend config: `package.json` (tauri 2.x, @tauri-apps/api, vite, vite-plugin-solid, solid-js, typescript), `tsconfig.json` (`strict: true`, `jsx: "preserve"`, `jsxImportSource: "solid-js"`), `vite.config.ts` (solidPlugin, Tauri env). Versions: tauri 2.x latest, Solid 1.x.
- [ ] **T02** — Entry point: `index.html`, `src/index.tsx` (`Object.freeze(Object.prototype)` then `render(() => <App/>, root)`), `src/App.tsx` (placeholder `<div>Actnet</div>` for now).
- [ ] **T03** — Tauri Rust skeleton: `src-tauri/Cargo.toml` (tauri 2.x, tauri-plugin-store; no app-core yet), `src-tauri/tauri.conf.json` (strict CSP: `default-src 'self'`, window 1200×800, identifier `net.actnet.desktop`), `src-tauri/src/lib.rs` (one `ping` command returning `"pong"`).
- [ ] **T04** — Workspace + Makefile: add `desktop/src-tauri` to `core/Cargo.toml` `[workspace] members`; add `make desktop` target (`cd desktop && cargo tauri dev`). Run `cargo check -p desktop-tauri` (or whatever the crate is named) to confirm it compiles.

### Phase 2 — TypeScript Models

- [ ] **T05** — Core models: `src/models/Account.ts`, `src/models/Conversation.ts`, `src/models/Message.ts`. Mirror the Swift structs field-for-field. Reference: `mobile/ios/Actnet/Sources/Models/`. Include `DeliveryStatus` enum. Barrel export via `src/models/index.ts`.
- [ ] **T06** — Secondary models: `src/models/InviteToken.ts` (base64url-decode, parse JSON, extract `s`=server_url / `d`=inviter_did), `src/models/ProjectInfo.ts`, `src/models/ServerInfo.ts`. Reference: matching `.swift` files in `mobile/ios/Actnet/Sources/Models/`.

### Phase 3 — Service Layer

- [ ] **T07** — `src/services/ActnetService.ts`: TypeScript interface with every method from `AppCoreProtocol` in iOS (`mobile/ios/Actnet/Sources/Services/ActnetService.swift` + any `AppCoreProtocol+*.swift` extension files). All methods return `Promise<T>`. Include `ServiceMode` enum (`mock` | `devServer`).
- [ ] **T08** — `src/services/MockActnetService.ts`: implements `ActnetService`. Seed 3 conversations (2 DMs, 1 group) with 4–5 messages each. Mirror `MockActnetService.swift` and `MockData` patterns. Reference: `mobile/ios/Actnet/Sources/Services/MockActnetService.swift`.
- [ ] **T09** — `src/services/DevServerActnetService.ts`: implements `ActnetService` by calling `invoke('command_name', args)` for every method. Also add stub `#[tauri::command]` entries for each method in `src-tauri/src/lib.rs` (return `todo!()` — just enough to compile).

### Phase 4 — AppContext

- [ ] **T10** — `src/state/AppContext.tsx` — account lifecycle: `createContext`, `createStore` with shape mirroring `AppState.swift` (`accounts`, `isOnboarding`, `serviceMode`, `selectedTab`). Implement: `createAccount`, `login`, `switchMode`, `logout`, `restoreAccounts` (via `tauri-plugin-store`), `joinServer`. Export `AppProvider` wrapping the app and `useApp()` hook. Reference: `mobile/ios/Actnet/Sources/App/AppState.swift` (account lifecycle MARK section).
- [ ] **T11** — AppContext messaging + events: add `conversations`, `messagesByConversation`, `sendMessage` (optimistic + invoke), `sendGroupMessage`, `loadMessagesFromStore`, `markAllMessagesRead`, `findOrCreateDMConversation`, `handleIncomingMessage`, `applyDeliveryStatusUpdates`, `unreadCount`, `startEventLoop` (subscribes to Tauri events emitted by a Rust background task). Reference: `mobile/ios/Actnet/Sources/App/AppState.swift` (Messaging + Connection state MARK sections).

### Phase 5 — Navigation Skeleton

- [ ] **T12** — Router + MainLayout: install `@solidjs/router`. Create `src/views/common/MainLayout.tsx` — left sidebar with Chats and Network icons, active-route highlight. Adapt iOS 2-tab pattern (`MainTabView.swift`) to sidebar. Plain CSS (no external framework). Reference: `mobile/ios/Actnet/Sources/Views/Common/MainTabView.swift`.
- [ ] **T13** — Placeholder views + routing wired: `src/views/chats/ChatsView.tsx` (renders `conversations` from AppContext as a flat list of titles), `src/views/network/NetworkView.tsx` (stub), `src/views/onboarding/SplashView.tsx` (wordmark + "Enter Invite Link" button). Wire `App.tsx`: `isOnboarding` → `<SplashView>`, else `<Router>` with `<MainLayout>` and child routes. Mock mode should now show a conversation list.

---

## Day 2 — Onboarding Flow

**Branch:** `desktop/day-2` (off `desktop/day-1`)
**Done when:** Full onboarding flow in mock mode: paste invite → identity picker → joining → account created → main layout.

- [ ] **T14** — `InviteLinkEntryView.tsx`: text field, parse `actnet://` and `https://…/invite/…`, validate, call `InviteToken.from()`, push to IdentityPicker. Reference: `Onboarding/InviteLinkEntryView.swift`.
- [ ] **T15** — `IdentityPickerView.tsx`: show existing accounts ("Add to [name]") or "New identity" path. Reference: `Onboarding/IdentityPickerView.swift`.
- [ ] **T16** — `NewAccountView.tsx`: display name input, `createAccount()` via AppContext, spinner during creation, navigate to main on success. Reference: `Onboarding/NewAccountView.swift`.
- [ ] **T17** — `JoiningServerView.tsx`: progress indicator with server name while join completes. Reference: `Onboarding/JoiningServerView.swift`.
- [ ] **T18** — `QRScannerView.tsx`: desktop-adapted — primary path is paste-a-link or file upload (read QR image). Optional webcam path via `tauri-plugin-barcode-scanner` if available. Reference: `Onboarding/QRScannerView.swift`.
- [ ] **T19** — Wire full onboarding navigation: `SplashView` → `QRScanner` / `InviteLinkEntry` → `IdentityPicker` → `JoiningServer` / `NewAccount` → main. Deep-link handling: listen for `actnet://` events from Rust, feed into same flow.

---

## Day 3 — Chats Tab

**Branch:** `desktop/day-3` (off `desktop/day-2`)
**Done when:** Full chats tab in mock mode — conversation list, message view, compose, delivery indicators, unread badges.

- [ ] **T20** — `ChatsView.tsx`: conversation list sorted newest-first, click to open. Reference: `Chats/ChatsView.swift`.
- [ ] **T21** — `ConversationRow.tsx`: avatar, name, last-message preview, timestamp, unread count badge. Reference: `Chats/ConversationRow.swift`.
- [ ] **T22** — `ConversationView.tsx`: scrollable message list, auto-scroll to bottom, `loadMessagesFromStore` on mount, `markAllMessagesRead` on view. Reference: `Chats/ConversationView.swift`.
- [ ] **T23** — `MessageBubble.tsx`: sent/received styles, delivery indicator (sending/sent/delivered/read checkmarks), timestamp, edited marker, deleted tombstone. Reference: `Chats/MessageBubble.swift`.
- [ ] **T24** — `ComposeMessageView.tsx`: text input, send button, optimistic append, clear on send. Reference: `Chats/ComposeMessageView.swift`.
- [ ] **T25** — `AccountAvatar.tsx` / `ContactAvatar.tsx`: initials fallback, circle for person / hexagon for bot (docs/54), unread ring. Reference: `Common/AccountAvatar.swift`, `Common/ContactAvatar.swift`.
- [ ] **T26** — `RecoveryKeyBanner.tsx`: banner when no recovery phrase set. Reference: `Chats/RecoveryKeyBanner.swift`.
- [ ] **T27** — `OfflineBanner.tsx`: show reconnecting/offline/server-down from AppContext `connectionState`. Reference: `Common/OfflineBanner.swift`.

---

## Day 4 — Network Tab · Rust Bridge

**Branch:** `desktop/day-4` (off `desktop/day-3`)
**Done when:** NetworkView shows projects, ProjectWebView opens in a modal window, DevServerActnetService calls real app-core Tauri commands (no `todo!()`s remain).

- [ ] **T28** — `NetworkView.tsx`: list servers, fetch projects per server, project cards with name/description/open button. Reference: `Network/NetworkView.swift`.
- [ ] **T29** — `ProjectWebView.tsx`: open project URL in a `Tauri WebviewWindow` modal. Intercept navigations to `go.theavalanche.net` as deeplinks. Follow `desktop/CLAUDE.md §Tauri Architecture`. Reference: `Network/ProjectWebView.swift`.
- [ ] **T30** — Wire app-core: add `app-core` as a path dependency in `src-tauri/Cargo.toml`. Implement Tauri commands for account ops: `create_account`, `login`, `send_dm`, `receive_messages`. Follow `desktop/CLAUDE.md §Adding a New FFI Method` checklist step 7 for each command.
- [ ] **T31** — Tauri commands: conversations + messages: `load_conversations`, `load_messages`, `save_message`, `mark_messages_read`. Add a background task that runs `next_events()` in a loop and emits results via `app_handle.emit("actnet-event", payload)`.
- [ ] **T32** — Tauri commands: groups: `send_group_message`, `create_group`, `invite_member`, `fetch_group_state`, `apply_pending_group_changes`, `is_group_member`.
- [ ] **T33** — Tauri commands: projects + profile: `fetch_projects`, `request_project_token`, `get_account_info`, `contact_display_name`, `refresh_contact_profile`.
- [ ] **T34** — AppContext event subscription: replace polling stub with `listen('actnet-event', handler)` from `@tauri-apps/api/event`. Parse event variants and dispatch to `handleIncomingMessage`, `applyDeliveryStatusUpdates`, `loadConversationsFromStore` etc.

---

## Day 5 — Settings · Polish · Parity Audit

**Branch:** `desktop/day-5` (off `desktop/day-4`)
**Done when:** DevSettingsView works, `docs/61-desktop-implementation.md` parity checkboxes are updated, app builds on Windows.

- [ ] **T35** — `DevSettingsView.tsx`: mode selector (mock ↔ devServer), server URL display, account count, logout. Reference: `Settings/` views in iOS.
- [ ] **T36** — `AccountsView.tsx` + `IdentityDetailView.tsx`: account list, leave server, delete identity. Reference: `Settings/AccountsView.swift`, `Settings/IdentityDetailView.swift`.
- [ ] **T37** — Deep link registration: `actnet://` URL scheme in `tauri.conf.json`. Handle in Rust via `on_url_open`, emit to frontend.
- [ ] **T38** — Native notifications: wire `tauri-plugin-notification` for incoming messages (respect scene focus, suppress if conversation is open).
- [ ] **T39** — Parity audit: tick completed items in `docs/61-desktop-implementation.md`. Run `/done` to trigger post-implementation review.
- [ ] **T40** — Windows build: run `cargo tauri build` on Windows, fix any platform-specific issues (path separators, DPAPI for db key, WebView2 requirement).
