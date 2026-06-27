# Android Implementation Plan

iOS is the reference implementation. This document tracks parity between the two
apps and the remaining gaps. See `mobile/CLAUDE.md` for the parity rule and workflow.

**Status legend:** `[x]` implemented · `[~]` partial / stubbed · `[ ]` not started

The app is essentially at file-for-file parity with iOS. The one missing source
file is the passkey ceremony (`PasskeyManager`); the remaining gaps are listed
under [Known gaps](#known-gaps).

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
| Rust bridge | UniFFI Kotlin in `mobile/android/Generated/` + `libapp_core.so` in `jniLibs/`, loaded via **JNA** (not an AAR) | UniFFI Swift bindings (XCFramework) |
| Push | FCM (Firebase Cloud Messaging) → relay | APNs → relay |
| Persistence (metadata) | SharedPreferences (JSON) | UserDefaults (JSON) |
| Local crypto DB | SQLCipher via UniFFI Rust core | SQLCipher via UniFFI Rust core |
| DB-key storage | Android Keystore (`KeystoreKeyManager`) | Secure Enclave (`SecureEnclaveKeyManager`) |

The Rust core is consumed directly (generated Kotlin as a source dir + per-ABI
`libapp_core.so`), **not** packaged as an AAR. Both `Generated/` and `jniLibs/`
are gitignored build artifacts produced by `make android-bindings`.

---

## Project Structure

Flat package `net.theavalanche.app` (the source layout under `kotlin/` is grouped
into folders but the package is intentionally flat — see the inspection-profile note
in `.gitignore`).

```
mobile/android/app/src/main/kotlin/
├── App/
│   ├── ActnetApplication.kt
│   ├── ActnetFirebaseMessagingService.kt   # FCM receiver (Android-only)
│   ├── AppViewModel.kt
│   ├── MainActivity.kt                      # hosts AppNavGraph (the NavHost)
│   ├── NotificationPresenter.kt
│   ├── PreviewSupport.kt                    # rememberPreviewAppViewModel (Android-only)
│   ├── PushManager.kt
│   └── RootView.kt                          # leftover shell; routing lives in MainActivity
├── Models/                                  # Account, Conversation, Message, InviteToken, ProjectInfo
├── Services/                                # ActnetService, Mock-, DevServer-, PublicServerInfo
├── Theme/                                   # Theme.kt, Type.kt
├── Utils/                                   # AppLog, AvalancheColors, Base64URL, KeystoreKeyManager
└── Views/
    ├── Chats/      # ChatsView, ConversationRow, ConversationView, MessageBubble,
    │               # ComposeMessageView, NameGroupView, GroupDetailView,
    │               # EditHistorySheet, RecipientTokenField, RecoveryKeyBanner
    ├── Common/     # AccountAvatar, ContactAvatar, CutCornerRectangle, Hexagon,
    │               # DisappearingMessagesPicker, LogViewerView, MainTabView,
    │               # OfflineBanner, QRCodeCameraView
    ├── Network/    # NetworkView, ProjectWebView
    ├── Onboarding/ # Splash, QRScanner, InviteLinkEntry, IdentityPicker, JoiningServer,
    │               # NewAccount, PasskeyExplainer, RecoveryExplainer, RecoveryConsole,
    │               # RecoveryPhraseSetup
    └── Settings/   # AccountsView, AddAccountView, BlockedContactsView,
                    # IdentityDetailView, ServerDetailView
```

---

## Parity Map

### App Shell

| iOS | Android | Status |
|---|---|---|
| `ActnetApp.swift` | `MainActivity.kt` + `ActnetApplication.kt` | `[x]` |
| `RootView.swift` | `AppNavGraph` in `MainActivity.kt` (`RootView.kt` is a superseded shell) | `[x]` |
| `AppState.swift` | `AppViewModel.kt` | `[x]` |
| `NotificationPresenter.swift` | `NotificationPresenter.kt` | `[x]` |
| `PushManager.swift` + AppDelegate push hooks | `PushManager.kt` + `ActnetFirebaseMessagingService.kt` | `[x]` (FCM wired; not yet exercised on a device) |

### Models

| iOS | Android | Status |
|---|---|---|
| `Account.swift` (Account, ServerInfo) | `Account.kt` | `[x]` |
| `Conversation.swift` | `Conversation.kt` | `[x]` |
| `Message.swift` (Message, DeliveryStatus) | `Message.kt` | `[x]` |
| `InviteToken.swift` | `InviteToken.kt` | `[x]` |
| `ProjectInfo.swift` | `ProjectInfo.kt` | `[x]` |

### Services

| iOS | Android | Status |
|---|---|---|
| `ActnetService.swift` protocol | `ActnetService.kt` interface | `[x]` |
| `AppCoreProtocol+Defaults.swift` | folded into `ActnetService.kt` (`AppCoreProtocol` defaults + `LiveAppCoreProtocol`) | `[x]` |
| `MockActnetService.swift` | `MockActnetService.kt` | `[x]` (cannot fabricate `PreparedAccount` — see gaps) |
| `DevServerActnetService.swift` | `DevServerActnetService.kt` | `[x]` |
| `PublicServerInfo.swift` | `PublicServerInfo.kt` | `[x]` |
| `PasskeyManager.swift` | — (Credential Manager) | `[ ]` **missing** |
| UniFFI `AppCore` / `AppCoreProtocol` | UniFFI-generated `AppCore` (`Generated/`, via JNA) | `[x]` |

### Onboarding

| iOS | Android | Status |
|---|---|---|
| `SplashView.swift` | `SplashView.kt` | `[x]` |
| `QRScannerView.swift` | `QRScannerView.kt` | `[x]` |
| `InviteLinkEntryView.swift` | `InviteLinkEntryView.kt` | `[x]` |
| `IdentityPickerView.swift` | `IdentityPickerView.kt` | `[x]` |
| `JoiningServerView.swift` | `JoiningServerView.kt` | `[x]` (existing-account join path is functional) |
| `NewAccountView.swift` | `NewAccountView.kt` | `[x]` (avatar photo picker stubbed) |
| `PasskeyExplainerView.swift` | `PasskeyExplainerView.kt` | `[~]` UI done; passkey ceremony stubbed |
| `RecoveryExplainerView.swift` | `RecoveryExplainerView.kt` | `[~]` UI done; Credential Manager call stubbed |
| `RecoveryConsoleView.swift` | `RecoveryConsoleView.kt` | `[~]` passkey-with-DID path only; PLC homeserver resolution stubbed |
| `RecoveryPhraseSetupView.swift` | `RecoveryPhraseSetupView.kt` | `[x]` |
| `LinkNewDeviceView.swift` | `LinkNewDeviceView.kt` | `[x]` device linking, new-device side (docs/04 §4) |

### Navigation

| iOS | Android | Status |
|---|---|---|
| `MainTabView.swift` | `MainTabView.kt` (Chats + Network bottom nav) | `[x]` |
| NavigationStack + sheets | `AppNavGraph` in `MainActivity.kt` | `[x]` |

### Chats

| iOS | Android | Status |
|---|---|---|
| `ChatsView.swift` | `ChatsView.kt` | `[x]` (recovery-key banner check hardcoded `false` — needs FFI) |
| `ConversationRow.swift` | `ConversationRow.kt` | `[x]` |
| `ConversationView.swift` | `ConversationView.kt` | `[x]` |
| `MessageBubble.swift` | `MessageBubble.kt` | `[x]` |
| `ComposeMessageView.swift` | `ComposeMessageView.kt` | `[x]` |
| `NameGroupView.swift` | `NameGroupView.kt` | `[x]` |
| `GroupDetailView.swift` | `GroupDetailView.kt` | `[x]` |
| `EditHistorySheet.swift` | `EditHistorySheet.kt` | `[x]` |
| `RecipientTokenField.swift` | `RecipientTokenField.kt` | `[x]` (currently unused; composer uses its own chip field) |
| `RecoveryKeyBanner.swift` | `RecoveryKeyBanner.kt` | `[x]` |

### Network

| iOS | Android | Status |
|---|---|---|
| `NetworkView.swift` | `NetworkView.kt` | `[x]` |
| `ProjectWebView.swift` | `ProjectWebView.kt` | `[x]` |

### Settings

| iOS | Android | Status |
|---|---|---|
| `AccountsView.swift` | `AccountsView.kt` | `[x]` |
| `AddAccountView.swift` | `AddAccountView.kt` | `[x]` |
| `BlockedContactsView.swift` | `BlockedContactsView.kt` | `[x]` |
| `IdentityDetailView.swift` | `IdentityDetailView.kt` | `[x]` |
| `LinkDeviceView.swift` | `LinkDeviceView.kt` | `[x]` device linking, existing-device side (docs/04 §4) |
| `ServerDetailView.swift` | `ServerDetailView.kt` | `[x]` |

### Common

| iOS | Android | Status |
|---|---|---|
| `AccountAvatar.swift` | `AccountAvatar.kt` | `[x]` |
| `ContactAvatar.swift` | `ContactAvatar.kt` | `[x]` |
| `CutCornerRectangle.swift` | `CutCornerRectangle.kt` | `[x]` |
| `Hexagon.swift` | `Hexagon.kt` | `[x]` |
| `DisappearingMessagesPicker.swift` | `DisappearingMessagesPicker.kt` | `[x]` |
| `LogViewerView.swift` | `LogViewerView.kt` | `[x]` |
| `OfflineBanner.swift` | `OfflineBanner.kt` | `[x]` |
| `QRCodeCameraView.swift` | `QRCodeCameraView.kt` | `[x]` |

### Utils

| iOS | Android | Status |
|---|---|---|
| `AppLog.swift` | `AppLog.kt` | `[x]` |
| `AvalancheColors.swift` | `AvalancheColors.kt` | `[x]` |
| `Base64URL.swift` | `Base64URL.kt` | `[x]` |
| `SecureEnclaveKeyManager.swift` | `KeystoreKeyManager.kt` (wired into login/createAccount/recovery) | `[x]` |

### State Behaviors (AppViewModel mirrors AppState)

| Behavior | Status |
|---|---|
| Account restoration on launch | `[x]` |
| `createAccount(...)` / `prepareAccount` / `finishAccountRegistration` | `[x]` |
| `joinServer(...)` / `leaveServer(...)` | `[x]` |
| `switchMode(mode)` | `[x]` |
| `sendMessage(...)` / `sendGroupMessage(...)` — optimistic + core | `[x]` |
| `addOptimisticMessage(...)` | `[x]` |
| `editMessage` / `deleteMessage` / edit-history | `[x]` |
| `toggleReaction` / `reactions` | `[x]` |
| Disappearing-messages timer (`setConversationTimer` / group expiry) | `[x]` |
| `markAllMessagesRead` / `markMessagesReadUpTo` / read receipts | `[x]` |
| `loadMessagesFromStore` / `loadConversationsFromStore` | `[x]` |
| Block / unblock / report; message-request gate | `[x]` |
| Group create / invite / join / approve / roles / leave | `[x]` |
| WebSocket loop per account (coroutine, reconnect on error) | `[x]` |
| `handleIncomingMessage()` — auto-create conversation, persist, notify | `[x]` |
| `applyDeliveryStatusUpdates()` | `[x]` |
| `fetchProjects()` / `requestProjectToken()` | `[x]` |
| Conversation persistence (SharedPreferences) | `[x]` |
| `unreadCount(...)` + notification badge | `[x]` |
| Push token registration (`registerPushToken` via FCM) | `[x]` (runtime-untested) |
| `recoverAccount(...)` from relay blob | `[~]` (works given a DID; phrase-only path needs PLC resolution) |

---

## Known gaps

Highest-impact first:

1. **Passkey ceremony (Credential Manager).** No `PasskeyManager` equivalent.
   `PasskeyExplainerView` / `RecoveryExplainerView` stub the `androidx.credentials`
   create/get with the PRF extension. This **dead-ends the new-account onboarding
   path** (nav is wired through to the passkey step, but "Create Passkey" does
   nothing) and blocks passkey-based recovery.
2. **PLC homeserver resolution.** `resolveHomeserverFromPlc(did)` in
   `RecoveryConsoleView` is stubbed, so phrase-based recovery can't find a DID's
   homeserver. Passkey recovery (DID embedded) is the only path, and it needs #1.
3. **`hasRecoveryKey` banner check.** Hardcoded `false` in `ChatsView`; needs a new
   Rust FFI method (full cross-platform cycle).
4. **Avatar photo picker.** `NewAccountView` has a stub where iOS lets the user pick
   an avatar image. Avatars display fine; selection is missing.
5. **Push: runtime-untested + no cold-process sync.** FCM is wired but never
   exercised on a device. A wakeup delivered to a killed process defers to the next
   app launch rather than syncing headlessly (`onMessageReceived` only drains when
   an `AppViewModel` is live).
6. **Mock `PreparedAccount`.** The mock service can't fabricate a `PreparedAccount`
   (UniFFI native-pointer object), so two-stage-creation previews/tests can't use
   the mock; they need the live service.

## Build status

Phases 1–6 of the original plan (Gradle + bindings, models + AppViewModel,
navigation, onboarding screens, Chats, Network) are complete. Phase 7 (polish) is
largely done: keyboard/IME insets, WebSocket reconnect, log viewer, notifications,
and Keystore-backed DB keys all landed. The runtime/build chain (`make
android-bindings` + `./gradlew assembleDebug`) is green; the app launches past the
FFI boundary on the emulator.

## Deferred / Known Limitations

- **SharedPreferences metadata exposure.** The identity list (own DIDs, display
  names, server URLs, DB filename) is stored in SharedPreferences as plain JSON,
  protected only by the app sandbox + file-based encryption, not a user-controlled
  key. Message content and the contact graph are *not* exposed — those live in the
  per-identity SQLCipher DBs, which are now keyed from the Android Keystore
  (`KeystoreKeyManager`). The remaining fix would be a small `manifest.db` keyed from
  the Keystore for the metadata too. Deferred: low sensitivity relative to cost.
