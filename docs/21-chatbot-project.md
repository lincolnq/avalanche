# 21 — Testbot (reference Project)

> **Status:** Built — a dev and demo Project: a web page with a "Text Me" button that spawns an ephemeral AI chatbot, plus a "Sign in with Avalanche" demo.
> **Last verified against code:** 2026-10-03

## Summary

Testbot is the first Project on the platform and the only complete example of one. It is a single TypeScript service, `node/packages/testbot/src/index.ts`, built on `@theavalanche/app-core` (napi). It demonstrates both halves of the Project model (`20-project-security.md`): a webview UI authenticated with a Project token, and bot accounts that talk to users over E2E DMs. It also hosts the OAuth login demo for `25-project-login.md`.

It is a dev tool, not a pattern to copy for production Projects (see *Known gaps*).

## Current design

**Process.** One Node service using `node:http` with no framework. Config comes from the environment (and the repo-root `.env` in dev): `HOMESERVER_URL`, `TESTBOT_BIND_ADDR` (default `0.0.0.0:3001`), `TESTBOT_BASE_PATH` (deploy: `/p/testbot/` behind Caddy), `TESTBOT_PUBLIC_URL`, `ANTHROPIC_API_KEY` / `ANTHROPIC_AUTH_TOKEN`, and `REGISTRATION_SHARED_SECRET`. The deploy bundle installs it as a web Project with a manifest (`infra/deploy/bundle/lib/common.sh`).

**HTTP surface.**

| Route | Auth | Purpose |
|---|---|---|
| `GET /` | none (page reads `?token=`) | The "Text Me" page |
| `POST /api/text-me` | Project token (Bearer) | Spawn a bot that DMs the caller |
| `GET /api/bots` | Project token | List the caller's live bots |
| `GET /login` | none | OAuth login demo page (`25`) |
| `POST /api/oauth/exchange` | none | Auth-code + PKCE exchange (same-device flow) |
| `POST /api/oauth/device/start`, `/poll` | none | Device-grant flow (phone authorizes a desktop browser) |

**Auth.** Every token is checked with `GET /v1/project-token/verify`; the returned DID is the caller (`verifyProjectToken`). OAuth access tokens are Project tokens, so the same check serves the login demo.

**Bot lifecycle.** Each "Text Me" tap registers a **new** bot account (`AppCore.createBotAccount`, display name "Testbot", a throwaway SQLCipher store in the OS temp dir) and sends an opening DM. A per-bot `for await (core.events())` loop handles each inbound DM: a short pause, a read receipt, a thumbs-up reaction (exercising `33`), then a reply. All bot state is in memory; bots die when the process restarts, and their server accounts are orphaned.

**Replies.** Claude Haiku (`claude-haiku-4-5-20251001`) via the Anthropic API, with the conversation history and the user's display name in the system prompt. With no API key, or on an API error, the bot echoes the user's message, so local dev needs no setup.

**Registration.** On a closed-registration server, bots register with a bootstrap token built from `REGISTRATION_SHARED_SECRET` (no Project link). adminbot suppresses join announcements for display name "Testbot" so `#admins` isn't flooded (`22`).

## Known gaps

- **Holds the master registration secret.** Testbot is internet-facing and LLM-driven, yet holds `REGISTRATION_SHARED_SECRET`. That secret also lets anyone who holds it register into the superuser Project (`22` §Known gaps). Worse, every bot it registers publishes the raw bootstrap token, secret included, to every `accounts.read` holder and to `server_events` for 30 days (`20` §Known gaps).
- **Stops working once a gatekeeper is installed.** The bootstrap secret is retired as soon as any `registration.gatekeeper` Project exists (`registration.rs`, `gate_registration`), so testbot bots can no longer register on such a server.
- **Doesn't check token audience.** It ignores the `project_url` returned by `verify`, so it accepts tokens minted for other Projects (`20` §Known gaps).
- **Unbounded account creation.** Each tap creates a permanent server account. Nothing caps taps per user.
- **Shares the homeserver origin** under `/p/testbot/` in the deploy bundle, so it has no origin isolation from other `/p/` Projects.

## Planned

- Register bots with a per-Project enrollment token (`22` §Planned) instead of the master secret, and link them to the testbot Project.
- Check the `verify` response's `project_url` (and pass `audience` once the server supports it).
- Split the OAuth demo from the chatbot so the example Projects stay small and copyable.

## Rationale

- **TypeScript on napi, not Rust.** The original testbot was a Rust binary; it was ported so the first Project demonstrates the "any language on app-core" story and exercises the same Node bindings third-party bots use. Node's single-threaded event loop also removes the dedicated-thread workaround the Rust version needed for libsignal's non-`Send` futures.
- **Ephemeral bots.** It is a dev tool for fake conversations; persistence would only add cleanup work.
