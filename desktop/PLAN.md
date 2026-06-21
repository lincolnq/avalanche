# Desktop Implementation Plan

Stacked branches: `main` → `desktop/day-1` → `desktop/day-2` → `desktop/day-3` → `desktop/day-4` → `desktop/day-5`

Each day's loop works on its own branch and opens a PR into the previous day's branch (not main).

---

## Loop agent instructions (read at the start of EVERY iteration)

1. Read `desktop/CLAUDE.md` — architecture, security constraints, checklists.
2. iOS reference: `mobile/ios/Actnet/Sources/` — source of truth for behavior and field names.
3. iOS has **2 tabs only**: Chats + Network. Calls tab was removed (`39a5cf3`).
4. Security invariants (from `desktop/CLAUDE.md`):
   - `npm ci` only, never `npm install`
   - Strict CSP in `tauri.conf.json`; `Object.freeze(Object.prototype)` in `src/index.tsx`
   - `strict: true` TypeScript, zero `any`
   - Message content arrives only via typed Tauri commands — never parse raw bytes in the frontend
5. Find the first task in the **current day** marked `[ ]`. Implement it completely. Mark it `[x]`.
6. **Do not mark Verify items** — those are for manual human sign-off.
7. When all tasks in a day are `[x]`, output: "Day N tasks complete — verify the checklist below before starting Day N+1" and stop the loop.

Available skills (use when applicable):
- `/new-ffi-method <name>` — only if adding a brand-new method to `app-core/src/lib.rs`
- `/done` — run post-implementation review before opening a PR

---

## Day 1 — Scaffold · Models · Services · AppContext · Navigation

**Branch:** `desktop/day-1` (off `main`)
**Done when:** `cargo tauri dev` opens a Tauri window; mock mode shows sidebar + seeded conversation list with no Rust calls needed.

### Phase 1 — Tauri Scaffold

- [ ] **T01** — Frontend config: `package.json` (tauri 2.x, @tauri-apps/api, vite, vite-plugin-solid, solid-js, typescript — pin exact versions), `tsconfig.json` (`strict: true`, `jsx: "preserve"`, `jsxImportSource: "solid-js"`), `vite.config.ts` (solidPlugin + Tauri env).
- [ ] **T02** — Entry point: `index.html`, `src/index.tsx` (`Object.freeze(Object.prototype)` then `render(() => <App/>, root)`), `src/App.tsx` (placeholder `<div>Actnet</div>` for now — wired properly in T13).
- [ ] **T03** — Tauri Rust skeleton: `src-tauri/Cargo.toml` (tauri 2.x, tauri-plugin-store; no app-core yet), `src-tauri/tauri.conf.json` (CSP `default-src 'self'`, window 1200×800, identifier `net.actnet.desktop`), `src-tauri/src/lib.rs` (one `ping` command returning `"pong"`).
- [ ] **T04** — Workspace + Makefile: add `desktop/src-tauri` to `core/Cargo.toml` `[workspace] members`. Add `make desktop` target (`cd desktop && cargo tauri dev`). Verify `cargo check` passes in the new crate.

### Phase 2 — TypeScript Models

- [ ] **T05** — Core models: `src/models/Account.ts`, `src/models/Conversation.ts`, `src/models/Message.ts`. Mirror Swift structs field-for-field including `DeliveryStatus` enum. Reference: `mobile/ios/Actnet/Sources/Models/Account.swift`, `Conversation.swift`, `Message.swift`. Barrel export via `src/models/index.ts`.
- [ ] **T06** — Secondary models: `src/models/InviteToken.ts` (base64url-decode JSON, extract `s`=serverUrl / `d`=inviterDid), `src/models/ProjectInfo.ts`, `src/models/ServerInfo.ts`. Reference matching `.swift` files. Add to `src/models/index.ts`.

### Phase 3 — Service Layer

