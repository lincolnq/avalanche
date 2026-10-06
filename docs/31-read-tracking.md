# 31 — Read tracking and read receipts

> **Status:** Built, with gaps — per-message `read_at`, derived unread counts, scroll-position read marking, delivery and read receipts, and Signal-style delivery checkmarks. Diverges from this doc's earlier plan: there's no read-receipt setting (receipts are always sent to curated contacts) and no receipt debounce. Read state syncs between your own devices via `SyncRead` (`04` §5.4).
> **Last verified against code:** 2026-10-05

## Summary

Each incoming message has a `read_at` timestamp. Unread counts are derived from it. Scrolling a message into view marks it read. Read and delivery receipts are small encrypted `ReceiptMessage`s sent back to the author, which drive sending → sent → delivered → read on outgoing messages. This follows Signal.

Code: store `messages.rs` (`mark_messages_read`, `unread_count`, `update_delivery_status`); app-core `lib.rs` (`mark_messages_read`, `unread_count`, `send_read_receipt`), `messaging.rs` (auto delivery receipt on inbound DMs, receipt handling); iOS `ConversationView.swift` (`onScrollTargetVisibilityChange`), `AppState.markMessagesReadUpTo` / `markAllMessagesRead`.

## Current design

### Per-message `read_at`

*Built.* `message_history.read_at`: `NULL` means unread, otherwise unix millis. Outgoing messages count as read. A timestamp rather than a boolean also drives disappearing messages: a received message's countdown starts when it's read (`03` §5), and marking messages read wakes the expiry reaper.

### Unread count: derived, not stored

*Built.* `COUNT(*) WHERE conversation_id = ? AND read_at IS NULL AND sender != me`. The app icon badge is the total across conversations.

### Marking messages as read

*Built.* `ConversationView` uses `onScrollTargetVisibilityChange` and, while the app is active, marks everything up to the last visible message read. Opening a conversation (and new arrivals while it's open) marks everything read via `markAllMessagesRead`. There's no timer: SwiftUI's scroll-visibility callback replaces Signal's 100 ms poll. The threshold uses the max of local time and the newest message's `sent_at`, so the sender's clock skew can't leave a just-received message unread.

### Receipts (wire)

*Built.* `ReceiptMessage { type: DELIVERY | READ, timestamps: [sent_at…] }` is a `ContentMessage` body, sent as an encrypted DM to the author.

- **Delivery receipts:** app-core auto-sends one on every successfully decrypted inbound **DM** (not to yourself, not to blocked senders). This includes DMs from un-accepted requesters, which today also leaks the profile key — see `52` Known gaps.
- **Read receipts:** after local marking, the platform calls `send_read_receipt(sender, timestamps)` once per sender per marking event. App-core **only sends to curated contacts** (`52`), so opening a message request isn't an acknowledgement.

### Delivery status on sent messages

*Built.* `message_history.delivery_status`: sending → sent (server accepted) → delivered → read. Shown as Signal-style checkmarks.

## Known gaps

1. **No read-receipt setting.** Receipts are always on for curated contacts. This doc used to specify opt-in, default off; Signal defaults them on with a toggle. **Decision needed:** recommend Signal's default (on, with a per-identity toggle in Settings).
2. **No debounce.** Each scroll-visibility change that newly reads messages sends one receipt per sender, with an FFI call and network send each. Batch with a short debounce (~3 s) per sender.
3. **Group read receipts are mostly not sent.** Group co-members aren't curated (`52`), so the curation gate suppresses receipts to them. Also, a group read receipt is a pairwise DM per author. Decide whether groups get read receipts at all (Signal sends them); if so, gate on group membership rather than curation.
4. **`send_read_receipt` holds the core lock across the network send** (it's on the crypto send path, the documented exception in `core/CLAUDE.md`).

## Planned

- Fix gaps 1–3.
- A `VIEWED` receipt type for view-once media and `PLAYED` for voice notes (reserved in the proto).

## Rationale and rejected alternatives

- **Stored unread counter on the conversation** — rejected: derived from `read_at` it can't drift.
- **Watermark (`lastReadAt`) instead of per-message `read_at`** — rejected: per-message timestamps also start disappearing-message timers.
- **Timer-based visibility polling (Signal)** — unnecessary with SwiftUI's scroll-visibility callbacks.
- **Receipts to un-accepted senders** — rejected for read receipts: opening a request to judge it isn't acknowledgement. Delivery receipts still go out (Signal does the same), but must not carry the profile key.
