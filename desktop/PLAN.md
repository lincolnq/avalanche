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
   - **Tauri 2.x capabilities:** every `#[tauri::command]` added to `generate_handler!` must also be listed under `"permissions"` in `src-tauri/capabilities/default.json`. Forgetting this causes `invoke()` to fail at runtime with a capability error, even though the command compiles fine. Update capabilities whenever you add new commands.
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

- [x] **T01** — Frontend config: `package.json` (tauri 2.x, @tauri-apps/api, @tauri-apps/plugin-store, @tauri-apps/plugin-notification, @solidjs/router, vite, vite-plugin-solid, solid-js, typescript, jsQR — pin exact versions), `tsconfig.json` (`strict: true`, `jsx: "preserve"`, `jsxImportSource: "solid-js"`), `vite.config.ts` (solidPlugin + Tauri env).
- [x] **T02** — Entry point: `index.html`, `src/index.tsx` (`Object.freeze(Object.prototype)` then `render(() => <App/>, root)`), `src/App.tsx` (placeholder `<div>Actnet</div>` for now — wired properly in T13).
- [x] **T03** — Tauri Rust skeleton: `src-tauri/Cargo.toml` (tauri 2.x, tauri-plugin-store; no app-core yet), `src-tauri/tauri.conf.json` (CSP `default-src 'self'`, window 1200×800, identifier `net.actnet.desktop`), `src-tauri/src/lib.rs` (one `ping` command; initialize store plugin in builder: `.plugin(tauri_plugin_store::Builder::new().build())`). Also create `src-tauri/capabilities/default.json` — **required in Tauri 2.x or `invoke()` fails at runtime** even if the command compiles: `{"identifier":"default","local":true,"windows":["main"],"permissions":["core:default","store:default"]}`. Each command registered with `generate_handler!` must also have a matching entry under `"permissions"` in this file.
- [x] **T04** — Workspace + Makefile: add `desktop/src-tauri` to `core/Cargo.toml` `[workspace] members`. Add `make desktop` target (`cd desktop && cargo tauri dev`). Verify `cargo check` passes in the new crate.

### Phase 2 — TypeScript Models

- [x] **T05** — Core models: `src/models/Account.ts`, `src/models/Conversation.ts`, `src/models/Message.ts`. Mirror Swift structs exactly — field names are non-obvious, do not guess. Verified fields:
  - `Account`: `id: string` (IS the DID), `displayName: string`, `avatarData: Uint8Array | null` (binary blob, not a URL), `servers: ServerInfo[]`
  - `ServerInfo`: `id: string` (IS the server URL), `name: string`, `url: string`, `displayHost: string`
  - `Conversation`: `id`, `title`, `accountId` (owning account DID — NOT `ownerId`), `serverUrl`, `recipientDid?: string`, `groupId?: string`, `lastMessage?: string`, `lastMessageDate?: number` (unix-ms, NOT a Date object), `lastMessageKind: number`, `lastMessageMetadata?: string`, `lastMessageSenderDid?: string`, `isGroup: boolean`, `isRequest: boolean`, `isBlocked: boolean`
  - `Message`: `id`, `conversationId`, `senderAccountId: string` (NOT `senderId` or `authorId`), `body: string` (NOT `content` or `text`), `sentAtMs: number` (unix-ms Int64 — NOT a Date), `editedAtMs?: number`, `readAtMs?: number`, `deliveryStatus: DeliveryStatus`, `editCount: number`, `isDeleted: boolean`, `kind: number`, `metadata?: string`, `expireTimerSecs: number`, `expireAtMs?: number`
  - `DeliveryStatus` enum: `sending = 0`, `sent = 1`, `delivered = 2`, `read = 3`, `failed = 4` (must match exactly for persistence round-trips)
  
  Reference: `mobile/ios/Actnet/Sources/Models/` — read the actual Swift files to catch any fields missed above. Barrel export via `src/models/index.ts`.
