# Deferred TODOs

## Dev Infra
- Make it super easy to launch Postgres, the main server & relevant Projects all at once in dev

## Mobile app
- Mobile app 'console': nerdly scrolling log which appears during long loads and debugging tools (currently everything is fast so maybe not needed)
- Account recovery is not yet implemented / working
- Written-down recovery phrase alternative to passkey (generate memorable phrase, encrypt recovery blob with it, cache derived key in Secure Enclave)
- Delivery receipts — auto-send on message receive (see docs/31-read-tracking.md, Stage D)
- Read receipt user preference toggle (send_read_receipts setting)
- Scroll position: remove invisible "bottom" anchor hack in ConversationView (Color.clear spacer) when scroll position saving is implemented

## Privacy / identity
- PLC directory privacy: the DID document currently includes the homeserver URL as a service endpoint, which means anyone can resolve a DID and learn which server a user is on. For small servers this effectively leaks group membership. Consider removing the homeserver URL from the PLC document entirely and relying on out-of-band discovery (invite links, contact exchange). The PLC document would only contain the identity key for verification.
- DID update operation for key rotation after recovery (submit new signing key to PLC directory, signed by rotation key)
- Re-encrypt and re-upload recovery blob to all servers when joining a new server (update server list)
- Cache recovery derived key in Secure Enclave so re-encryption doesn't require re-prompting passkey/phrase

## Crypto / protocol
- Stale device detection: when a device re-registers (new identity key, new prekeys), the server should reject messages sent to the old device state. `POST /v1/messages` should check that the sender's session is compatible with the recipient's current registration (e.g., compare `registration_id`). On rejection, the sender's client should fetch the new prekey bundle and re-establish the session. Without this, messages encrypted to old keys are silently undeliverable after a key reset.

## Server
- WebSocket request/response framing: tunnel HTTP-style request/response pairs over the WebSocket (like Signal does), with request IDs and correlated responses. Move message sends and acks onto the WS transport, replacing the current split of HTTP sends + WS acks. This gives persistent-connection benefits while keeping clear success/failure semantics per operation.
- Timer change sync message: add a `TimerChangeMessage` body variant to the ContentMessage protobuf so that when a user changes the conversation expiry timer, a control message is sent to the other participant(s) to update their local setting

## Project-wide
- Mass rename: rename repo, update bundle IDs, update all remaining `actnet` references in code and docs to `avalanche`

## Big milestones (not yet started)
- Groups: action-bound (zkgroup) and cross-server casual (Sender Keys)
- Invite links & onboarding: QR codes, deep links, auto-enrollment into groups/Projects
- Projects framework: SDK, scoped bot permissions, JS bridge for webviews
- First-party Projects: channel directory, team assignment, action-day map, Q&A bot, collab docs, engagement tracking
- Federation: server-to-server protocol, cross-server DMs, full DID portability (PLC directory), guest access
- Calls: voice and video (VoIP)
- Public profiles: client-owned profile blobs (display name, avatar, bio) pushed to servers
- Multi-account support in mobile app

## Mesh Fallback / BitChat protocol (optional — implement only after core features are stable)

See `docs/32-bitchat-fallback.md` for the full design. BLE mesh transport as a fallback when the homeserver is unreachable.

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