- [ ] **T07a** — `src/services/ActnetService.ts` — part 1: `ServiceMode` enum; interface methods for account ops (`createAccount`, `login`, `prepareAccount`, `finalizeAccount`, `recoverFromBlob`) and core messaging (`sendDm`, `sendGroupMessage`, `receiveMessages`, `nextEvents`, `saveMessage`, `loadConversations`, `loadMessages`, `markMessagesRead`). All return `Promise<T>`. Reference: `mobile/ios/Actnet/Sources/Services/ActnetService.swift`.
- [ ] **T07b** — `src/services/ActnetService.ts` — part 2: append group methods (`createGroup`, `inviteMember`, `fetchGroupState`, `applyPendingGroupChanges`, `isGroupMember`, `groupExpirySeconds`, `leaveServer`, `deleteIdentity`), contact/profile methods (`touchContact`, `contactDisplayName`, `getAccountInfo`, `refreshContactProfile`, `fetchAndCacheProfile`), project methods (`fetchProjects`, `requestProjectToken`), reaction/edit/delete methods (`sendReaction`, `sendEdit`, `sendDelete`, `loadReactions`), and connection method (`connectionState`, `waitForConnectionStateChange`, `ownDisplayName`). Reference: `mobile/ios/Actnet/Sources/Services/AppCoreProtocol+*.swift` extension files (glob for all of them).
- [ ] **T08** — `src/services/MockActnetService.ts`: implements `ActnetService`. Implement only the methods needed for mock mode: `createAccount` and `login` return a mock core object; `loadConversations` returns 3 seeded conversations (2 DMs, 1 group); `loadMessages` returns 4–5 seeded messages per conversation; `sendDm`/`sendGroupMessage` resolve immediately; `nextEvents` never resolves (returns a Promise that never settles — the event loop sleeps in mock mode); all other methods resolve with a sensible zero/empty value. Keep the file focused — no need to mirror every mock behavior from iOS.
- [ ] **T09** — `src/services/DevServerActnetService.ts`: implements `ActnetService` by calling `invoke('snake_case_command', args)` for every method. Add a matching stub `#[tauri::command]` in `src-tauri/src/lib.rs` for each (return `Err("not implemented".to_string())` — just enough to compile and register). Register all commands in the `tauri::Builder` call.

### Phase 4 — AppContext

- [ ] **T10a** — `src/state/AppContext.tsx` — shell + account state: `createContext`, `createStore` with fields mirroring `AppState.swift` (`accounts`, `isOnboarding`, `serviceMode`, `selectedTab`, `connectionStates`). `AppProvider` component selects service based on `serviceMode`, reads persisted mode from `tauri-plugin-store`. Export `useApp()` hook. Reference: `mobile/ios/Actnet/Sources/App/AppState.swift` top of file through `init`.
- [ ] **T10b** — AppContext account lifecycle: add `restoreAccounts` (load persisted JSON from store, call `login` per account), `createAccount`, `switchMode` (resets all state), `logout`, `joinServer`. Persist accounts to `tauri-plugin-store` on create/remove. Reference: `AppState.swift` `// MARK: - Account lifecycle` section.
- [ ] **T11a** — AppContext messaging — basic ops: add `conversations`, `messagesByConversation` to store. Implement `sendMessage` (optimistic append → invoke → update status), `sendGroupMessage`, `loadMessagesFromStore` (load-once guard), `markAllMessagesRead`, `findOrCreateDMConversation`, `findOrCreateGroupConversation`, `unreadCount`. Reference: `AppState.swift` `// MARK: - Messaging` section.
- [ ] **T11b** — AppContext event loop + incoming messages: implement `startEventLoop` (calls `nextEvents()` in a loop via `setInterval`-style recursion, dispatches events), `handleIncomingMessage` (appends to `messagesByConversation`, updates conversation preview, fires display-name resolution), `applyDeliveryStatusUpdates`, `connectionStateLoop` (polls `connectionState` and updates store). Reference: `AppState.swift` `// MARK: - Connection state` and the `eventLoop` / `handleIncomingMessage` private methods.

### Phase 5 — Navigation Skeleton

