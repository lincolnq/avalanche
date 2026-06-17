# Vetted Onboarding Project (Gatekeeper)

A first-party Project that gates account creation behind human vetting. An applicant fills out a web form; approvers in an end-to-end-encrypted approvals group review it; on approval the applicant receives a single-use signed invite (via email/SMS) that lets them register — and the homeserver admits **no one** without such a token.

This is the realization of something `51-invite-tokens.md` already anticipated: token signing, expiry, usage limits, and **closed registration are explicitly "server/Project concerns"** (`51-invite-tokens.md:65-69`). This Project is what plugs into that seam.

## The bootstrapping problem (why this Project is shaped oddly)

Every other Project authenticates a user who *already has an account* — the app mints a project token or a magic link (`20-project-security.md`). This one is different: **the applicant has no account, no DID, and possibly not even the app during vetting.** Identity is established at the *end* (signup), not the start.

Two consequences drive the whole design:

- **The front half runs out-of-band.** There is no in-app channel to a non-member, so the approval has to reach the applicant over email or SMS. That's not a wart — it's structural. The out-of-band hop is also a weak possession check (the applicant controls that inbox/number).
- **Approvers are normal users.** They *do* have accounts, so the review side uses ordinary project-token/webview auth and the visible-bot group model. Only the applicant side is special.

## Architecture

```
 Applicant (no account)        Vetting Project service          Homeserver
        │                              │                             │
        │  GET / (public form)         │                             │
        │─────────────────────────────▶│                             │
        │  POST /apply {fields, email} │  store application (own DB)  │
        │─────────────────────────────▶│                             │
        │                              │  bot posts to #approvals ───▶│ (E2E; server can't read)
        │                              │                             │
        │                   Approver opens magic link (project token) │
        │                              │◀────────────────────────────│
        │                              │  webview: full application   │
        │                              │  Approve / Decline           │
        │                              │                             │
        │   email/SMS: invite URL      │  mint signed single-use token│
        │◀═════════════════════════════│  (out-of-band delivery)     │
        │                              │                             │
        │  tap invite → app → register │                             │
        │──────────────────────────────────────────────────────────▶│ POST /v1/accounts
        │                              │   validate token ◀──────────│ (closed registration:
        │                              │   (gatekeeper)  ────────────▶│  no valid token, no account)
        │                              │                             │
        │   account created → join event → adminbot and/or the issuing │
        │                      gatekeeper route them into channels      │
```

## Trust and gating model

- **Closed registration.** The homeserver runs in a mode where `POST /v1/accounts` is refused unless it carries an `invite_token` that validates against an installed **gatekeeper**. Today registration is open (`51-invite-tokens.md:67`); closing it is the load-bearing server change this Project depends on. It must **fail closed**: if no gatekeeper vouches for the token, registration is rejected, never waved through.
- **Many gatekeepers, not one.** `registration.gatekeeper` is a per-Project capability that *any number* of Projects may hold — different invite flows (human vetting, regional signup, event registration) are different gatekeepers, each minting its own tokens. A registration succeeds if its token validates against **any** installed gatekeeper. So every token names its **issuer** (which gatekeeper minted it), and the server validates the signature against that issuer's pinned key.
- **Gatekeeper designation.** Granting `registration.gatekeeper` (via adminbot, like `subscribe.account_joined` — `22-adminbot.md:147-162`) registers that Project's token-signing public key with the server. The server keeps a set of `issuer → signing key` and verifies each token locally against its claimed issuer (preferred: no per-registration round-trip); delegating to the issuer's `GET /v1/invites/<token>` (`51-invite-tokens.md:29`) is the alternative. Multiple issuers make the pinned-key approach the natural fit.
- **The token is the hand-off.** Admission (*who may register*) and routing (*which channels they land in*) stay separate, and the **token carries the bridge between them**: its issuer stamp plus a routing payload the gatekeeper controls. Post-join routing is resolved from that token — see *Post-join hand-off* below — with no live call between bots.

## Components

### 1. Application form — Project-served, anonymous

Served by the Project over HTTPS on its own origin, **unauthenticated** (the applicant has nothing to authenticate with). Submissions are stored in the Project's own database.

We serve the form ourselves rather than ingesting an external form (Google/Typeform) deliberately: an external form routes applicant PII through a third-party processor the admin never vetted, undercutting the platform's server-seizure / minimize-what-anyone-learns posture. Keeping it in-Project keeps applicant data inside the admin's trust domain. (An external-source adapter can be added later behind the same ingestion interface.)

Because it's the one open endpoint, it's the main abuse surface:

- Rate-limit by IP; captcha or proof-of-work to blunt automated spam.
- Cap stored application size; treat every field as hostile input.
- Collect a **delivery handle** (email or phone) — load-bearing, since that's the only way approval can reach the applicant.

### 2. The `#approvals` group — modeled on `#admins`

