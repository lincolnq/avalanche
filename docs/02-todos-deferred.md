# Deferred TODOs

## Mobile app
- Recovery key UI: setup and backup flows (banner currently always shows, hardcoded false)
- Scroll-position-based read marking (see docs/31-read-tracking.md, Stage B)
- Delivery receipts — auto-send on message receive (see docs/31-read-tracking.md, Stage D)
- Read receipt user preference toggle (send_read_receipts setting)
- Scroll position: remove invisible "bottom" anchor hack in ConversationView (Color.clear spacer) when scroll position saving is implemented

## Android app

The iOS app (`mobile/ios/`) is the reference implementation. The Android app (Kotlin/Jetpack Compose) should mirror it structurally. See `docs/33-android.md` for the full implementation guide, including directory structure, iOS→Android mapping table, build setup, and code sketches for each layer.

### Infrastructure
- [ ] Scaffold Gradle project at `mobile/android/` (see `docs/33-android.md` §3 for directory structure)
- [ ] Add `make android-ndk` Makefile target: compile `libapp_core.so` for `arm64-v8a` and `x86_64` via `cargo-ndk`
- [ ] Add `make android` Makefile target: `make bindings` + `make android-ndk`
- [ ] Update `CLAUDE.md` build commands section to document Android targets and prerequisites
- [ ] Configure `gradle/libs.versions.toml` with Compose BOM, Navigation, ViewModel, DataStore, JNA, CameraX, ML Kit

### Core layer
- [ ] `AppCoreInterface.kt` — Kotlin interface mirroring `AppCoreProtocol` from `ActnetService.swift`
- [ ] `MockActnetService.kt` + `MockAppCore` — mock implementation (mirrors `MockActnetService.swift`): 100 ms send delay, echo reply after 1.5 s, seed conversations on `createAccount`
- [ ] `DevServerActnetService.kt` — wraps UniFFI-generated `AppCore` class directly
- [ ] `AppViewModel.kt` — `AndroidViewModel` with `StateFlow` (mirrors `AppState.swift`):
  - `restoreAccounts()` called from `init`; loads from `DataStore<Preferences>`
  - Per-account `AppCoreInterface` instances in `MutableMap<String, AppCoreInterface>`
  - WebSocket loop as `viewModelScope.launch(Dispatchers.IO)` coroutine per account, 2 s backoff on error
  - `ServiceMode` enum (MOCK / DEV_SERVER); switching mode clears all state

### Models
- [ ] `Account.kt` — data class with `id` (DID), `displayName`, `avatarData`, `servers`
- [ ] `Conversation.kt` — `@Serializable`; exclude `lastMessage` from JSON (`@Transient`) for same security reason as iOS
- [ ] `Message.kt` — `DeliveryStatus` enum (SENDING/SENT/DELIVERED/READ matching raw Int values from iOS)
- [ ] `ProjectInfo.kt`, `InviteToken.kt`

### UI — Onboarding
- [ ] `SplashScreen.kt` — scan QR / enter invite link entry points
- [ ] `InviteLinkEntryScreen.kt` — parse `actnet://invite/<server>/<token>` deep links
- [ ] `IdentityPickerScreen.kt` — existing account list + "Create fresh identity"
- [ ] `NewAccountScreen.kt` — display name input, optional avatar, calls `vm.createAccount(…)`
- [ ] `JoiningServerScreen.kt` — join new server with existing account
- [ ] `QRScannerScreen.kt` — CameraX preview + ML Kit `BarcodeScanning.getClient()`

### UI — Chats tab
- [ ] `ChatsScreen.kt` — `LazyColumn` sorted by `lastMessageDateMs`; unread badge; FAB for compose
- [ ] `ConversationScreen.kt` — message thread, auto-scroll to bottom, mark read on appear
- [ ] `MessageBubble.kt` — sent (right, blue) / received (left, gray); delivery icons ⏱/✓/✓✓gray/✓✓blue
- [ ] `ComposeMessageScreen.kt` — recipient DID input, account picker

### UI — Calls tab
- [ ] `CallsScreen.kt` — placeholder (mirrors iOS `CallsView.swift`)

### UI — Network tab
- [ ] `NetworkScreen.kt` — server/project list, async load, token request on tap
- [ ] `ProjectWebScreen.kt` — `AndroidView { WebView(context) }` for Project UIs

### UI — Common
- [ ] `AccountAvatar.kt` — avatar composable with initials fallback (mirrors `AccountAvatar.swift`)
- [ ] `DevSettingsScreen.kt` — service mode toggle, account/conversation counts (mirrors `DevSettingsView.swift`)

### Permissions & manifest
- [ ] `INTERNET` and `CAMERA` permissions
- [ ] `POST_NOTIFICATIONS` permission (API 33+) for future push
- [ ] `actnet://invite` intent filter for deep links (mirrors iOS URL scheme)
- [ ] FCM service stub for when push notifications are implemented