- [ ] **T12** — Router + MainLayout: install `@solidjs/router`. Create `src/views/common/MainLayout.tsx` — left sidebar with Chats and Network icon links, active-route highlight via `useLocation`. Plain CSS, no external framework. Reference: `mobile/ios/Actnet/Sources/Views/Common/MainTabView.swift`.
- [ ] **T13** — Wire routing + placeholder views: `src/views/chats/ChatsView.tsx` (flat list of `conversation.title` from AppContext), `src/views/network/NetworkView.tsx` (stub "Network" heading), `src/views/onboarding/SplashView.tsx` (wordmark text + "Enter Invite Link" button + "Scan QR Code" button). Update `App.tsx`: `isOnboarding` → `<SplashView>`, else `<Router>` with `<MainLayout>` and child routes for `/chats` and `/network`.

### Verify before starting Day 2
> Human sign-off required. Do not mark these `[x]` — leave for the user.

- [ ] `cargo tauri dev` runs without compile errors
- [ ] Tauri window opens (not blank)
- [ ] No JS console errors on load (F12 → Console)
- [ ] Mock mode shows 3 seeded conversations in ChatsView
- [ ] Sidebar Chats / Network links both work; active link is highlighted
- [ ] SplashView shown on first launch (before any account is created)

---

## Day 2 — Onboarding Flow

**Branch:** `desktop/day-2` (off `desktop/day-1`)
**Done when:** Full onboarding in mock mode: paste invite → identity picker → joining → account created → main layout. Re-launch restores session.

- [ ] **T14** — `SplashView.tsx` (full): replace placeholder. "Enter Invite Link" button opens InviteLinkEntryView route. "Scan QR Code" button opens QRScannerView route. No other buttons needed for mock mode. Reference: `Onboarding/SplashView.swift`.
- [ ] **T15** — `InviteLinkEntryView.tsx`: text field for invite URL, parse `actnet://` and `https://…/invite/…`, validate (show inline error for bad format), call `InviteToken.from()`, on success navigate to IdentityPickerView passing the token. Reference: `Onboarding/InviteLinkEntryView.swift`.
- [ ] **T16** — `IdentityPickerView.tsx`: receives parsed `InviteToken`. Shows existing accounts as "Add [name] to this server" rows + a "New identity" option. Selecting an existing account calls `joinServer` and navigates to main. Selecting new identity navigates to NewAccountView. Reference: `Onboarding/IdentityPickerView.swift`.
- [ ] **T17** — `NewAccountView.tsx`: display name input, submit calls `createAccount(serverUrl, serverName, displayName, inviteToken)` via AppContext, shows spinner, on success navigates to main layout. Blocks empty name. Reference: `Onboarding/NewAccountView.swift`.
- [ ] **T18** — `JoiningServerView.tsx`: full-screen progress indicator shown during `joinServer`. Shows server name. Reference: `Onboarding/JoiningServerView.swift`.
- [ ] **T19** — `QRScannerView.tsx`: desktop-adapted. Primary: file-upload input (reads image file, attempts QR decode via a JS QR library — use `jsQR` or `@zxing/browser`). On decode success, treat result as an invite URL and navigate to InviteLinkEntryView. Reference: `Onboarding/QRScannerView.swift` for UX shape only (not the camera logic).
- [ ] **T20** — Wire onboarding routes: set up `@solidjs/router` routes for `/onboarding/link`, `/onboarding/qr`, `/onboarding/identity`, `/onboarding/joining`, `/onboarding/new-account`. SplashView navigates to these. After account creation, `isOnboarding` flips false and router renders main layout.

### Verify before starting Day 3
> Human sign-off required.

- [ ] "Enter Invite Link" opens the link entry view
- [ ] Pasting a bad link shows an inline error
- [ ] Pasting a valid-format link (e.g. `https://example.com/invite/abc123`) reaches IdentityPickerView
- [ ] "New identity" → NewAccountView → entering "Alice" → spinner → main layout with conversation list
- [ ] Re-launching the app (stop + restart `cargo tauri dev`) shows the main layout, not onboarding

---

## Day 3 — Chats Tab

**Branch:** `desktop/day-3` (off `desktop/day-2`)
**Done when:** Full chats tab in mock mode — conversation list, message view, compose, delivery indicators, unread badges, avatars.

