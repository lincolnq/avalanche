# 15 — Push notifications

> **Status:** Built — the push relay is deployed at `https://relay.theavalanche.net` and dispatches APNs, FCM HTTP v1, and UnifiedPush. iOS reliability comes from the NSE (`16`). Not built: distributor picker, foreground-service keepalive for Android without a distributor, lost-push detection.
> **Last verified against code:** 2026-10-03

## Summary

iOS and standard Android can only wake a backgrounded app through Apple's or Google's push services (or a UnifiedPush distributor on de-Googled Android). If homeservers held device tokens, they — and Apple and Google — would learn too much. So the app developer runs a **push relay**:

- Homeservers send **content-free wakeups** addressed to per-(user, server) **pseudonyms**.
- The relay maps pseudonym → token and fires an empty payload.
- The app wakes and fetches its messages itself.

Code: relay `core/crates/relay/src/main.rs`; app-core `register_push_token` and the group pseudonym registration in `lib.rs`; Android `App/PushManager.kt`, `App/UnifiedPushService.kt`; ops in `41`.

## Platform dispatch

| Platform | Mechanism |
|---|---|
| iOS | APNs via the relay; alert push + NSE fetch (`16`) |
| Android (standard) | FCM via the relay (data-only, high priority) |
| Android (de-Googled) | UnifiedPush via the relay if a distributor is installed; otherwise foreground WebSocket only |
| Desktop (Tauri) | Live WebSocket → local OS notification (`tauri-plugin-notification`); never registers a push token |

**Every external transport goes through the relay.** The homeserver only ever POSTs content-free wakeups to pseudonyms (`/v1/wakeup`). It never holds a device token, FCM token, or endpoint URL, and never contacts a third party itself. The relay stores `(pseudonym → token, platform, environment)` and dispatches by `platform`. The client picks the transport; to the homeserver they're indistinguishable.

## Relay / privacy model

*Status: Built.*

- Apple, Google, and the distributor see only "this app got pinged".
- The relay sees pseudonym-level timing, but not identity, content, or which server a user is on.
- The relay keeps a rotated pseudonym for a 7-day grace period. Scheduled rotation (default weekly) is Planned; today a pseudonym changes only when the client re-registers or rotates it explicitly (`03` §3.7).
- Users who need it can opt out of push entirely and rely on the foreground connection.
- The protocol supports multiple relays, so the Avalanche-operated relay isn't a privileged singleton.
- One relay serves both APNs sandbox and production, routed by the client-supplied `environment`.
- The relay is a tiny SQLite database. Losing it only forces clients to re-register.

**What the relay does learn** (detailed in `09`): every registration from one device shares one device token. So the relay can link that device's DM pseudonym and **all its group pseudonyms**, across every server the device uses. Someone holding both the relay database and a homeserver database could join pseudonyms back to groups. Pseudonym rotation doesn't break this, because the device token is the stable join key.

## FCM (standard Android)

*Status: Built.*
- The client registers its FCM token as platform `fcm`.
- The relay sends via **FCM HTTP v1**, minting an OAuth2 access token from a service-account-signed RS256 JWT (the legacy server-key API is decommissioned).
- Wakeups are **data-only, high-priority** messages — no `notification` block — so `onMessageReceived` runs even in the background. They carry no content.
- Relay config: `FCM_SA_PATH`, `FCM_PROJECT_ID` (optional). If unset, FCM wakeups are only logged.

## UnifiedPush (de-Googled Android)

*Status: Built.*

The app registers with the user's distributor (ntfy, NextPush, …) and gets an **endpoint URL**. It registers that URL with the relay exactly like a token: platform `unifiedpush`, URL in `device_token`. The **relay** POSTs a content-free body to the URL. Routing this through the relay rather than the homeserver keeps the homeserver out of the token business.

The endpoint is client-supplied and relay registration is unauthenticated, so the POST is **SSRF-guarded**:
- https only;
- the resolved host must be a global address (loopback, private, link-local including `169.254.169.254`, CGNAT, and ULA ranges are rejected);
- redirects disabled, short timeout.

No Web Push (RFC 8291) payload encryption is used: there's no content to encrypt, and the endpoint URL is the secret.

**Transport selection (client):** on login and on every foreground, pick one transport:
1. Google Play Services present (including microG) → FCM;
2. else a UnifiedPush distributor is installed → UnifiedPush;
3. else no push (foreground WebSocket only).

The client re-registers on each foreground, so a removed or changed distributor is picked up.

## Known gaps

1. **Relay registration is unauthenticated** (`POST /v1/register`, `INSERT OR REPLACE` keyed on pseudonym). Anyone who learns a pseudonym can repoint its wakeups at their own token: the victim stops getting pushes, and the attacker learns when the victim gets messages. Group members can learn each other's group pseudonyms (`03`), which makes this practical within a group. Fix direction: pseudonyms backed by a secret, where the server stores `H(secret)` and the device presents the preimage to register (see `03` and `09`).
2. **The relay links all of a device's pseudonyms** through the shared device token (above).
3. **No lost-push detection** (`16`).

## Planned

- Fix the relay registration gap with secret-backed pseudonyms (contract change across the server, relay, and clients).
- A distributor picker when several UnifiedPush distributors are installed.
- A persistent foreground-service WebSocket keepalive for Android without a distributor.
- Folding the relay into the standard deploy and update path (`41`, `42`).

## Rationale and rejected alternatives

- **Homeservers holding device tokens** — rejected: the homeserver and Apple/Google would learn the identity-to-device mapping.
- **Homeserver-direct UnifiedPush** — rejected: the homeserver would need outbound push and per-device endpoint storage, breaking the "homeserver never holds a token" rule.
- **Web Push payload encryption** — unnecessary: there's no payload.