### Testing
- [ ] `MockServiceTest.kt` — verify `MockAppCore.receiveMessagesWs()` delivers echo reply after ≥1.5 s
- [ ] Cross-platform interop test: iOS sends encrypted DM, Android decrypts it against a real test homeserver (add to `core/crates/app-core/tests/`)
## Crypto / protocol
- Kyber prekey pool: upload one-time Kyber prekeys with server-side atomic consumption (like EC one-time prekeys), keep one last-resort key. Currently only a single last-resort key is used.
- Protobuf message envelope: plaintext is raw bytes, design calls for ContentMessage protobuf (proto/content.proto)
- DB encryption key from Secure Enclave instead of hardcoded "dev-placeholder-key"

## Server
- WebSocket request/response framing: tunnel HTTP-style request/response pairs over the WebSocket (like Signal does), with request IDs and correlated responses. Move message sends and acks onto the WS transport, replacing the current split of HTTP sends + WS acks. This gives persistent-connection benefits while keeping clear success/failure semantics per operation.
- Message expiry: background task to delete expired messages, configurable per-group/DM
- Timer change sync message: add a `TimerChangeMessage` body variant to the ContentMessage protobuf so that when a user changes the conversation expiry timer, a control message is sent to the other participant(s) to update their local setting
- DID document resolution endpoint (GET /.well-known/did/:did)

## Project-wide
- Settle on a better name: rename repo, update bundle IDs, update `actnet://` URL scheme to match, update all references in code and docs

## Big milestones (not yet started)
- Groups: action-bound (zkgroup) and cross-server casual (Sender Keys)
- Invite links & onboarding: QR codes, deep links, auto-enrollment into groups/Projects
- Push notifications (see Push Notifications section below)
- Projects framework: SDK, scoped bot permissions, JS bridge for webviews
- First-party Projects: channel directory, team assignment, action-day map, Q&A bot, collab docs, engagement tracking
- Federation: server-to-server protocol, cross-server DMs, full DID portability (PLC directory), guest access
- Android app (see `docs/33-android.md` for full implementation guide)
- Calls: voice and video (VoIP)
- Public profiles: client-owned profile blobs (display name, avatar, bio) pushed to servers
- Multi-account support in mobile app

## Push Notifications

### 1. Push relay service (`core/crates/relay/`)
- [ ] DB table: `(pseudonym) → (device_token, platform, registered_at)`
- [ ] Client endpoint: register/update/delete pseudonym-to-token mapping
- [ ] Homeserver endpoint: accept wakeup-by-pseudonym, fire content-free push to APNs/FCM
- [ ] Pseudonym rotation: grace period (~1 week) where old pseudonym still works
- [ ] APNs integration (content-free wakeup payload)
- [ ] FCM integration (content-free wakeup payload)

### 2. Server integration
- [ ] On message delivery to offline device, look up push pseudonym and ping relay
- [ ] Hook into existing WebSocket connection tracking to determine online/offline
- [ ] Server config: relay URL

### 3. Mobile client (iOS first, then Android)
- [ ] Request push permission during signup
- [ ] Register device token with APNs/FCM
- [ ] Register per-(user, server) pseudonym with relay on account creation
- [ ] On wakeup: connect WebSocket, fetch queued messages
- [ ] Periodic pseudonym rotation (default weekly)
- [ ] Opt-out setting for high-risk users (poll-only mode)

### 4. Testing & privacy
- [ ] Verify relay payloads contain zero user-identifiable content
- [ ] Verify relay logs contain only pseudonyms + timestamps
- [ ] Pseudonym rotation grace period test
- [ ] APNs/FCM sandbox integration test

## Desktop client (future)

A desktop client (macOS, Windows, Linux) can share most of its codebase with the Android app via **Kotlin Multiplatform + Compose Multiplatform** (JetBrains). The UniFFI-generated Kotlin bindings use JNA under the hood, and JNA loads native libraries on the desktop JVM too (`.dylib` on macOS, `.so` on Linux, `.dll` on Windows) — so the same `AppCoreInterface` / `AppViewModel` layer from the Android app is reusable with minimal changes.

Defer until the Android app reaches a stable milestone.

- [ ] Evaluate Compose Multiplatform maturity for production desktop use
- [ ] Add `make desktop-macos`, `make desktop-linux`, `make desktop-windows` Makefile targets to compile `libapp_core` as a shared library for each OS (requires cross-compilation toolchain setup)
- [ ] Scaffold a `desktop/` Kotlin Multiplatform module that shares `AppViewModel`, models, and service interfaces from `mobile/android/`
- [ ] Handle per-OS secure storage: macOS Keychain, Windows Credential Manager, Linux Secret Service / `libsecret`
- [ ] Handle per-OS notification APIs: macOS `UserNotifications`, Windows `Windows.UI.Notifications`, Linux `libnotify`
- [ ] Handle per-OS deep link / URL scheme registration for `actnet://`