- [ ] **T21** — `AccountAvatar.tsx`: initials from display name, circle frame for humans, hexagon frame for bots (check `isBot` from AppContext). Solid color background derived from DID hash. Reference: `Common/AccountAvatar.swift`.
- [ ] **T22** — `ConversationRow.tsx`: `AccountAvatar`, display name, last-message preview (truncated), relative timestamp, unread count badge. Reference: `Chats/ConversationRow.swift`.
- [ ] **T23** — `ChatsView.tsx` (full): sorted conversation list using `ConversationRow`, click navigates to `/chats/:conversationId`. Shows unread total in sidebar. Reference: `Chats/ChatsView.swift`.
- [ ] **T24** — `MessageBubble.tsx`: own messages right-aligned (primary color), received left-aligned (surface color). Delivery indicator icons (⏱ sending, ✓ sent, ✓✓ delivered, ✓✓ read in blue). Timestamp. "edited" label. Deleted tombstone ("This message was deleted"). Reference: `Chats/MessageBubble.swift`.
- [ ] **T25** — `ConversationView.tsx`: loads messages via `loadMessagesFromStore` on mount, renders `MessageBubble` list, auto-scrolls to bottom on new message, calls `markAllMessagesRead` on mount and when tab is focused. Reference: `Chats/ConversationView.swift`.
- [ ] **T26** — `ComposeMessageView.tsx`: text input pinned to bottom of ConversationView, Send button, calls `sendMessage` / `sendGroupMessage` based on conversation type, clears on send, blocks empty send. Reference: `Chats/ComposeMessageView.swift`.
- [ ] **T27** — `RecoveryKeyBanner.tsx` + `OfflineBanner.tsx`: recovery banner shown in ChatsView header when no recovery phrase (stub condition for now — always hidden in mock mode). Offline banner shown from `aggregateConnectionState` in AppContext. Reference: `Chats/RecoveryKeyBanner.swift`, `Common/OfflineBanner.swift`.

### Verify before starting Day 4
> Human sign-off required.

- [ ] Conversation list shows 3 mock conversations, sorted newest-first
- [ ] Each row: avatar with initials, name, last-message preview, timestamp, unread badge
- [ ] Clicking a conversation opens the message view
- [ ] Own messages right, received messages left
- [ ] Sending a message appends it optimistically with a "sending" indicator
- [ ] Compose field clears after send; empty send is blocked
- [ ] Unread badge clears after opening a conversation

---

## Day 4 — Network Tab · Rust Bridge

**Branch:** `desktop/day-4` (off `desktop/day-3`)
**Done when:** NetworkView shows projects, ProjectWebView opens as a modal, DevServerActnetService calls real app-core Tauri commands (no `todo!()`s remain in `src-tauri/src/lib.rs`).

- [ ] **T28** — `NetworkView.tsx`: lists servers from accounts in AppContext, fetches projects per server via `fetchProjects`, renders project cards (name, description, "Open" button). Reference: `Network/NetworkView.swift`.
- [ ] **T29** — `ProjectWebView.tsx`: "Open" calls `requestProjectToken` then opens a `WebviewWindow` modal loading `projectUrl?token=…`. Navigation to `go.theavalanche.net` closes the modal and emits a deeplink event. Follow `desktop/CLAUDE.md §Tauri Architecture` for the WebviewWindow pattern. Reference: `Network/ProjectWebView.swift`.
- [ ] **T30** — Wire app-core into `src-tauri/Cargo.toml`: add `app-core` as a path dep. Read `core/crates/app-core/src/lib.rs` (top-level exports only) to understand the API. Implement Tauri commands: `create_account`, `login`, `send_dm`, `receive_messages`. Each command takes a `State<AppCoreState>` (an `Arc<Mutex<Option<AppCore>>>`).
- [ ] **T31a** — Tauri commands — conversations + messages: implement `load_conversations`, `load_messages`, `save_message`, `mark_messages_read`. Return serializable JSON structs that match the TypeScript models in `src/models/`.
- [ ] **T31b** — Background event emitter: add a Tauri command `start_event_loop(app_handle: AppHandle, state: State<AppCoreState>)`. Spawn a `tokio::task` that loops calling `core.next_events()` and emits each event via `app_handle.emit("actnet-event", payload)`. Called once from AppContext on login.
- [ ] **T32** — Tauri commands — groups: implement `send_group_message`, `create_group`, `invite_member`, `fetch_group_state`, `apply_pending_group_changes`, `is_group_member`. Read `core/crates/app-core/src/groups.rs` for signatures.
- [ ] **T33** — Tauri commands — profile + projects: implement `fetch_projects`, `request_project_token`, `get_account_info`, `contact_display_name`, `refresh_contact_profile`.
- [ ] **T34** — AppContext event subscription: in `startEventLoop`, replace the never-settling mock Promise with `listen('actnet-event', handler)` from `@tauri-apps/api/event` (devServer mode only — mock mode keeps the no-op). Parse event type field and dispatch to `handleIncomingMessage`, `applyDeliveryStatusUpdates`, or `loadConversationsFromStore`.

