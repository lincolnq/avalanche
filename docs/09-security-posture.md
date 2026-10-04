# 09 — Security posture

> **Status:** Living document. It describes what each adversary actually learns **today**,
> including where the code falls short of the design, and is the security register for known
> gaps.
> **Last verified against code:** 2026-10-03

## Summary

Avalanche is designed against two threats: **server seizure** and **surveillance of
membership**. Message content is end-to-end encrypted using libsignal, and that part holds up.
The metadata story is weaker than the subsystem docs historically claimed. Group tables are
structurally free of member identities, but the DM layer, the push relay, and some server
tables leak co-membership and timing to a live operator, and to a lesser degree to a seized
database. There are also several concrete vulnerabilities (the register below), a few of
which are urgent.

Any privacy claim made in another doc should be read against this one. When you fix a gap,
update its row here and the subsystem doc's Known gaps in the same change.

## Threat model

**In scope:**

- **Server seizure** — law enforcement or a hostile party takes a homeserver's disk and
  database. It should not yield contacts, group memberships, message history, or real names.
  Users should be able to carry on elsewhere.
- **Surveillance of membership** — membership lists are targeting data for activists. Limit
  what any party (server operator, relay, other servers, the public) can learn about who is
  in which org or group, and limit linking a person across servers and identities.
- **Hostile participants** — strangers who know your identifier, malicious group members,
  malicious or careless Projects.
- **Device seizure** — limited: disappearing messages bound what a seized, unlocked phone
  yields; data at rest is encrypted under platform-held keys.

**Out of scope (deliberately):** targeted state-level surveillance of a specific person
(no onion routing, cover traffic, or mixnets — users can add Tor or a VPN); network traffic
analysis beyond TLS; a compromised OS; coercion of a user to unlock their device.

## What holds today

These properties are implemented and are what the rest of the system relies on:

- **Content confidentiality.** DMs use the Signal protocol (X3DH/PQXDH + Double Ratchet); groups
  use Sender Keys. Servers, the relay, and Projects without a member bot see only ciphertext
  (`01`, `03`).
- **Group tables name no members.** Group state is an encrypted blob; the server-visible
  routing set is keyed by encrypted member IDs; no table maps DIDs to groups (`03` §3.9 rules
  1–4 hold in the schema). Group fetches return 404 to non-members.
- **Anonymous group send.** `POST /v1/groups/{id}/send` rejects session credentials and
  authorizes by a zkgroup group-send token, so the server doesn't learn which member sent a
  group message (`03` §3.11). The source IP is visible; see below.
- **Encrypted profiles.** Display names and avatars are encrypted under a profile key the
  server doesn't hold (`52`, `55`).
- **Homeservers never see push tokens.** They wake pseudonyms; the relay holds tokens (`15`).
- **Bots are always visible.** There is no out-of-band read path for Projects (`20`).
- **Data at rest on iOS and Android** is in SQLCipher, keyed from the Secure Enclave or
  Keychain on iOS and the Keystore on Android (`06`). **Desktop is not:** see S-05.

## What each adversary learns today

| Adversary | Learns today | Intended | Main gaps |
|---|---|---|---|
| **Seized homeserver (database at rest)** | Which DIDs are registered; the encrypted profile and group blobs; up to 30 days of undelivered DM rows, each carrying the sender's account (co-membership via SKDM and invite bursts); group change history that is readable JSON with pseudonyms and link passwords, never pruned, exact timestamps; about an hour of IP addresses tied to anonymous group sends and token issuance; raw invite tokens in `server_events`. | DIDs and ciphertext only. | S-09, S-10, S-11, S-12 |
| **Live homeserver operator** | All of the above in real time, plus: who is online; IP addresses; account-to-pseudonym-to-group links while connected (accepted, `03` §3.7); the full identified DM graph on this server (no sealed sender for 1:1). | Social graph within its own org (accepted), but not group membership. | S-09 (sealed sender for DMs is the main fix) |
| **Push relay operator** | Every device's DM pseudonym and all of its group pseudonyms (registered as one batch), and wakeup timing. One relay serves every server. | Pseudonym timing only. | S-13 |
| **Relay and homeserver together** | Device to DM pseudonym to account, and device to group pseudonyms to groups: full group membership. | Nothing beyond each alone. | S-13 |
| **The public (PLC log)** | Every DID's genesis operation, which commits to the **signup server URL**, plus every later key rotation, all permanent. Also the recovery-blob endpoint lets anyone with a DID test which servers hold it. | DID-to-server association optional. | S-06, S-07; Proposed identity changes (`50`) |
| **Stranger who knows your DID** | That their message was delivered (the automatic delivery receipt, without your profile key since S-02 was fixed). Can invite you to groups, but the invite waits as a request until you Join (S-04 fixed). Attachment pointers that point at the stranger's own server make your device fetch from it automatically (IP leak, unbounded download). | Nothing until you accept. | S-08 |
| **Malicious group member** | Can hijack another member's group delivery (pseudonym squatting) and redirect their push wakeups. Removed members keep current Sender Keys (no rotation on removal), and clients don't check that a sender is a member. Can set a group message expiry beyond the server's backstop. | Ordinary member view. | S-14, S-15, S-16 |
| **Project operator** | Its own Project's bot signup key, which admits bots linked to that Project only (S-01, fixed). Project tokens have no audience, so a token issued for one Project can be replayed to another. Join events no longer carry raw tokens. Any human user on a server running adminbot can install Projects (S-29). | Its own granted capabilities only. | S-17, S-29 |
| **Stolen or compromised device** | The PLC **rotation key** (stored on every device, sent to linked devices): permanent, unrevocable DID takeover. No device list or revocation. On Desktop, the database key is a constant. Attachments of deleted or expired messages survive in plaintext caches. | That device's sessions and unexpired history. | S-05, S-06, S-18, S-25 |
| **A page on `*.theavalanche.net`** | The passkey RP is `theavalanche.net` with a fixed PRF salt, and the demo server serves Projects at `av.theavalanche.net/p/<slug>/`. A malicious or compromised page there can request the root PRF secret in a browser. | Only first-party apps. | S-07 |
| **Network observer** | IP-to-server connections, timing (out of scope). | Same. | — |

