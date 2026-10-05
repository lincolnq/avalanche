# 37 — Chat organization (tabs, multi-account, threads entry)

> **Status:** Partial — account tabs are built on iOS, Android, and Desktop: with more than one identity, the Chats screen shows one avatar tab per identity with an unread badge, filtering the inbox. Not built: the tab organizer, custom or topic tabs, the on-device classifier, the Threads catch-up entry, per-conversation mute.
> **Last verified against code:** 2026-10-04

## Summary

The default inbox is plain and Signal-like: one row per conversation, sorted by recency, **no tab row**. Tabs appear only once they earn their place. The first trigger, and the only one built, is having **more than one identity**. Over time, tabs can be filled either by deterministic rules ("everything from my pseudonymous identity") or by an on-device classifier proposing topic tabs: the same surface, fed by two sources.

Code: iOS `Views/Chats/ChatsView.swift` (`accountTabStrip`, `AppState.selectedChatsAccountTab`); Android `Views/Chats/ChatsView.kt`; Desktop `desktop/src/views/chats/ChatsView.tsx` (`selectedAccountTab`, tab strip as a row under the header). Motivated by the "conversation intelligence" post (theavalanche.net/blog/2026-07-intelligence) and real multi-account confusion in testing.

## Current design: account tabs

*Built (iOS, Android, Desktop).*

- **Shown only with more than one identity** (`appState.accounts.count > 1`). A single-identity user sees a plain unified inbox.
- **One tab per identity** (avatar, unread badge summing that identity's conversations). Selecting a tab filters the inbox to that identity's conversations. The selection lives in app state and survives navigation. The effective tab is computed on the first render, so cold launch never flashes an unfiltered list.
- Compose sits in the header, outside the tab strip.
- The Search tab mirrors the header geometry so the two top bars line up.

Note that today's tabs are filters with an implicit "pick one", not the exhaustive-partition "homes" described below. With account tabs the two coincide, because every conversation belongs to exactly one identity.

## Multi-account: organization replaces marking

*Decided; built.* **We don't mark which identity or server a conversation belongs to in inbox rows** — no badge, tint, or dot.

- A conversation is bound to exactly one of your identities **by construction** (different `my_did`, different conversation id). The conversation itself is the context, so you can't act as the wrong persona inside it, and there's no per-message identity choice to get wrong.
- If you want separation, **make it a tab**.
- **Compose-time identity default:** a *new* conversation has no context, so compose picks the acting identity (`30`, `52`).

This supersedes `30`'s earlier per-row identity indicator and in-conversation identity switcher.

## Planned: per-conversation mute

*Planned; not built on any platform.* Mute a conversation (for a period or indefinitely), and choose whether mentions still notify. Device-local, and also synced via the storage service's conversation settings (`05`). This matters more than threading for people sitting in large organizing groups. Muted conversations don't contribute to the app badge (Signal's behavior).

## Speculative: the full tab model

### Tabs are homes, not filters

Tabs form an **exhaustive partition**: every conversation lives in exactly one tab, and there's no "All" tab (the Gmail-tabs model). Tabs are user-configurable.

- **Homes vs. lenses.** A tab is a *home*. Cross-cutting collections like Mentions or Saved are *lenses* and don't belong in the tab row.
- **Threads** are neither: they surface as a single **catch-up entry** (below).

### Assignment

The design commits to the *shape* of assignment, not the mechanism:

- **Single-home partition.**
- **Two sources on one surface:**
  - **structural** rules (by identity, server, or any hard predicate; deterministic, every device);
  - **AI-suggested** topic tabs from on-device classification, degrading to manual where models aren't available.
- **One evolving configuration, never a migration between regimes.** Account tabs drain into topic tabs as the user adds them.
- **Automatic behavior never stomps the user's setup** (the one load-bearing rule). The classifier *proposes*; the user decides. A manual move is a sticky per-conversation override.
- **One axis at a time.** Server and topic are orthogonal, and a single-home partition can express only one. The other axis is a search or lens.

An ordered first-match-wins rule list (like mail filters) is the likely mechanism, but nothing depends on it.

### The tab organizer

One screen — reached from Settings → "Organize conversations", or offered automatically when complexity crosses a threshold (default: adding a second identity) — shows the tabs and lets you move conversations between them. v1 would be thin. The point is to reserve one place where organization lives.

### The Threads catch-up entry

Gated on threads existing (`32`). It's a compact row pinned atop the primary tab's list when there are unread threads:
- shorter than a conversation row;
- the title is the count of unread threads;
- the badge is the total unread messages in them;
- the preview names the groups ("in Canvass NW, Action Day Leads");
- hidden on other tabs.

This is the canonical threads entry; `32`'s earlier "shelf" is superseded.

### Open

- Classifier mechanics and model-availability tiers.
- Per-tab notification rules and message-level overrides.
- Auto-prompt thresholds beyond a second identity.
- Whether the primary tab is special.

## Known gaps

1. No per-conversation mute (above). P1.

## Rationale and rejected alternatives

- **Per-row account marking** (badge, tint, or dot) — rejected: organization by tabs replaces it, and a bare marker reads as noise without a legend.
- **A filter bar with an "All" view** — rejected for the full model: tabs are an exhaustive partition.
- **Threads as their own tab** — rejected: a thread is nested, not a home.
- **A discrete switch from server tabs to topic tabs** — rejected: one evolving configuration.
- **A "shelf" of icons above the inbox** (`32`'s earlier entry point) — superseded by the single pinned catch-up row.
