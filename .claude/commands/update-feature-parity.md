Update the feature parity matrix in `docs/feature-parity.md`.

$ARGUMENTS should describe what changed, e.g.: "iOS groups send" or "Android account creation" or "Desktop DMs napi".

## Step 1 — Read the current matrix

Read `docs/feature-parity.md` in full. Identify the row(s) and column(s) that need updating based on `$ARGUMENTS`.

If `$ARGUMENTS` is empty, ask the user: "Which feature and platform did you just implement? (e.g. 'iOS delivery receipts', 'Android account creation')"

## Step 2 — Determine the new status

Status values:
- `✅` — fully implemented and working
- `🚧` — partially implemented (note what's missing in a comment or the Notes section)
- `⬜` — not started
- `n/a` — not applicable to this platform

## Step 3 — Update the cell

Edit `docs/feature-parity.md` to change the relevant cell. Show the user the before/after diff and confirm it looks correct.

If the feature being marked complete was previously listed as a gap in `/new-feature`'s test plan (cross-platform conformance, recovery flow, state machine edge cases), also check whether the known-gap note in any test file (`// TODO(tests):`) can now be resolved.

## Step 4 — Commit note

Remind the user: if this update corresponds to a completed TODO from `docs/02-todos-deferred.md`, run `/done` to handle the full post-implementation checklist including PR creation.

Report: which cell(s) were updated and the new status.
