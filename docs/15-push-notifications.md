# Push Notifications

## Platform dispatch

| Platform | Mechanism |
|---|---|
| iOS | APNs via push relay |
| Android (standard) | FCM via push relay |
| Android (degoogled) | UnifiedPush if distributor installed; WebSocket keepalive otherwise |
| Desktop (Electron) | WebSocket frame → local OS notification; no external push |

The homeserver picks dispatch per device based on what's registered: FCM token → FCM; UnifiedPush endpoint URL → POST to it; neither → WebSocket frame. Desktop never registers a push token.

## Relay / privacy model (iOS + standard Android)

Homeservers never hold device tokens. Instead they send content-free wakeups to per-(user, server) **pseudonyms** at the push relay (`https://relay.theavalanche.net`, not yet deployed). The relay maps pseudonyms → tokens and fires empty payloads. Apple/Google see only a ping; the relay sees pseudonym-level timing but no identity, content, or cross-server linkage. Pseudonyms rotate periodically to limit linkability. High-risk users can opt out and poll manually. Multiple relays are supported so the Avalanche-operated relay is not a privileged singleton. See `docs/41-relay-deployment.md` for ops.

## UnifiedPush (degoogled Android)

[UnifiedPush](https://unifiedpush.org) lets users choose their own push distributor (e.g. ntfy, Gotify). The app registers with the distributor and gets an endpoint URL; the homeserver POSTs to that URL when a wakeup is needed; the distributor forwards it to the app via local Android broadcast.

**Server schema:** `notification_endpoint: Option<String>` per device, stored alongside the FCM token. The three-way dispatch is designed in from the start.

**Registration:** on every app foreground, the app checks for a distributor via Android Intent. If found, it registers and sends the endpoint URL to the homeserver (`PUT /devices/push-endpoint`). Switching from WebSocket to UnifiedPush is automatic — no user action needed beyond installing a distributor app.
