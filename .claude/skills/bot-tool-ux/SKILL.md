---
name: bot-tool-ux
description: >-
  UX playbook for chat/command-line bot tools. Use when designing or implementing a bot
  command that takes input or performs an action: multi-parameter
  "interview" flows, /help, errors, status. Apply whenever adding or changing a bot command's user-facing behavior.
---

# Bot tool UX

It is very useful to be able to control important things through texting. Well-
designed text-mode bots are quite powerful, especially in the age of AI.

When interfacing with a bot through tools, it should feel fast, use simple language
and be self-explanatory to the maximum degree without feeling slow.

These UX principles are defaults, applied case by case.
Use the technique that fits the command and the platform's capabilities.

## Principles

1. **Ask for decisions, not data.** A command's job is to capture a human
   *judgment* — approve, choose, trigger, grant — not to transcribe facts. If
   you're asking someone to *describe* a thing rather than *decide* about it, the
   input belongs somewhere else (or shouldn't be asked). See
   [Ask for decisions, not data](#ask-for-decisions-not-data).
2. **Self-explaining in plain language.** Commands that need input should be able
   to *ask* for it, with short, jargon-free explanations of what's at stake. See
   [Plain language](#plain-language).
3. **Keep shared channels clean.** Push interaction into a DM; leave only a
   small status signal (usually a reaction) behind.
4. **Confirm before you commit, and say what happened.** Every command ends in a
   clear terminal state: done, failed, or cancelled.

## Ask for decisions, not data

Before adding any prompt, ask: *is this a decision only this human can make right
now, or is it data that lives somewhere better?* Solicit the former; never the
latter.

Data always has an owner, and it usually isn't the operator:

- **The thing being acted on owns its own description.** An external component
  (a project, integration, bot) should declare its metadata and the permissions
  it wants in a **manifest** it publishes; the command points at it, reads it,
  and asks the human only to *authorize*. Interviewing the operator for the
  thing's own attributes (name, URL, permissions) is a smell — a stopgap for a
  missing manifest, not the target design.
- **Derivation owns what follows from another answer.** Compute it; don't ask.
  Take the name "Beagle" → codename "beagle"; don't ask for both.
- **Defaults own the insignificant and the changeable.** If a setting carries
  little weight or can be edited later, give it a sensible default and move on.
  Every question you *do* ask should bear real weight.

Strip all of that away and what's left is the genuine human decisions — usually
just a **pointer** ("which one?") and an **authorization** ("approve? grant
what?"). So the ideal install is close to a single confirmation: *"Beagle wants
to be installed and is asking to [do X]. Approve? 👍"* — pointer in, manifest
read, human authorizes.

This is the general test behind "don't build a five-question wizard": most
questions in a bad wizard are data the command shouldn't be collecting at all.

## Plain language

When you *do* ask a real question, ask it in words anyone understands — name each
thing by what it means to the user, and explain the minimum.

- **Translate internal names.** `slug` → "codename". `capability`/`scope` →
  "permissions". `token` → "setup code".
- **Offer plain choices, not raw identifiers.** Ask "Should this bot be able to
  see who's on the server? (yes/no)" instead of listing
  `accounts.read`.
- **One short parenthetical for format, only when it prevents a mistake**
  (`(lowercase, no spaces)`) — never a spec dump.

Minimum-without-jargon is not terse-and-cryptic: aim for a friendly, one-line
question a first-timer gets immediately.

## Input model: accept args OR interview

Support both, in one command:

- **Inline args for power users.** `/install-project beagle https://beagle.io`
  runs with no further prompting if everything's valid.
- **Interview for what's missing or invalid.** `/install-project` with nothing
  (or a bad value) drops into a guided flow that asks only for the
  missing/invalid parameters.

Don't force experts through a five-turn wizard, and don't make newcomers
reverse-engineer the argument order. The interview is the *fallback*, not the
only path.

## The interview pattern

When a command needs to interview:

1. **Acknowledge in place, converse in DM.** If invoked in a shared channel
   (e.g. `#admins`), react on the trigger message (see [Status
   signaling](#status-signaling)) and run the actual Q&A in a **DM** with the
   invoker. The reaction is the channel's "on it — check your DMs" signal.
   Interview content (and any secrets) stays out of shared history. If invoked
   in a DM already, just continue there.
2. **One question per turn, and number them** (`2/4 — ...`) so the user knows
   how far along they are.
3. **Ask in plain words, with the default shown** (see [Plain
   language](#plain-language)):
   `1/3 — What should we call this bot? (people will see this, e.g. "Beagle")`,
   `2/3 — Web address people open to use it?`.
   Ask only what carries weight and can't be derived — take the codename from
   the name, don't ask for it separately.
4. **Validate every answer; on bad input, don't advance.** Echo what they typed,
   say what's wrong in plain words, and invite retry or `/cancel`:
   `That doesn't look like a web address — it should start with https://. Try again, or /cancel.`
5. **End with review-and-edit, then confirm.** Show every answer with plain
   labels, let the user fix any one before committing (`to change it, reply
   "web address https://..."`), then commit. This covers ~all correction needs
   without per-step "back" machinery.
6. **`/cancel` is always available** and clears the trigger reaction.
7. **Expire idle interviews** (~10 min): mark the trigger failed/cancelled,
   tell the user it timed out, and drop the state. Interview state is
   ephemeral/in-memory — a bot restart dropping it is fine (the user re-runs).
8. **One active interview per user.** Re-invoking supersedes the previous one
   (re-acknowledge on the new trigger). Route the user's subsequent DM lines to
   their active interview, not the command dispatcher.

## Confirmations

Confirm before committing anything with side effects.

- **Preferred: reaction-confirm.** The bot posts the review summary and the user
  reacts 👍 to confirm. Fewest keystrokes, and the confirmation is legible in
  history. *Requires the bot to receive inbound reaction events* — if it can't
  (see [Capability awareness](#capability-awareness)), fall back to a **typed
  confirm** (`reply "yes"`).
- **Destructive/irreversible ops** (uninstall, kick, revoke, delete) always
  confirm, name the exact target, and use a short TTL:
  `React 👍 within 60s to remove "beagle".` (Convention from `docs/22`.)
- Never treat an ambiguous reply as consent. Anything that isn't the explicit
  confirm re-prompts or cancels — it never commits.

## Status signaling

Give continuous, low-noise feedback. Two tools, used together where available:

**Reaction palette on the trigger message** (send-only; widely available):

| Emoji | Meaning |
|-------|---------|
| 👀 | Working / interview in progress (set on start; the terminal state replaces it) |
| ✅ | Done / succeeded |
| ❌ | Failed, cancelled, or timed out |
| 👍 | *User's* confirm on a bot prompt (see [Confirmations](#confirmations)) |

A fresh reaction from the same reactor replaces the prior one (`docs/33`), so
👀 → ✅ is just a second reaction — no removal needed.

**Edit a status message in place** rather than spamming new lines, where
message-edit is supported: `installing…` → `Installed "beagle".` When edit
isn't available, send **one concise** terminal message instead of a running
commentary.

## Secrets

Tokens, keys, and other credentials:

- **DM them to the requester only.** Never post a secret into a shared channel —
  it lives in group history for every current and future member. (Running the
  interview in DM means secrets naturally land there.)
- **Label them** and say how to revoke: `bootstrap token (sensitive — don't
  share; rotate REGISTRATION_SHARED_SECRET to revoke): …`.
- **Don't echo secrets back** in confirmations or logs.

## Discoverability & help

- `/help` lists every command with a one-line summary; keep summaries verb-first
  and note admin-only gating.
- An unknown command replies with a hint to `/help`, never silence.
- Command names are **verbs** (`/install-project`, not `/project`).
- If a command is gated (e.g. `#admins` membership), check the gate *before*
  starting any interview and say plainly who may run it.

## Errors & idempotency

- Surface the underlying error message (e.g. the server's) — don't swallow it
  into a generic "failed".
- Treat benign conflicts as non-fatal where it makes sense: an "already exists"
  on create can continue to the next step rather than aborting.
- Partial success is reportable: `Granted: X. Failed to grant: Y (reason).`

## Output hygiene

- Short messages. Group long/rare output (a token, a list) into its own DM
  rather than a wall of text in a channel.
- Prefer editing a status line over posting each step, where supported.

## Capability awareness

Not every client/bot stack supports every technique. Detect or know the target
platform's capabilities and degrade gracefully:

| Technique | Needs | Fallback if unavailable |
|-----------|-------|-------------------------|
| React on trigger | send-reaction | Post a brief "on it — check your DMs" line |
| Reaction-confirm | *inbound* reaction events | Typed `yes` confirm |
| Edit-in-place status | message edit | One concise terminal message |

**Current @theavalanche/app-core Node bots (as of this writing):** can *send*
reactions (`sendReaction`) and DMs, but the Node wrapper does **not** surface
inbound reaction/edit/delete events and exposes no `sendEdit`. So for Node bots
today, use: 👀→✅/❌ reactions on the trigger, DM Q&A, **typed-yes** confirm, and
concise new messages instead of edit-in-place. Wiring inbound reaction events +
`sendEdit` through napi/TS would unlock reaction-confirm and edit-in-place — do
that as a deliberate, separate change if a flow needs them, not implicitly.

## Implementation notes (@theavalanche/app-core bots)

- **Interview state:** a `Map` keyed by the interviewee's DID holding the current
  step + collected answers + the trigger's `{target, author, sentAt}`. Intercept
  a user's DM lines in the event loop and route to their active interview before
  the normal command dispatch.
- **React to a received message:** `core.sendReaction(target, msg.senderDid,
  msg.sentAt, emoji, false)`. `sentAt` is the message's wire identity; skip the
  reaction gracefully if it's absent (legacy messages).
- **Trigger target:** the `SendTarget` the command arrived on — the group for an
  in-channel invoke, the DM peer for a DM invoke.
- **Gating:** for privileged commands, verify authorization against the E2E
  source of truth (e.g. `#admins` membership via `fetchGroupState`) — the server
  trusts the bot's session unconditionally (the superuser pin), so the check
  must live in the bot.

## Checklist

Before shipping a bot command:

- [ ] Every question & message in plain language — no jargon, minimal explanation
- [ ] Runs from inline args when complete; interviews for what's missing/invalid
- [ ] Interview: DM Q&A, numbered questions, defaults + rules shown, per-answer
      validation, review-and-edit, `/cancel`, idle timeout
- [ ] Confirms before side effects (reaction-confirm where available, else typed;
      destructive ops name the target + TTL)
- [ ] Status signaled (👀→✅/❌ on the trigger; edit-in-place or one terminal msg)
- [ ] Secrets DM'd, labeled, revocation noted; never in shared channels
- [ ] Listed in `/help`; gating checked up front and explained
- [ ] Errors surfaced verbatim; benign conflicts non-fatal
- [ ] Degrades gracefully to the target platform's capabilities
