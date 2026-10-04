# 12 — Abuse handling

> **Status:** Partial — message requests, block, and local spam reports are built. Report forwarding to the reportee's server, the enforcement ladder, profile reports and group abuse are not. Federation trust scoring is Speculative.
> **Last verified against code:** 2026-10-03

## Summary

How users control their inbox and how operators act on abuse when the server
can't read content. Four layers, in increasing severity: **message requests**
(first contact is gated), **block** (local), **spam report** (account-level, no
content), **account-level enforcement** (operator policy). This follows Signal,
which has repeatedly passed App Store review under Guideline 1.2. The per-DID
contact row that carries block and request state is specified in `52`.

## Goals and non-goals

Goals: give users control of their inbox; give operators a way to act on abusive
accounts using metadata only; satisfy App Store 1.2 without the server ever seeing
content (**reports never contain content**); stay close to Signal.

Non-goals: global content moderation, a global ban list, client-side ML
moderation, perfect anti-abuse (a determined attacker with many identities can
always re-contact a target; the aim is to make casual abuse unattractive).

Adversaries: spammers (automated accounts, harvested DIDs), harassers (targeted,
possibly multi-identity), stalkers (defeated mainly by E2E, not this doc), and a
hostile operator who ignores reports about its own users.

## Known gaps

- **Self-declared bots skipped the request gate (S-03, fixed).** `SenderGate::passes` is now
  `is_curated || is_project_bot`: only a bot linked to an installed Project on your server
  (the server-vouched `project_bot` on its account record) skips requests, matching §1's
  "a bot trusted by the homeserver" (`54`).
- **Delivery receipts to un-accepted requests carried your profile key (S-02, fixed).**
  They now carry it only to accepted contacts (`messaging.rs` `delivery_receipt`), as
  Signal does. Owned by `52`.
- **Group invites from strangers were joined automatically (S-04, fixed).** An
  invite now goes through the same gate as a DM: an accepted contact's or a Project
  bot's invite joins; a blocked inviter's is dropped; anyone else's is held as a
  request (`pending_group_invites`, local-only) and shown with Join / Delete / Block
  (`messaging.rs` `group_invite_disposition`, `groups.rs` `hold_inbound_group_invite`).
  Remaining: the request row doesn't show the group's title yet (the client can only
  fetch state for a stored group), and deleting a request on one device doesn't clear
  it on your other devices.
- **Reports never leave the reporter's server** (§3), so the reportee's operator
  never hears about them.

## 1. Message requests — Built

A message from a DID the recipient hasn't accepted shows in the conversation list
with a request label; opening it shows the thread read-only with **Accept**,
**Delete**, **Report Spam and Block**. Until accepted: no read receipts, no typing
indicators, no replies. Delivery receipts are still sent (as Signal does). The
sender can't tell whether the request was accepted, declined, or unseen.

