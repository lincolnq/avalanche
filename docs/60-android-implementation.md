# Android Implementation Plan

iOS is the reference implementation. This document tracks what needs to be built
to reach functional parity and how to maintain it going forward.

See `mobile/CLAUDE.md` for the parity rule and workflow.

---

## Tech Stack

| Concern | Android | iOS equivalent |
|---|---|---|
| Language | Kotlin | Swift |
| UI framework | Jetpack Compose | SwiftUI |
| State management | ViewModel + StateFlow | ObservableObject + @Published |
| Navigation | Navigation Compose | NavigationStack |
| Async | Coroutines + Flow | async/await + Task |
| Camera (QR) | CameraX + ZXing (`zxing-android-embedded`) | AVFoundation + VisionKit |
| WebView | Android WebView | WKWebView |
| Rust bridge | UniFFI-generated Kotlin bindings (AAR) | UniFFI-generated Swift bindings (XCFramework) |
| Persistence (metadata) | SharedPreferences (JSON) | UserDefaults (JSON) |
| Local crypto DB | SQLCipher via UniFFI Rust core | SQLCipher via UniFFI Rust core |

---

## Project Structure

```
mobile/android/
├── app/
│   ├── src/main/
│   │   ├── kotlin/app/actnet/
│   │   │   ├── ActnetApplication.kt
│   │   │   ├── MainActivity.kt
│   │   │   ├── models/
│   │   │   │   ├── Account.kt
│   │   │   │   ├── Conversation.kt
│   │   │   │   ├── Message.kt
│   │   │   │   ├── InviteToken.kt
│   │   │   │   └── ProjectInfo.kt
│   │   │   ├── viewmodels/
│   │   │   │   └── AppViewModel.kt
│   │   │   ├── services/
│   │   │   │   ├── ActnetService.kt
│   │   │   │   ├── MockActnetService.kt
│   │   │   │   └── DevServerActnetService.kt
│   │   │   └── ui/
│   │   │       ├── theme/
│   │   │       ├── navigation/
│   │   │       ├── onboarding/
│   │   │       │   ├── SplashScreen.kt
│   │   │       │   ├── QRScannerScreen.kt
│   │   │       │   ├── InviteLinkEntryScreen.kt
│   │   │       │   ├── IdentityPickerScreen.kt
│   │   │       │   ├── JoiningServerScreen.kt
│   │   │       │   └── NewAccountScreen.kt
│   │   │       ├── chats/
│   │   │       │   ├── ChatsScreen.kt
│   │   │       │   ├── ConversationRow.kt
│   │   │       │   ├── ConversationScreen.kt
│   │   │       │   ├── MessageBubble.kt
│   │   │       │   ├── ComposeDialog.kt
│   │   │       │   └── RecoveryKeyBanner.kt
│   │   │       ├── calls/
│   │   │       │   └── CallsScreen.kt
│   │   │       ├── network/
│   │   │       │   ├── NetworkScreen.kt
│   │   │       │   └── ProjectWebScreen.kt
│   │   │       └── common/
│   │   │           ├── AccountAvatar.kt
│   │   │           └── DevSettingsSheet.kt
│   │   └── AndroidManifest.xml
│   ├── libs/
│   │   └── app_core.aar
│   └── build.gradle.kts
├── build.gradle.kts
└── settings.gradle.kts
```

---

## Parity Map

Every row maps an iOS file to its Android equivalent. Update status as work lands.

### App Shell

| iOS | Android | Status |
|---|---|---|
| `ActnetApp.swift` | `MainActivity.kt` + `ActnetApplication.kt` | `[ ]` |
| `RootView.swift` | Root composable in `NavGraph.kt` | `[ ]` |
| `AppState.swift` | `AppViewModel.kt` | `[ ]` |

### Models

| iOS | Android | Status |
|---|---|---|
| `Account.swift` (Account, ServerInfo) | `Account.kt` | `[ ]` |
| `Conversation.swift` | `Conversation.kt` | `[ ]` |
| `Message.swift` (Message, DeliveryStatus) | `Message.kt` | `[ ]` |
| `InviteToken.swift` | `InviteToken.kt` | `[ ]` |
| `ProjectInfo.swift` | `ProjectInfo.kt` | `[ ]` |

### Services

| iOS | Android | Status |
|---|---|---|
| `ActnetService.swift` protocol | `ActnetService.kt` interface | `[ ]` |
| `MockActnetService.swift` | `MockActnetService.kt` | `[ ]` |
| `DevServerActnetService.swift` | `DevServerActnetService.kt` | `[ ]` |
| UniFFI `AppCore` / `AppCoreProtocol` | UniFFI-generated `AppCore` (from AAR) | `[ ]` |

### Onboarding

| iOS | Android | Status |
|---|---|---|
| `SplashView.swift` | `SplashScreen.kt` | `[ ]` |
| `QRScannerView.swift` | `QRScannerScreen.kt` | `[ ]` |
| `InviteLinkEntryView.swift` | `InviteLinkEntryScreen.kt` | `[ ]` |
| `IdentityPickerView.swift` | `IdentityPickerScreen.kt` | `[ ]` |
| `JoiningServerView.swift` | `JoiningServerScreen.kt` | `[ ]` |
| `NewAccountView.swift` | `NewAccountScreen.kt` | `[ ]` |

### Navigation

| iOS | Android | Status |
|---|---|---|
| `MainTabView.swift` | `MainScreen.kt` with bottom nav | `[ ]` |

### Chats Tab

