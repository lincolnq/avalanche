Create a new documentation file for `$ARGUMENTS`.

## Step 1 — Determine the category and next number

Read `docs/CLAUDE.md` to see the numbering scheme and existing doc numbers. Pick the right category prefix:

| Prefix | Category |
|---|---|
| `0x` | Core design (goals, architecture, overall decisions) |
| `1x` | Server & protocol (homeserver, API, federation) |
| `2x` | Projects framework (SDK, first-party Projects) |
| `3x` | Messaging & conversation UX (mobile flows, crypto UX) |
| `4x` | Deploy & infra (deployment guides, relay) |
| `5x` | Identity & accounts (identity, recovery, contacts) |

List the existing files in `docs/` to find the highest number used in that category, then use the next available number.

## Step 2 — Create the file

Create `docs/NN-$ARGUMENTS.md` with this structure:

```markdown
# <Title>

<One-paragraph overview: what problem this doc addresses and what it decides.>

## <Section>

<Content>
```

Follow the style of existing docs: prefer concrete decisions over open questions, use headings to separate topics, use code blocks for API shapes or data structures.

## Step 3 — Update the docs index

**a) `docs/CLAUDE.md`** — add a row to the lookup table:
```
| <topic keywords> | `NN-$ARGUMENTS.md` |
```

**b) `docs/00-design.md`** — find the documentation map table at the top and add an entry:
```
| [`NN`](NN-$ARGUMENTS.md) | <one-line description> |
```

in the correct category section.

## Step 4 — Verify

Confirm the file number doesn't conflict with any existing doc. Run a quick search: `ls docs/NN-*.md` to verify no collision.

Report: the file path, the category chosen, and the rows added to both indexes.
