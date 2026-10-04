# 24 — Vetted Onboarding (Gatekeeper)

> **Status:** Partial — the server side is built: closed registration (the default), the `registration.gatekeeper` capability with a pinned signing key, signed single-use invite tokens, and the operator bootstrap secret. The vetting Project itself (form, `#approvals`, review webview, delivery) and post-join routing are not built. The bootstrap path has a P0 escalation bug (`22`).
> **Last verified against code:** 2026-10-03

## Summary

A gatekeeper is a Project that decides who may create an account. The motivating one is human vetting: an applicant fills out a web form; approvers in an E2E `#approvals` group review it; on approval the applicant receives a single-use signed invite (by email or SMS) that lets them register. The homeserver admits **no one** without a valid token.

The design is shaped by one fact: **the applicant has no account and no DID until the end.** Every other Project authenticates an existing account. So the front half runs out-of-band (email/SMS), which doubles as a weak possession check, while approvers are ordinary users on the normal Project-token and visible-bot model.

## Current design

### Trust and gating model

**Built** (`core/crates/server/src/routes/registration.rs`, `gate_registration`; `core/crates/server/src/invite_token.rs`; `core/crates/server/src/config.rs`).

- **Closed registration is the default.** `REGISTRATION_MODE=open|closed`, and anything unrecognized means closed. In closed mode `POST /v1/accounts` is refused unless its `invite_token` admits it. **Fail-closed:** any validation failure rejects.
- **Two admitting credentials:**
  1. **A signed gatekeeper invite**, verified locally against the issuing Project's pinned Ed25519 key. The server never calls the Project.
  2. **The operator bootstrap secret** (`REGISTRATION_SHARED_SECRET`), honored only while no gatekeeper is installed. A bootstrap token may name a Project to link the new account into; naming `adminbot` is how superuser is bootstrapped (`22`).
- **Open mode** admits anyone, but a supplied token is still validated, and a bootstrap token still links into its named Project.
- **Many gatekeepers.** `registration.gatekeeper` is a per-Project capability any number of Projects may hold. Granting it requires and pins that Project's 32-byte Ed25519 public key (`routes/admin.rs`, `grant_capability`). Revoking it clears the key, fail-closed.
- **The token is the hand-off.** Admission (who may register) and routing (where they land) stay separate; the token carries the bridge, its issuer stamp plus a routing payload the gatekeeper controls.

### Token format

`base64url(JSON)` with single-character keys to keep QR codes small (`invite_token.rs`):

- **Envelope:** `{ s: server_url, i: issuer_slug, c: base64url(claims), g: base64url(sig) }`. `s` and `i` are untrusted hints for which server to call and which pinned key to check.
- **Signed claims:** `{ s: server_url, i: issuer_slug, e: exp_unix, j: jti, u: purpose, r?: routing }`. The signature covers the exact `c` string, so there is no JSON-canonicalization hazard. The server checks signature, issuer match, server URL, `purpose == "invite"`, and expiry.
- **Single use:** `jti` is inserted into `token_redemptions` before the account is created; a replay conflicts and is rejected. A token redeemed by a registration that later fails is still spent (fail-closed).
- **Bootstrap token:** `{ s: server_url, k: secret, p?: project_slug }`, unsigned; the secret is the credential.

The invite URL is the standard `https://go.theavalanche.net/i/<token>`, so app onboarding handles it unchanged (`51`).

### Post-join hand-off

**Partially built.** Every registration emits `AccountJoined` carrying the **raw** registration token to `accounts.read` holders, and logs it in `server_events` (`22` §Join event API). Nothing consumes the routing payload yet: adminbot invites every new human into every group it admins, regardless of token.

## Known gaps

1. **Bootstrap tokens escalate to superuser (P0).** Setup codes handed to Project operators are bootstrap tokens containing the master secret; rewriting `p` to `adminbot` grants superuser. Details in `22` §Known gaps.
2. **Raw tokens in join events (P0).** Bootstrap tokens, and so the master secret, are pushed to every `accounts.read` holder and kept for 30 days (`20` §Known gaps).
3. **`GET /v1/invites/<token>` doesn't validate gatekeeper tokens.** It decodes only `s` (and `d`, the inviter DID) and checks the server URL (`routes/invites.rs`). A forged, expired or spent invite looks valid until `POST /v1/accounts` rejects it, after the user has created a passkey and DID.
4. **Gatekeepers can't be installed through adminbot.** `/install-project` never supplies the signing key, so a `registration.gatekeeper` grant fails; the manifest-dir path skips it. Today it takes a direct admin API call.
5. **Bootstrap cliff.** Installing the first gatekeeper silently retires the bootstrap path, which also breaks setup codes and testbot registration (`22`).
6. **No gatekeeper Project exists.** Nothing in `node/packages/` mints signed invites.

## Planned

- **Per-Project bot-enrollment tokens** replace bootstrap setup codes (`22` §Planned). Same envelope and redemption table with `purpose: "bot"`, minted by the server rather than signed by a Project, and unable to name the superuser Project.
- **Join events carry parsed claims** (issuer, purpose, routing), never the raw token.
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
- **Many gatekeepers, not one.** Different invite flows (vetting, regional signup, event registration) are different issuers, each with its own key.
