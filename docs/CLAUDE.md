# docs/ — documentation guide

## DIGEST.md

`DIGEST.md` is a compressed, single-file index of every design doc here — decisions,
their rationale, and rejected alternatives, with `(03 §3.9)`-style pointers back to the
source doc for detail. It exists so a session can hold the whole design in context for
~12K tokens instead of ~170K. The root `CLAUDE.md` points sessions at it first. It is
**derived** from the other docs and is **lossy** — the source docs remain authoritative.

### Regenerating it

Regenerate when the design docs have changed materially (new subsystem doc, a reversed/
added DECIDED decision, a new rejected alternative worth recording). To regenerate:

1. **Read every design doc** — all of `docs/*.md` *except* `DIGEST.md` itself and the
   `signal-research/` folder (background reference, not decisions). This is a large read
   (~170K+ tokens)
2. **Write `DIGEST.md`**, preserving — this is the point of the doc:
   - every **DECIDED** decision **and its why**;
   - **rejected designs and the reason** they were rejected (these are scattered and
     easily lost — keep them inline *and* in the consolidated rejected-designs index at
     the end);
   - load-bearing **invariants** (e.g. the §3.9 membership-opacity discipline, the
     401-vs-403 contract) and current **implementation status** (✅ built / 🚧 partial /
     📐 design-only).
   Drop: TODO/backlog items, step-by-step deploy commands, exact SQL/protobuf/wire tables,
   and screen-by-screen UI copy — point at the source doc for those instead.
3. **Tag each section with its source doc number** (e.g. `(03)`, `(52)`) so any
   over-compression is traceable back.

If only one doc changed in a small way, edit the corresponding `DIGEST.md` section in
place rather than doing a full regeneration.

## Numbering scheme

Doc filenames follow `NN-description.md`. The first digit is the category:

| Prefix | Category | What's here |
|---|---|---|
| `0x` | Core design | Goals, architecture, threat model, backlog |
| `1x` | Server & protocol | Homeserver implementation, API, abuse, federation |
| `2x` | Projects framework | Project security model, first-party Projects |
| `3x` | Messaging & conversation UX | Mobile UX, read tracking, identity, invites, contacts, connection state |
| `4x` | Deploy & infra | Deployment guides, relay deployment |
| `5x` | Identity & accounts | (reserved for future identity/account docs) |

## signal-research/

Background reading on how Signal handles specific problems (push notifications, profile key transmission, etc.). Reference material — not design decisions for this project.

## Adding a new doc

Pick the next available number in the appropriate category. Update the documentation map table in `docs/00-design.md` and add a row to the lookup table above.
