# 02 — Todo list and roadmap

> **Status:** Living document. Rebuilt 2026-10-03 from a full review of the docs against the
> code; the previous backlog's open items are carried over below.
> **Last verified against code:** 2026-10-03

One line per item: what, then the doc that holds the detail. Security items carry their
`09` register ID. Delete an item when it ships (don't strike it through); if it changes a
contract, it needs owner review first (root `CLAUDE.md`).

**Priorities:** **P0** security, fix now · **P1** correctness or important gap ·
**P2** planned feature · **P3** cleanup. Items under "Awaiting owner review" are Proposed
contract changes and are blocked on that review.

## Now: P0 security

Small, contained fixes; no design work needed. Each is a bug fix, so per the root
`CLAUDE.md` it lands on the affected platform(s) and is verified there.

- **Operator action after the S-01 fix deploys:** rotate `REGISTRATION_SHARED_SECRET` on servers
  where old setup codes were handed out (at least `av.theavalanche.net`), and re-issue each
  Project's bot signup key with `/install-project` (S-01, `22`).
- Attachments: restrict download hosts to known homeservers, cap reads at the pointer's size,
  don't auto-download for un-accepted senders, and fetch off the core lock (S-08, `35`).
- Project tokens: mandatory audience on `verify`; mint only for installed Project origins
  (S-17, `20`).

## Next: P1 correctness and important gaps

**Security hardening (needs some design, mostly within existing contracts)**
- Give the passkey RP a dedicated domain that serves nothing else; move Project hosting off
  `*.theavalanche.net` paths (S-07, `50`, `20`).
- Authenticate and rate-limit `GET /v1/recovery/{did}`; stop returning `device_ids` (S-19,
  `50`).
- Device-link confirmation code shown on both screens, plus a "new device linked" notice on
  all devices (S-21, `04` §4.3).
- Device list and revocation, with a server endpoint and FFI (S-18, `04` §9).
- `/link` and `/replace`: require the identity's existing identity key, run in a transaction,
  IP rate-limit before the PLC fetch; add timeouts to all PLC fetches; make the PLC URL
  configurable (S-18, `04`, `10`).
- Recovery revokes the identity's whole device set on every server, not one slot on the
  primary (`04` §7).
- Client-side group checks: accept SKDMs and group messages only from cached members, re-seed
  your own Sender Key when a member is removed, enforce `announcement_only` on receive (S-15,
  `03`, `32`).
- Prune group history to the 256-revision ring; keep pseudonyms and link passwords out of
  server-readable history; day-align `created_at` columns (S-10, S-11, `03`).
- Move IP rate-limit counters out of Postgres into memory (S-12, `03`).
- Authenticate relay registration; register pseudonyms so the relay can't link a device's
  full set (S-13, `15`, `41`).
- Delete attachment and link-preview rows and cached plaintext when a message is tombstoned,
  deleted for me, or expires; encrypt or clear plaintext attachment caches on all platforms
  (S-25, `35`, `36`).
- Separate the avatar and attachment blob namespaces; salt profile-avatar ids (S-27, `55`).
- Delete-identity must wipe `message_attachments` and `message_link_previews` too
  (`IdentityStore::wipe_identity`) (S-31, `53`).
- Storage sync: keep dirty local edits on pull, retain unknown-type payloads, bind record
  versions against server rollback (S-22, `05`).
- Webviews: origin-locked navigation and per-Project data stores; Network tab and
  `conversation/` deep links use the correct identity (S-23, `20`, `23`).
- Fix the identity key's multicodec in DID documents (published as Ed25519, actually
  Curve25519) (`50`).
- Gate adminbot's `/audit` on `#admins` membership (`22`).
- Validate gatekeeper tokens fully in `GET /v1/invites` (`24`).
- Reserve well-known `did:local:` suffixes (or stop letting bots choose them) so `adminbot`
  can't be squatted on a fresh server (S-30, `22`).
- Desktop link-preview fetch: reject redirects to non-public IPs, require `image/*` for
  og:image, clamp the body cap, open the validated URL (S-28, `35`).
- Desktop Project webview: decide whether non-loopback `http:` needs a developer-mode
  setting (it currently sends the token in cleartext to any `http:` host) (`20`).

**Reliability and correctness**
- Server-side WebSocket ping with idle timeout, so half-open sockets stop suppressing push
  (S-24, `10`, `34`).
- Cancellation path for parked `next_events` / `wait_for_connection_state_change`, so logout
  actually closes the authenticated socket (`07`).
- Connection-state Layer 2: outage tiers so a dead server stops pinning the banner and stops
  hammering reconnect (`34`).
- Build the no-blob recovery path, or stop promising it (`50`).
- "Add a server" with an existing identity must actually register on that server (`53`, `30`).
- Per-conversation mute (`37`).
- CI: add `cargo audit`/`cargo deny`, app-core end-to-end tests, and iOS, Android, relay, and
  bot builds (`01`).