- [x] **T06** — Secondary models: `src/models/ProjectInfo.ts` (reference `mobile/ios/Actnet/Sources/Models/ProjectInfo.swift`). `ServerInfo` is already defined in T05. For `InviteToken`: **TypeScript never decodes the token bytes** — the token format is complex (two shapes, Gatekeeper envelope and Bootstrap) and is opaque to the frontend. Instead: (a) create a `parseInviteUrl(url: string): string | null` helper that extracts the raw token string from URL path components `/i/<token>` or `/invite/<token>` — pure string parsing, no base64; (b) create an `InviteInfo` interface for the server-validated response: `{ token: string; serverUrl: string; serverName: string; inviterDid?: string; inviterDisplayName?: string; postOnboardingRedirect?: string }`. The raw token string is passed to `invoke('validate_invite', { token })` which returns an `InviteInfo`. There is **no `d`=inviterDid field in the token** — that comes from server validation. Add to `src/models/index.ts`.

### Phase 3 — Service Layer

- [x] **T07a** — `src/services/ActnetService.ts` — part 1: `ServiceMode` enum; interface methods for account ops (`createAccount`, `login`, `prepareAccount`, `finalizeAccount`, `recoverFromBlob`) and core messaging (`sendDm`, `sendGroupMessage`, `receiveMessages`, `nextEvents`, `saveMessage`, `loadConversations`, `loadMessages`, `markMessagesRead`, `unreadCount`). All return `Promise<T>`. Reference: `mobile/ios/Actnet/Sources/Services/ActnetService.swift`.
- [x] **T07b** — `src/services/ActnetService.ts` — part 2: append all remaining methods. Read every `AppCoreProtocol+*.swift` file (glob `mobile/ios/Actnet/Sources/Services/AppCoreProtocol+*.swift`) and add a TypeScript signature for each method found. Known groups: core group ops (`createGroup`, `inviteMember`, `fetchGroupState`, `applyPendingGroupChanges`, `isGroupMember`, `groupExpirySeconds`, `cachedGroupState`, `listGroups`), group member management (`acceptInvite`, `declineInvite`, `leaveGroup`, `removeMember`, `setGroupTitle`, `setGroupExpiry`, `changeMemberRole`, `cancelJoinRequest`, `approveJoinRequest`, `denyJoinRequest`), account/identity (`leaveServer`, `deleteIdentity`, `hasRecovery`, `setDisplayName`, `did`, `deviceId`), contact/profile (`touchContact`, `contactDisplayName`, `getAccountInfo`, `refreshContactProfile`, `fetchAndCacheProfile`, `listContacts`, `blockContact`, `unblockContact`, `primeContactProfile`), projects (`fetchProjects`, `requestProjectToken`), reactions/edit/delete (`sendReaction`, `sendEdit`, `sendDelete`, `loadReactions`, `loadMessageRevisions`), connection (`connectionState`, `waitForConnectionStateChange`, `ownDisplayName`), and any remaining methods in the extension files. Methods not needed for mock or devServer MVP can have `Promise<void>` return types for now.
- [x] **T08** — `src/services/MockActnetService.ts`: implements `ActnetService`. Implement only the methods needed for mock mode: `createAccount` and `login` return a mock core object; `loadConversations` returns 3 seeded conversations (2 DMs, 1 group); `loadMessages` returns 4–5 seeded messages per conversation; `sendDm`/`sendGroupMessage` resolve immediately; `nextEvents` never resolves (returns a Promise that never settles — the event loop sleeps in mock mode); all other methods resolve with a sensible zero/empty value. Keep the file focused — no need to mirror every mock behavior from iOS.
- [x] **T09** — `src/services/DevServerActnetService.ts`: implements `ActnetService` by calling `invoke('snake_case_command', args)` for every method. Add a matching stub `#[tauri::command]` in `src-tauri/src/lib.rs` for each (return `Err("not implemented".to_string())` — just enough to compile and register). Register all commands in the `tauri::Builder` call. **Also add `validate_invite` as a separate no-state free command** (it takes only a `token: String` arg and calls the free function `app_core::validate_invite(token)` — no `State<AppCoreState>` needed). Register it in the builder and add it to `capabilities/default.json`. **Update `src-tauri/capabilities/default.json`** — add every new command name to the `"permissions"` array, or `invoke()` will fail at runtime.

