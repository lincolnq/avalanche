# Message Editing & Deletion — UX & Wire Proposal

Status: draft for review.

## Why

People send a message, spot the typo a second later, and want to fix it without a "*the" follow-up cluttering the feed. Or they send the wrong thing entirely and want it *gone*. In an organizing channel both matter more than in a DM: an announcement with a wrong time or a dead link should be *corrected in place*, not re-posted, and a message posted to the wrong channel should be retractable. These are small, low-risk features — like reactions, each does one obvious thing — and they mirror Signal closely. Most of this design is uncontroversial; we copy Signal except where noted.

**Editing and deletion are two operations on one shared substrate.** Both target a prior message by `(author, sent_at)`, both ride the same wire pattern, both are governed by the same authorship rule and edit/delete windows, and both converge under last-writer-wins. The doc specs editing first, then deletion as the absorbing sibling; the shared machinery is written once and referenced.

## Core model

- An edit is a small encrypted message in the same conversation as its target. It is **not** a feed message — it never creates a conversation row, never re-sorts, never bumps the conversation in the inbox.
- An edit **targets the original message by `(original author, original `sent_at`)`** — the same identity key receipts (`31-read-tracking.md`) and reactions (`33-reactions.md`) already use. There is no separate global message id on the wire.
- An edit **carries a full replacement `TextMessage`**. On receipt, the client swaps the displayed content for the new one, keeps the **original `sent_at` for ordering and position**, and marks the message **"Edited."**
- **You can only edit your own messages.** This is enforced by the recipient, not the server (see Security).
- **Only text-side content is editable:** `body`, mentions, formatting (`BodyRange`s), and the link preview. An edit **does not add, remove, or replace attachments** — editing media is not a thing (matches Signal). Attachment pointers carry through unchanged.

## What an edit is *not*

- **Not a delete.** Editing to empty is disallowed; removing a message is the separate delete primitive below. An edit always has a non-empty body or a surviving attachment.
- **Not a re-send.** The message keeps its original position and read state; an edit does not mark the conversation unread or move it.

## Wire format

Two new `ContentMessage` body variants, each consuming one number from the reserved `oneof` block (`7 to 14`):

```proto
message ContentMessage {
  oneof body {
    TextMessage    text         = 1;
    // ...
    EditMessage    edit         = 7;   // takes the next free reserved slot
    DeleteMessage  delete       = 8;
  }
  // ...
}

message EditMessage {
  // sent_at of the message being edited, authored by THIS sender in this
  // conversation. Recipients locate the target by (authenticated sender,
  // this timestamp). 0 / missing is invalid.
  uint64      target_sent_at = 1;
  // Full replacement content. Body, mentions, formatting, and link preview
  // replace the original's. The attachments field is ignored — attachments
  // are carried from the original message (editing media is not supported).
  TextMessage replacement    = 2;
  reserved 3 to 10;
}

message DeleteMessage {
  // sent_at of the target message.
  uint64 target_sent_at = 1;
  // DID of the target's author. For FOR_EVERYONE this MUST equal the
  // authenticated sender (recipients verify). For FOR_ME it may be anyone —
  // you can locally remove someone else's message from your own view.
  bytes  target_author  = 2;
  Scope  scope          = 3;
  enum Scope {
    FOR_EVERYONE = 0;  // tombstone for all members; sent to the conversation
    FOR_ME       = 1;  // remove from my own devices only; sent to self-devices
  }
  reserved 4 to 10;
}
```

The edit/delete message itself has its own `ContentMessage.timestamp_ms` (its `sent_at`). That timestamp is used for **last-writer-wins ordering of competing operations** and to display *when* the message was last edited — but it is **not** the message's feed position; the original `target_sent_at` is.

## Security — the load-bearing rule