## Security register

Severity: **Critical** (exploitable now for admin takeover, identity takeover, or broad
deanonymization), **High**, **Medium**, **Low**. "Verified" means confirmed by reading the
code on 2026-10-03; "Reported" means found in review but not independently confirmed.

| ID | Severity | Gap | Where | Status | Fix (doc) |
|---|---|---|---|---|---|
| S-01 | Critical | Project setup codes embedded `REGISTRATION_SHARED_SECRET`; editing the token's project slug to `adminbot` registered a superuser bot. The secret also reached testbot's env and, via raw invite tokens in `server_events`, every Project with `accounts.read`. | `adminbot/src/index.ts`, `server/src/routes/registration.rs` `gate_registration` | Fixed in code (not yet deployed) | Per-Project reusable **bot signup keys** replace setup codes; the shareable registration secret can no longer link any Project; a separate `SUPERUSER_BOOTSTRAP_SECRET`, generated on the box, claims superuser once; join events no longer carry raw tokens and stored ones are purged (migration 026). After deploy, rotate `REGISTRATION_SHARED_SECRET` where old setup codes were handed out (`22`, `24`, `51`) |
| S-02 | High | Automatic delivery receipt to an un-accepted message request carried your profile key, so any stranger who DMs you could decrypt your name and avatar. | `app-core/src/messaging.rs` `delivery_receipt` | Fixed in code (not yet deployed) | Receipts carry the key only to accepted (curated) contacts; unit-tested (`52`, `12`) |
| S-03 | High | A self-declared bot (`is_bot` at open registration) bypassed the message-request gate. | `messaging.rs` `SenderGate`, `routes/accounts.rs` | Fixed in code (not yet deployed) | Only bots linked to an installed Project on your server (`project_bot` on the account record) skip requests; e2e-tested (`54`) |
| S-04 | High | Group invites were joined automatically from anyone, blocked senders included, publishing your membership and profile to a stranger's group. | `messaging.rs` `group_invite_disposition`, `groups.rs` `hold_inbound_group_invite` | Fixed in code (not yet deployed) | Only an accepted contact's or a Project bot's invite auto-joins; a blocked inviter's is dropped; anyone else's is a request (Join / Delete / Block) on all three platforms; e2e-tested (`12`, `03`) |
| S-05 | High | Desktop's SQLCipher key is the constant `"dev-placeholder-key"`; the database holds the identity and rotation keys. | `desktop/src/state/createAccounts.ts:179,225,432`, `createDeviceLink.ts:69` | Verified | OS keychain-backed key (`61`) |
| S-06 | High | The PLC rotation key is persisted on every device and included in the device-link bundle; it is the only rotation key, so any device compromise is permanent DID takeover. | `store/src/account.rs:109`, `provisioning.proto:27`, `plc.rs:186` | Verified | Priority-ordered rotation keys; the passkey key stays top-priority and is never stored; linking authorized by an existing device's signature (`50` Proposed, `04`) |
| S-07 | High | Passkey RP `theavalanche.net` with a fixed salt, while Project content is served under `av.theavalanche.net`. A page on any subdomain can run the ceremony and obtain the root PRF. | `PasskeyManager.swift:21,26`, `infra/deploy/bundle/lib/common.sh:209` | Verified (config); exploit path reported | Dedicated passkey domain with nothing else on it; per-Project origins outside it (`50`, `20`) |
| S-08 | High | Attachment pointers carry a full URL that recipients fetch automatically, with no host restriction and no size cap: a sender, including a stranger, can harvest recipients' IP addresses, or exhaust their memory. | `net/src/lib.rs:1259-1273`, `messaging.rs` ~1118 (fetch runs under the core lock, `app-core/src/lib.rs:2273-2277`) | Verified | Constrain the host to known homeservers; cap reads at the pointer's size; don't auto-fetch from un-accepted senders; fetch off-lock (`35`) |
| S-09 | High | Identified DM-plane traffic undercuts group membership opacity: SKDMs and invites travel as identified DMs and the queue stores `sender_account_id`. | `001_initial.sql:64`, `routes/messages.rs:140` | Verified | Sealed sender for 1:1 and SKDM traffic (`03` Planned, `13`) |
| S-10 | Medium | Group change history is server-readable JSON (member IDs, roles, pseudonyms, link password), never pruned, and annotated "opaque". | `routes/groups.rs:589`, `010_groups.sql` | Verified | Prune to the 256-revision ring; keep pseudonyms and passwords out of readable history (`03`) |
| S-11 | Medium | Exact timestamps on `group_state_history` and `group_member_pseudonyms` (§3.9 rule 5). | `db/groups.rs:456` | Verified | Day-align or drop (`03`) |
| S-12 | Medium | Raw IPs persisted in Postgres `ip_rate_limit_counters` (about an hour) for anonymous group sends and token issuance, linkable by minute. | `routes/groups.rs:528,633,727`, `db/ip_rate_limits.rs` | Verified | In-memory, keyed counters (`03`) |
| S-13 | Medium | The relay sees each device's full pseudonym set; relay registration is an unauthenticated `INSERT OR REPLACE`. | `app-core/src/lib.rs` ~3205, `relay/src/main.rs:556` | Verified | Register pseudonyms separately and unlinkably; authenticate registration; pseudonyms backed by a secret (`15`, `41`) |
| S-14 | High | WebSocket group subscription is last-writer-wins with no ownership check, and draining acknowledges and deletes rows; members can see each other's pseudonyms in change history. A member can silently take over another's group delivery. | `routes/websocket.rs:398-423`, `routes/groups.rs:480-498` | Verified | Pseudonym backed by a secret (the server stores a hash; subscribe presents the preimage); interim: refuse to steal a live subscription (`03`) |
| S-15 | High | Clients install SKDMs and decrypt group messages without checking that the sender is a member, and Sender Keys aren't re-seeded when someone is removed; `announcement_only` isn't enforced on receive. | `messaging.rs:1319,1766,1802` | Verified (exploitability of injection reported) | Membership checks on receive; re-seed on removal; enforce announcement-only (`03`) |
| S-16 | Medium | Group message expiry is not clamped by the server, so a sender can extend server retention. | `routes/groups.rs:866` (DMs clamp at `routes/messages.rs:133`) | Verified | Clamp like DMs (`03`) |
| S-17 | High | Project tokens have no audience: `verify` is unauthenticated and audience-free, and `issue` accepts any `project_url`. A token for one Project is valid at another. Tokens also travel in the URL query string. | `routes/projects.rs:72-124` | Verified | Mandatory audience on verify; mint only for installed origins; move tokens out of the query string (`20`) |
| S-18 | Medium | No device list or revocation. Recovery replaces only one device slot on the primary server; `/link` and `/replace` don't check the identity key against the identity's, `/replace` isn't transactional or IP-rate-limited, and PLC fetches have no timeout. | `routes/devices.rs:84-180`, `plc.rs:42` | Verified | Device management; harden these endpoints (`04`, `50`) |
| S-19 | Medium | `GET /v1/recovery/{did}` is unauthenticated and unrate-limited, and returns device IDs: a test of which servers hold a DID. | `routes/recovery.rs:39-51` | Verified | Authenticate with a rotation-key-signed challenge; drop `device_ids` (`50`) |
| S-20 | Medium | The genesis PLC operation publishes the signup server permanently. | `plc.rs:172-193` | Verified | Proposed unpublished identity (`50`) |
| S-21 | Medium | Device linking accepts pasted codes with no on-screen confirmation, and hands over identity, rotation, and storage keys (the 2025 Signal linked-device phishing pattern). | `04` §4 | Verified (design) | Matching confirmation code; "new device linked" notice on all devices (`04` §4.3) |
| S-22 | Medium | Storage-sync records aren't bound to their version, so a server can replay stale records (for example, undo a block). | `storage_sync.rs:77-105` | Verified | Bind version in associated data; client-assigned versions (`05`) |
| S-23 | Medium | The Project webview on iOS is a default `WKWebView` that allows any navigation; Network-tab and `conversation/` deep links use the first account, not the right identity. | `ProjectWebView.swift`, `NetworkView.swift`, `AppState.swift` `handleDeepLink` | Verified | Origin-locked webviews; identity-correct routing (`20`, `23`) |
| S-24 | Medium | No server-side WebSocket ping: half-open sockets suppress push for that device. | `routes/websocket.rs` | Verified | Server ping with idle timeout (`10`) |
| S-25 | Medium | Deleting or expiring a message leaves its attachment and link-preview rows (including attachment keys) in the store, and decrypted attachments are cached in plaintext outside SQLCipher on all three platforms, never cleaned up. Defeats disappearing messages against device seizure. | `store/src/messages.rs:553-575,761-835`, iOS `AppState.swift:1457-1476`, Android `AppViewModel.kt:1556-1576`, Desktop `src-tauri/src/lib.rs` | Verified | Delete rows and cache on tombstone/expiry; encrypt or clear caches (`35`, `36`) |
| S-27 | Medium | Group-avatar routes act on any blob UUID in the shared blob store (avatars and attachments share a namespace), and profile-avatar ids derive from sequential account ids. | `server/src/routes/avatar.rs:171-214`, `blobstore.rs` | Verified | Separate namespaces; salted avatar ids (`55`) |
| S-28 | Low | Desktop link-preview fetch has no SSRF guard; deep links accept unvalidated DIDs and create conversation rows. | `desktop/src-tauri/src/lib.rs`, `handleDeepLink` on all platforms | Reported (from backlog) | See `02` |
| S-31 | Medium | Deleting an identity doesn't clear `message_attachments` or `message_link_previews`, so attachment keys and preview data survive in identity.db. | `store/src/account.rs` `wipe_identity` | Verified | Add both tables to the wipe (`53`) |
| S-29 | Accepted (by design) | Adminbot auto-invites every new human into `#admins`, and admin commands check `#admins` membership, so on a server running adminbot everyone who signs up is an admin. This is intended: a new org's first members usually don't need admin to be controlled. The risk arrives when a server grows or shares its invite widely, and today nothing tells the operator when or how to tighten it. | `adminbot/src/index.ts` `inviteToAdminGroups` (~405-434), `requireAdminsMember` (~631-638) | Verified (intended) | Planned: onboarding material that explains open admin and how to close it, plus a simple way to turn off auto-admin (`22`) |
| S-30 | Medium | Any bot registration may choose its own `did:local:` suffix, so a holder of an admission credential can squat reserved names such as `did:local:adminbot` on a fresh server before adminbot registers, and impersonate them. | `server/src/routes/registration.rs` `validate_reserved_suffix` | Verified | Reserve well-known suffixes (or drop caller-chosen suffixes, `22` random `did:local:`) |
| S-26 | Low | Mesh fallback design keys BLE tags on a public identity key (design-only; not built). | `14` | Design | Key tags from session secrets before building (`14`) |

