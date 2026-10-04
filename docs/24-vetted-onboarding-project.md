# 24 — Vetted Onboarding (Gatekeeper)

> **Status:** Partial — the server side is built: closed registration (the default), the `registration.gatekeeper` capability with a pinned signing key, signed single-use invite tokens, per-Project bot signup keys, and the two operator bootstrap secrets (branch `lincoln/bot-signup-keys` closes the S-01 escalation). The vetting Project itself (form, `#approvals`, review webview, delivery) and post-join routing are not built.
> **Last verified against code:** 2026-10-03

## Summary

A gatekeeper is a Project that decides who may create an account. The motivating one is human vetting: an applicant fills out a web form; approvers in an E2E `#approvals` group review it; on approval the applicant receives a single-use signed invite (by email or SMS) that lets them register. The homeserver admits **no one** without a valid token.

The design is shaped by one fact: **the applicant has no account and no DID until the end.** Every other Project authenticates an existing account. So the front half runs out-of-band (email/SMS), which doubles as a weak possession check, while approvers are ordinary users on the normal Project-token and visible-bot model.

## Current design

### Trust and gating model

**Built** (`core/crates/server/src/routes/registration.rs`, `gate_registration`; `core/crates/server/src/invite_token.rs`; `core/crates/server/src/config.rs`).

- **Closed registration is the default.** `REGISTRATION_MODE=open|closed`, and anything unrecognized means closed. In closed mode `POST /v1/accounts` is refused unless its `invite_token` admits it. **Fail-closed:** any validation failure rejects.
- **Three admitting credentials:**
  1. **A signed gatekeeper invite**, verified locally against the issuing Project's pinned Ed25519 key. The server never calls the Project.
  2. **A bootstrap token** carrying an operator secret. Two secrets are accepted:
     - **`REGISTRATION_SHARED_SECRET`** is shareable: the configure tool puts it in the first-members invite (`42`). It admits plain accounts only while no gatekeeper is installed, and **can never link a Project** (a token naming one gets a 403).
     - **`SUPERUSER_BOOTSTRAP_SECRET`** is never shared; the deploy generates it on the box. It always admits, and it is the only way to claim the superuser `adminbot` Project, once, while that Project has no linked account (`22` §Superuser authority).
  3. **A Project's bot signup key**, minted by the server (`POST /v1/admin/projects/{slug}/bot-signup-key`, adminbot only, never for the superuser Project). It admits **bot accounts only** and links each to the Project that owns the key. One key per Project, reusable; minting again revokes the old one. Stored as a SHA-256 hash in `project_bot_signup_keys` (migration 026), so the key can't choose which Project it links into. It keeps working after a gatekeeper is installed.
- **Open mode** admits anyone, but a supplied token is still validated and links as above.
- **Many gatekeepers.** `registration.gatekeeper` is a per-Project capability any number of Projects may hold. Granting it requires and pins that Project's 32-byte Ed25519 public key (`routes/admin.rs`, `grant_capability`). Revoking it clears the key, fail-closed.
- **The token is the hand-off.** Admission (who may register) and routing (where they land) stay separate; the token carries the bridge, its issuer stamp plus a routing payload the gatekeeper controls.

### Token format

`base64url(JSON)` with single-character keys to keep QR codes small (`invite_token.rs`):

- **Envelope:** `{ s: server_url, i: issuer_slug, c: base64url(claims), g: base64url(sig) }`. `s` and `i` are untrusted hints for which server to call and which pinned key to check.
- **Signed claims:** `{ s: server_url, i: issuer_slug, e: exp_unix, j: jti, u: purpose, r?: routing }`. The signature covers the exact `c` string, so there is no JSON-canonicalization hazard. The server checks signature, issuer match, server URL, `purpose == "invite"`, and expiry.
- **Single use:** `jti` is inserted into `token_redemptions` before the account is created; a replay conflicts and is rejected. A token redeemed by a registration that later fails is still spent (fail-closed).
- **Bootstrap token:** `{ s: server_url, k: secret, p?: project_slug }`, unsigned; the secret is the credential. `p` may only be `adminbot`, with the superuser secret.
- **Bot signup key:** `{ s: server_url, b: key }`, unsigned; the server looks up the key's hash.

The invite URL is the standard `https://go.theavalanche.net/i/<token>`, so app onboarding handles it unchanged (`51`).

### Post-join hand-off

**Partially built.** Every registration emits `AccountJoined { did, joined_at_ms }` to `accounts.read` holders and logs it in `server_events` (`22` §Join event API). The event no longer carries the raw registration token (it could contain a secret; S-01), so token-based routing waits on parsed claims in the event (Planned). Today adminbot invites every new human into every group it admins, regardless of token (`22` §Known gaps, S-29).

## Known gaps

