# 62 — Feature parity matrix

> **Status:** Built — the single source of truth for which user-facing features exist on
> which client. Re-baselined against the code on the verified date.
> **Last verified against code:** 2026-10-04

This is the **only** parity tracker; docs/60 (Android) and docs/61 (Desktop) hold platform
implementation notes, not tables. Update this file in the same change that adds, removes, or
changes a feature on any platform.

**Platforms:** iOS (`mobile/ios/`, reference implementation) · Android (`mobile/android/`) ·
Desktop (`desktop/`, Tauri) · Bots (`node/packages/`, napi `@theavalanche/app-core`).

**Cell values:** **Yes** — built · **Partial** — see note · **No** — not built ·
**n/a** — not applicable to the platform. For Bots, "Yes" means a shipped bot uses it;
"API" means the napi surface exposes it but no shipped bot uses it.

## Identity and accounts

| Feature | iOS | Android | Desktop | Bots |
|---|---|---|---|---|
| Create account with passkey (PRF) | Yes | Yes | No (by design, docs/61) | n/a |
| Create account with recovery phrase | Yes | Yes | Yes (only path) | n/a |
| Bot account (`did:local:`) | n/a | n/a | n/a | Yes |
| Recover account (passkey or phrase → blob) | Yes | Yes | Yes (phrase) | n/a |
| Link a new device (pairing code / QR) | Yes | Yes | Yes | n/a |
| Several accounts at once, one merged inbox | Yes | Yes | Yes | n/a |
| Per-account tabs in the chat list (docs/37) | Yes | Yes | Yes | n/a |
| Set own avatar | Yes | No | No | No |
| Set display name | Yes | Yes | Yes | Yes |
| Recovery-key reminder banner | Partial (inert stub) | Partial (hardcoded off) | Partial (inert stub) | n/a |

## Messaging (DMs and groups)

DMs and groups share one send path in app-core; rows apply to both unless noted.

| Feature | iOS | Android | Desktop | Bots |
|---|---|---|---|---|
| Send / receive text (live WebSocket + catch-up) | Yes | Yes | Yes | Yes |
| Delivery and read receipts (sent by app-core) | Yes | Yes | Yes | Yes (delivery) |
| Local history, conversation list, unread counts | Yes | Yes | Yes | API |
| Reactions, edit, delete-for-everyone | Yes | Yes | Yes | Yes (reactions) |
| Image attachments + full-screen viewer | Yes | Yes | Yes | Yes (send) |
| Link previews (sender-generated) | Yes | Yes | Yes | No |
| Shared contact cards | Yes | Yes | Yes | No |
| Disappearing-message timers | Yes | Yes | Yes | Yes (group expiry) |
| Paste image from clipboard | Yes | Yes | Yes (also drag-and-drop) | n/a |
| Share an image in from another app | Yes (share extension) | Yes (`ACTION_SEND`) | n/a | n/a |
| Conversation search | Yes | Yes | Yes (search field + Cmd/Ctrl+K) | n/a |
| Own-device sync of sent messages and read state (docs/04) | Partial | Partial | Partial | n/a |
| Quote-reply / threads (docs/32) | No | No | No | No |
| Per-conversation mute | No | No | No | n/a |

Own-device sync is Partial everywhere: sent transcripts sync, but read state is applied when
received and never sent (docs/04).

## Groups

| Feature | iOS | Android | Desktop | Bots |
|---|---|---|---|---|
| Create group, invite member | Yes | Yes | Yes | Yes |
| Accept / decline invite | Yes | Yes | Yes | API |
| Join via invite link | Yes | Yes | Yes | API |
| Roles: promote, remove member | Yes | Yes | Yes | API |
| Leave group | Yes | Yes | Yes | API |
| Group system messages in the timeline | Yes | Yes | Yes | n/a |
| Group avatar: display | Yes | Yes | Yes | n/a |
| Group avatar: set / remove | Yes | No | No | No |

## Contacts and safety

| Feature | iOS | Android | Desktop | Bots |
|---|---|---|---|---|
| Contact list, profile fetch + cache | Yes | Yes | Yes | API |
| Contact avatars: display | Yes | Yes | Yes | n/a |
| Message-request gate (accept / delete) | Yes | Yes | Yes | n/a |
| Group invite requests (join / delete / block) | Yes | Yes | Yes | Bots get `isRequest` and decide (adminbot accepts) |
| Block / unblock, report-and-block | Yes | Yes | Yes | n/a |
| Show own QR code / invite link | Yes | Yes | Yes | n/a |
| Scan a QR code | Yes | Yes | No (by design for now: paste the link instead) | n/a |

## Projects and Network tab

| Feature | iOS | Android | Desktop | Bots |
|---|---|---|---|---|
| Network tab: servers → Project directory | Yes | Yes | Yes | n/a |
| Project webview (bridgeless, token via URL) | Yes | Yes | Yes (separate window) | n/a |
| "Sign in with Avalanche" consent (docs/25) | Yes | Yes | No | API |

## Platform infrastructure

| Feature | iOS | Android | Desktop | Bots |
|---|---|---|---|---|
| Push wakeups via relay | Yes (APNs) | Yes (FCM, UnifiedPush) | n/a (persistent WS) | n/a |
| Decrypt + show notification while suspended | Yes (Notification Service Extension) | No (syncs on next launch) | n/a | n/a |
| OS notifications | Yes | Yes | Yes | n/a |
| Deep links (`go.theavalanche.net/…`) | Yes | Yes | Yes | n/a |
| Connection state / offline banner | Yes | Yes | Yes | n/a |
| Recovery blob upload / refresh | Yes | Yes | Yes | n/a |
| Hardware-backed DB key | Yes (Secure Enclave) | Yes (Keystore) | **No — constant placeholder key** (docs/61) | Operator-supplied |
| System tray / close to background | n/a | n/a | Yes | n/a |
| Dark mode | Yes | Yes | Yes | n/a |
| Calls | No | No | No | n/a |

## Notes

- **Desktop does not currently build** against `app-core` (async `next_events` /
  `wait_for_connection_state_change`; docs/61). The Desktop column describes the code, not a
  shippable binary.
- **Bots** (adminbot, testbot) use account creation, DMs, reactions, attachment sends, group
  create/invite/expiry, group sends in `#admins`, and the admin event stream; OAuth calls are
  exposed via napi but unused.