## Hardening roadmap

The order matters more than the list. Roughly:

1. **Close the critical and stranger-facing gaps** (S-01..S-04 fixed;
   S-05, S-08, S-14, S-17, S-30), then the at-rest cleanup gap (S-25). These are small, contained fixes and don't need design work.
2. **Move the identity root off devices** (S-06, S-07, S-19, S-21): passkey domain isolation,
   priority rotation keys, link confirmation. Parts are contract changes; see `50` Proposed.
3. **Sealed sender for 1:1 and SKDM traffic, with delivery keys** (S-09). This is the biggest
   single privacy win, and it's also the foundation for client-side federation (`13`).
4. **Server metadata hygiene** (S-10, S-11, S-12, S-16) and **relay unlinkability** (S-13).
5. **Group membership enforcement on clients** (S-15).

## Proposed changes to the security model

Pending project-owner review; not to be implemented until approved:

- **Identity** (`50`): a random root key wrapped under each unlock method (passkey on the
  primary domain, passkey on a backup domain, recovery phrase); priority-ordered PLC rotation
  keys with the top key never stored; a self-certifying private identity that is not
  published, with `did:plc` as an optional opt-in public link.
- **Federation** (`13`): servers never talk to each other; clients deliver to a recipient's
  server under sealed sender, authorized by delivery keys that only contacts hold, with
  identified, rate-limited first contact for strangers.

## Audit readiness

Not yet ready for an external audit:

- **CI:** no `cargo audit` or `cargo deny`, no app-core end-to-end tests, and no iOS, Android,
  relay, or bot builds run in CI (`01`).
- **Untested invariants:** the `03` §9 membership-opacity invariant tests don't exist; three
  realistic ones are planned.
- **Builds:** no reproducible builds.
- **Maintenance:** this register needs to stay current. An auditor should be able to start
  from this doc.
