# Deferred TODOs

## Next
- iOS app defaults to mock `ActnetService` mode instead of the live one, which is confusing — a fresh install looks like it's working but isn't actually talking to a homeserver. Default to live mode and make mock an explicit opt-in (debug menu toggle, env-var, or build config).
- Rename everything to avalanche

## Mobile app
- Mobile app 'console': nerdly scrolling log which appears during long loads and debugging tools (currently everything is fast so maybe not needed)
- Written-down recovery phrase alternative to passkey (generate memorable phrase, encrypt recovery blob with it, cache derived key in Secure Enclave)
- Delivery receipts — auto-send on message receive (see docs/31-read-tracking.md, Stage D)
- Read receipt user preference toggle (send_read_receipts setting)
- Scroll position: remove invisible "bottom" anchor hack in ConversationView (Color.clear spacer) when scroll position saving is implemented
- Account switcher UI for multi-account support
- My QR Code screen uses `accounts.first` — should use the active/selected account once multi-account is implemented

## Privacy / identity
- Consider allowing `did:local:` DIDs for human (non-bot) accounts, not just bots. Allowing `did:local:` for humans would let small orgs run a homeserver without publishing identities globally.
- PLC directory privacy: the DID document currently includes the homeserver URL as a service endpoint, which means anyone can resolve a DID and learn which server a user is on. For small servers this effectively leaks group membership. Consider removing the homeserver URL from the PLC document entirely and relying on out-of-band discovery (invite links, contact exchange). The PLC document would only contain the identity key for verification.
- DID update operation for key rotation after recovery (submit new signing key to PLC directory, signed by rotation key)
- Re-encrypt and re-upload recovery blob to all servers when joining a new server (update server list)
- Cache recovery derived key in Secure Enclave so re-encryption doesn't require re-prompting passkey/phrase
- Consider whether we want to bother moving the persisted account list out of UserDefaults into a Secure-Enclave-keyed SQLCipher `manifest.db`. Today the list of accounts (own DID, display name, server URLs, db filename) lives in UserDefaults, which is encrypted at rest by the device data-protection class but not by a user-controlled key. An attacker pulling the iOS sandbox snapshot gets the list of homeservers the user is on plus their own DIDs — enough to link the device to specific orgs. The contact graph and message history are not exposed (they're inside the SQLCipher per-account DBs) so it's maybe not that important. A small manifest DB keyed from the Secure Enclave (same approach as the per-account DBs) could list the other DBs while closing this particular loophole.
- Contact list backup: we're interested in persisting the user's contacts separately from their identity keys, in hopes that if they lose identity keys at least they can reestablish contact with the people they were previously communicating with under a new ID. The contacts aren't that sensitive, but the tricky bit is that each of your contact is attached to one of your own identities and we don't want to mix them up. You might also want to be able to manually export your contacts list in some standard format that can be processed by other apps too.

## Crypto / protocol

## Server

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

## App Store readiness
- Implement abuse handling per `docs/12-abuse-handling.md`: message-request gate for unknown senders, block list (client-side, multi-device synced), Report Spam in the request UI, homeserver-mediated cross-server abuse report endpoint, account-level enforcement ladder on receiving server.
- Display-name profanity filter (client-side, on by default, tap-to-reveal). Satisfies the "filter objectionable material" prong of App Store 1.2 at the profile layer.
- Projects framework App Store 4.7 compliance: (1) maintain a Project index with universal links (4.7.4); (2) per-Project consent prompt before granting data/permissions, re-prompt on permission expansion (4.7.3); (3) age-restriction mechanism for mature Projects with verified or declared age (4.7.5); (4) keep the JS bridge surface conservative — no exposing native APIs without prior Apple approval (4.7.2). Document policy that all in-Project digital-goods purchases route through IAP (4.7.1 / 3.1).
- Privacy policy URL plumbing: homeserver metadata endpoint exposes the operator's privacy policy URL; client displays it during signup alongside the app's own policy. Required because each homeserver is a separate data controller under GDPR.
- Reviewer demo flow: passkey-only signup is hostile to App Review. Either ship a debug build flag that bypasses passkey with a synthetic key, or pre-provision a reviewer account with embedded credentials and document in review notes. Without this, first submission will be rejected for "couldn't complete signup."
- Support contact info: support email shown in Settings → About (mailto link is fine, no ticketing system needed) and Support URL set in App Store Connect. Required by 1.2 (UGC contact) and 1.5 (developer info).
- Age rating: aim for 12+ (matching Signal), set via honest answers to the App Store Connect rating questionnaire — acknowledge UGC exists but no shipped objectionable content, no gambling, no unrestricted web. Don't use "Kids" or "Children" anywhere in metadata (2.3.8).
- Account deletion flow (required by App Store guideline 5.1.1(v)). In-app: Settings → Delete account → confirmation → server deletion → wipe local SQLCipher DB + keychain → return to onboarding. Server-side: cascade delete across accounts/devices/prekeys/signed_prekeys/kyber_prekeys/message_queue/did_documents in a single transaction. Design decisions to make: (1) tombstone row vs hard delete — a tombstone (account_id + deleted_at, no keys/profile) lets the server return a definitive "deleted" signal instead of bare 404, friendlier for clients distinguishing deleted/hiccup/never-existed (Signal does roughly this); (2) client UX for deleted contacts — keep the thread, mark contact as deleted, disable send, don't auto-delete the user's history; (3) group membership propagation — action-bound groups can do server-side membership updates, cross-server Sender Keys groups are messier (no authoritative member list). Federation caveat: other servers may have cached the DID document; staleness window resolves on next fetch (404). Disclose in privacy policy.


## Push Notifications

### 4. Testing & privacy
- [ ] Verify relay payloads contain zero user-identifiable content
- [ ] Verify relay logs contain only pseudonyms + timestamps
- [ ] Pseudonym rotation grace period test
- [ ] APNs/FCM sandbox integration test
