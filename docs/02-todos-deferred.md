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

## Privacy / identity
- Consider allowing `did:local:` DIDs for human (non-bot) accounts, not just bots. Allowing `did:local:` for humans would let small orgs run a homeserver without publishing identities globally.
- PLC directory privacy: the DID document currently includes the homeserver URL as a service endpoint, which means anyone can resolve a DID and learn which server a user is on. For small servers this effectively leaks group membership. Consider removing the homeserver URL from the PLC document entirely and relying on out-of-band discovery (invite links, contact exchange). The PLC document would only contain the identity key for verification.
- DID update operation for key rotation after recovery (submit new signing key to PLC directory, signed by rotation key)
- Re-encrypt and re-upload recovery blob to all servers when joining a new server (update server list). Currently `update_recovery_blob` only writes to the primary; the auto-refresh on group join inherits that limit.
- Implement the no-blob recovery fallback (docs/50-identity-auth-recovery.md §"Recovering an identity after device loss", step 9). Today `recover_from_blob` errors out if the homeserver can't return a blob; the planned fallback generates a fresh identity key, submits a PLC update replacing the old verification method (signed by the rotation key), and re-registers without the blob's server list — user manually re-enters servers later.
- Sender-key recovery after device loss: when a recovered device can't decrypt group messages that other members sent under previous Sender Keys, prompt those peers (via a `DecryptionErrorMessage`-style nudge) to redistribute SKDMs. Without this, group history across the recovery boundary is unreadable until peers happen to rotate or send something new.
- Move group master keys out of the recovery blob → make storage sync the sole path (docs/DIGEST.md §recovery: "group keys are moving out of the blob … sequenced after group-key sync is sole path"). Blocked on the group-re-establishment reconciler (Crypto/protocol) and per-device push bindings (Server). Today `recover_from_blob` restores groups from the blob via `restore_group` (fetch state + register push pseudonym + re-seed Sender Key + distribute SKDM); storage sync's `GroupKeyAdapter` only stores the master key. Removing the blob path without the reconciler would leave a recovered/linked device holding group keys but with no push pseudonym → it silently never receives group fan-out.
- Consider whether we want to bother moving the persisted identity list out of UserDefaults into a Secure-Enclave-keyed SQLCipher `manifest.db`. Today the list of identities (own DID, display name, server URLs, db filename) lives in UserDefaults, which is encrypted at rest by the device data-protection class but not by a user-controlled key. An attacker pulling the iOS sandbox snapshot gets the list of homeservers the user is on plus their own DIDs — enough to link the device to specific orgs. The contact graph and message history are not exposed (they're inside the SQLCipher per-identity DBs) so it's maybe not that important. A small manifest DB keyed from the Secure Enclave (same approach as the per-identity DBs) could list the other DBs while closing this particular loophole.
- Contact list backup: we're interested in persisting the user's contacts separately from their identity keys, in hopes that if they lose identity keys at least they can reestablish contact with the people they were previously communicating with under a new ID. The contacts aren't that sensitive, but the tricky bit is that each of your contact is attached to one of your own identities and we don't want to mix them up. You might also want to be able to manually export your contacts list in some standard format that can be processed by other apps too.

## Android app

The iOS app (`mobile/ios/`) is the reference implementation. The Android app (Kotlin/Jetpack Compose) should mirror it structurally. See `docs/33-android.md` for the full implementation guide, including directory structure, iOS→Android mapping table, build setup, and code sketches for each layer.

### Infrastructure
- [ ] Scaffold Gradle project at `mobile/android/` (see `docs/33-android.md` §3 for directory structure)
- [ ] Add `make android-ndk` Makefile target: compile `libapp_core.so` for `arm64-v8a` and `x86_64` via `cargo-ndk`
- [ ] Add `make android` Makefile target: `make bindings` + `make android-ndk`
- [ ] Update `CLAUDE.md` build commands section to document Android targets and prerequisites
- [ ] Configure `gradle/libs.versions.toml` with Compose BOM, Navigation, ViewModel, DataStore, JNA, CameraX, ML Kit

### Core layer
- [ ] `AppCoreInterface.kt` — Kotlin interface mirroring `AppCoreProtocol` from `ActnetService.swift`
- [ ] `MockActnetService.kt` + `MockAppCore` — mock implementation (mirrors `MockActnetService.swift`): 100 ms send delay, echo reply after 1.5 s, seed conversations on `createAccount`
- [ ] `DevServerActnetService.kt` — wraps UniFFI-generated `AppCore` class directly
- [ ] `AppViewModel.kt` — `AndroidViewModel` with `StateFlow` (mirrors `AppState.swift`):
  - `restoreAccounts()` called from `init`; loads from `DataStore<Preferences>`
  - Per-account `AppCoreInterface` instances in `MutableMap<String, AppCoreInterface>`
  - WebSocket loop as `viewModelScope.launch(Dispatchers.IO)` coroutine per account, 2 s backoff on error
  - `ServiceMode` enum (MOCK / DEV_SERVER); switching mode clears all state

### Models
- [ ] `Account.kt` — data class with `id` (DID), `displayName`, `avatarData`, `servers`
- [ ] `Conversation.kt` — `@Serializable`; exclude `lastMessage` from JSON (`@Transient`) for same security reason as iOS
- [ ] `Message.kt` — `DeliveryStatus` enum (SENDING/SENT/DELIVERED/READ matching raw Int values from iOS)
- [ ] `ProjectInfo.kt`, `InviteToken.kt`

### UI — Onboarding
- [ ] `SplashScreen.kt` — scan QR / enter invite link entry points
- [ ] `InviteLinkEntryScreen.kt` — parse `actnet://invite/<server>/<token>` deep links
- [ ] `IdentityPickerScreen.kt` — existing account list + "Create fresh identity"
- [ ] `NewAccountScreen.kt` — display name input, optional avatar, calls `vm.createAccount(…)`
- [ ] `JoiningServerScreen.kt` — join new server with existing account
- [ ] `QRScannerScreen.kt` — CameraX preview + ML Kit `BarcodeScanning.getClient()`

### UI — Chats tab
- [ ] `ChatsScreen.kt` — `LazyColumn` sorted by `lastMessageDateMs`; unread badge; FAB for compose
- [ ] `ConversationScreen.kt` — message thread, auto-scroll to bottom, mark read on appear
- [ ] `MessageBubble.kt` — sent (right, blue) / received (left, gray); delivery icons ⏱/✓/✓✓gray/✓✓blue
- [ ] `ComposeMessageScreen.kt` — recipient DID input, account picker

### UI — Calls tab
- [ ] `CallsScreen.kt` — placeholder (mirrors iOS `CallsView.swift`)

### UI — Network tab
- [ ] `NetworkScreen.kt` — server/project list, async load, token request on tap
- [ ] `ProjectWebScreen.kt` — `AndroidView { WebView(context) }` for Project UIs

### UI — Common
- [ ] `AccountAvatar.kt` — avatar composable with initials fallback (mirrors `AccountAvatar.swift`)
- [ ] `DevSettingsScreen.kt` — service mode toggle, account/conversation counts (mirrors `DevSettingsView.swift`)

### Permissions & manifest
- [ ] `INTERNET` and `CAMERA` permissions
- [ ] `POST_NOTIFICATIONS` permission (API 33+) for future push
- [ ] `actnet://invite` intent filter for deep links (mirrors iOS URL scheme)
- [ ] FCM service stub for when push notifications are implemented

### Testing
- [x] `MockServiceTest.kt` — verify `MockAppCore.receiveMessagesWs()` delivers echo reply after ≥1.5 s
- [x] Cross-platform interop test: iOS sends encrypted DM, Android decrypts it against a real test homeserver (add to `core/crates/app-core/tests/`)
## Crypto / protocol
- Bot edit-history suppression + revision capping (docs/36): recipients currently store a prior-body revision for every inbound edit. The spec says bot-authored messages should retain no edit history (a live-tally bot editing hundreds of times would bloat every recipient's device). Add a cheap local is-bot check on the receive path (or a per-message revision cap) once high-frequency bot editing is in use.
- Receive-side edit/delete window clamping (docs/36): the authorship rule is enforced on receive (security-critical), but the 24h/30-day windows are only enforced by the sending UI today. Add defense-in-depth clamping of out-of-window inbound edits / FOR_EVERYONE deletes.
- State-driven group re-establishment reconciler: for every group with a master key but no registered push pseudonym, register one (`rotate_group_push_binding`) + subscribe + fetch state (Sender Key / SKDM is already lazy on first send). Must trigger on storage-sync apply, NOT gated on "am I recovering?" — a linked second device gets group keys via sync without ever recovering, and is just as much a fresh device needing a pseudonym. This generalizes today's recovery-only `restore_group` and is the prerequisite for moving group keys out of the recovery blob (Privacy/identity). Note: it inherits the single-active-device assumption until per-device push bindings land (Server).
- Legacy raw-text group messages decode heuristically on receive (`process_decrypted` tries `ContentMessage::decode`, falls back to raw text). All new group messages carry the envelope; the fallback only matters for messages sent before this migration. Pre-launch there are none, so the heuristic is effectively dead code — drop the fallback (require the envelope) once there's confidence no pre-migration group messages remain in any store.

## Server
- Adminbot routing config (docs/22): map an `AccountJoinedEvent`'s invite-token issuer + routing tags → channels (declarative rules), now that the event carries `invite_token`.
- Adminbot node bot: consume the catch-up endpoint `GET /v1/admin/events?since=` on reconnect (events are now persisted in `server_events`); today it only acts on live WS pushes.
- Future `bots.provision` capability + `purpose = "bot"` registration tokens (docs/24 end state — bots sign up with a token signed by their Project). The token format already carries `purpose` and the redemption table is generic, so this is additive: add the capability string, a `purpose == "bot"` admission arm, and auto-link the new bot to the issuing Project via the token's `iss`.
- Per-device group push bindings (concurrent multi-device group receive). Today `member_credentials(group_id, encrypted_member_id, group_push_pseudonym)` holds one pseudonym per (group, account-member EMI) and `rotate_member_pseudonym` replaces it — so two active devices of one account clobber each other's binding and only the last to register gets group fan-out. Needs per-device pseudonyms (multiple per EMI), fan-out delivering each member's slice to all their devices, and possibly sender-side fanout changes. Gates moving group keys out of the recovery blob for the multi-device case (Privacy/identity).
- Push group-state changes (membership/title/policy) over the WebSocket. Today the only WS group frames are message fan-outs; clients learn of state changes by polling `fetch_group_state` on navigation. A "group changed" push would let clients trust cached state and stop polling on every open (see the `fetchGroupState` coalescing item under Mobile app).

## Infra
- Right now foreground apps poll the server every minute for storage updates; implement something that reduces this poll rate -- probably proactive sync of some sort. Part of multi-device implementation.

## Project-wide
- Mass rename: rename repo, update bundle IDs, update all remaining `actnet` references in code and docs to `avalanche`

## Big milestones (not yet started)
- First-party Projects: channel directory, team assignment, action-day map, Q&A bot, collab docs, engagement tracking
- Federation: server-to-server protocol, cross-server DMs, full DID portability (PLC directory), guest access
- Android app (see `docs/33-android.md` for full implementation guide)
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

## Desktop client (future)

A desktop client (macOS, Windows, Linux) can share most of its codebase with the Android app via **Kotlin Multiplatform + Compose Multiplatform** (JetBrains). The UniFFI-generated Kotlin bindings use JNA under the hood, and JNA loads native libraries on the desktop JVM too (`.dylib` on macOS, `.so` on Linux, `.dll` on Windows) — so the same `AppCoreInterface` / `AppViewModel` layer from the Android app is reusable with minimal changes.

Defer until the Android app reaches a stable milestone.

- [ ] Evaluate Compose Multiplatform maturity for production desktop use
- [ ] Add `make desktop-macos`, `make desktop-linux`, `make desktop-windows` Makefile targets to compile `libapp_core` as a shared library for each OS (requires cross-compilation toolchain setup)
- [ ] Scaffold a `desktop/` Kotlin Multiplatform module that shares `AppViewModel`, models, and service interfaces from `mobile/android/`
- [ ] Handle per-OS secure storage: macOS Keychain, Windows Credential Manager, Linux Secret Service / `libsecret`
- [ ] Handle per-OS notification APIs: macOS `UserNotifications`, Windows `Windows.UI.Notifications`, Linux `libnotify`
- [ ] Handle per-OS deep link / URL scheme registration for `actnet://`

## Push Notifications (remaining work)
- Android client: FCM token registration, pseudonym lifecycle, wakeup handling
- Relay: real APNs sending via `a2` crate (env vars: APNS_KEY_PATH, APNS_KEY_ID, APNS_TEAM_ID, APNS_BUNDLE_ID)
- Relay: real FCM sending
- iOS: periodic pseudonym rotation (weekly timer)
- iOS: opt-out setting for high-risk users (poll-only mode)
- Testing: verify relay payloads contain zero user-identifiable content; APNs/FCM sandbox integration test
