# Message status checkmark indicator — group delivery & read receipts

What drives the appearance and status change of the checkmark indicator shown under each
outgoing message. Covers both 1:1 and group threads; group-specific aggregation is called out.

## TL;DR

The checkmark is a single icon in the message footer, driven by a per-message status enum
(`MessageReceiptStatus`). For a message, the app keeps a **per-recipient** state
(`TSOutgoingMessageRecipientState`). Incoming delivery / read / viewed receipts bump the
matching recipient's state forward (never backward). The footer then collapses all recipient
states into one icon using a **"best recipient wins"** rule:

```
viewed  → message_status_read   (filled double-check)
read    → message_status_read
delivered (any recipient) → message_status_delivered (double-check)
sent    → message_status_sent   (single check)
sending/uploading → message_status_sending (animated spinner)
pending → message_status_sending (static spinner)
failed  → no icon (shows a failure affordance / "!" instead)
```

So in a **group**, the icon reflects the *furthest-along* recipient for read/viewed/delivered,
but the *least-far-along* recipient for sending/pending/failed (see "Two aggregation rules" below).

---

## The pieces

### 1. The icon view — `CVComponentFooter`
`Signal/ConversationView/Components/CVComponentFooter.swift`

- `struct StatusIndicator` (≈L13): holds `imageName` + `isAnimated`. Area is fixed at 18×12 so
  every status occupies the same space (icons differ, some animate).
- `configureForRendering` (≈L217-246): loads `UIImage(named: statusIndicator.imageName)`, renders
  as a template tinted with the footer text color, and calls `animateSpinningIcon()` when
  `isAnimated` is true (the "sending" spinner).
- `buildState(...)` (≈L452-560): the mapping. Calls
  `MessageRecipientStatusUtils.receiptStatusAndMessage(outgoingMessage:transaction:)` to get the
  per-message `MessageReceiptStatus`, then switches on it to pick the image name:
  - `.uploading, .sending` → `"message_status_sending"`, animated
  - `.pending` → `"message_status_sending"`, static
  - `.sent, .skipped` → `"message_status_sent"`
  - `.delivered` → `"message_status_delivered"`
  - `.read, .viewed` → `"message_status_read"`
  - `.failed` → **no** indicator icon
  - If `wasRemotelyDeleted` and not actively sending → indicator cleared.

The same enum→icon mapping is reused in `MessageDetailViewController` (per-recipient receipt list)
and `ChatListCell` (conversation-list preview).

### 2. The UI status enum — `MessageReceiptStatus`
`SignalUI/Utils/MessageRecipientStatusUtils.swift` (L8-18)

```swift
public enum MessageReceiptStatus: Int {
    case uploading, sending, sent, delivered, read, viewed, failed, skipped, pending
}
```
This is the UI-layer enum. It is *derived*, not stored.

### 3. The stored per-recipient state — `TSOutgoingMessageRecipientState`
`SignalServiceKit/Messages/Interactions/TSOutgoingMessageRecipientState.swift`

- A `TSOutgoingMessage` holds `recipientAddressStates: [SignalServiceAddress: TSOutgoingMessageRecipientState]`
  (declared in `TSOutgoingMessage.h`). One entry per recipient — for a group, one per group member.
- Each state stores `status: OWSOutgoingMessageRecipientStatus`, a `statusTimestamp`, `wasSentByUD`,
  and an optional `errorCode`.
- `OWSOutgoingMessageRecipientStatus` (L193-257) cases + **priorityValue** (L225-243):
  ```
  sending/pending/failed → 1   (freely swap among these while sending)
  skipped                → 2
  sent                   → 3
  delivered              → 4
  read                   → 5
  viewed                 → 6
  ```
- **Monotonic forward-only updates**: `updateStatusIfPossible(_:statusTimestamp:)` (L76-89) refuses
  any update whose `priorityValue` is *lower* than the current one. This is why a late-arriving "sent"
  confirmation can never overwrite an already-"delivered"/"read" state. It logs a warning and returns.
- `statusTimestamp` semantics (L11-28): local clock for `.sending`/`.pending`/`.sent`(this device);
  remote receipt timestamp for `.delivered`/`.read`/`.viewed`. The decoder (L102-149) migrates legacy
  records that stored separate `deliveryTimestamp`/`readTimestamp`/`viewedTimestamp` keys into the
  unified `status` + `statusTimestamp` representation.

### 4. Where receipts mutate the state
`SignalServiceKit/Messages/OWSReceiptManager.swift`
- `processDeliveryReceipts(...)` (L916-939) → `message.update(withDeliveredRecipient:...)`
- `processReadReceipts(...)` (L946-972) → `message.update(withReadRecipient:...)`,
  **gated** on `Self.areReadReceiptsEnabled(transaction:)` (L953) — if the *local user* has read
  receipts disabled, incoming read receipts are dropped entirely (returns `[]`), so messages never
  advance past "delivered" for that user.
- `processViewedReceipts(...)` (L979+) → `message.update(withViewedRecipient:...)`. Viewed is only
  for explicitly-viewed media (view-once, voice notes) and stories. Not gated by the read-receipt pref.
- Linked-device sync variants exist (`processReadReceiptsFromLinkedDevice`, L563+).

`SignalServiceKit/Messages/Interactions/TSOutgoingMessage.swift`
- `update(withDeliveredRecipient:)` / `update(withReadRecipient:)` / `update(withViewedRecipient:)`
  (L722-770) all funnel into `handleReceipt(...)` (L786-847), which:
  1. Ignores receipts for remotely-deleted messages.
  2. Clears the Message Send Log entry for that recipient/device.
  3. Looks up the recipient's state, normalizing PNI↔ACI addresses if the direct lookup misses
     (`RecipientStateMerger`). If no state exists, `owsFailDebug` and bail.
  4. Calls `recipientState.updateStatusIfPossible(receiptType.asRecipientStatus, statusTimestamp:)`.