### Phase 4 — AppContext

- [x] **T10a** — `src/state/AppContext.tsx` — shell + account state: `createContext`, `createStore` with fields mirroring `AppState.swift` (`accounts`, `isOnboarding`, `serviceMode`, `selectedTab`, `connectionStates`). `AppProvider` component selects service based on `serviceMode`, reads persisted mode from `tauri-plugin-store` (import: `import { load } from '@tauri-apps/plugin-store'`; usage: `const store = await load('actnet.json')`). Export `useApp()` hook. Reference: `mobile/ios/Actnet/Sources/App/AppState.swift` top of file through `init`. **Solid `createStore` mutation rule:** never mutate nested store fields directly (`store.x.y = z` does nothing in Solid). Use path-based setters (`setStore('x', 'y', z)`) or `produce()` for complex updates — every reactive write must go through the setter or the UI will not update.
- [x] **T10b** — AppContext account lifecycle: add `restoreAccounts` (load persisted JSON from store, call `login` per account), `createAccount`, `switchMode` (resets all state), `logout`, `joinServer`. Persist accounts to `tauri-plugin-store` on create/remove. Reference: `AppState.swift` `// MARK: - Account lifecycle` section.
- [x] **T11a** — AppContext messaging — basic ops: add `conversations`, `messagesByConversation` to store. Implement `sendMessage` (optimistic append → invoke → update status), `sendGroupMessage`, `loadMessagesFromStore` (load-once guard), `loadConversationsFromStore` (load-once guard, parallel to `loadMessagesFromStore`), `markAllMessagesRead`, `findOrCreateDMConversation`, `findOrCreateGroupConversation`, `unreadCount`. Reference: `AppState.swift` `// MARK: - Messaging` section.
- [x] **T11b** — AppContext event loop + incoming messages: implement `startEventLoop` as an async recursive function — **not a timer**. `nextEvents()` is a long-poll that resolves only when Rust has events (it parks until data arrives); calling it in a `setInterval` would generate spurious invokes. Pattern: `const loop = async () => { if (!running.current) return; const events = await invoke('next_events'); handle(events); loop(); }; loop()`. Store `running` as a ref, set on login, cleared on logout/switchMode. In mock mode `nextEvents()` never settles — assign the promise to a ref and abandon it (do not await) on cleanup. Also implement `startConnectionStateLoop` with the same pattern using `waitForConnectionStateChange(last)`. Implement `handleIncomingMessage` (append to `messagesByConversation`, update conversation preview, fire display-name resolution) and `applyDeliveryStatusUpdates`. Add `aggregateConnectionState` as a derived accessor (a Solid `createMemo` over `connectionStates` — returns `'connected'` if all accounts connected, `'disconnected'` if any disconnected). T27 reads this for the `OfflineBanner`. Reference: `AppState.swift` `// MARK: - Connection state` and the `eventLoop` / `handleIncomingMessage` private methods.

### Phase 5 — Navigation Skeleton

