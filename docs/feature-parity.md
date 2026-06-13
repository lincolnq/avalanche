# Feature Parity Matrix

Tracks which user-facing features are implemented on each client platform. Update this file when implementing a feature on any platform.

**Platforms:**
- **iOS** — Swift/SwiftUI, UniFFI bindings (`mobile/ios/`)
- **Android** — Kotlin/Jetpack Compose, UniFFI bindings (`mobile/android/`) — not started
- **Desktop** — Electron + napi-rs bindings (`node/`) — not started
- **Bots/Node** — napi-rs bindings (`node/packages/app-core/`) — used by adminbot

Status: ✅ done · 🚧 partial · ⬜ not started · n/a not applicable

## Identity & accounts

| Feature | iOS | Android | Desktop | Bots/Node |
|---|---|---|---|---|
| Create account (passkey PRF) | ✅ | ⬜ | ⬜ | n/a |
| Create account (no passkey, bot flow) | n/a | n/a | n/a | ✅ |
| Login (re-open existing store) | ✅ | ⬜ | ⬜ | ✅ |
| Account recovery from blob | ✅ | ⬜ | ⬜ | ⬜ |
| Written-down recovery phrase | ⬜ | ⬜ | ⬜ | ⬜ |
| DID display / copy | ✅ | ⬜ | ⬜ | n/a |
| Multi-account switcher | ⬜ | ⬜ | ⬜ | n/a |

## Messaging — direct messages

| Feature | iOS | Android | Desktop | Bots/Node |
|---|---|---|---|---|
| Send DM | ✅ | ⬜ | ⬜ | ✅ |
| Receive DM (poll) | ✅ | ⬜ | ⬜ | ✅ |
| Receive DM (live WebSocket) | ✅ | ⬜ | ⬜ | ✅ |
| Delivery receipts (send) | ⬜ | ⬜ | ⬜ | ⬜ |
| Read receipts (send) | ⬜ | ⬜ | ⬜ | ⬜ |
| Message history (load stored) | ✅ | ⬜ | ⬜ | ⬜ |
| Conversation list with unread counts | ✅ | ⬜ | ⬜ | ⬜ |

## Messaging — groups

| Feature | iOS | Android | Desktop | Bots/Node |
|---|---|---|---|---|
| Create group | ✅ | ⬜ | ⬜ | ✅ |
| Invite member | ✅ | ⬜ | ⬜ | ✅ |
| Accept invite | ✅ | ⬜ | ⬜ | ⬜ |
| Decline invite | ✅ | ⬜ | ⬜ | ⬜ |
| Send group message | ✅ | ⬜ | ⬜ | ⬜ |
| Receive group messages (poll) | ✅ | ⬜ | ⬜ | ⬜ |
| Receive group messages (live WebSocket) | ✅ | ⬜ | ⬜ | ⬜ |
| Promote / remove member (admin) | ✅ | ⬜ | ⬜ | ⬜ |
| Join via invite link | ✅ | ⬜ | ⬜ | ⬜ |
| Group state / member list | ✅ | ⬜ | ⬜ | ⬜ |

## Contacts & profiles

| Feature | iOS | Android | Desktop | Bots/Node |
|---|---|---|---|---|
| Contact list | ✅ | ⬜ | ⬜ | ⬜ |
| Fetch & cache contact profile | ✅ | ⬜ | ⬜ | ⬜ |
| Set own display name | ✅ | ⬜ | ⬜ | ✅ |
| QR code / invite link sharing | ✅ | ⬜ | ⬜ | n/a |

## Infrastructure

| Feature | iOS | Android | Desktop | Bots/Node |
|---|---|---|---|---|
| Push notifications (APNs/FCM) | ✅ | ⬜ | n/a | n/a |
| Connection state display | ✅ | ⬜ | ⬜ | ⬜ |
| WebSocket reconnect with backoff | ✅ | ⬜ | ⬜ | ✅ |
| Recovery blob upload / refresh | ✅ | ⬜ | ⬜ | ⬜ |

## Notes

- **Android**: UniFFI generates Kotlin bindings as a byproduct of the iOS build (`make bindings`). The Kotlin glue exists; the UI layer does not.
- **Desktop**: The napi-rs bindings (`node/packages/app-core/`) expose the full `app-core` surface and are Electron-compatible. The UI layer does not exist yet.
- **Bots/Node**: Adminbot uses account creation, DMs, groups (create/invite), and admin events. Other features are available via the napi API but not exercised by any shipped bot.
