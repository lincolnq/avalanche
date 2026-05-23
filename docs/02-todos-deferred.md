# Deferred TODOs

## Mobile app
- Recovery key UI: setup and backup flows (banner currently always shows, hardcoded false)
- Delivery receipts — auto-send on message receive (see docs/31-read-tracking.md, Stage D)
- Read receipt user preference toggle (send_read_receipts setting)

## Crypto / protocol
- Kyber prekey pool: upload one-time Kyber prekeys with server-side atomic consumption (like EC one-time prekeys), keep one last-resort key. Currently only a single last-resort key is used.
- Protobuf message envelope: plaintext is raw bytes, design calls for ContentMessage protobuf (proto/content.proto)

## Server
- WebSocket request/response framing: tunnel HTTP-style request/response pairs over the WebSocket (like Signal does), with request IDs and correlated responses. Move message sends and acks onto the WS transport, replacing the current split of HTTP sends + WS acks. This gives persistent-connection benefits while keeping clear success/failure semantics per operation.
- Timer change sync message: add a `TimerChangeMessage` body variant to the ContentMessage protobuf so that when a user changes the conversation expiry timer, a control message is sent to the other participant(s) to update their local setting

## Project-wide
- Settle on a better name: rename repo, update bundle IDs, update `actnet://` URL scheme to match, update all references in code and docs

## Big milestones (not yet started)
- Groups: action-bound (zkgroup) and cross-server casual (Sender Keys)
- Invite links & onboarding: QR codes, deep links, auto-enrollment into groups/Projects
- Projects framework: SDK, scoped bot permissions, JS bridge for webviews
- First-party Projects: channel directory, team assignment, action-day map, Q&A bot, collab docs, engagement tracking
- Federation: server-to-server protocol, cross-server DMs, full DID portability (PLC directory), guest access
- Calls: voice and video (VoIP)
- Public profiles: client-owned profile blobs (display name, avatar, bio) pushed to servers
- Multi-account support in mobile app

## Push Notifications (remaining work)
- Android client: FCM token registration, pseudonym lifecycle, wakeup handling
- Relay: real APNs sending via `a2` crate (env vars: APNS_KEY_PATH, APNS_KEY_ID, APNS_TEAM_ID, APNS_BUNDLE_ID)
- Relay: real FCM sending
- iOS: periodic pseudonym rotation (weekly timer)
- iOS: opt-out setting for high-risk users (poll-only mode)
- Testing: verify relay payloads contain zero user-identifiable content; APNs/FCM sandbox integration test