So a single recipient's checkmark progression is: `sending → sent → delivered → read (→ viewed)`,
each step driven by a receipt, never moving backward.

---

## Two aggregation rules (the group story)

There are **two different** collapse functions, and they use opposite tie-breaking. Both live such
that any recipient can determine the message-level result.

### A. "Worst recipient wins" — for sending/pending/failed/sent
`SignalServiceKit/Messages/Interactions/TSMessage+RecipientState.swift` →
`messageStateForRecipientStates(_:)` (L8-38) returns a `TSOutgoingMessageState`:

```
any recipient .sending  → .sending
else any .pending        → .pending
else any .failed         → .failed
else                     → .sent     (covers delivered/read/viewed, which all imply sent)
```
This decides whether the message is still in flight / partially failed. In a group, if even one
member is still being sent to, the whole message shows the spinner; if one member failed (and none
are still sending/pending), the message shows failed.

### B. "Best recipient wins" — for the delivered/read/viewed checkmark
`SignalUI/Utils/MessageRecipientStatusUtils.swift` →
`receiptStatusAndMessage(outgoingMessage:hasBodyAttachments:)` (L116-162). This is what the footer
actually calls. It first switches on `outgoingMessage.messageState` (rule A above) and, **only when
that is `.sent`**, upgrades the displayed status by scanning recipients in priority order:

```swift
case .sent:
    if outgoingMessage.viewedRecipientAddresses().count > 0 { return .viewed }
    if outgoingMessage.readRecipientAddresses().count   > 0 { return .read }
    if outgoingMessage.wasDeliveredToAnyRecipient            { return .delivered }
    return .sent
```

The `*RecipientAddresses()` helpers (`TSOutgoingMessage.swift` L40-94) are **inclusive of higher
states**: `deliveredRecipientAddresses()` counts delivered+read+viewed; `readRecipientAddresses()`
counts read+viewed; `viewedRecipientAddresses()` counts only viewed.

**Net effect for a group:** the double-check "delivered" appears as soon as *any one* member's device
acknowledges delivery; the filled "read" appears as soon as *any one* member reads it — it does **not**
wait for all members. The conversation footer therefore shows the most-advanced recipient's status.
(The per-member breakdown — who delivered vs. read vs. pending — is only visible in
`MessageDetailViewController`, which buckets recipients by their individual `MessageReceiptStatus`.)

### Why the two rules don't conflict
`messageState` stays `.sending`/`.pending`/`.failed` until every recipient is at least `sent`/`skipped`.
Once it flips to `.sent`, rule B takes over and only ever upgrades the icon. Because of the monotonic
`priorityValue` guard, recipient states never regress, so the displayed checkmark only moves forward
(or, for the message-level spinner, resolves once all sends complete).

---

## Quick reference — file map

| Concern | File | Symbol |
|---|---|---|
| Icon render + enum→image map | `Signal/ConversationView/Components/CVComponentFooter.swift` | `buildState`, `configureForRendering`, `StatusIndicator` |
| UI status enum | `SignalUI/Utils/MessageRecipientStatusUtils.swift` | `MessageReceiptStatus` |
| Per-message status (best-wins) | `SignalUI/Utils/MessageRecipientStatusUtils.swift` | `receiptStatusAndMessage` |
| Per-recipient status (detail view) | `SignalUI/Utils/MessageRecipientStatusUtils.swift` | `recipientStatusAndStatusMessage` |
| Stored recipient state | `SignalServiceKit/.../TSOutgoingMessageRecipientState.swift` | `OWSOutgoingMessageRecipientStatus`, `updateStatusIfPossible`, `priorityValue` |
| Message-level state (worst-wins) | `SignalServiceKit/.../TSMessage+RecipientState.swift` | `messageStateForRecipientStates` |
| Recipient filters | `SignalServiceKit/.../TSOutgoingMessage.swift` | `deliveredRecipientAddresses`, `readRecipientAddresses`, `viewedRecipientAddresses` |
| Receipt → state mutation | `SignalServiceKit/.../TSOutgoingMessage.swift` | `update(withDeliveredRecipient:)`, `handleReceipt` |
| Receipt intake + read-pref gate | `SignalServiceKit/Messages/OWSReceiptManager.swift` | `processDeliveryReceipts`, `processReadReceipts`, `processViewedReceipts` |
| Per-recipient detail UI | `Signal/src/ViewControllers/MessageDetailViewController.swift` | recipient bucketing by status |

## Edge cases worth remembering
- **Read receipts disabled (local pref):** incoming read receipts are dropped at
  `OWSReceiptManager.processReadReceipts` → messages cap at "delivered" double-check.
- **Skipped recipients** (left the group / unregistered): `.skipped` has priority 2, treated as
  "sent" for icon purposes and excluded from delivered/read counts, so they don't hold the message in
  "sending".
- **Viewed vs. read:** `.viewed` only applies to view-once media / voice notes / stories; for ordinary
  text it never appears, and the icon for both `.read` and `.viewed` is identical (`message_status_read`).
- **Remotely-deleted messages:** receipts are ignored, and the footer clears the indicator unless still
  sending.
- **PNI/ACI normalization:** `handleReceipt` normalizes addresses before matching, so a receipt from a
  recipient's ACI still updates a state originally keyed by their PNI.
