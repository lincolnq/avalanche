# docs/ — documentation guide

## Where to start

- `00-design.md` — product premise, architecture overview, and the **documentation map**.
- `09-security-posture.md` — the threat model as it actually stands today: what each
  adversary learns, the known gaps, and the planned fixes. Read before any security-relevant
  change.
- `02-todos-deferred.md` — the prioritized todo list / roadmap.
- `DIGEST.md` — compressed index of every doc (decisions, rationale, rejected alternatives,
  status). Derived and lossy; source docs are authoritative.

## Status vocabulary

Every doc starts with a status block, and sections that differ from the doc's overall status
carry their own tag. Use exactly these words (no emoji — plain words only):

| Status | Meaning |
|---|---|
| **Built** | In the code today. Describes the actual contract, with code pointers. |
| **Partial** | Some of it is built; the doc says precisely which parts. |
| **Planned** | Committed direction, not yet built. Expected to be built roughly as written. |
| **Proposed** | A change agreed in principle with the maintainer that alters a contract (wire format, FFI, identity, schema, cross-platform behavior). **Needs project-owner review before implementation.** |
| **Speculative** | An idea worth keeping. No commitment; may never be built. |
| **Superseded** | A design we moved away from. Kept only for its rationale. |

The status block at the top of each doc looks like:

```
> **Status:** Partial — one-line summary of what exists and what doesn't.
> **Last verified against code:** YYYY-MM-DD
```

"Verified" means someone read the code and confirmed the doc's Built claims. If you change
behavior a doc describes, update the doc (and its verified date) in the same change.

## Doc layout

Subsystem docs follow this shape, in this order, omitting sections that would be empty:

1. **Status block** (above).
2. **Summary** — a few sentences: what this subsystem is and where it stands.
3. **Current design** — what is built, written as the contract, with code pointers
   (`path/to/file.rs:fn_name` or `file:line`). No aspirational text here.
4. **Known gaps** — places where the code falls short of the design or has a bug. Security
   gaps also go in `09-security-posture.md`; todos also go in `02`.
5. **Planned** — committed but unbuilt work.
6. **Proposed** — contract changes awaiting owner review.
7. **Speculative** — maybes.
8. **Rationale and rejected alternatives** — why we chose what we chose, and what we
   rejected and why. This is often the most valuable part of a doc; keep it.

**Section numbers cited from code are stable.** Code comments cite sections like `(03 §3.9)`
or `docs/35 §Staging` (~450 such citations). Before renumbering or renaming a heading, run
`git grep -n 'docs/NN'` and `git grep -n '(NN §'` and keep every cited heading reachable at
the same number/name. In heavily cited docs, keep the existing numbering and put status tags
on sections instead of restructuring. Doc file numbers are likewise stable — never renumber
a doc.

Write plainly and briefly. Delete history that no longer informs a decision (struck-through
goals, resolved open questions, "earlier drafts said…") unless it explains a rejected
alternative.

## DIGEST.md

`DIGEST.md` is a compressed, single-file index of every design doc here — decisions,
their rationale, rejected alternatives, and status, with `(03 §3.9)`-style pointers back to
the source doc. It exists so a session can hold the whole design in context cheaply. It is
**derived** and **lossy** — the source docs remain authoritative.

Regenerate when the docs change materially (new doc, a reversed or added decision, a status
change, a new rejected alternative). To regenerate:

1. Read every doc in `docs/*.md` except `DIGEST.md` itself and `signal-research/`.
2. Write `DIGEST.md`, preserving: every decision **and its why**; rejected designs and the
   reason (inline *and* in the consolidated index at the end); load-bearing invariants; and
   each section's **status** using the vocabulary above. Drop todo items, deploy commands,
   exact SQL/protobuf tables, and UI copy.
3. Tag each section with its source doc number so over-compression is traceable.

If only one doc changed in a small way, edit the matching DIGEST section in place.

## Numbering scheme

Doc filenames follow `NN-description.md`. The first digit is the category:

| Prefix | Category |
|---|---|
| `0x` | Core design: premise, architecture, security posture, roadmap, cross-cutting subsystems |
| `1x` | Server & protocol: homeserver, abuse, federation, mesh, push |
| `2x` | Projects framework and first-party Projects |
| `3x` | Messaging & conversation UX |
| `4x` | Deployment & infra |
| `5x` | Identity, accounts, contacts, profiles |
| `6x` | Platforms: Android, Desktop, feature parity |

## signal-research/

Background reading on how Signal handles specific problems. Reference material, not design
decisions for this project.

## Adding a new doc

Pick the next free number in the right category, use the layout above, and add it to the
documentation map in `00-design.md`.
