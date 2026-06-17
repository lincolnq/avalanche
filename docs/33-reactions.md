# Emoji Reactions — UX Proposal

Status: draft for review. This is a UX proposal.

## Why

Reactions are the cheapest possible response: acknowledgement, a vote, a "got it" without adding a message to the feed. In an organizing channel that matters — it lets a hundred people signal agreement on an announcement without a hundred "+1" messages burying it. They're also the lowest-risk social feature we can ship, because unlike threading there's no real predictability problem: tapping an emoji does one obvious thing.

Most of this design is uncontroversial and mirrors Signal. The one real decision — **one reaction per person per message, or many** — is resolved Signal-style: **one** (see below).

## Core model

- A reaction is an `(emoji, reactor, target message)` tuple — a small encrypted message in the same conversation as its target, not a content message in the feed.
- Reactions are **visible to every member of the conversation**, same as the message they're on. No private reactions, consistent with `32-threading.md` (no subset visibility).
- Reactions render as a **cluster on the target bubble** — each distinct emoji with a count, your own reaction highlighted. Tapping the cluster shows who reacted with what.
- **Reactions never enter the feed and never create a conversation row.** They decorate a message; they are not messages you scroll past.

## One reaction per person per message (Signal-style)

Each person has **at most one reaction on a given message**. Reacting with a new emoji **replaces** the old one; tapping your current emoji again **removes** it. (Slack's many-per-message model was the alternative — rejected as more Slack-shaped, and because it bloats the on-bubble cluster in large channels.)

Why this fits:

- Consistent with the app's "feel like Signal" ethos.
- The picker is a single decisive tap — no per-emoji toggling.
- The on-bubble cluster stays small and legible even in a channel with hundreds of members.

## How they look and behave

- **Adding:** long-press (or hover, desktop) a message → a reaction bar with a few recent/frequent emoji + a "more" affordance to the full picker. One tap applies.
- **Removing / changing:** tap your own emoji in the cluster to remove it; pick a different one to replace it.
- **Cluster display:** distinct emoji each with a count, ordered by first-applied, your own visually marked. Overflow collapses to a count past some width.
- **Who reacted:** tap the cluster → a small sheet listing reactors grouped by emoji.
- **Reactions on a thread reply** behave exactly like reactions anywhere else — the target is just a message that happens to live in a thread. Nothing thread-specific.

## Notifications

- **A reaction to your own message may notify you** (a low-priority notification, e.g. "Dana reacted 👍 to your message"), respecting the conversation's mute state. Reactions to *other people's* messages never notify you.
- **Quiet by default in noisy contexts.** In large/announcement-shaped channels a flood of reactions on an announcement should not generate a notification per reaction — coalesce, or suppress, rather than push each one. (Exact threshold is an implementation detail.)
- Reactions **do not touch the app-icon badge** and do not mark a conversation unread — receiving a reaction is not unread "discussion you'd otherwise miss." This mirrors the `32-threading.md` badge discipline: the badge is for messages, and a reaction is not a message in the feed.

## What we are explicitly NOT doing

- **No reactions feed / activity tab.** There is no aggregated "who reacted to my stuff" surface. (Contrast the cross-channel Threads browser in `32-threading.md` — reactions don't earn a shelf slot.)
- **No badges or unread counts from reactions.** They never contribute to any count.
- **No private or subset-visibility reactions.**
- **No custom/uploaded emoji** in the first cut — system emoji only. (Custom emoji is a possible later addition; out of scope here.)
- **No super-reactions / paid reactions / reaction effects.**

## Behavior to pin down (not blockers)

- **Read/expiry interaction.** A reaction is a tiny message; it should inherit its conversation's expiry timer and not independently resurrect or outlive its target. When the target expires, its reactions go with it.
- **Reaction to an edited/deleted message.** If the target is deleted, its reactions are dropped. (Editing leaves reactions in place — see `36-message-editing-deletion.md`.)
- **Ordering & races.** Two people reacting with the same emoji at once must converge to a count of 2, not two separate entries — reactions key on `(emoji, reactor)`, so re-applying the same emoji is idempotent per person.