**A recipient applies an edit, or a `FOR_EVERYONE` delete, only if the operation's cryptographically-authenticated sender equals the target message's author.** Sender identity is established by the libsignal session (and, in groups, by sender keys under sealed sender — the recipient still learns the true author even though the server doesn't). So the check needs no extra signing: when an `EditMessage` or `DeleteMessage` arrives, look up the message at `target_sent_at`; if its stored author ≠ the authenticated sender, **drop it.** Without this rule anyone could rewrite or erase anyone's messages by guessing a timestamp.

`FOR_ME` deletes are exempt from the authorship rule — they only mutate the sender's *own* view (and sync to the sender's own other devices), so removing someone else's message from your own client is fine.

The server cannot enforce authorship (it never sees plaintext, and under sealed sender it doesn't even see the sender) — enforcement is entirely client-side, which is fine because it's a correctness check on data the recipient already authenticated.

## Limits (copy Signal — for human authors)

- **Edit window: 24 hours** from the original `sent_at`. Edits arriving (or composed) later are rejected/disabled.
- **Edit cap: ~10 edits per message.** Past the cap the "edit" affordance disappears.
- Both limits are **client-honored**, like the authorship rule. A peer that ignores them gains nothing interesting; recipients clamp on their end (an out-of-window edit is dropped).

**Bot authors get a wider envelope: no edit cap, and a 30-day window.** The canonical bot pattern is a single message that updates in place: a live poll tally, a countdown, a build/deploy status, an event card whose details change. Capping those at ~10 would force the bot to spam new messages instead, and a 24h window is too short for a card tracking an event days out — so bots get **unlimited edits within 30 days** of the original `sent_at`. The recipient already knows the author's DID and role, so the exemption is a client-side branch on "is this author a bot" — same place the human limits are enforced. (Edits are last-writer-wins and never notify/bump, so a stream of bot edits is cheap and silent; bot messages retain no revision history — see below.)

## "Edited" label and revision history

- An edited message renders an **"Edited" indicator** (optionally with the last-edit time).
- Clients **retain prior revisions locally** so the user can long-press → **view edit history** (a small sheet listing each revision and its time). Revisions are local state; we do not re-fetch them from anyone.
- We keep revisions for the same lifetime as the message — they expire with it (see Expiry).
- **Bot messages retain no edit history at all** — only the current content is kept. A live tally that edits hundreds of times must not accumulate revisions on every recipient's device, and the "view edit history" sheet on a live-updating card isn't meaningful anyway. There is no "Edited" history affordance on a bot-authored message.

## Deletion (remote delete)

Deletion has **two modes**, matching Signal:

- **Delete for everyone** (`FOR_EVERYONE`) — retract a message you sent. A `DeleteMessage` goes to the conversation; every recipient **tombstones** the target: the bubble becomes a "This message was deleted" placeholder in its original position, attachments are dropped, and the message's **reactions are dropped** (per `33-reactions.md`). Only your own messages, subject to the authorship rule and the same windows as editing (24h human / 30-day bot).
- **Delete for me** (`FOR_ME`) — remove a message from *your* view only. Works on **any** message (yours or someone else's), with **no window**, and is **not** subject to the authorship rule. It is sent only to your own other devices (so the message disappears everywhere you're signed in) and never touches anyone else's client. This is local cleanup, not a retraction.

Deletion specifics:

- **A tombstone is terminal/absorbing.** Once a message is deleted for everyone, the row is replaced by a tombstone that **wins over any edit regardless of timestamp** — a late edit (even one with a greater `sent_at`) is ignored, and the message stays deleted. Delete is the top of the LWW lattice, not just the latest writer.
- **The tombstone keeps the message's position and `sent_at`** so the feed doesn't reflow and a late edit has something to be absorbed by. Replies/threads that referenced the message still resolve — they render against the "deleted" placeholder.
- **Orphaned attachments** left on the CDN by a delete are reclaimed by the normal TTL sweep (`35-attachments.md`); we do not proactively delete the blob (the server can't tell which blob a tombstone referenced, and the encrypted key is gone anyway).
- **Bot deletes** follow the same modes; the 30-day window applies to a bot's `FOR_EVERYONE` delete just as it does to its edits.

## Ordering, races, and out-of-order delivery

- **Last-writer-wins by operation `sent_at`,** with **delete as the absorbing state.** Among edits, the greatest timestamp wins; an edit older than the currently-applied revision is ignored. A `FOR_EVERYONE` delete, once applied, wins over *all* edits regardless of timestamp (see Deletion). This makes the sequence idempotent and order-insensitive on delivery.
- **Operation arrives before its target** (reordering, or the target is still in flight): hold it **pending**, keyed on `target_sent_at`, and apply it when the original lands — exactly the pattern reactions use for an unseen target. If the original never arrives within its window, drop the pending operation. A pending delete that outraces its target still tombstones on arrival.

## Notifications, read state, badges

- An edit **or delete does not notify, does not mark unread, does not touch the badge, and does not bump the conversation.** It silently updates (or tombstones) content in place (Signal's behavior). The original message's read state is preserved.
- Rationale matches `33-reactions.md`: neither is new discussion you'd otherwise miss — one corrects, the other retracts, something already in the feed.

## Interactions with other features

- **Reactions:** editing **leaves reactions in place**; deleting **drops them** (both pinned in `33-reactions.md`). Reactions key on the target message, not its content, so an edit doesn't disturb them but a tombstone takes them with it.
- **Threading (`32-threading.md`):** a thread reply is just a message, so it edits/deletes like any other; a surfaced reply's channel representation and its thread representation are one message, so the operation updates both views at once. A deleted thread parent leaves the thread intact, rendered against the tombstone.
- **Disappearing messages:** editing **does not reset the expiry timer** (Signal's behavior); the edited message and its revisions expire on the original's schedule. A delete-for-everyone is itself a tiny message that inherits the conversation timer; the tombstone expires with the slot it replaced.
- **Attachments (`35-attachments.md`):** untouched by edits — the pointers carry through; the only editable part of a media message is its **caption** (which lives in the `TextMessage` body / formatting, not in the `AttachmentPointer`). A delete drops the pointers; the orphaned blob is reclaimed by TTL.
- **Groups:** edits and deletes ride the normal `GroupMessage` / sender-key transport; the authorship rule holds because sender keys authenticate the author to members.

## Multi-device

Edits and `FOR_EVERYONE` deletes are sent to the author's **own other devices** too (app-core already encrypts per recipient device including self), so every device the author owns reflects the change. A `FOR_ME` delete is sent **only** to the author's own devices — that's its entire delivery set. A recipient's multiple devices each apply operations independently via LWW (delete absorbing) — no cross-device reconciliation needed.

## Storage (sketch)

- The `message_history` row is **updated in place** for an edit (new content; `sent_at` unchanged; an `edited_at` column records the last edit time; an `edit_count` enforces the human cap).
- A `FOR_EVERYONE` delete replaces the row with a **tombstone** (content cleared, a `deleted_at` set, attachment pointers dropped) while **keeping `sent_at`** for position and late-edit absorption. A `FOR_ME` delete removes the row from the local store outright.
- Prior revisions live in a side table (or a JSON column) keyed by the message, for the history sheet. They are deleted when the message expires, is deleted, or — for bot authors — never written at all.

## Version skew

A client that predates the `EditMessage` / `DeleteMessage` variants decodes a `ContentMessage` whose `oneof body` is an unrecognized field — it has no displayable body. Such a client should **silently ignore** the operation (the original message stays as-is, undeleted) rather than render an empty bubble. Since we control all clients this is a transient concern, but the ignore-unknown-variant behavior should be explicit — note that an ignored delete means the message lingers on a stale client, which is the conservative failure (no data loss, no spurious tombstone).

## Behavior to pin down (not blockers)

- Exact "Edited" affordance (inline label vs. only-in-history) and whether to show the edit *time* inline.
- Whether the windows (24h human / 30-day bot) are hard product constants or per-conversation/admin settings later.
- Whether edit history is purgeable by the user independently of the message.
