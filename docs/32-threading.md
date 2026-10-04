# 32 — Replies and threads

> **Status:** Proposed (quote-reply — an additive wire change awaiting owner review) / Speculative (full threading model). Nothing is built: `TextMessage` has no `reply_to` field yet (reserved fields 5–10), and no client has a reply action.
> **Last verified against code:** 2026-10-03

## Summary

Ship **Signal-style quote-reply first**, built on a data model that can later grow into threads without a migration. The full threading model below (surfacing knob, Threads catch-up entry, unread buckets, promotion) is the long-term direction. It's speculative until large organizing channels show the need. It's the most Slack-shaped feature we'd add, and WhatsApp's slow rollout suggests users struggle to predict what a reply will do.

## Proposed: quote-reply on the thread primitive

Add to `TextMessage` (from the reserved block):

```protobuf
message ReplyTo {
  string author_did   = 1;  // target message's author
  uint64 sent_at      = 2;  // target message's sent_at (the existing message identity)
  bool   unsurfaced   = 3;  // reserved for threads; always false in the first cut
  string quote_text   = 4;  // short snippet so the quote renders if the target is missing/expired
}
// TextMessage: ReplyTo reply_to = 5;
```

- The reply targets `(author, sent_at)` — the same identity receipts, reactions, edits and deletes use.
- It renders inline as an ordinary quoted reply in **every** conversation (DMs and groups alike). Tapping the quote scrolls to the original.
- A reply to a deleted or expired message renders against the tombstone or the snippet.
- Edits and deletes of the reply itself work like any message (`36`).
- DMs and groups ship together (DM/group parity rule).

This keeps the **one-primitive property** of the full model: every reply is structurally a thread message with `unsurfaced = false`. Turning on threads later changes a default, not the data.

## Speculative: the full threading model

### Core model: one primitive, one knob

**Every reply is a thread message.** The only variable is a per-reply **"surface to channel"** flag, whose *default* flips by conversation shape:

- **Chat-shaped (DMs, casual groups):** surfaced by default. Replies render inline as quote-replies, and the thread structure stays **latent** (no "N replies", no thread pane).
- **Broadcast-shaped (channels, announcement groups):** not surfaced by default. The parent shows "N replies" and a facepile, and tapping opens the thread. The sender can opt in to "also send to channel", or **promote** a reply after the fact (one-way).

Why: WhatsApp's confusion came from the *same gesture behaving structurally differently* across an invisible line. Here the gesture and data are identical everywhere; only a default changes, along a line the user can see. If we draw the line wrong, we change a default, not a mechanism. A noisy chat can be flipped to unsurfaced with its history already organized, because every reply always was a thread message.

The default would eventually be a per-channel admin setting.

### Visibility

Replies are visible to every member of the conversation. No private or subset threads; for a side conversation, start a new group.

### The Threads catch-up entry

A single compact row **pinned atop the primary inbox tab** when there are unread threads (`37`). Its title is the count of unread threads; its badge is the total unread messages in them; its preview names the groups. Tapping opens a cross-channel threads browser, newest reply first. This replaces this doc's earlier "shelf" of icons above the inbox (superseded by `37`). Either way, thread activity never bolds a channel's own inbox row, which keeps one conversation = one row.

### Following and notifications

- You follow a thread by replying (or explicitly). Following is private.
- You're notified about a thread reply only if you follow it **and** are @-mentioned, or the reply was surfaced. **Following is quiet by default.**

### Unread accounting (no double-counting by construction)

A surfaced reply is one message with one read state shown in two views, never a copy. Each message counts in exactly one place:
- A **surfaced** reply counts toward the channel's unread count.
- A **non-surfaced** reply counts toward its thread's unread count.

Consequences:
- No phantom unreads.
- No double-counted app badge.
- The Threads view counts exactly the discussion you'd otherwise miss.
- A thread *mention* rolls into the app badge; a plain followed reply only bolds the Threads entry.

**Promotion** posts a new **surface message** into the channel that references the original. It sorts by its own send time and has its own read state. The promoted original leaves the thread's unread count, and reading the surface message also marks the original read. Net: one unread, not two.

### Posting permissions (announcement groups)

Announcement groups gate **top-level posting** to admins, but **replying in a thread** is open, so the announcement feed stays clean while members discuss underneath. **Surfacing is posting to the channel**, so it uses the same permission: in an admin-gated channel a non-admin can reply but can't surface or promote.

**Prerequisite:** this needs recipients to enforce `announcement_only`. Today group sends are anonymous to the server (sealed sender, `03` §3.11), so the server can't enforce it, and recipients don't either: app-core never checks the group's `announcement_only` policy when accepting a group message. A non-admin's client can post top-level messages into an announcement group, and every recipient will display them. **Planned fix (independent of threading):** recipients drop non-admin top-level posts in announcement-only groups (`03`).

### To pin down

- Reply-to-a-reply: proposal is a flat follow-up within the same thread (one level deep).
- Expiry and orphaning: reference a stable thread identity, not the root message, so an expiring root doesn't orphan the discussion; threads inherit the group's timer.
- Followed threads in muted or archived channels still show in the Threads view.
- Whether a surface message renders as "promoted from thread" or as a plain post.

## Known gaps

1. No reply capability at all (Planned above).
2. `announcement_only` is not enforced by recipients (`03`).

## Rationale and rejected alternatives

- **Two reply primitives** (inline quote-reply in chats, thread reply in channels, chosen by conversation shape) — rejected. It bakes the chat/channel line into the data model: guessing wrong means changing mechanisms, and a noisy chat can't be reorganized into threads without lossy conversion. Its only advantage, that a chat can never surface a thread by accident, is achieved by keeping threads latent.
- **Building the full threading model now** — deferred. Quote-reply on the same primitive delivers most of the value today without the unread-bucket, shelf, and promotion machinery.
- **Per-thread rows in the inbox** — rejected: one conversation = one row.
- **Private / subset threads** — rejected.