- Adminbot consumes the event catch-up endpoint on reconnect (`22`).
- Re-enable the `admin_list_accounts` server test (currently `#[ignore]`d because pagination
  makes it flaky on a shared dev DB) (`10`).

## Awaiting owner review: Proposed contract changes

- **Identity root** — random root key wrapped under each unlock method (passkey, backup-domain
  passkey, recovery phrase); priority-ordered PLC rotation keys with the top key never stored
  (S-06) (`50` Proposed).
- **Unpublished identity** — self-certifying private identifier; `did:plc` becomes an opt-in
  public link (S-20) (`50` Proposed).
- **Sealed sender for 1:1 and SKDM traffic, with delivery keys** — the largest privacy win
  (S-09) and the base for federation (`03`, `13`, `52`).
- **Client-side federation** — servers never talk to each other; clients deliver to the
  recipient's server (`13`). Resolve how abuse-report forwarding works without
  server-to-server calls (`12`).
- **Acquisition path** — `/project/<t>` deep links that survive App Store install, and opaque
  per-conversation launch handles passed to Project webviews (`23`).
- **OIDC-conformant "Sign in with Avalanche"** as the main developer story (`25`).
- **Project tokens out of the URL query string** (`20`).
- **Quote-reply** via `reply_to{author, sent_at, unsurfaced}` (`32`; additive proto field).
- **Secret-backed group pseudonyms** — closes S-14 (members hijacking each other's group
  delivery), and part of S-10/S-13 (`03` Proposed).
- **Group/DM flag on `ConversationSummaryFfi`** so clients stop parsing `conversation_id`
  prefixes (`07`).
- **Validate `conversation/<did>` deep links** before creating a conversation row (S-28; all
  platforms).

## Later: P2 planned features

**Messaging**
- Group invite requests: clear a deleted request on your other devices, and refresh the iOS
  conversation title after Join (`12`, S-04).
- Share your profile when you accept a message request (Signal parity): apply profile keys
  carried on any message from an accepted contact, and send yours on accept (`52`).
- Quote-reply, once approved (`32`).
- Read-receipt preference toggle and send debounce (`31`).
- `profile_version` on the envelope, for profile liveness (`52`).
- Edits and deletes: recipient-side window and cap, hold out-of-order ops, previews on edits,
  no stored revisions for bot edits, Desktop 24h window (`36`).
- Reactions: notifications, who-reacted sheet, drop on expiry (`33`).
- Send queue: outbound messages persist and drain on reconnect; network-path monitoring;
  403 (membership revoked) handling (`34`).
- Disappearing messages: keep an empty conversation when all messages expire; system message
  on DM timer change; Android expiry handling (reaper refresh, persist timer on send).
- Scroll to the first unread message on open (capture the index before marking read).
- Day dividers in the timeline ("Today", "Yesterday", "Sat, Oct 4") on all three platforms:
  messages older than today show only a time, so a thread spanning days is ambiguous. Desktop
  already shows the full date on hover.
- Block and report from an accepted conversation; view another user's profile.
- Coalesce `fetchGroupState` and push group-state changes over the WebSocket instead of
  polling on every open (`03`).
- Unexplained (2026-10-04): in one group on av.savethedogs.io, the phone showed one of Desktop's
  messages live but none of Desktop's messages survived a relaunch; a fresh group made the same
  way worked fully. The failing group was created while the phone ran an older build. If
  incoming group messages vanish after a relaunch again, capture the phone's console
  (filter `groups`) before relaunching (`04`).
