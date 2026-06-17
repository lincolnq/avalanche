# Deferred TODOs

## Build samples of the following projects
- $ Gatekeeper project + onboarding flow (see 24-vetted-onboarding-project); the infra should be there but there's no project yet. `#approvals` group, approve/decline review flow, invite tokens.
- $ Chatbot to answer questions
- $ Full participant CRM project: list everyone who has signed up & oversee them
- $ Training modules inside CRM: browse to the training site via Network tab, complete modules

## Mobile app
- Mobile app 'console': nerdly scrolling log which appears during long loads and debugging tools (currently everything is fast so maybe not needed)
- Written-down recovery phrase alternative to passkey (generate memorable phrase, encrypt recovery blob with it, cache derived key in Secure Enclave)
- Delivery receipts — auto-send on message receive (see docs/31-read-tracking.md, Stage D)
- Read receipt user preference toggle (send_read_receipts setting)
- Scroll position: remove invisible "bottom" anchor hack in ConversationView (Color.clear spacer) when scroll position saving is implemented
- Account switcher UI for multi-account support
- My QR Code screen uses `accounts.first` — should use the active/selected account once multi-account is implemented
- Consider whether we should hit `validateInvite` endpoint during 'compose recipient' scan/paste for a server invite token.

## Privacy / identity
- Consider allowing `did:local:` DIDs for human (non-bot) accounts, not just bots. Allowing `did:local:` for humans would let small orgs run a homeserver without publishing identities globally.
- PLC directory privacy: the DID document currently includes the homeserver URL as a service endpoint, which means anyone can resolve a DID and learn which server a user is on. For small servers this effectively leaks group membership. Consider removing the homeserver URL from the PLC document entirely and relying on out-of-band discovery (invite links, contact exchange). The PLC document would only contain the identity key for verification.
- DID update operation for key rotation after recovery (submit new signing key to PLC directory, signed by rotation key)
- Re-encrypt and re-upload recovery blob to all servers when joining a new server (update server list). Currently `update_recovery_blob` only writes to the primary; the auto-refresh on group join inherits that limit.
- Implement the no-blob recovery fallback (docs/50-identity-auth-recovery.md §"Recovering an identity after device loss", step 9). Today `recover_from_blob` errors out if the homeserver can't return a blob; the planned fallback generates a fresh identity key, submits a PLC update replacing the old verification method (signed by the rotation key), and re-registers without the blob's server list — user manually re-enters servers later.
- Sender-key recovery after device loss: when a recovered device can't decrypt group messages that other members sent under previous Sender Keys, prompt those peers (via a `DecryptionErrorMessage`-style nudge) to redistribute SKDMs. Without this, group history across the recovery boundary is unreadable until peers happen to rotate or send something new.
- Consider whether we want to bother moving the persisted identity list out of UserDefaults into a Secure-Enclave-keyed SQLCipher `manifest.db`. Today the list of identities (own DID, display name, server URLs, db filename) lives in UserDefaults, which is encrypted at rest by the device data-protection class but not by a user-controlled key. An attacker pulling the iOS sandbox snapshot gets the list of homeservers the user is on plus their own DIDs — enough to link the device to specific orgs. The contact graph and message history are not exposed (they're inside the SQLCipher per-identity DBs) so it's maybe not that important. A small manifest DB keyed from the Secure Enclave (same approach as the per-identity DBs) could list the other DBs while closing this particular loophole.
- Contact list backup: we're interested in persisting the user's contacts separately from their identity keys, in hopes that if they lose identity keys at least they can reestablish contact with the people they were previously communicating with under a new ID. The contacts aren't that sensitive, but the tricky bit is that each of your contact is attached to one of your own identities and we don't want to mix them up. You might also want to be able to manually export your contacts list in some standard format that can be processed by other apps too.

## Crypto / protocol
- Bot edit-history suppression + revision capping (docs/36): recipients currently store a prior-body revision for every inbound edit. The spec says bot-authored messages should retain no edit history (a live-tally bot editing hundreds of times would bloat every recipient's device). Add a cheap local is-bot check on the receive path (or a per-message revision cap) once high-frequency bot editing is in use.
- Receive-side edit/delete window clamping (docs/36): the authorship rule is enforced on receive (security-critical), but the 24h/30-day windows are only enforced by the sending UI today. Add defense-in-depth clamping of out-of-window inbound edits / FOR_EVERYONE deletes.
- Legacy raw-text group messages decode heuristically on receive (`process_decrypted` tries `ContentMessage::decode`, falls back to raw text). All new group messages carry the envelope; the fallback only matters for messages sent before this migration. Pre-launch there are none, so the heuristic is effectively dead code — drop the fallback (require the envelope) once there's confidence no pre-migration group messages remain in any store.

## Server
- Adminbot routing config (docs/22): map an `AccountJoinedEvent`'s invite-token issuer + routing tags → channels (declarative rules), now that the event carries `invite_token`.
- Adminbot node bot: consume the catch-up endpoint `GET /v1/admin/events?since=` on reconnect (events are now persisted in `server_events`); today it only acts on live WS pushes.
- Future `bots.provision` capability + `purpose = "bot"` registration tokens (docs/24 end state — bots sign up with a token signed by their Project). The token format already carries `purpose` and the redemption table is generic, so this is additive: add the capability string, a `purpose == "bot"` admission arm, and auto-link the new bot to the issuing Project via the token's `iss`.

## Infra
- Right now foreground apps poll the server every minute for storage updates; implement something that reduces this poll rate -- probably proactive sync of some sort. Part of multi-device implementation.

## Project-wide
- Mass rename: rename repo, update bundle IDs, update all remaining `actnet` references in code and docs to `avalanche`

## Big milestones (not yet started)
- First-party Projects: channel directory, team assignment, action-day map, Q&A bot, collab docs, engagement tracking
- Federation: server-to-server protocol, cross-server DMs, full DID portability (PLC directory), guest access
- Calls: voice and video (VoIP)
- Public profiles: client-owned profile blobs (display name, avatar, bio) pushed to servers

## Mesh Fallback / BitChat protocol (optional — implement only after core features are stable)

See `docs/14-bitchat-fallback.md` for the full design. BLE mesh transport as a fallback when the homeserver is unreachable.

## App Store readiness
- Privacy policy URL plumbing: homeserver metadata endpoint exposes the operator's privacy policy URL; client displays it during signup alongside the app's own policy. Required because each homeserver is a separate data controller under GDPR.


## Push Notifications

### 4. Testing & privacy
- [ ] Verify relay payloads contain zero user-identifiable content
- [ ] Verify relay logs contain only pseudonyms + timestamps
- [ ] Pseudonym rotation grace period test
- [ ] APNs/FCM sandbox integration test
