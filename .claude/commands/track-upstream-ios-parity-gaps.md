Sync from the upstream remote, identify new iOS-only changes, and populate deferred todos for Android and Desktop parity gaps.

**What this skill does and why:** lincolnq pushes iOS features directly to upstream/main without opening PRs. When you rebase onto upstream, new iOS functionality can land silently with no Android or Desktop equivalent. This skill surfaces those gaps automatically and turns them into tracked backlog items before they're forgotten.

---

## Step 1 — Fetch upstream and report what's new

Run `git fetch upstream` and report the result (new tags, updated refs, or "already up to date").

Then run:
```
git log upstream/main --not HEAD --oneline
```

Display the full list of commits that are in `upstream/main` but not yet in your current branch. If the list is empty, report "Already up to date with upstream/main — nothing to sync" and stop.

Label each commit with its author so it's clear which commits are from lincolnq vs. other contributors.

---

## Step 2 — Identify iOS-relevant changes

Run:
```
git diff HEAD..upstream/main --name-only
```

Filter the changed files into three groups and display them:

1. **iOS source files** — anything under `mobile/ios/` (these are the ones that need parity)
2. **Shared Rust/server files** — `core/`, server code, protocol changes (these affect all platforms via FFI — flag separately)
3. **Everything else** — docs, CI, config (no parity action needed)

If there are no iOS source files changed, report "No iOS source changes in upstream commits — no parity gaps to track" and skip to Step 5.

---

## Step 3 — Understand what changed on iOS

For each iOS source file that changed, run:
```
git diff HEAD..upstream/main -- <file>
```

Read the diffs and reason about what **user-facing features or behaviors** were added, changed, or removed. You are looking for:

- New SwiftUI views or screens
- New FFI calls (`core.methodName()`) — these expose new Rust functionality to the UI
- New model fields or state in `AppState.swift`
- Changed onboarding flows, navigation, or settings

Group the changes into a list of **feature descriptions** — plain-English summaries like "account recovery from written-down phrase" or "block/report contact". One feature may span multiple files.

Display the feature list with the files that back each one, so the user can verify your interpretation before you act on it.

---

## Step 4 — Cross-reference against the parity matrix

Read `docs/62-feature-parity.md` in full.

For each feature you identified in Step 3:

- Find the matching row in the matrix (or note that no row exists yet)
- Check the Android and Desktop columns
- Classify as one of:
  - **Already done** — both Android ✅ and Desktop ✅: no action needed
  - **Already tracked** — both columns show 🚧 or there's a known TODO: no action needed
  - **Gap** — Android ⬜ or Desktop ⬜ (or row missing entirely): action needed

Display a gap summary table:

| Feature | iOS | Android | Desktop | Action |
|---|---|---|---|---|
| ... | ✅ | ⬜ | ⬜ | Add TODOs |

If there are no gaps, report that and stop.

---

## Step 5 — Check for existing TODOs

Before adding anything, read `docs/02-todos-deferred.md` in full.

For each gap feature, check whether an Android or Desktop TODO already exists (search for the feature name or related keywords). Skip any that are already tracked to avoid duplicates.

Report which gaps already have TODOs and which are genuinely new.

---

## Step 6 — Populate deferred todos

For each genuinely new gap, add entries to `docs/02-todos-deferred.md` under an appropriate section heading. Use this format:

```
- Android parity: [feature description] (lincolnq added iOS in upstream commit <short-sha>)
- Desktop parity: [feature description] (lincolnq added iOS in upstream commit <short-sha>)
```

If Android is done but Desktop is not (or vice versa), add only the missing one.

If the feature row doesn't exist in `docs/62-feature-parity.md` yet, add it with iOS ✅ and Android/Desktop ⬜.

If the row exists but Android/Desktop shows ⬜ rather than 🚧, update it to 🚧 to signal "known gap, tracked in backlog."

---

## Step 7 — Rebase and report

Run:
```
git rebase upstream/main
```

If there are conflicts, resolve them following the conventions in `CLAUDE.md` (our additions take precedence over upstream for docs we own; for shared files, take both).

After the rebase completes, produce a summary report:

```
Synced with upstream/main.

New upstream commits: N
iOS files changed: list
Shared Rust/server changes: list (may require FFI or API updates — check separately)

Parity gaps found: N
  - [feature]: Android + Desktop TODOs added to docs/02-todos-deferred.md
  - [feature]: Desktop TODO added (Android already done)

Already tracked / no action: N features
docs/62-feature-parity.md updated: yes/no
```

Remind the user: if any shared Rust/server changes were flagged in Step 2, those may require FFI updates (`make bindings`) or API changes on Android/Desktop — review the diffs manually.