- [x] **T12** — Router + MainLayout: `@solidjs/router` is already in `package.json` from T01. Create `src/views/common/MainLayout.tsx` — left sidebar with Chats and Network icon links, active-route highlight via `useLocation`. Plain CSS, no external framework. Reference: `mobile/ios/Actnet/Sources/Views/Common/MainTabView.swift`.
- [x] **T13** — Wire routing + placeholder views: `src/views/chats/ChatsView.tsx` (flat list of `conversation.title` from AppContext), `src/views/network/NetworkView.tsx` (stub "Network" heading), `src/views/onboarding/SplashView.tsx` (wordmark text + "Enter Invite Link" button + "Scan QR Code" button). Update `App.tsx`: `isOnboarding` → `<SplashView>`, else `<Router>` with `<MainLayout>` and child routes for `/chats` and `/network`.

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
- [ ] **T15** — `InviteLinkEntryView.tsx`: text field for invite URL, parse `actnet://` and `https://…/invite/…` using `parseInviteUrl(url)` from `src/models/InviteToken.ts` — returns a raw token string or `null` on bad format (show inline error). On success navigate to IdentityPickerView passing the raw token string. **Do not call `InviteToken.from()` — that function does not exist.** Reference: `Onboarding/InviteLinkEntryView.swift`.
- [ ] **T16** — `IdentityPickerView.tsx`: receives the raw token string from T15. On mount, calls `invoke('validate_invite', { token })` to get an `InviteInfo` (shows loading spinner, then server name + inviter name from result). Shows existing accounts as "Add [name] to [serverName]" rows + a "New identity" option. Selecting an existing account calls `joinServer(inviteInfo)` and navigates to main. Selecting new identity navigates to NewAccountView passing `inviteInfo`. Reference: `Onboarding/IdentityPickerView.swift`.
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

- [ ] **T21** — `AccountAvatar.tsx`: initials from display name, circle frame for regular accounts, hexagon frame for bot accounts. **`Account` has no `isBot` field** — bot detection is by DID prefix or a lookup; read `Common/AccountAvatar.swift` to see how iOS determines the frame shape, and match it exactly. Solid color background derived from DID hash. Reference: `Common/AccountAvatar.swift`.
- [ ] **T22** — `ConversationRow.tsx`: `AccountAvatar`, display name, last-message preview (truncated), relative timestamp, unread count badge. Reference: `Chats/ConversationRow.swift`.
- [ ] **T23** — `ChatsView.tsx` (full): sorted conversation list using `ConversationRow`, click navigates to `/chats/:conversationId`. Shows unread total in sidebar. Reference: `Chats/ChatsView.swift`.
- [ ] **T24** — `MessageBubble.tsx`: own messages right-aligned (primary color), received left-aligned (surface color). Delivery indicator icons (⏱ sending, ✓ sent, ✓✓ delivered, ✓✓ read in blue). Timestamp. "edited" label. Deleted tombstone ("This message was deleted"). **To determine "is this my message":** compare `message.senderAccountId` against the active account's `account.id` (the DID) via `useApp()` — there is no `isOwn` or `isMine` field on Message. Reference: `Chats/MessageBubble.swift`.
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
**Done when:** NetworkView shows projects, ProjectWebView opens as a modal, DevServerActnetService calls real app-core Tauri commands (no `todo!()`s remain in `src-tauri/src/lib.rs`). Note: group member management commands (`acceptInvite`, `leaveGroup`, `removeMember`, etc.) and `leaveServer`/`deleteIdentity` remain as `Err("not implemented")` stubs from T09 — this is acceptable for Day 4; they return a user-visible error in devServer mode but do not crash.