A regular action-bound E2E group whose membership *is* the set of approvers, exactly like adminbot's `#admins` (`22-adminbot.md:6-8`). The server can't read the membership, so it has **no opinion on who may approve** — the bot mediates, because it's a member and can decrypt the roster.

The bot posts each new application as a message containing a **low-PII summary plus a magic link** to the full application. The full detail lives in the webview, not the message body — so applicant PII isn't sprayed across group history and backups. (`identity:magic-links` + a webview behind a project token; see `23-messaging-extensions.md`.)

### 3. Review and decision

- An approver taps the magic link → the Project webview opens (project-token authed; approvers have accounts) → shows the full application → **Approve / Decline**, with an optional reason.
- **Authority: any `#approvals` member may approve.** The bot verifies the decider's DID is in the group's current member list — the same authority check adminbot uses (`22-adminbot.md:72-76`).
- A ✅/❌ reaction on the bot's summary message is offered as a quick shortcut for the common case; the webview is the canonical, audit-friendly path (it captures who, when, and why). Decisions are recorded in the Project DB for accountability.
- Quorum / N-of-M is a deferred config knob; v1 admits on a single approval.

### 4. Approval → signed invite token

On approval the Project mints a **single-use, short-expiry, signed** invite token (signed with the Project's own secret per `51-invite-tokens.md:69`), stamped with the gatekeeper's **issuer id** and bound to the application id (and ideally the delivery handle). It's wrapped into the standard invite URL `https://go.theavalanche.net/invite/<token>` so the existing app onboarding handles it unchanged (`51-invite-tokens.md:3,24-41`). It also carries a **routing payload** the gatekeeper controls (e.g. `audience=northeast-volunteers`, `role=organizer`) that the post-join router reads.

### 5. Out-of-band delivery (email / SMS)

Either the approving admin can reach out and send the invite token to the recipient themselves, or the application process can automate it.

In the latter case, a delivery adapter sends the invite URL to the recipient and they can click/scan it from their Avalanche app.

### 6. Registration (closed)

The applicant taps the invite and the app runs the normal invite flow (`51-invite-tokens.md:4-8`): `GET /v1/invites/<token>` (server validates against the gatekeeper — signature, not expired, not already redeemed), then identity creation (passkey + DID genesis per `50-identity-auth-recovery.md`), then `POST /v1/accounts` with the token. The server, in closed-registration mode, admits only a valid token and marks it **redeemed** so the same link can't onboard a second person. An optional `server_step_url` (`51-invite-tokens.md:40`) could run a final in-app onboarding webview, though vetting is already complete.

### 7. Post-join hand-off

Once the account exists, the new member must land in the right channels — and *which* channels depends on **which gatekeeper approved them**. The hand-off rides the token plus adminbot's existing join-event fan-out, so no bot has to command another.

`AccountJoinedEvent` already delivers the registering token — including its issuer stamp and routing payload — to **any** bot holding `subscribe.account_joined` (`22-adminbot.md:173-199`). Whichever bot *owns the relevant channels* acts on it:

- **(a) Central routing (default).** adminbot reads the token, branches on the issuer + routing tags, and invites the member into the shared org channels via its existing rule config (`22-adminbot.md:413-432`). The gatekeeper expresses *intent* (tags); adminbot resolves intent → channels. Gatekeepers need no group membership and no channel knowledge. Best when channel routing is an org-wide admin policy.
- **(c) Self-routing gatekeeper.** A gatekeeper that owns its own channels *also* holds `subscribe.account_joined`, recognizes its own issuer stamp on the event, and invites the member into the channels it administers directly. It needs admin membership in those channels (a bot can only invite to groups it's in — `22-adminbot.md:82`). Best when a flow is an autonomous sub-Project with its own channels.

Both reuse the same primitive — the join event carrying the token — so they compose: adminbot handles the shared channels, a self-routing gatekeeper handles its own, each keyed off the issuer.

What we deliberately *don't* build is **(b) a gatekeeper → adminbot command** ("add this DID to channels A, B"). It would couple the two bots with a new RPC and split invite authority awkwardly — the gatekeeper decides, adminbot executes. The cleaner decomposition: if the gatekeeper knows the channels, let it invite directly (c); if it doesn't, let adminbot decide from the token's tags (a). The token, not a cross-bot call, is the hand-off.

## Scopes and permissions (against `20-project-security.md`)

- **Identity: `real-did`.** Bot-bearing toward approvers (the bot is a member of `#approvals`), so pseudonymous is incoherent — identity is real-DID per *Identity is derived from the scope set*. (Applicants have no DID until the very end, so there's nothing to pseudonymize there anyway.)
- **`identity:magic-links`** — the "review this application" link the bot posts is a magic link into the Project webview.
- **New privileged capability: `registration.gatekeeper`** — the authority to mint tokens the server accepts under closed registration. Held by *any number* of Projects (one per invite flow); granting it registers the Project's token-signing public key with the server. In the same family as `subscribe.account_joined` (`22-adminbot.md:147-162`). This is the genuinely new server-side hook this Project introduces.
- **`subscribe.account_joined`** — needed only by a *self-routing* gatekeeper (option (c) in *Post-join hand-off*) that invites its own members; central-routing gatekeepers leave routing to adminbot and don't need it.
- The bot's membership in `#approvals` is arranged by an admin adding it (group membership, not a manifest scope, per the `20` model).
- `profile:read` optional (to show approver names); not essential. `dm:initiate` not required for v1.

## Security considerations

- **The open form is the abuse surface.** Spam applications are the obvious attack. Rate-limit, captcha/PoW, dedupe by delivery handle, size-cap. It's the only unauthenticated endpoint — everything downstream is gated.
- **The token is a bearer admission credential sent over a non-E2E channel.** Risks: interception and forwarding (approved Alice forwards her link to Bob). Mitigations: single-use, short expiry, and binding to the delivery handle (require the same email/phone — or an embedded code — at signup). Absent strong binding, possession of the channel *is* the identity gate; state that assumption explicitly.
- **Applicant PII residency.** The application sits in the Project's DB — inside the admin's trust domain (good, and the reason we rejected the external-form option) but still a new PII store. Minimize fields, encrypt at rest, and set a retention policy that **purges declined and stale applications**.
- **PII in the approvals group.** It's E2E among approvers, but keep the full application behind the webview/project-token, not in the message body, so it doesn't persist in group history or per-member backups.
- **Single-approver trust.** Any-member approval means one rogue or compromised approver can admit anyone. That's the accepted trust model for v1; quorum is the mitigation if a deployment needs it.
- **Fail-closed is load-bearing.** If closed registration ever degrades to open — including when the gatekeeper is unreachable — the entire gate evaporates. The server must reject rather than admit on validation failure.
- **Auditability.** Record approver DID, decision, timestamp, and reason in the Project DB, mirroring adminbot's "commands are legible in group history" principle (`22-adminbot.md:391`).

## Deployment considerations

- Runs in the admin's trust domain on its own origin over HTTPS; the admin installs and configures it (the trust chain).
- A bot account registered on the homeserver (full Signal participant); the admin adds the bot to `#approvals`.
- **The server must be configured for closed registration with this Project as gatekeeper** — via the `registration.gatekeeper` capability (adminbot `/grant`) or a server config setting plus the Project's pinned token-signing public key.
- Email/SMS provider credentials (server-side secret).
- Own database for applications, token state (issued / redeemed / expired), and the decision audit log.
- A token-signing key (server-side secret); rotating it invalidates outstanding invites.

## Open questions

1. **Token validation mechanism.** Server pins the Project's public key and verifies locally (no per-registration round-trip, fail-closed by default) vs. server delegates to the Project's `GET /v1/invites/<token>` (live, but couples registration to Project uptime). Lean: pin the key.
2. **Token binding.** Bind to the delivery handle (re-present email/phone or a code at signup — resists link-forwarding, adds friction) vs. unbound single-use (possession suffices). 
3. **Approval policy.** Quorum / N-of-M (deferred; v1 is any-member single approval).
4. **Existing identities / second server.** Does an existing DID joining *this* server also go through vetting (`50-identity-auth-recovery.md` stories 2 and 3)? Vetting gates the *server*, not the identity, so probably yes — confirm.
5. **Routing ownership per gatekeeper.** Default is central routing — adminbot maps issuer + tags → shared channels (option (a)); a gatekeeper that owns its channels self-routes (option (c)). Which gatekeepers are autonomous vs. centrally-routed is a per-deployment choice. (Adminbot is not itself a gatekeeper unless it mints tokens.)
6. **Decline UX.** Re-application after decline, appeals, and whether declined applicants are notified or it's silent.
7. **Handle verification timing.** Verify the email/phone before approval (so approvers aren't reviewing a bogus contact) or after (simpler, but a typo'd handle wastes an approval).

## Assumptions audit

- **Closed registration is enforced at `POST /v1/accounts`** by validating `invite_token` against a gatekeeper. The server currently has *open* registration (`51-invite-tokens.md:67`); this enforcement is not yet built and is a prerequisite. *(Not verified against server code — design references the doc-level seam only.)*
- The existing invite-token + `GET /v1/invites` + `server_step` machinery (`51-invite-tokens.md:65-88`) is the right vehicle for delivering approval and registering.
- Approvers hold normal accounts, so webview/project-token auth and the visible-bot group model apply to the review side.
- Adminbot owns post-join channel routing (`22-adminbot.md` Future), so the vetting Project doesn't.
- The bot's `#approvals` membership is arranged by an admin, consistent with the `20` model where bot group membership isn't a manifest scope.
- `registration.gatekeeper` is a new capability not present in the current capability set (`22-adminbot.md:147-162` lists only `subscribe.account_joined` / `subscribe.account_left`).
</content>
</invoke>
