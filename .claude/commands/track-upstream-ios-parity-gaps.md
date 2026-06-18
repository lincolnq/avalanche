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

**Important:** treat this commit list as attribution context only — not as the unit of analysis. A single feature often materialises across many commits (model first, then view, then bug fixes). Feature detection happens in Step 3 on the cumulative diff, which sees the final state of all those commits combined.

---

## Step 2 — Identify iOS-relevant changes

Run the following against the **cumulative diff** — the total change between your current HEAD and upstream/main, regardless of how many commits it spans:
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

Read the **cumulative diff** for all iOS source files at once:
```
git diff HEAD..upstream/main -- mobile/ios/
```

Do not split this by commit. You are reading the net result of everything lincolnq shipped since your last sync — a feature built across five commits appears here as one coherent set of additions, which is exactly what you want.

Reason about what **user-facing features or behaviors** were added, changed, or removed across the whole diff. You are looking for:

- New SwiftUI views or screens
- New FFI calls (`core.methodName()`) — these expose new Rust functionality to the UI
- New model fields or state in `AppState.swift`
- Changed onboarding flows, navigation, or settings

For each feature you identify, also assess its **completeness** in the diff:
- **Complete** — the full user-facing flow is present (view + state + FFI wired up)
- **Partial** — scaffolding or stubs exist but the feature isn't fully wired (e.g. view exists, FFI call is `// TODO`)

Mark partial features as 🚧 in the parity matrix (Step 4) rather than adding a TODO — they're not yet something Android/Desktop needs to implement.

Group the complete features into a list of **feature descriptions** — plain-English summaries like "account recovery from written-down phrase" or "block/report contact". One feature will often span multiple files; that's expected.

Display the feature list with completeness status and the files that back each one, so the user can verify your interpretation before you act on it.

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
- Android parity: [feature description] (iOS landed upstream <old-HEAD-sha>..<upstream-sha>)
- Desktop parity: [feature description] (iOS landed upstream <old-HEAD-sha>..<upstream-sha>)
```

Use the SHA range rather than a single commit SHA — since a feature may span multiple commits, the range is the accurate citation and lets anyone run `git log <range> -- mobile/ios/` to see exactly what was shipped.

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
