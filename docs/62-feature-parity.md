# Feature Parity Matrix

Tracks which user-facing features are implemented on each client platform. Update this file when implementing a feature on any platform.

**Platforms:**
- **iOS** — Swift/SwiftUI, UniFFI bindings (`mobile/ios/`)
- **Android** — Kotlin/Jetpack Compose, UniFFI bindings (`mobile/android/`) — not started · see [`docs/60-android-implementation.md`](60-android-implementation.md)
- **Desktop** — Tauri + Solid/TypeScript (`desktop/`) — messaging/groups/contacts/device-linking implemented · see [`docs/61-desktop-implementation.md`](61-desktop-implementation.md)
- **Bots/Node** — napi-rs bindings (`node/packages/app-core/`) — used by adminbot

Status: ✅ done · 🚧 partial · ⬜ not started · n/a not applicable

## Identity & accounts

| Feature | iOS | Android | Desktop | Bots/Node |
|---|---|---|---|---|
| Create account (passkey PRF) | ✅ | ⬜ | n/a | n/a |
| Create account (no passkey, bot flow) | n/a | n/a | n/a | ✅ |
| Create account (recovery-phrase credential) | ⬜ | ⬜ | ✅ | n/a |
| Login (re-open existing store) | ✅ | ⬜ | ✅ | ✅ |
| Account recovery from blob | ✅ | ⬜ | ✅ | ⬜ |
| Written-down recovery phrase | ⬜ | ⬜ | ✅ | ⬜ |
| DID display / copy | ✅ | ⬜ | ✅ | n/a |
| Link a new device (pairing code) | ✅ | ⬜ | ✅ | n/a |
| Multi-account switcher | ⬜ | ⬜ | 🚧 | n/a |

## Messaging — direct messages

| Feature | iOS | Android | Desktop | Bots/Node |
|---|---|---|---|---|
| Send DM | ✅ | ⬜ | ✅ | ✅ |
| Receive DM (poll) | ✅ | ⬜ | ✅ | ✅ |
| Receive DM (live WebSocket) | ✅ | ⬜ | ✅ | ✅ |
| Delivery receipts (send) | ⬜ | ⬜ | ✅ | ⬜ |
| Read receipts (send) | ⬜ | ⬜ | ✅ | ⬜ |
| Message history (load stored) | ✅ | ⬜ | ✅ | ⬜ |
| Conversation list with unread counts | ✅ | ⬜ | ✅ | ⬜ |
| Reactions / edit / delete | ✅ | ⬜ | ✅ | ⬜ |
| Attachments + link previews | ✅ | ⬜ | ✅ | ⬜ |
| Disappearing-message timers | ✅ | ⬜ | ✅ | ⬜ |
| Clickable links (open in browser) | ✅ | ⬜ | ✅ | ⬜ |
| Multi-device sync (linked devices) | ✅ | ⬜ | ✅ | n/a |

## Messaging — groups

| Feature | iOS | Android | Desktop | Bots/Node |
|---|---|---|---|---|
| Create group | ✅ | ⬜ | ✅ | ✅ |
| Invite member | ✅ | ⬜ | ✅ | ✅ |
| Accept invite | ✅ | ⬜ | ✅ | ⬜ |
| Decline invite | ✅ | ⬜ | ✅ | ⬜ |
| Send group message | ✅ | ⬜ | ✅ | ⬜ |
| Receive group messages (poll) | ✅ | ⬜ | ✅ | ⬜ |
| Receive group messages (live WebSocket) | ✅ | ⬜ | ✅ | ⬜ |
| Promote / remove member (admin) | ✅ | ⬜ | ✅ | ⬜ |
| Join via invite link | ✅ | ⬜ | ✅ | ⬜ |
| Group state / member list | ✅ | ⬜ | ✅ | ⬜ |
| Group system messages (timeline) | ✅ | ⬜ | ✅ | ⬜ |

## Contacts & profiles

| Feature | iOS | Android | Desktop | Bots/Node |
|---|---|---|---|---|
| Contact list | ✅ | ⬜ | ✅ | ⬜ |
| Fetch & cache contact profile | ✅ | ⬜ | ✅ | ⬜ |
| Block / unblock + report | ✅ | ⬜ | ✅ | ⬜ |
| Set own display name | ✅ | ⬜ | ✅ | ✅ |
| QR code / invite link sharing | ✅ | ⬜ | 🚧 | n/a |

## Infrastructure

| Feature | iOS | Android | Desktop | Bots/Node |
|---|---|---|---|---|
| Push notifications (APNs/FCM) | ✅ | ⬜ | n/a | n/a |
| Native notifications (OS) | ✅ | ⬜ | ✅ | n/a |
| System tray / close-to-background | n/a | n/a | ✅ | n/a |
| Deep links (open conversation/invite) | ✅ | ⬜ | ✅ | n/a |
| Dark mode | ✅ | ⬜ | ✅ | n/a |
| Connection state display | ✅ | ⬜ | ✅ | ⬜ |
| WebSocket reconnect with backoff | ✅ | ⬜ | ✅ | ✅ |
| Recovery blob upload / refresh | ✅ | ⬜ | ✅ | ⬜ |
| Project webview (token-scoped, IPC-isolated) | ✅ | ⬜ | ✅ | n/a |
| Project login "Sign in with Avalanche" (OAuth, docs/25) | ✅ | ✅ | ⬜ | ⬜ |

## Notes

- **Android**: UniFFI generates Kotlin bindings as a byproduct of the iOS build (`make bindings`). The Kotlin glue exists; the UI layer does not.
- **Desktop**: Uses Tauri with a Solid/TypeScript frontend. `app-core` is exposed via Tauri commands (`src-tauri/src/lib.rs`) — no napi layer. The messaging UI (DMs, groups, contacts, attachments, link previews, reactions/edit/delete, disappearing timers, device linking, tray, dark mode) is implemented; remaining divergences are passkey signup (recovery-phrase only by design), QR generation (paste-link path only — 🚧), and full multi-account (single-account model today — 🚧). See `docs/61-desktop-implementation.md`.
- **Bots/Node**: Adminbot uses account creation, DMs, groups (create/invite), and admin events. Other features are available via the napi API but not exercised by any shipped bot.
- **Project login (docs/25)**: Desktop is intentionally deferred — the desktop app registers no deep-link handler yet, so it can't be a login *authorizer*; desktop *users* are served by authorizing from their phone (the cross-device QR flow). Tracked in `docs/02-todos-deferred.md` (desktop deep-link handler → desktop-as-authorizer). Bots/Node expose `oauthIssueCode`/`oauthApproveDevice` via napi (⬜ = no shipped bot uses them yet, but the surface is present).
