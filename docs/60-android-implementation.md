# 60 — Android implementation notes

> **Status:** Built — the Android app is a near file-for-file port of iOS. Feature-level parity
> is tracked only in docs/62.
> **Last verified against code:** 2026-10-03

## Summary

The Android app (`mobile/android/`) is Kotlin + Jetpack Compose over the same Rust
`app-core` as iOS, reached through UniFFI-generated Kotlin. iOS is the reference
implementation; Android mirrors its structure screen for screen (`AppViewModel` mirrors
`AppState`, and so on). This doc covers how the Android app is built and wired. **For which
features exist on which platform, see docs/62** — this doc deliberately has no parity table.

## Current design

### Tech stack

| Concern | Android | iOS equivalent |
|---|---|---|
| UI | Jetpack Compose | SwiftUI |
| State | `ViewModel` + `StateFlow` (`AppViewModel`) | `ObservableObject` (`AppState`) |
| Navigation | Navigation Compose (`AppNavGraph` in `MainActivity.kt`) | `NavigationStack` |
| Async | Coroutines; FFI calls via `withContext(Dispatchers.IO)` | async/await; `Task.detached` for sync FFI |
| Rust bridge | UniFFI Kotlin in `mobile/android/Generated/` + per-ABI `libapp_core.so` in `jniLibs/`, loaded via JNA (not an AAR) | UniFFI Swift + XCFramework |
| Push | FCM (`ActnetFirebaseMessagingService`) or UnifiedPush (`UnifiedPushService`) → relay | APNs → relay |
| Passkeys | Credential Manager + WebAuthn PRF (`PasskeyManager.kt`) | AuthenticationServices |
| QR camera | CameraX + ZXing | AVFoundation + VisionKit |
| Metadata persistence | SharedPreferences (JSON) | UserDefaults (JSON) |
| DB key | Android Keystore (`KeystoreKeyManager`) | Secure Enclave (`SecureEnclaveKeyManager`) |

`Generated/` and `jniLibs/` are gitignored build artifacts from `make android-bindings`.
`make android` builds the debug APK; Gradle needs JDK 17+ (the Makefile falls back to Android
Studio's bundled JBR). Release signing and AAB upload: `make android-release` /
`make android-bundle` (see `RELEASE.md`).

### Source layout

Flat package `net.theavalanche.app`; files under `app/src/main/kotlin/` are grouped into
folders that mirror iOS `Sources/`:

```
App/         ActnetApplication, MainActivity (AppNavGraph), AppViewModel, PushManager,
             ActnetFirebaseMessagingService, UnifiedPushService, NotificationPresenter
Models/      Account, Conversation, Message, InviteToken, ProjectInfo
Services/    ActnetService (interface + LiveAppCoreProtocol), Mock-, DevServer-, PublicServerInfo
Theme/, Utils/  AvalancheColors, AppLog, Base64URL, KeystoreKeyManager
Views/Chats, Views/Common, Views/Network, Views/Onboarding, Views/Settings
```

### FFI usage

All UniFFI calls go through the `ActnetService` interface (never the generated `AppCore`
directly from views). Sync exports run in `withContext(Dispatchers.IO)`. The two long-wait
methods (`next_events`, `wait_for_connection_state_change`) are native async exports and are
awaited directly from `viewModelScope` coroutines — do not wrap them in `Dispatchers.IO`
(root `CLAUDE.md`, pattern 4). Per-account event and connection loops run in
`viewModelScope` and are cancelled when the ViewModel clears.

### Passkeys: operational notes

Passkey create and recover work on-device. The Digital Asset Links file is served at
`https://theavalanche.net/.well-known/assetlinks.json` (source:
`web/static/.well-known/assetlinks.json`) with both
`delegate_permission/common.get_login_creds` and `common.handle_all_urls` and the app's
signing fingerprints — the Android analog of iOS's `webcredentials:theavalanche.net`.

- **Cloudflare must not cache `/.well-known/*`.** A stale or negative edge-cached
  `assetlinks.json` makes Google Play Services fail RP-ID validation (`50152 RP ID cannot be
  validated`). Purge on change and keep a cache-bypass rule for `/.well-known/*` (this also
  protects the iOS AASA file). Play Services caches validation results too.
- **Degoogled devices** need a framework credential provider that supports PRF (e.g.
  1Password), since the Play Services provider is unavailable.

## Known gaps

- **Recovery-key banner** — `ChatsView.kt` hardcodes `hasRecoveryKey = false` pending an FFI
  method.
- **Avatar setting** — avatars display, but there is no own-avatar or group-avatar picker or
  upload (iOS has `setOwnAvatar` / `setGroupAvatar`). `NewAccountView` has a TODO stub.
- **Push from a killed process** — a wakeup with no live `AppViewModel` is logged and deferred
  to the next launch rather than syncing headlessly (`ActnetFirebaseMessagingService.onMessageReceived`).
  Android has no equivalent of the iOS Notification Service Extension.
- **Mock `PreparedAccount`** — the mock service can't fabricate this UniFFI object, so
  two-stage-signup previews and tests need the live service.
- **Metadata in SharedPreferences** — the identity list (own DIDs, display names, server URLs,
  DB filenames) is plain JSON protected only by the app sandbox and file-based encryption.
  Message content and contacts are in the Keystore-keyed SQLCipher DBs. Fix: a small
  Keystore-keyed `manifest.db`. Low priority.

## Rationale

- **Native Compose rather than a shared UI toolkit**, matching the Signal-style
  "Rust core + native UI" architecture (docs/01). The cost is a hand-maintained port of every
  iOS screen.
- **JNA + generated sources rather than an AAR** keeps the build simple: no separate library
  module to publish, and Rust changes only recompile the `.so`.
