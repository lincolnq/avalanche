# Foreground Notification Behavior

How Signal-iOS handles notifications when the app is in the foreground.

## System-Level Setup

In `Signal/AppLaunch/AppDelegate.swift:1304-1318`, the `willPresent` delegate **always** returns `[.badge, .banner, .list, .sound]`. It never suppresses at the iOS system level. Instead, suppression is handled by Signal's own logic that conditionally omits the notification body/title.

## Suppression Logic

In `NotificationPresenterImpl.swift:285-316`, the app checks the **frontmost view controller** to determine a suppression rule:

| Frontmost UI | Suppression Rule |
|---|---|
| `ConversationSplit` showing thread X | Suppress notifications for thread X only |
| `StoryGroupReplier` for story Y | Suppress group story replies for that story |
| `FailedStorySendDisplayController` | Suppress failed story send notifications |
| `LinkAndSyncProgressUI` (if flagged) | Suppress **all** notifications |
| Anything else | No suppression — show everything |

## What Gets Suppressed vs. Always Shown

### Always shown regardless of what's on screen

- Missed calls
- Identity change alerts
- Transfer/deregistration notices
- New linked device
- Backups notifications (enabled, media tier quota, attachment backfill)

### Suppressed only when viewing the relevant thread

- Incoming messages (with or without actions)
- Incoming reactions
- Info/error messages
- Poll notifications (end and vote)

### Suppressed only when viewing the relevant story reply sheet

- Incoming group story replies

### Suppressed only when viewing the failed story send controller

- Failed story send notifications

## How Suppression Actually Works

When a notification is suppressed (`UserNotificationsPresenter.swift:175-182`), the notification is still *delivered* to iOS, but with **no title or body set** — so no banner appears. Badge and sound may still apply.

## Sound Behavior

- A **quieter** version of the notification sound is used when the app is active (`UserNotificationsPresenter.swift:130`).
- Sound respects the user's "sound in foreground" preference.
- Sound is **throttled** — won't play more than a fixed number of times within a short interval.

## No Custom In-App UI

Signal does **not** have a custom in-app notification banner. It relies entirely on iOS system notifications. If a notification is suppressed (e.g., you're already viewing that conversation), you simply don't see a banner — the only indicators are the optional sound/vibration.