- [ ] **T28** — `NetworkView.tsx`: lists servers from accounts in AppContext, fetches projects per server via `fetchProjects`, renders project cards (name, description, "Open" button). Reference: `Network/NetworkView.swift`.
- [ ] **T29** — `ProjectWebView.tsx`: "Open" calls `requestProjectToken` then opens a `WebviewWindow` modal loading `projectUrl?token=…`. Navigation to `go.theavalanche.net` closes the modal and emits a deeplink event. Follow `desktop/CLAUDE.md §Tauri Architecture` for the WebviewWindow pattern. **Also add `"webviewWindow:allow-new"` to `capabilities/default.json`** — Tauri 2.x requires this capability to create a new window from TypeScript; without it the `WebviewWindow` constructor silently fails. Reference: `Network/ProjectWebView.swift`.
- [ ] **T30** — Wire app-core into `src-tauri/Cargo.toml`: add `app-core` as a path dep. Read `core/crates/app-core/src/lib.rs` (top-level exports only) to understand the API. Implement Tauri commands: `create_account`, `login`, `send_dm`, `receive_messages`, `validate_invite` (promote the T09 stub to a real implementation). Each command takes a `State<AppCoreState>` (an `Arc<Mutex<Option<AppCore>>>`). **`prf_output` handling:** `create_account` and `prepare_account` take `prf_output: Vec<u8>` — pass `vec![]` for the desktop no-passkey path (explicitly documented in app-core). **`recover_from_blob` exception:** this command hard-validates `prf_output.len() == 32` and rejects empty bytes — it must receive the output of `recovery_phrase_to_seed(phrase)` (a free function in app-core). Add a separate `recover_from_phrase(phrase: String, ...)` Tauri command that calls `recovery_phrase_to_seed` internally and passes the result.
- [ ] **T31a** — Tauri commands — conversations + messages: implement `load_conversations`, `load_messages`, `save_message`, `mark_messages_read`. Return serializable JSON structs that match the TypeScript models in `src/models/`.
- [ ] **T31b** — Background event emitter: add a Tauri command `start_event_loop(app_handle: AppHandle, state: State<AppCoreState>)`. Use `tokio::task::spawn_blocking` — **not** `tokio::task::spawn` — because `AppCore::next_events()` is synchronous blocking (it calls `block_on(rx.recv())` internally). Using a regular async task would starve Tauri's executor thread pool. Pattern: `tokio::task::spawn_blocking(move || loop { let events = core.next_events(); app_handle.emit("actnet-event", &events).ok(); })`. Called once from AppContext on login.
- [ ] **T32** — Tauri commands — groups: implement `send_group_message`, `create_group`, `invite_member`, `fetch_group_state`, `apply_pending_group_changes`, `is_group_member`. Read `core/crates/app-core/src/groups.rs` for signatures.
- [ ] **T33** — Tauri commands — profile + projects: implement `fetch_projects`, `request_project_token`, `get_account_info`, `contact_display_name`, `refresh_contact_profile`.
- [ ] **T34** — AppContext event subscription: in `startEventLoop`, replace the never-settling mock Promise with `listen('actnet-event', handler)` from `@tauri-apps/api/event` (devServer mode only — mock mode keeps the no-op). Parse event type field and dispatch to `handleIncomingMessage`, `applyDeliveryStatusUpdates`, or `loadConversationsFromStore`. **`listen()` returns an `UnlistenFn`** — store it in a ref and call it during cleanup (logout, `switchMode`) or duplicate handlers will fire on every re-login.

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
- [ ] **T37** — Deep link registration: In Tauri 2.x, URL scheme handling requires `tauri-plugin-deep-link` (add to `src-tauri/Cargo.toml` and initialize in builder) — it is **not** a bare `tauri.conf.json` URL scheme entry as in Tauri 1.x. Register the `actnet://` scheme in the plugin config, handle via the plugin's `on_open_url` callback, emit `actnet-deeplink` event to the frontend. Add `"deep-link:default"` to `capabilities/default.json`. AppContext listens and feeds the token into the onboarding flow (same as `pendingInviteToken` in iOS).
- [ ] **T38** — Native notifications: add `tauri-plugin-notification` to `src-tauri/Cargo.toml`, initialize in builder (`.plugin(tauri_plugin_notification::init())`), add `"notification:default"` to `capabilities/default.json`. Import `isPermissionGranted`, `requestPermission`, `sendNotification` from `@tauri-apps/plugin-notification` (already in `package.json` from T01). In `handleIncomingMessage`, fire a notification when the app is not focused or the conversation is not currently open. Reference: `NotificationPresenter` in iOS.
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
