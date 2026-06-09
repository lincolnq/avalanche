# Threaded Replies — UX Proposal

Status: draft for review. This is a UX proposal.

## Why

Large action-bound groups and channels get noisy. A linear stream forces every side-conversation through the main feed, which buries announcements and makes it hard to follow more than one topic. Threading lets a reply hang off the message it answers instead of interleaving into everything else.

We are deliberately cautious here. The design ethos is "feel like Signal, not Slack/Discord," and threading is the most Slack-shaped feature we'd add. WhatsApp has been stuck rolling this out gradually for over a year, which we read as a sign that **users struggle to predict what a reply will do**. That predictability problem is the thing this proposal has to solve, not just "add threads."

## Core model: one primitive, one knob

**Every reply is a thread message.** There is no separate "inline reply" mechanism — replying to a message always creates or extends a thread under it. This is uniform across DMs, casual groups, channels, and announcement groups.

The only variable is a single per-reply flag — **"surface to channel"** — which controls whether the reply also renders in the main feed. Its *default* flips by conversation shape:

- **Chat-shaped conversations (DMs, casual groups):** default **surfaced-on**. The reply renders inline in the flow and looks exactly like an ordinary quoted reply today. The thread structure exists underneath but stays **latent** — no "N replies" indicator, no thread pane — so a chat never sprouts threading UI unbidden.
- **Broadcast-shaped conversations (channels, announcement / action-bound groups):** default **surfaced-off**. The reply stays in the thread; the channel shows a collapsed indicator on the parent. The sender can opt in to "also send to channel" per reply, and can **promote an already-posted thread reply to surfaced after the fact** (one-way; there is no demote) — promotion posts a surface message into the channel (see "Promotion mechanism").

The default is a **per-channel setting an admin controls** (deferred — not in the first cut, but the model assumes it). Until then, defaults are derived from conversation shape as above.

Why this shape: WhatsApp's confusion came from the *same gesture behaving structurally differently* across an invisible line. Here the gesture and the data model are identical everywhere — only a *default* changes, along a line the user can already see (am I in a chat, or reading a broadcast?). Because the channel/chat distinction is just a default and not a structural fork, **if we guess the line wrong we change a default, not a mechanism** — and a noisy chat can be flipped to surfaced-off with every past reply already organized into threads, because they always were threads.

## How threads look

- In broadcast-shaped conversations, a message with replies shows a **collapsed indicator** ("N replies" + a small participant facepile) on the parent. Tapping opens the thread (a pushed view / sheet on mobile).
- In chat-shaped conversations, the thread is **latent**: replies render inline and no thread affordance appears unless/until the conversation is flipped to surfaced-off.
- Replies are **visible to every member of the group** — a thread is part of the same encrypted conversation, so there's no such thing as a private/subset thread. For a private side-conversation, start a new group.

## The Threads browser (the homescreen piece)

### Entry point: the shelf

The chats page gains a **shelf** at the top — a horizontally-scrollable band of icons, sitting above the Signal-style unified inbox. **Threads is the one icon we specify here**; the shelf is a home for other cross-conversation views of your own activity later (mentions, saved, etc.), governed by a simple rule: *if it belongs to one conversation it lives in that conversation, and if it's a place you go to do org work it's the Projects tab — the shelf is only for cross-conversation views of your own activity.* We start with Threads alone; a one-item shelf is intentional, a six-item shelf is Slack.

Behavior of the shelf:

