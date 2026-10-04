# 36 — Message editing and deletion

> **Status:** Built, with gaps — `EditMessage` and `DeleteMessage` (FOR_EVERYONE and FOR_ME) on the wire, the recipient-side authorship rule, last-writer-wins with delete as the absorbing state, the "Edited" label, and revision history, in DMs and groups on iOS, Android, and Desktop. Not as designed: edit window and cap are only enforced in the sender's UI (and not at all on Desktop); out-of-order edits and deletes are dropped rather than held; edits replace only the body; bot messages keep revision history; deletes leave attachment keys and cached plaintext behind.
> **Last verified against code:** 2026-10-03

## Summary

Fix a typo in place, or retract a message entirely. **Editing and deletion are two operations on one substrate.** Both target a prior message by `(author, sent_at)`, ride the same wire pattern, are authorized by the same authorship rule, and converge under last-writer-wins with delete absorbing. This mirrors Signal.

Code:
- proto `EditMessage` (body 7), `DeleteMessage` (body 8) in `content.proto`
- app-core `send_edit` / `send_delete` (via `send_to_target`), `apply_inbound_edit` / `apply_inbound_delete` (`messaging.rs`)
- store `apply_edit`, `tombstone_message`, `delete_message_for_me`, `message_revisions` (`store/src/messages.rs`)
- UI: iOS `ConversationView.swift` (`canEdit`), `EditHistorySheet.swift`; Android `ConversationView.kt`; Desktop `MessageBubble.tsx`

## Current design

### Wire format

*Built.*

```protobuf
message EditMessage {
  uint64      target_sent_at = 1;  // sent_at of the sender's own earlier message
  TextMessage replacement    = 2;  // full replacement content
  reserved 3 to 10;
}

message DeleteMessage {
  enum Scope { FOR_EVERYONE = 0; FOR_ME = 1; }
  uint64 target_sent_at = 1;
  string target_author  = 2;  // DID; must equal the authenticated sender for FOR_EVERYONE
  Scope  scope          = 3;
  reserved 4 to 10;
}
```

The operation's own `ContentMessage.timestamp_ms` is its LWW clock and its "edited at" time. The feed position stays the original `target_sent_at`.

### Security — the load-bearing rule

*Built.* **A recipient applies an edit, or a FOR_EVERYONE delete, only if the operation's cryptographically authenticated sender is the target's author.**
- **Edits** look up the target by `(conversation, authenticated sender, target_sent_at)`, so an edit can never touch another author's message.
- **Deletes** compare `target_author` with the authenticated sender and drop the operation on mismatch.

Sender identity comes from the libsignal session, or from sender certificates in groups under sealed sender. The server can't enforce any of this and doesn't need to.

**FOR_ME** deletes only change your own view and are only honored when they come from your own devices.

### Applying operations

*Built.*

- **Edit:**
  - replaces the message `body` in place and keeps `sent_at` and position;
  - records `edited_at` and increments `edit_count`;
  - pushes the superseded body to `message_revisions` for the history sheet.

  An edit older than the applied one is ignored. An edit to a tombstoned message is ignored.
- **FOR_EVERYONE delete:**
  - clears the body and shared contacts and sets `deleted_at`, keeping position;
  - drops the message's reactions and revisions.

  The tombstone absorbs every later edit regardless of timestamp. It renders as "This message was deleted".
- **FOR_ME delete:** removes the row, its reactions and its revisions locally. It is sent only to your own devices.
- Edits and deletes **don't notify, mark unread, touch the badge, or bump the conversation.**
- An edit doesn't reset the disappearing timer.

### Limits

*Partial.* The intended limits (Signal):
- **Humans:** edits within **24 hours**, at most ~**10** edits per message.
- **Bots:** **no cap, 30-day window**, and **no retained revision history**. That's the canonical bot pattern of one message updated in place: a live tally, a countdown, a status card.

**As built:**
- iOS and Android hide the edit action after 24 hours.
- Desktop has no window.
- No platform enforces the edit cap.
- Recipients enforce neither the window nor the cap.
- The bot exemption isn't wired: inbound edits always store a revision.

### Multi-device

*Built.* Edits and deletes go to the author's other devices via the Sent transcript (`04`). Each device applies operations independently under the same LWW rules.

## Known gaps

1. **Deletes leave attachment keys and plaintext behind (P1).** A FOR_EVERYONE or FOR_ME delete clears the message row but not its `message_attachments` / `message_link_previews` rows (including the decryption keys and thumbnails), and not the platforms' decrypted file caches. See `35` Known gaps 3. The "attachments are dropped" promise is unmet.
2. **Out-of-order operations are dropped**, not held pending. An edit or delete that arrives before its target is discarded (`apply_edit` returns false; `tombstone_message` updates nothing). Rare, since an operation causally follows its message, but a delete that outruns its target leaves the message undeleted.
3. **Edits replace only the body.** A replacement `TextMessage`'s link preview (and any future mentions or formatting) is ignored, so editing a URL leaves a stale preview card.
4. **Window and cap not enforced by recipients**, and no 24-hour window on Desktop.
5. **Bot messages accumulate revision history** (inbound edits always pass `store_revision = true`), defeating the update-in-place pattern on every recipient's device.
6. **No best-effort server delete of the blob** on delete-for-everyone (`35` Planned).

## Planned

- Fix gaps 1–5. For 4: recipients drop edits outside 24 h (30 days for bots, where "bot" means a server-vouched bot per `54`, not a self-declared one) and beyond the cap. For 2: hold pending operations keyed on `target_sent_at` until the target arrives or the window lapses.

## Version skew

A client that predates a body variant sees an unknown `oneof` field and must **silently ignore** it. An ignored delete leaves the message on that stale client, which is the conservative failure.

## Rationale and rejected alternatives

- **Separate global message ids on the wire** — rejected: `(author, sent_at)` is already the identity receipts and reactions use.
- **Server-enforced authorship** — impossible under sealed sender and unnecessary; recipients already authenticate senders.
- **Delete as just the latest LWW write** — rejected: a delayed edit could un-delete a message. Delete is the top of the lattice.
- **Editing attachments** — not supported (matches Signal); only text-side content is editable.
- **Editing to empty as a delete** — disallowed; delete is its own operation.
- **Deleting the whole message when its media is removed locally** — rejected in `35`.