- Sender-key recovery on demand (Signal's retry receipts): a receiver that can't decrypt asks
  the sender to re-share its key and re-send the message from a short send log. New message
  type (contract change) and a ~1-day send log (privacy trade-off) — needs a design note
  (`04`).
- Nickname: sync via storage service; match nicknames in compose autocomplete; refuse
  perspective words ("You", "Me") as saved nicknames (`52`).

**Groups**
- `modify_policy` / `modify_description` FFI and UI: invite links, join policy,
  announcement-only (`03`).
- Scheduled 7-day group pseudonym rotation (`03` §3.7).
- Per-device group push bindings, so multiple devices of one account all receive group
  traffic (`03`, `04`).
- Report forwarding and the enforcement ladder (`12`).

**Identity, devices, accounts**
- Register an existing identity on a second server; recovery blob written to all servers
  (`53`, `06`).
- Trust-store sync adapter (`05`, `06`).
- Drop group keys from the recovery blob; enforce `MAX_RECOVERY_BLOB` (`05`).
- Reachability rows, remove-from-device, change home server (`53`).
- Desktop passkey bridge via external browser (`56`).
- Random `did:local:` for adminbot, with client migration (`22`).
- `preferred_identity`: compose defaults the sending identity per contact; groups and DMs can
  be founded on a chosen server (`52`).
- My QR Code and other single-identity screens use the active identity, not
  `accounts.first` (`53`).
- Server onboarding step webview; group auto-enroll from invites carried in the URL fragment
  (`51`).

**Projects**
- Open-admin onboarding (S-29, by design): tell operators every member is an admin for now and
  how to change it; a simple command to stop auto-inviting new members into `#admins`; a nudge
  when the server grows or installs a gatekeeper (`22`).
- Settable officialness and the checkmark badge (`22`, `54`).
- Gatekeeper install through adminbot; a sample gatekeeper Project (`24`).
- Identity picker on the login consent screen; Desktop as a login authorizer (`25`).
- Adminbot routing rules (invite-token issuer and tags to channels) (`22`).
- Adminbot reconciles removed manifests (uninstall Projects whose manifest is gone) (`22`).
- Sample Projects: gatekeeper/onboarding, Q&A bot, participant CRM with training modules.

**Push**
- Apple filtering entitlement (requested), then the suppressed-placeholder NSE path (`16`).
- Lost-push detection and token re-registration (`16`).
- NSE badge count (`16`).
- UnifiedPush: distributor picker, no-distributor foreground keepalive, unregister on logout
  (`15`).
- Privacy checks: relay payloads and logs contain only pseudonyms; rotation grace-period test;
  APNs/FCM sandbox test (`15`).

**Platforms**
- Android: avatar setting, killed-process push sync, recovery-key banner (`60`, `62`).
- Desktop: Project login, avatars, account tabs, search, QR scanning, image paste,
  long-press reaction overlay and recents (`61`, `62`).
- Cross-platform "one link": a web landing page that forwards `go.theavalanche.net` links to
  the desktop app or the app store (`23`).

**Architecture and ops**
- Actor-model app-core: one thread owns libsignal state, async exports, retire the global
  `Mutex<AppCoreInner>` (`07`).
- Generate the Tauri and napi bindings from the UniFFI surface, or at least fail CI when an
  export is unbound (`07`).
- Relay in the deploy bundle; separate dev and prod relays (`41`).
- Upgrades: rollback, pre-upgrade database dumps, N-1 migration checks, `/upgrade` from
  `#admins` (`42`).
- Storage sync: reduce the one-minute foreground poll (`05`).

## P3 cleanup

- Fix stale code comments: `ADMINBOT_DIDS` in `middleware/auth.rs`; comments citing
  `docs/35` for content now in `52` (`groups.rs:2596`, `lib.rs:2133`) or the nonexistent
  `docs/35-profiles.md`; `recover_from_blob` comment about the profile key.
- The three realistic §3.9 invariant tests: migration annotations, forbidden columns, send
  handler without an auth extractor (`03` §9).
- Negative-path end-to-end tests for groups (expired certificate, bad token) (`03`).
- Verify clients skip unknown `ContentMessage` variants (`08`).
- Async recovery end-to-end test harness with a PLC stub (`05`).
- Testbot: check `project_url` on verify; split the OAuth demo out (`21`).
- Neutral passkey labels, so a vault search doesn't link personas (`50`).
- Record the production `APNS_PUSH_MODE` somewhere authoritative (`16`).
- Split `desktop/src-tauri/src/lib.rs`; unify Desktop's chat-list preview builder; add a
  `resetGuards` helper for Desktop session guards.
- Desktop: unified full-height sidebar under a transparent macOS title bar.
- Drop the legacy raw-text group message decode fallback once no pre-envelope messages remain.
- Remove the invisible bottom-anchor spacer in iOS `ConversationView` when scroll position
  saving lands.
- Persisted identity list: consider a Secure-Enclave-keyed manifest DB instead of
  UserDefaults (iOS), and Keystore-keyed equivalents on Android and Desktop.
- Consider validating a scanned or pasted server invite during compose (`51`).
- Mass rename `actnet` → `avalanche` (repo, bundle IDs, relay paths).
- `.claude/commands/update-feature-parity.md` and `track-upstream-ios-parity-gaps.md` still
  describe emoji cells; update to the plain-word matrix in `62`.
- Watch: the iOS share extension launches the app via an unsanctioned responder-chain
  `openURL:` call; fallbacks if it breaks are a local notification or App Group shared-DB
  sends (`35`).

## Speculative

Kept for direction; no commitment.

- Calls (1:1 WebRTC; group via an SFU with insertable-streams E2E) (`01`).
- Large broadcast channels (`08`), full threading model (`32`), chat-organization tab model
  (`37`).
- Mesh fallback over BLE (`14`).
- Project-to-Project federation (`13`).
- Messenger-bot HTTP API Project, `create-avalanche-project` scaffold, offline-capable Project
  webviews (`23`).
- Trust scoring for federation abuse (`12`).
- Client-assigned record versions so every server is a dumb mirror for storage sync (`05`).
- Contact-list backup independent of identity keys; export in a standard format (`52`).
- `did:local:` identities for humans on private servers (largely subsumed by the Proposed
  unpublished identity, `50`).
- Short-code (PAKE) device linking for two camera-less desktops (`04`).
- First-party Projects listed in `00`.