### Verify before starting Day 5
> Human sign-off required. Requires `make dev-all` running.

- [ ] NetworkView shows mock server and project list (mock mode)
- [ ] Clicking a project opens a modal window
- [ ] Modal can be closed
- [ ] Switching to devServer mode + pasting a real invite link creates an account
- [ ] Sending a message to testbot receives a reply
- [ ] Messages persist across app restart (devServer mode)
- [ ] Connection banner appears when `make dev-all` is stopped

---

## Day 5 — Settings · Polish · Parity Audit

**Branch:** `desktop/day-5` (off `desktop/day-4`)
**Done when:** DevSettings works, `docs/61-desktop-implementation.md` parity table updated, app builds on Windows.

- [ ] **T35** — Settings access: add a gear icon button to `MainLayout.tsx` sidebar footer. Create `src/views/settings/DevSettingsView.tsx`: mode selector (mock ↔ devServer), server URL, account count, logout button. Reference: iOS `Settings/` views.
- [ ] **T36** — `AccountsView.tsx` + `IdentityDetailView.tsx`: account list with server names, "Leave server" (calls `leaveServer` → removes account → returns to onboarding if last), "Delete identity" with confirmation. Reference: `Settings/AccountsView.swift`, `Settings/IdentityDetailView.swift`.
- [ ] **T37** — Deep link registration: add `actnet://` URL scheme to `tauri.conf.json`. Handle in Rust via `on_url_open` handler, emit `actnet-deeplink` event to frontend. AppContext listens and feeds the token into the onboarding flow (same as `pendingInviteToken` in iOS).
- [ ] **T38** — Native notifications: add `tauri-plugin-notification`. In `handleIncomingMessage`, fire a notification when the app is not focused or the conversation is not currently open. Reference: `NotificationPresenter` in iOS.
- [ ] **T39** — Parity audit: tick completed rows in `docs/61-desktop-implementation.md`. Run `/done` for post-implementation review. Commit the updated parity doc.
- [ ] **T40** — Windows build: run `cargo tauri build` in `desktop/`. Fix any Windows-specific issues (path separators, WebView2 availability check). Confirm `.msi` installer is generated.

### Verify — Day 5 complete
> Human sign-off required.

- [ ] Gear icon opens DevSettingsView from anywhere in the app
- [ ] Mode switch resets to onboarding
- [ ] AccountsView lists accounts; "Leave server" removes an account
- [ ] Clicking an `actnet://invite/TOKEN` link from outside the app opens onboarding
- [ ] Incoming message in devServer mode fires an OS notification (app not focused)
- [ ] `cargo tauri build` completes; `.msi` is generated
- [ ] No `any` in TypeScript (`grep -r "as any" desktop/src` returns nothing)
- [ ] `npm run build` completes with zero TypeScript errors

---

## Cross-cutting (check any day)

- [ ] No `console.error` during normal happy-path usage
- [ ] CSP is strict — no blocked resource warnings in DevTools Network tab
- [ ] `npm run build` (Vite production build) passes TypeScript with zero errors
