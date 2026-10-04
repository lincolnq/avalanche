# 08 — Supergroups (large broadcast channels)

> **Status:** Speculative — not scheduled. Records the shape and the reasoning so `03` isn't stretched to cover a problem it isn't built for. Nothing here is built.
> **Last verified against code:** 2026-10-03

## Summary

A supergroup is a large, mostly one-way channel: a few admins post, hundreds to many
thousands of members read, react and reply. Normal groups (`03`), including
announcement-only ones, are the right tool up to roughly 200 members. Past that, a
supergroup is **a shared-key broadcast channel**: content is encrypted once under a
channel read key that all members hold, stored once, and pulled on demand. Evaluate
MLS before building it.

## Why the normal-group path doesn't scale

The `03` send path already encrypts the payload once (Sender Keys), but three costs
stay linear or worse in membership N:
1. per-recipient sealed-sender envelope slots (sender CPU);
2. per-recipient storage — the server stores the full message once per recipient
   device (`db::group_messages::enqueue`);
3. Sender Key distribution — every joiner sends an SKDM to every member, O(N²) over
   the group's life.

Plus "anyone can post to everyone" makes a 10k-member group a spam megaphone.

The organizing insight: **the cost is in delivery, not readability.** Anything pushed
to all N members is the wall. Anything pulled on demand (announcements, reply
threads) or counted server-side (reactions, reply counts) sidesteps it. A supergroup
pushes nothing but content-free wakeups.

## Design sketch

- **Promotion, not a creation-time choice.** Groups are born normal. Crossing ~200
  members prompts admins to promote; past a hard ceiling it's required. Promotion is
  explicit, visible to members (it weakens security properties), one-way, and keeps
  `group_id` and membership.
- **UX-transparent.** It should feel like an announcement-only group. Scaling
  mechanics stay below the UI; the tie-breaker for any choice is "which option keeps
  the announcement-only experience intact."
- **Announcements.** Only admins send, so only admins seed Sender Keys (kills the
  O(N²) cost). Content and sender certificate are encrypted once under the channel
  read key, stored as one opaque blob, delivered by wakeup + membership-gated pull
  (404 for non-members, `03` §3.4).
- **Admin sends are pseudonymous among admins.** Each post carries a zkgroup
  `AuthCredential` presentation verified against the opaque admin rows of
  `member_credentials` — no DID, no admin roster, `03` §3.9 intact. The server can
  link one admin's posts to each other via a stable opaque id but never to a DID.
  This also lets the server enforce admin-only posting, which it can't for normal
  announcement-only groups.
- **Replies are pull-based threads per announcement.** "5 replies, tap to read":
  stored once, never pushed, count tallied server-side. Readers lazily fetch repliers'
  Sender Keys, wrapped under the read key. Consistent with `32` (in-channel threads,
  quiet by default).
- **Reactions are server-counted opaque tokens.** A reactor posts a token derived
  under the group key; the server counts `(message, token)` without knowing the emoji
  or the reactor. Members pull counts. Per-reactor faces are lost at scale.

**What's given up (broadcast path only):** admin posts are linkable to each other by
the server, and forward secrecy on content is weaker (a long-lived read key; mitigate
with epoch rotation). **Kept:** membership opacity, content confidentiality, reader
and reactor anonymity.

## Open problems

1. **Key schedule:** long-lived read key with rotation, a channel key, or MLS.
   Evaluate MLS first.
2. **Reaction de-duplication.** Anonymous counting can't tell a repeat reactor without
   a per-(member, message) nullifier proved in zero knowledge. Ship approximate counts
   (rate limits) first.
3. Reply moderation, read-key rekeying at scale, push-pseudonym bookkeeping at tens of
   thousands of rows.
4. **Old clients.** Receivers can't be forced to update. Keep a legacy path in mind
   (an updated admin client can also emit ordinary `03` group messages), and make sure
   clients skip unknown `ContentMessage` variants gracefully — worth checking now,
   independently of this doc. A per-device protocol-version signal would help the
   transition.

## Rationale and rejected alternatives

- **Rejected: just use `announcement_only` at any size** — the three linear costs above
  remain.
- **Rejected: identified admin sends** — needs a `(group → admin DIDs)` roster on the
  server, which hands a seizer the organizer list.
- **Rejected: fully unlinkable admin sends** (endorsements + sealed sender) — the whole
  anonymous-send pipeline for little gain over pseudonymous with a small admin set.
- **Rejected: replies in a separate opt-in discussion group** — breaks tap-to-read and
  UX transparency. The worry was conflating *readable by all* with *pushed to all*;
  pull is not push.
- **Rejected: author-mediated reaction tallies** — depends on the author being online
  and isn't visible to everyone.
- **Rejected: create-as-supergroup as the main path** — at creation you rarely know a
  group's eventual size.
- Note: WhatsApp and Telegram channels are not end-to-end encrypted. That's a
  deliberate confidentiality tradeoff at scale; ours (weaker FS, linkable admin posts)
  is narrower and documented.