1. **`GET /v1/invites/<token>` doesn't validate gatekeeper tokens.** It decodes only `s` (and `d`, the inviter DID) and checks the server URL (`routes/invites.rs`). A forged, expired or spent invite looks valid until `POST /v1/accounts` rejects it, after the user has created a passkey and DID.
2. **Gatekeepers can't be installed through adminbot.** `/install-project` never supplies the signing key, so a `registration.gatekeeper` grant fails; the manifest-dir path skips it. Today it takes a direct admin API call.
3. **The shared secret retires when a gatekeeper is installed.** That is intended for human signups. Project bots are unaffected (bot signup keys), and adminbot's claim uses the superuser secret, which is unaffected too.
4. **No gatekeeper Project exists.** Nothing in `node/packages/` mints signed invites.

## Planned

- **Join events carry parsed claims** (issuer, purpose, routing) so post-join routing can work without raw tokens.
- **Validate fully in `GET /v1/invites/<token>`** (signature, expiry, redemption) so the app fails before identity creation, not after. The response shape is unchanged; only error behavior tightens.
- **Gatekeeper install via adminbot:** the manifest declares the signing public key, shown to the admin for confirmation.
- **The vetting Project**, as designed below.

### The vetting Project (design)

1. **Application form, Project-served and anonymous.** HTTPS on the Project's own origin, unauthenticated, stored in the Project's own database. It's the one open endpoint and the main abuse surface: rate-limit by IP, captcha or proof-of-work, size caps, every field treated as hostile. It collects a delivery handle (email or phone), the only way approval can reach the applicant.
2. **`#approvals`, modeled on `#admins`.** An ordinary E2E group whose membership is the approver set; the server has no opinion on who may approve. The bot posts each application as a low-PII summary plus a link to the full application, so PII stays out of group history and per-member backups.
3. **Review.** An approver opens the link (Project token; approvers have accounts), reads the full application, and approves or declines with an optional reason. Any `#approvals` member may decide; the bot checks the decider's DID against freshly fetched group state. Decisions are recorded in the Project database (who, when, why).
4. **Approval → signed invite.** Single-use, short expiry, the Project's issuer slug, bound to the application id and ideally the delivery handle, plus a routing payload (`audience=northeast-volunteers`).
5. **Out-of-band delivery** by email or SMS, or handed over by the approving admin.
6. **Registration** through the normal invite flow (`51`, `50`).
7. **Post-join routing**, either central (adminbot maps issuer + tags to shared channels) or self-routing (a gatekeeper with `accounts.read` invites people into channels it administers). Both key off the same join event, so they compose.

Scopes: real-DID (bot-bearing toward approvers); `registration.gatekeeper`; `accounts.read` only if self-routing.

### Security considerations for the vetting Project

- **The open form is the abuse surface** (rate limits, captcha/PoW, dedupe by handle, size caps).
- **The token is a bearer admission credential over a non-E2E channel.** Approved Alice can forward her link to Bob. Mitigations: single use, short expiry, binding to the delivery handle. Without binding, possession of the channel is the identity gate; say so.
- **Applicant PII residency.** Minimize fields, encrypt at rest, purge declined and stale applications.
- **Single-approver trust.** One rogue approver can admit anyone; quorum is the knob if a deployment needs it.
- **Fail-closed is load-bearing.** If closed registration ever degrades to open, including when a gatekeeper is unreachable, the gate evaporates.

## Speculative

- **Quorum / N-of-M approval.**
- **Token binding** to the delivery handle (re-present the email/phone or an embedded code at signup).
- **External form adapters** behind the same ingestion interface (see the rejection below; only if PII flows through an operator-vetted processor).
- **Existing identities joining this server** go through the same vetting: vetting gates the server, not the identity.

## Rationale and rejected alternatives

- **Pin the gatekeeper's key; don't call it.** Local verification has no per-registration round-trip and fails closed. Delegating to the Project's own validation endpoint would couple registration to Project uptime.
- **Rejected: an external form (Google Forms, Typeform).** It routes applicant PII through a processor the admin never vetted, against the minimize-what-anyone-learns posture.
- **Rejected: a gatekeeper → adminbot command** ("add this DID to channels A, B"). It couples two bots with a new RPC and splits invite authority. If the gatekeeper knows the channels it invites directly; if not, adminbot decides from the token's tags. The token is the hand-off.
- **Split operator secrets (S-01).** The first-members invite from the configure tool is shared by design, so its secret may only admit signups. Claiming superuser needs a separate secret that never leaves the server box, and works once.
- **Bot signup keys are server-minted, per-Project, and reusable.** Server-minted so no Project signing key or shared secret is involved; resolved by hash so a key can't pick its Project; reusable because testbot-style Projects create a bot per user. A leaked key only adds bots to its own Project, and re-minting revokes it. Rejected: single-use bot tokens (one operator round-trip per bot) and reusing the gatekeeper envelope with `purpose: "bot"` (it needs a Project signing key most Projects don't have).
- **Many gatekeepers, not one.** Different invite flows (vetting, regional signup, event registration) are different issuers, each with its own key.