- It is **chrome, not a conversation.** This is the whole point — thread activity lives in the launcher, conversation activity lives in the inbox below, so every row in the inbox is still a real conversation (one conversation = one row). It protects the Signal-style inbox rather than diluting it.
- The shelf **scrolls away** with the inbox (it's the top of the scroll content, not pinned), so scrolling down gives the inbox full height.
- The **Threads icon is always present, quiet when empty,** and **shows the unread count** when there's something to catch up on. (Exact treatment — numeric vs. dot — is an implementation detail.)
- Because the count lives on the icon, a channel with thread-only activity you follow **does not bold its own row in the inbox.** The Threads icon's count is the *only* signal of that activity. That separation is the reason the shelf exists.

### Tapping Threads: the browser

Tapping the icon pushes the **threads browser** — a cross-channel list of threads, newest-reply-first. Followed-but-already-read threads stay accessible but secondary, so the default view is exactly "discussion you'd otherwise miss."

A thread row carries more context than an inbox row, because it must say *which channel* as well as *which thread*. We can specify all the interactions later.

## Following and notifications

- **You follow a thread by replying to it** (or by explicitly following). Following is private — nobody else sees who's subscribed.
- **Notify rule:** you're notified about a thread reply only if you're **following it**, are **@-mentioned** in it, or it was **surfaced to the channel**.
- **Following is quiet by default.** Replying follows the thread so it surfaces in the Threads view, but does **not** push-notify on every new reply unless you're @-mentioned. (Closer to WhatsApp's mute-by-default than Slack's notify-everything — chosen for people sitting in big organizing channels.)

## Unread accounting (no double-counting, by construction)

A surfaced reply is **one message with one read state, shown in two views** — never a copy. Reading it in the channel *is* reading it in the thread, and vice versa. On top of that, **each message lives in exactly one unread bucket:**

- **Surfaced reply → counted as a channel message.** It contributes to the channel's unread and is **excluded** from its thread's unread count.
- **Non-surfaced reply → counted as a thread message.** It contributes to its thread's unread (shown in the Threads view) and is excluded from the parent channel's unread.

Consequences:
- A surfaced reply you read in the channel can never leave a phantom unread on the thread, because it was never in the thread's bucket — no "catch the thread up to that point" reconciliation is needed.
- The app-icon badge can't double-count it, because it's in one bucket only.
- The **Threads view therefore counts exactly the discussion you'd otherwise miss** (the non-surfaced replies) — surfaced replies are the channel's responsibility.
- **Badge math:** a thread *mention* rolls into the app-icon badge and bolds the Threads entry; a plain followed (non-surfaced) reply bolds only the Threads entry, not the app badge.

Two rules we deliberately keep:
- In the thread view, a surfaced-but-unread reply still renders with its true (unread) styling even though it isn't in the thread's badge count. Per-message styling = real read state; the badge = "stuff you'd otherwise miss."
- Reading a surfaced reply does **not** auto-mark the surrounding non-surfaced replies read. Those are genuinely unseen discussion; auto-catching-up would hide them.

**Promotion creates a surface message, and that keeps the buckets clean.** Promoting a thread reply after the fact does *not* mutate the original or give it a synthetic sort key. It posts a new **surface message** into the channel that references the original and renders its content inline — an ordinary channel message that sorts by its own send time (so it lands where people will see it) with its own read state. Two rules keep this from producing a double-unread:

- **A reply that has been surfaced — at creation via the surfaced flag, or later via a surface message — is excluded from the thread bucket.** Its channel representation owns the unread. So promotion pulls the original out of the thread bucket and the surface message carries the single unread.
- **Reading the surface message cascades read to its referent** (a targeted one-message → one-referent cascade, not "mark the whole thread to that point"), so the thread later shows that reply as read.

Net: a follower who hadn't read it gets exactly one unread (the channel surface), not two; a non-follower gets the channel surface unread, which is the point of promoting; a follower who *had* already read it sees the promotion as a new channel event, which is correct — promotion is a distinct act.

## Posting permissions (announcement groups)

Announcement groups gate **top-level posting** to admins, but **replying in a thread** is a separate, more open permission. This is what makes "admin posts an announcement, members discuss underneath, the announcement feed stays clean" work — the pattern WhatsApp and Telegram both landed on. Posting to the channel and replying in a thread are distinct capabilities.

**Surfacing is the post-to-channel capability.** Surfacing a reply (at send time or via promotion) puts it in the channel feed, which *is* posting to the channel — so it's gated by the same permission. In an admin-gated channel, a non-admin can reply in-thread but cannot surface or promote. No new permission concept is needed; surfacing just exercises post-to-channel.

## Behavior to pin down (not blockers)

- All interactions on the Threads view.
- **Reply-to-a-reply.** Since every message can anchor a thread, we need one universal rule for replying to a message that is itself a thread reply. Proposal: it's a **flat follow-up within the same thread**, not a new nested thread — threads are one level deep. (Matches WhatsApp's "follow-up reply.")
- **Promotion mechanism (specify now, implement later).** After-the-fact promotion posts a **promotion message** into the channel — a normal channel message that references the original thread reply and renders it inline — rather than re-sorting the original. (This keeps the frontend sorting everything by a single send-time.) Unread/read behavior is covered under "Unread accounting" (surfaced replies leave the thread bucket; reading the surface cascades read to its referent). Open sub-question: whether the surface message renders as its own provenance-tagged bubble ("promoted from thread") or as a plain post.
- **Expiry / orphaning.** Messages expire (substrate-level). A thread should reference a stable thread identity rather than the root message, so an expiring root doesn't orphan a live discussion; a thread inherits its group's expiry timer. (Mechanism out of scope here; flagging the behavior.)
- **Threads view in muted/archived channels.** A thread you follow should still surface in the Threads view even if its parent channel is muted or archived — that's the point of a cross-channel catch-up surface.

## Decisions (resolved)

- **Following default = quiet.** Replying follows the thread but doesn't push-notify unless you're @-mentioned.
- **Plain followed (non-surfaced) replies do not touch the app-icon badge** — only mentions do. They bold the Threads entry only.
- **The surface default is a per-channel setting controlled by an admin** — deferred (not in the first cut), but the model assumes it exists. Users can always surface/promote per-reply where they have post-to-channel permission.

## What we are explicitly NOT doing

- No private or subset-visibility threads.
- No per-thread rows floating in the main inbox (only the single Threads aggregator).
- No second reply mechanism: everything is a thread; only the surface default differs.

## Rejected alternative

An earlier draft used **two reply primitives** — an inline quote-reply (no thread structure) in chats and a thread reply in channels — with conversation shape selecting which one a reply *becomes*. Rejected because it bakes the channel/chat line into the data model: guessing the line wrong means changing mechanisms, and a chat that gets noisy can't be reorganized into threads without lossy conversion of quote-reply chains. Its only advantage was that a chat could *never* surface a thread by accident (the structure simply wasn't there); the current model achieves the same outcome by keeping threads latent in chat-shaped conversations, at the cost of relying on that discipline rather than the data model.
