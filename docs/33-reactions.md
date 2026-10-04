# 33 — Emoji reactions

> **Status:** Built — `ReactionMessage` on the wire; one reaction per person per message; on-bubble clusters on iOS, Android, and Desktop; DMs and groups. Not built: reaction notifications, a "who reacted" sheet, and removing reactions when their target expires.
> **Last verified against code:** 2026-10-03

## Summary

A reaction is the cheapest response — acknowledgement, a vote, "got it" — without adding a message to the feed. In an organizing channel it lets a hundred people agree with an announcement without a hundred "+1"s. It mirrors Signal.

Code: proto `ReactionMessage` (`content.proto`, body variant 9); app-core `send_reaction` (via `send_to_target`, so DMs and groups share one path), `apply_inbound_reaction` (`messaging.rs`); store `reactions` table (`upsert_reaction`, `remove_reaction`, `load_reactions`); UI iOS `MessageBubble.swift` / `EmojiPickerView.swift`, Android `MessageBubble.kt` / `EmojiPicker.kt`, Desktop `MessageBubble.tsx`.

## Current design

*Built.*

- A reaction is an `(emoji, reactor, target)` tuple sent as a small encrypted message in the target's conversation. The target is identified by `(target_author, target_sent_at)`, the same identity receipts, edits and deletes use.
- **One reaction per person per message.** Picking a new emoji replaces the old one; `remove = true` clears it. The store keys on `(target, reactor)`, so reactions are idempotent and converge regardless of arrival order. A reaction can arrive before its target and still attach when the target lands.
- Reactions are **visible to every member** of the conversation. There are no private reactions.
- Reactions render as a **cluster on the target bubble**: each distinct emoji with a count, in first-applied order, with your own highlighted. Tapping your own emoji removes it.
- Reactions **never enter the feed, never create a conversation row, never touch unread or the badge.**
- **Deleting a message for everyone drops its reactions** (`36`). Editing leaves them.

## Known gaps

1. **Reactions outlive expired targets.** `delete_expired_messages` removes the message row but not its reactions. They're invisible, but they linger on disk past the disappearing timer. Delete them with the target.
2. **No reaction notifications** (below).
3. **No "who reacted" sheet** on iOS.

## Planned

- **Notifications:** a reaction to *your own* message may notify you at low priority ("Dana reacted [emoji] to your message"), respecting mute. Coalesce or suppress floods in large or announcement-shaped groups. Reactions to other people's messages never notify.
- **Who reacted:** tapping the cluster opens a sheet listing reactors grouped by emoji.
- Fix gap 1.

## Speculative

- Custom or uploaded emoji packs (`23` puts these in Projects).
- A server-counted reaction design for very large broadcast channels (`08`).

## What we are explicitly NOT doing

- No reactions feed or activity tab.
- No badges or unread counts from reactions.
- No private or subset-visibility reactions.
- No super-reactions, paid reactions, or effects.

## Rationale and rejected alternatives

- **Many reactions per person per message (Slack)** — rejected: more Slack-shaped, and it bloats the cluster in large channels. One decisive tap, Signal-style.
- **Reactions as feed messages** — rejected: they decorate a message; they aren't discussion you'd otherwise miss.
