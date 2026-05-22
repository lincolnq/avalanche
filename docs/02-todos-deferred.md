# Deferred TODOs

## Mobile app
- Recovery key UI: setup and backup flows (banner currently always shows, hardcoded false)
- Scroll-position-based read marking (see docs/31-read-tracking.md, Stage B)
- Read receipt user preference toggle (send_read_receipts setting)
- Scroll position: remove invisible "bottom" anchor hack in ConversationView (Color.clear spacer) when scroll position saving is implemented

## Crypto / protocol
- DB encryption key from Secure Enclave instead of hardcoded "dev-placeholder-key"

## Server
- WebSocket request/response framing: tunnel HTTP-style request/response pairs over the WebSocket (like Signal does), with request IDs and correlated responses. Move message sends and acks onto the WS transport, replacing the current split of HTTP sends + WS acks. This gives persistent-connection benefits while keeping clear success/failure semantics per operation.
- Timer change sync message: add a `TimerChangeMessage` body variant to the ContentMessage protobuf so that when a user changes the conversation expiry timer, a control message is sent to the other participant(s) to update their local setting

## Project-wide
- Settle on a better name: rename repo, update bundle IDs, update `actnet://` URL scheme to match, update all references in code and docs

## Big milestones (not yet started)
- Groups: action-bound (zkgroup) and cross-server casual (Sender Keys)
- Invite links & onboarding: QR codes, deep links, auto-enrollment into groups/Projects
- Push notifications (see Push Notifications section below)
- Projects framework: SDK, scoped bot permissions, JS bridge for webviews
- First-party Projects: channel directory, team assignment, action-day map, Q&A bot, collab docs, engagement tracking
- Federation: server-to-server protocol, cross-server DMs, full DID portability (PLC directory), guest access
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