**Known sender** (skips the gate) = the contact row is curated (`is_curated`, `52`:
you messaged them, added them via QR/invite, or accepted) **or** the sender is a
bot. Implemented in app-core: `sender_gate` / `SenderGate` (`messaging.rs:185-211`,
`1180`), FFI `accept_request`, `delete_request` (`lib.rs:3768,3781`). The bot
exemption should require server vouching (`54`'s `official` flag), not
self-declaration (Known gaps).

Message requests are the App Store 1.2 "filtering" mechanism: unsolicited content is
hidden until the user opts in.

## 2. Block — Built

Blocking is local and unilateral. State is the `is_blocked` flag on the contact row
(`52`), synced across your devices as part of the contact record in the storage
service (`05`; `app-core/src/storage_sync.rs` contact adapter). FFI `block_contact` /
`unblock_contact` (`lib.rs:3704,3717`).

Effects: inbound messages from a blocked DID are still decrypted (to advance the
ratchet) and then dropped — no event, no notification, no receipt
(`messaging.rs:1270-1274`). Outbound: the UI replaces the composer with an unblock
prompt (iOS `ConversationView.blockedBar`); app-core itself does not refuse sends to
a blocked DID. History is preserved. Unblocking reverses all of this; messages dropped while
blocked are gone.

**Server-side block enforcement — rejected for v1.** Pushing the block list to the
server would leak each user's cut-off list and diverge from Signal, whose server
knows nothing about blocks. Revisit only if queue-flooding by blocked senders is
seen in practice; preferred fixes then are per-(sender, recipient) rate limits or an
opt-in server block list for users under active harassment.

## 3. Spam report — Partial

Reports are exposed **only on message requests** (and, when built, group invites
from strangers), not in established conversations, as Signal does. A request is the
highest-value signal; after you've accepted a conversation the remedy is block. This
keeps reports high-quality and blunts weaponized reporting.

**What a report contains:** the reported DID and a reason enum (`spam`,
`harassment`, `impersonation`, `other`). Never content, hashes of content,
conversation history, or device info.

**Built:** `report_and_block(did, reason)` (`lib.rs:3798`) calls
`POST /v1/abuse/report` on the reporter's own homeserver
(`server/src/routes/abuse.rs`), which rate-limits per account and stores the row in
`abuse_reports(reported_did, reason, reporter_account, reported_at)`
(`infra/migrations/018_abuse_reports.sql`) for operator review, then blocks locally.
The reporter's server already knows you were DMing that DID, so storing the reporter
adds no new leak.

**Planned: forwarding to the reportee's server.** The reporter's server signs
`{reported_did, reporter_homeserver, reported_at, reason}` and forwards it, without
the reporter's DID. The reportee's operator learns "a user of server X reported your
user", like email abuse reports between postmasters. Rejected alternatives:
- *Direct client → reportee-server report:* leaks the reporter's identity or IP to
  the adversary's server.
- *Anonymous unsigned report:* trivially forgeable; can't be rate-limited.

**Open conflict with `13`.** Forwarding is a server-to-server call, and the Proposed
client-side federation model has no server-to-server channel. Options: keep
abuse-report forwarding as the one narrow server-to-server endpoint; or have the
reporter's server issue a signed, reporter-anonymous report token that the client
submits to the reportee's server itself. Decide when federation is built.

## 4. Account-level enforcement — Planned

Operator policy, not protocol. Signals available without content: incoming reports
(count, distinct reporters, recency, reasons), send-rate metrics, account age,
prekey churn, registration metadata. Suggested default ladder:

| Trigger | Action |
|---|---|
| 5+ distinct reporters in 24h | Throttle sends (e.g. 1/min for 24h) |
| 20+ distinct reporters in 7d, or 50+ total | Suspend: can't send, can still receive (so they learn why) |
| Operator review, or 100+ reports | Ban: remove the account from this server, delete its prekeys and queues |

A ban removes the account **from this server only**. The DID is the user's (only the
rotation-key holder can change it), so it can't be deleted, and the user can still
exist on other servers. For local reports "distinct reporters" means distinct local
accounts; for forwarded reports, distinct reporting servers (so one hostile server
can't manufacture volume).

Suspended or banned users get a system message explaining the action and how to
appeal. Operators should publish aggregate enforcement stats.

## 5. Federation trust and contact attestation — Speculative

Earlier drafts had each server compute a per-peer trust score and a per-user
"attestation quality" score from contact-QR attestations, reports and declines, and
gate inbound federation on them ("default deny with attestation as the gate").

Kept here only as a record. Reasons not to build it:
- It assumes server-to-server federation, which `13` now proposes to replace with
  client-side delivery. Under that model the receiving server sees deliveries to its
  own users and needs per-sender rate limits and delivery keys, not peer scores.
- The attestation store is itself a seizable record of which local users have
  contacts on which servers — exactly the metadata the threat model protects.
- With one or two servers in existence, there's no signal to score.

If reputation sharing is ever needed, the least-bad starting point is opt-in signed
blocklists published by trusted third parties (an operator coalition), ingested as
local inputs and never overriding local policy.

## 6. Profile-level abuse — Planned

Display names and avatars are abuse vectors because they're shown to anyone with the
profile key. Plans: a client-side display-name filter (on by default; tap to
reveal); profile reports reusing §3 with reasons `impersonation` /
`objectionable_profile` and a signed profile snapshot; operator action = force a
profile reset, repeat offenses suspend. None built.

## 7. UI surfaces

| Surface | Affordance | Status |
|---|---|---|
| Message request | Accept / Delete / Report Spam and Block | Built |
| Conversation menu (accepted) | Block / Mute / Disappearing messages | Block and timer built; mute not built |
| Profile view | Block / Report Profile | Block built; report not built |
| Settings → Blocked | Unblock | See `62` |
| Group invite from a stranger | Join / Delete / Block (Block reports and blocks the inviter, and declines) | Built (S-04) |

## 8. What we explicitly do not build

- No content reporting; the server never receives plaintext or hashes of it.
- No report button in established conversations — use block.
- No global ban list.
- No client-side ML moderation.
- No proactive content scanning on device. If legally compelled somewhere, that is a
  hard design conflict for legal review.
- No "who reported me" lookup.

## 9. App Store 1.2 mapping

| Apple requirement | Our implementation |
|---|---|
| Filtering objectionable material | Message requests |
| Reporting + timely response | Report Spam and Block → operator review (forwarding and ladder planned) |
| Blocking abusive users | Per-identity block, client-enforced |
| Published contact info | Support email in app and listing |

Review note: "This is an end-to-end encrypted messaging app. The server cannot read
message content. Abuse handling is account-level, following Signal and WhatsApp."

## 10. Open questions

- **Group abuse.** Mass-add-to-group spam, and reporting a sealed-sender group
  message (needs selective sender-certificate disclosure, `03` §3.11). Stranger group
  invites now land as requests, and Block on one reports the inviter (S-04).
- **Project abuse** (`20`): reports against a Project rather than a user.
- **Appeals** after coordinated false reporting.
- **Cross-server report aggregation** without centralizing trust.
