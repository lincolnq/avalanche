# Read Tracking & Read Receipts

Design for unread counts, read receipts, and scroll-position-based read marking. Follows Signal's approach.

## Data Model

### Per-message `read_at` timestamp

Each incoming message in the `message_history` table gets a `read_at` column — `NULL` means unread, a unix-millis timestamp means read at that time. Outgoing messages are always considered read (set `read_at` to `sent_at` on creation).

```sql
ALTER TABLE message_history ADD COLUMN read_at INTEGER;
-- NULL = unread, non-NULL = unix millis when marked read
```

Using a timestamp instead of a boolean future-proofs for disappearing messages. Signal starts the disappear timer based on when the message was read:
- **Sent messages**: timer starts immediately on send (sender's copy).
- **Received messages**: timer starts when the recipient reads it.

The `read_at` timestamp provides this for free. `NULL`/non-`NULL` gives us the boolean semantics for unread counts.

### Unread count: derived, not stored

No stored `unreadCount` on conversations. Instead, computed via query:

```sql
SELECT COUNT(*) FROM message_history
WHERE conversation_id = ? AND read_at IS NULL AND sender_did != ?
```

The Swift `Conversation.unreadCount` becomes a computed property that reads from `messagesByConversation` (in-memory) or falls back to a store query.

The app icon badge is the total unread count across all conversations.

### No `lastReadAt` or `activeConversationId` on Conversation

No timestamp watermark or active-view tracking needed. The per-message `read_at` is the source of truth.

## Marking Messages as Read

### Scroll-position tracking

When a conversation is open, scroll position changes trigger marking visible messages as read.

**Implementation:**
1. ConversationView uses SwiftUI's `ScrollPosition` (iOS 17+) to track the last visible message.
2. `.onChange(of: scrollPosition)` calls `appState.markMessagesRead(upTo: lastVisibleMessageId, in: conversationId)`.
3. `markMessagesRead` sets `read_at = now` on all unread messages with `sent_at <=` the target message's timestamp, both in-memory and in SQLCipher.

Signal uses a 100ms polling timer because UIKit's `UIScrollView` doesn't have declarative scroll observation. SwiftUI's `ScrollPosition` gives us reactive change tracking, so no timer is needed.

**Guards** (don't mark as read when):
- The conversation view is not the topmost view (a sheet/modal is presented)
- The app is in the background

### On conversation open

When opening a conversation, `.onAppear` triggers the same `markMessagesRead` path for initially visible messages.

## Read Receipts (Wire Protocol)

### Protobuf definition

Add to `proto/content.proto` (when created), or define as a simple JSON envelope for now:

```protobuf
message ReceiptMessage {
  enum Type {
    DELIVERY = 0;
    READ = 1;
  }
  Type type = 1;
  repeated uint64 timestamps = 2;  // sent_at timestamps of the messages being acknowledged
}
```

A receipt is sent as an encrypted DM to the message sender, using the same E2E channel as regular messages. The plaintext is a `ReceiptMessage` instead of a `ContentMessage`.

### When to send

After messages are marked as read locally, queue a read receipt to the sender. Batch with a 3-second debounce to avoid sending a receipt per message during scroll.

**Flow:**
1. `markMessagesRead()` collects newly-read message timestamps, grouped by sender DID.
2. A 3-second debounce timer fires per sender.
3. On fire, encrypt and send a `ReceiptMessage { type: READ, timestamps: [...] }` to that sender.
4. On receive, the sender updates the corresponding outgoing messages' delivery status.

### Message request gating

If the sender is not yet accepted (message request pending), do NOT send read receipts. Queue them. Send once accepted. (Not needed for MVP — all users are explicitly invited.)

### User preference

Read receipts are opt-in per account. Stored in the local SQLCipher `account` table:

```sql
ALTER TABLE account ADD COLUMN send_read_receipts INTEGER NOT NULL DEFAULT 0;
```

If disabled, skip step 3 above. Still mark messages as read locally.

## Delivery Status on Sent Messages

Outgoing messages gain a delivery status enum:

```
sending -> sent -> delivered -> read
```

Stored in `message_history`:

```sql
ALTER TABLE message_history ADD COLUMN delivery_status INTEGER NOT NULL DEFAULT 0;
-- 0 = sending, 1 = sent, 2 = delivered, 3 = read
```

- **sent**: server accepted the message
- **delivered**: recipient's device received it (delivery receipt)
- **read**: recipient read it (read receipt)

Displayed in the UI as checkmarks (single = sent, double = delivered, blue double = read) — Signal-style.

## Implementation Stages

### Stage A: Per-message read_at + derived unread count (do now)

Changes:
- **store schema**: Add `read_at` column to `message_history`.
- **store**: Add `mark_messages_read(conversation_id, up_to_sent_at)` and `unread_count(conversation_id, own_did)` methods.
- **app-core**: Expose `mark_messages_read` and `unread_count` via UniFFI.
- **Swift Message model**: Add `readAt: Date?` field.
- **Swift Conversation**: Replace stored `unreadCount` with computed property derived from in-memory messages. Remove `activeConversationId`.
- **Swift ConversationView**: On appear, mark all loaded messages as read (sets `read_at = now`).
- **Swift ConversationRow**: Compute unread count from `messagesByConversation`.
- **Swift AppState.handleIncomingMessage**: No unread count logic — just append the message with `read_at = nil`.

### Stage B: Scroll-position-based read marking

Changes:
- **Swift ConversationView**: Use `ScrollPosition` (iOS 17+) to track last visible message. `.onChange(of: scrollPosition)` marks visible messages as read via `markMessagesRead(upTo:)`.
- **app-core**: `mark_messages_read` already supports partial marking (up to a timestamp).
- Stage A marks all messages on appear; Stage B refines to only mark visible ones.

### Stage C: Read receipt wire protocol

Changes:
- **proto**: Define `ReceiptMessage` in protobuf envelope.
- **app-core**: Send batched read receipts (3-second debounce) after marking messages read. Receive and apply incoming read receipts.
- **store**: Add `delivery_status` column. Methods to update status on receipt.
- **Swift UI**: Show delivery status indicators (checkmarks) on sent messages.
- **Settings**: Toggle for `send_read_receipts` preference.

### Stage D: Delivery receipts

Changes:
- **app-core**: On successful decryption of an incoming message, send a `ReceiptMessage { type: DELIVERY }` back to the sender.
- **app-core**: Handle incoming delivery receipts, update `delivery_status` from `sent` to `delivered`.