| iOS | Android | Status |
|---|---|---|
| `ChatsView.swift` | `ChatsScreen.kt` | `[ ]` |
| `ConversationRow.swift` | `ConversationRow.kt` | `[ ]` |
| `ConversationView.swift` | `ConversationScreen.kt` | `[ ]` |
| `MessageBubble.swift` | `MessageBubble.kt` | `[ ]` |
| `ComposeMessageView.swift` | `ComposeDialog.kt` | `[ ]` |
| `RecoveryKeyBanner.swift` | `RecoveryKeyBanner.kt` | `[ ]` |

### Calls Tab

| iOS | Android | Status |
|---|---|---|
| `CallsView.swift` | `CallsScreen.kt` | `[ ]` |

### Network Tab

| iOS | Android | Status |
|---|---|---|
| `NetworkView.swift` | `NetworkScreen.kt` | `[ ]` |
| `ProjectWebView.swift` | `ProjectWebScreen.kt` | `[ ]` |

### Common

| iOS | Android | Status |
|---|---|---|
| `AccountAvatar.swift` | `AccountAvatar.kt` | `[ ]` |
| `DevSettingsView.swift` | `DevSettingsSheet.kt` | `[ ]` |

### State Behaviors (AppViewModel mirrors AppState)

| Behavior | Status |
|---|---|
| Account restoration on launch | `[ ]` |
| `createAccount(serverUrl, serverName, displayName)` | `[ ]` |
| `joinServer(serverUrl, serverName, existingAccountId)` | `[ ]` |
| `switchMode(mode)` | `[ ]` |
| `sendMessage(...)` — optimistic + core | `[ ]` |
| `markAllMessagesRead(conversationId, accountId)` | `[ ]` |
| `loadMessagesFromStore(conversationId, accountId)` | `[ ]` |
| `findOrCreateDMConversation(recipientDid, accountId)` | `[ ]` |
| WebSocket loop per account (coroutine, reconnect on error) | `[ ]` |
| `handleIncomingMessage()` — auto-create conversation, persist | `[ ]` |
| `applyDeliveryStatusUpdates()` | `[ ]` |
| `fetchProjects(serverUrl)` | `[ ]` |
| `requestProjectToken(accountId, projectUrl)` | `[ ]` |
| Conversation persistence (SharedPreferences) | `[ ]` |
| `unreadCount(for:)` | `[ ]` |

---

## Implementation Phases

### Phase 1 — Gradle project + Rust bindings

- Create `mobile/android/` Gradle project (Kotlin DSL, AGP 8.x, Compose BOM, min SDK 26)
- Add `make android-bindings` Makefile target: cross-compile `app-core` for `aarch64-linux-android` + `x86_64-linux-android`, generate Kotlin bindings via UniFFI, package as AAR in `mobile/android/app/libs/`
- `ActnetApplication.kt` and `MainActivity.kt` stubs
- `AndroidManifest.xml` with INTERNET and CAMERA permissions

**Done when:** `./gradlew assembleDebug` succeeds and `AppCore` is importable.

### Phase 2 — Models + AppViewModel

- All data classes (Account, Conversation, Message, DeliveryStatus, InviteToken, ProjectInfo)
- `ActnetService` interface, `MockActnetService`, `DevServerActnetService`
- `AppViewModel` with all StateFlow fields and methods from the behaviors table
- SharedPreferences JSON persistence

**Done when:** unit tests cover createAccount, sendMessage, markAllMessagesRead, handleIncomingMessage using mock service.

### Phase 3 — Navigation skeleton

- Material 3 theme
- `NavGraph.kt` with all destinations
- Bottom navigation: Calls / Chats / Network
- Root routing: onboarding vs. main based on `isOnboarding`

**Done when:** app launches, can tap through all destinations (screens can be stubs).

### Phase 4 — Onboarding screens

- `SplashScreen.kt`: logo, scan QR button, enter link button, dev settings icon
- `QRScannerScreen.kt`: CameraX + ZXing (`zxing-android-embedded`), parse `actnet://` or `https://…/invite/…` — ZXing chosen over ML Kit to avoid Google Play Services dependency (works on de-Googled Android)
- `InviteLinkEntryScreen.kt`: text field, parse on submit
- `IdentityPickerScreen.kt`: existing accounts list or straight to NewAccount
- `JoiningServerScreen.kt`: existing account display, join button
- `NewAccountScreen.kt`: display name field, avatar placeholder, continue button

**Done when:** can scan QR or enter link, create account against dev server, land in Chats.

### Phase 5 — Chats tab

- `AccountAvatar.kt`: initial letter circle, real image if avatarData set
- `RecoveryKeyBanner.kt`: yellow banner, stubbed dismissed like iOS
- `ChatsScreen.kt`: sorted list, gear + compose icons, empty state
- `ConversationRow.kt`: avatar, title, last message, timestamp, unread badge
- `ConversationScreen.kt`: message list, scroll to first unread/bottom, mark read, compose bar
- `MessageBubble.kt`: right/left alignment, delivery indicator icons, timestamp, "Edited" label
- `ComposeDialog.kt`: account picker (multi-account), recipient DID field

**Done when:** can send and receive messages in real time; delivery status updates; unread clears on open.

### Phase 6 — Network + Calls tabs

- `NetworkScreen.kt`: servers as section headers, projects with name + description
- `ProjectWebScreen.kt`: WebView with project URL + auth token
- `CallsScreen.kt`: empty state placeholder

### Phase 7 — Dev settings + polish

- `DevSettingsSheet.kt`: mode selector, server URL, account/conversation counts
- Keyboard handling in ConversationScreen (IME insets)
- WebSocket reconnect on network loss

---

## Open Questions

1. **NDK setup.** Need `cargo-ndk` and Android NDK installed. Document exact steps in `mobile/CLAUDE.md` once resolved.
2. **AAR packaging.** Exact `uniffi-bindgen` invocation + AAR assembly steps TBD.
3. **SQLCipher on Android.** Verify `libsqlcipher.so` is bundled correctly via the NDK build.
