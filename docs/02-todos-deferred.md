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
- Coalesce/throttle `fetchGroupState`: opening a group conversation, then group-info, then returning each triggers a full `GET /v1/groups/{id}` (ConversationView's `refreshGroupTitle` on every `onAppear` + GroupDetailView on open). Add a short freshness TTL (skip the network refetch if the local group state was refreshed within ~N seconds) and have the conversation view read the cached group title rather than always fetching. Best paired with server-side group-state change push (Server) so the cache can be trusted between changes.
- Device linking — Desktop (Tauri/Solid) UI. iOS + Android shipped the QR show/scan flows (docs/04 §4); the node FFI (`linkCreatePairing`/`linkAcceptPairing`/`linkSendBundle`, `DeviceLinkNew`) is already wired, so Desktop just needs the two screens: existing-device "Link a device" in settings and a new-device "Link to an existing device" onboarding path. Mirror `LinkDeviceView` / `LinkNewDeviceView`.
- Linked-device management screen (all platforms): list an account's active devices and allow revoking one. No FFI exists to enumerate or revoke devices yet — needs a server endpoint + `app-core` method first. Today the UI only *initiates* a link.

## Privacy / identity
- De-anonymization guard for compose: implement `preferred_identity` (docs/52 §"More about `preferred_identity`") so the composer defaults the sending identity per-contact instead of using the foregrounded one. Depends on the unified, cross-identity contact book (today contacts are siloed in each identity's SQLCipher DB, so `preferred_identity` is degenerate). Also covers founding a group/DM on a chosen (non-home) server: `create_group` currently always uses the account's pinned client server (`app-core` `create_group` → `inner.client.server_url()`), and the Name Group screen's server picker gates non-home options until this lands. Groups need their own preferred-identity tracking separate from contacts (docs/52 §"More about `preferred_identity`").
- Consider allowing `did:local:` DIDs for human (non-bot) accounts, not just bots. Allowing `did:local:` for humans would let small orgs run a homeserver without publishing identities globally.
- PLC directory privacy: the DID document currently includes the homeserver URL as a service endpoint, which means anyone can resolve a DID and learn which server a user is on. For small servers this effectively leaks group membership. Consider removing the homeserver URL from the PLC document entirely and relying on out-of-band discovery (invite links, contact exchange). The PLC document would only contain the identity key for verification.
- DID update operation for key rotation after recovery (submit new signing key to PLC directory, signed by rotation key)
- Re-encrypt and re-upload recovery blob to all servers when joining a new server (update server list). Currently `update_recovery_blob` only writes to the primary; the auto-refresh on group join inherits that limit.
- Implement the no-blob recovery fallback (docs/50-identity-auth-recovery.md §"Recovering an identity after device loss", step 9). Today `recover_from_blob` errors out if the homeserver can't return a blob; the planned fallback generates a fresh identity key, submits a PLC update replacing the old verification method (signed by the rotation key), and re-registers without the blob's server list — user manually re-enters servers later.
- Sender-key recovery after device loss: when a recovered device can't decrypt group messages that other members sent under previous Sender Keys, prompt those peers (via a `DecryptionErrorMessage`-style nudge) to redistribute SKDMs. Without this, group history across the recovery boundary is unreadable until peers happen to rotate or send something new.
- Move group master keys out of the recovery blob → make storage sync the sole path (docs/DIGEST.md §recovery: "group keys are moving out of the blob … sequenced after group-key sync is sole path"). Blocked on the group-re-establishment reconciler (Crypto/protocol) and per-device push bindings (Server). Today `recover_from_blob` restores groups from the blob via `restore_group` (fetch state + register push pseudonym + re-seed Sender Key + distribute SKDM); storage sync's `GroupKeyAdapter` only stores the master key. Removing the blob path without the reconciler would leave a recovered/linked device holding group keys but with no push pseudonym → it silently never receives group fan-out.
- Consider whether we want to bother moving the persisted identity list out of UserDefaults into a Secure-Enclave-keyed SQLCipher `manifest.db`. Today the list of identities (own DID, display name, server URLs, db filename) lives in UserDefaults, which is encrypted at rest by the device data-protection class but not by a user-controlled key. An attacker pulling the iOS sandbox snapshot gets the list of homeservers the user is on plus their own DIDs — enough to link the device to specific orgs. The contact graph and message history are not exposed (they're inside the SQLCipher per-identity DBs) so it's maybe not that important. A small manifest DB keyed from the Secure Enclave (same approach as the per-identity DBs) could list the other DBs while closing this particular loophole.
- Contact list backup: we're interested in persisting the user's contacts separately from their identity keys, in hopes that if they lose identity keys at least they can reestablish contact with the people they were previously communicating with under a new ID. The contacts aren't that sensitive, but the tricky bit is that each of your contact is attached to one of your own identities and we don't want to mix them up. You might also want to be able to manually export your contacts list in some standard format that can be processed by other apps too.

## Crypto / protocol
- Bot edit-history suppression + revision capping (docs/36): recipients currently store a prior-body revision for every inbound edit. The spec says bot-authored messages should retain no edit history (a live-tally bot editing hundreds of times would bloat every recipient's device). Add a cheap local is-bot check on the receive path (or a per-message revision cap) once high-frequency bot editing is in use.
- Receive-side edit/delete window clamping (docs/36): the authorship rule is enforced on receive (security-critical), but the 24h/30-day windows are only enforced by the sending UI today. Add defense-in-depth clamping of out-of-window inbound edits / FOR_EVERYONE deletes.
- State-driven group re-establishment reconciler: for every group with a master key but no registered push pseudonym, register one (`rotate_group_push_binding`) + subscribe + fetch state (Sender Key / SKDM is already lazy on first send). Must trigger on storage-sync apply, NOT gated on "am I recovering?" — a linked second device gets group keys via sync without ever recovering, and is just as much a fresh device needing a pseudonym. This generalizes today's recovery-only `restore_group` and is the prerequisite for moving group keys out of the recovery blob (Privacy/identity). Note: it inherits the single-active-device assumption until per-device push bindings land (Server).
- Legacy raw-text group messages decode heuristically on receive (`process_decrypted` tries `ContentMessage::decode`, falls back to raw text). All new group messages carry the envelope; the fallback only matters for messages sent before this migration. Pre-launch there are none, so the heuristic is effectively dead code — drop the fallback (require the envelope) once there's confidence no pre-migration group messages remain in any store.

## Server
- Reconcile the two "projects" concepts. The client-facing directory (`PROJECTS` env → `GET /v1/projects`, just `{name,url,description}`) and the server-side `projects` table (slug + bot-account linkage via `project_bots` + the adminbot superuser pin) are separate and confusingly named — they were never deliberately decoupled, it just ended up that way. Today testbot only needs the directory entry; project-token issue/verify is URL-based and never consults the table, and there's no API to create table rows (only `ensure_adminbot_project` at startup + registration-time linking to existing rows). When the Project install flow (docs/24) lands, unify these — e.g. derive the directory from the table — so installing a Project does both at once.
- Adminbot routing config (docs/22): map an `AccountJoinedEvent`'s invite-token issuer + routing tags → channels (declarative rules), now that the event carries `invite_token`.
- Adminbot node bot: consume the catch-up endpoint `GET /v1/admin/events?since=` on reconnect (events are now persisted in `server_events`); today it only acts on live WS pushes.
- Future `bots.provision` capability + `purpose = "bot"` registration tokens (docs/24 end state — bots sign up with a token signed by their Project). The token format already carries `purpose` and the redemption table is generic, so this is additive: add the capability string, a `purpose == "bot"` admission arm, and auto-link the new bot to the issuing Project via the token's `iss`.
- Per-device group push bindings (concurrent multi-device group receive). Today `member_credentials(group_id, encrypted_member_id, group_push_pseudonym)` holds one pseudonym per (group, account-member EMI) and `rotate_member_pseudonym` replaces it — so two active devices of one account clobber each other's binding and only the last to register gets group fan-out. Needs per-device pseudonyms (multiple per EMI), fan-out delivering each member's slice to all their devices, and possibly sender-side fanout changes. Gates moving group keys out of the recovery blob for the multi-device case (Privacy/identity).
- Push group-state changes (membership/title/policy) over the WebSocket. Today the only WS group frames are message fan-outs; clients learn of state changes by polling `fetch_group_state` on navigation. A "group changed" push would let clients trust cached state and stop polling on every open (see the `fetchGroupState` coalescing item under Mobile app).

## Infra
- Right now foreground apps poll the server every minute for storage updates; implement something that reduces this poll rate -- probably proactive sync of some sort. Part of multi-device implementation.
- In-place server upgrades (see [`42-server-upgrades.md`](42-server-upgrades.md)). **Phase 1:** rewrite `avalanche-update` to be tag + tarball based and cover the bots (download `av-{server,adminbot,testbot}-$TARGET` for a release tag, migrate, swap all installed components, restart, rollback; stamp the tag into `bootstrap.env` and show it in `avalanche-status`). The shipped updater is still bare-binary/server-only. **Phase 2:** the `av-deploy-<tag>.tar.gz` deploy-bundle model — move the systemd units / Caddyfile / env templates / scripts into `infra/deploy/bundle/`, add the artifact to `release.yml`, thin the configure-page cloud-init down to "fetch bundle + run install.sh", and have `update.sh` refresh the infra glue. **Phase 3:** `#admins` `/upgrade [tag]` command (docs/22) so operators upgrade from inside the app.

## Project-wide
- Mass rename: rename repo, update bundle IDs, update all remaining `actnet` references in code and docs to `avalanche`

## Big milestones (not yet started)
- First-party Projects: channel directory, team assignment, action-day map, Q&A bot, collab docs, engagement tracking
- Federation: server-to-server protocol, cross-server DMs, full DID portability (PLC directory), guest access
- Calls: voice and video (VoIP)
- Public profiles: client-owned profile blobs (display name, avatar, bio) pushed to servers

## Mesh Fallback / BitChat protocol (optional — implement only after core features are stable)

See `docs/14-bitchat-fallback.md` for the full design. BLE mesh transport as a fallback when the homeserver is unreachable.

## Push Notifications

### 4. Testing & privacy
- [ ] Verify relay payloads contain zero user-identifiable content
- [ ] Verify relay logs contain only pseudonyms + timestamps
- [ ] Pseudonym rotation grace period test
- [ ] APNs/FCM sandbox integration test
