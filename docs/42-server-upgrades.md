# 42 — Server deployment and upgrades

> **Status:** Partial — provisioning and the forward upgrade path are built and shipping
> (`av-deploy` since the v0.5 releases); rollback, pre-upgrade dumps, and in-app upgrades are
> not built.
> **Last verified against code:** 2026-10-03

## Summary

**Setup starts at the configure tool, and it has to stay easy.** A big part of what Avalanche
offers is that a non-technical organizer can run their org's homeserver. The website's "Set up
your homeserver" page (<https://theavalanche.net/configure/>, sourced from
`web/layouts/_default/configure.html` and `web/assets/configure/`) asks for a server URL and a
name and generates two things:

- **A cloud-init** to paste into DigitalOcean's "user data" field. It fetches the deploy bundle
  and runs `install.sh`, which installs and configures everything with no further input.
- **A first-members invite** (link and QR). The organizer scans it to sign up first, then
  shares it with their first members. It carries the shareable `REGISTRATION_SHARED_SECRET`,
  which only admits signups: it can't link a Project or grant admin (`24`, `09` S-01).

What is generated where:

| Secret | Where it's generated | Where it lives |
|---|---|---|
| `REGISTRATION_SHARED_SECRET` | In the browser, by the configure tool | Server env and the invite link (shareable by design) |
| `SUPERUSER_BOOTSTRAP_SECRET` | On the box, by `install.sh` / `update.sh` | Server env and adminbot env only; never in the browser, cloud-init, or any invite |
| `ADMINBOT_DB_KEY` | On the box | adminbot env |
| Per-Project bot signup keys | By the server, minted through adminbot | `$SHARED/bot-signup-keys/<slug>.key` for first-party Projects; DM'd to the operator for others |

Any change to server config, env vars, or the deploy must keep this a one-paste setup
(root `CLAUDE.md`). Prefer generating secrets on the box, and make `install.sh` and
`update.sh` carry existing servers forward automatically (`migrate_env_files`).

**Operator commands** on the box: `avalanche-status`, `avalanche-update [TAG]`,
`avalanche-backup`, `avalanche-install-project` / `avalanche-remove-project`, and
`avalanche-reset-adminbot` (recover adminbot after its state is lost, `22`).

A homeserver is provisioned from the configure tool and upgraded in place by
the **deploy bundle** (`infra/deploy/bundle/`), which ships inside every release. Each
release installs into its own immutable deployment directory, and switching versions is an
atomic flip of a `current` symlink — the model Zulip uses. Because the updater and systemd
units ship inside each release, they upgrade along with the binaries. `.env` files stay
operator-owned; the deploy only adds missing lines or retires a secret a component no longer
needs (*Environment files*). The push relay is deployed separately (docs/41).

## Current design

### Provisioning

The organizer tutorial and configure page live on the website:
<https://theavalanche.net/getting-started/organizer/>, sourced from
`web/layouts/_default/configure.html` and `web/assets/configure/*`. The page produces a
cloud-init (`web/assets/configure/cloudinit-template.yaml`) that installs prerequisites
(Node, Postgres, Caddy, qrencode, ufw, persistent journald), downloads
`av-deploy-<RELEASE_TAG>.tar.gz`, and hands off to its `install.sh` with the operator's
inputs (`SERVER_URL`, `SERVER_NAME`, `RELEASE_TAG`, `RELAY_URL`, `REGISTRATION_SHARED_SECRET`,
the invite URL, Project opt-ins).

### What an upgrade changes

1. **Server binary** (`avalanche-server`), from `av-server-<target>.tar.gz`, gated by a DB
   migration.
2. **Bot/Project bundles** (adminbot, testbot, …), each `av-<name>-<target>.tar.gz`: a Node
   tree the updater swaps and restarts identically, with no per-Project knowledge.
3. **DB schema** — `avalanche-server migrate`, forward-only, run only by the updater (never on
   service start).
4. **Release-owned glue** — systemd units and the updater itself.

All first-party artifacts share **one git tag**, so "upgrade" means "move the whole stack to
tag *T*". Components ship as separate per-arch artifacts, so a Project may run on its own host
pointing at a remote server (recommended for sensitive bots like adminbot, docs/22); every
host runs the same install/update machinery over whatever it hosts.

### On-disk layout

```
/opt/avalanche/
  deployments/
    <tag>/                     # one complete, immutable release tree
      server/avalanche-server
      adminbot/                # if installed
      testbot/                 # if installed
      deploy/                  # the av-deploy bundle for this tag
      VERSION
    current -> <tag>           # the single atomic switch
  shared/                      # per-Project local state; the updater never touches it
    adminbot-state/
    manifests/                 # first-party Project manifests adminbot installs at startup
    bot-signup-keys/           # <slug>.key, written by adminbot, read by first-party bots (0700)
```

Operator-owned config lives outside the trees: `/etc/avalanche/*.env` and
`/etc/caddy/Caddyfile`, written once by `install.sh`. Unit files (refreshed from
`current/deploy/systemd` on each upgrade) point at `current/`.

### The deploy bundle

`av-deploy-<tag>.tar.gz` is arch-independent (scripts only), built by the `bundle` job in
`.github/workflows/release.yml`:

| Path | Purpose |
|---|---|
| `install.sh` | First-time provision (called by cloud-init); idempotent. |
| `update.sh` | `avalanche-update [TAG]` — in-place upgrade. |
| `lib/common.sh` | Arch detection, fetch, reconcile, per-bot env, `migrate_env_files`. |
| `systemd/` | `avalanche.service`, `avalanche-adminbot.service`, `avalanche-testbot.service`. |
| `bin/avalanche-status` | Health and inventory readout. |
| `bin/avalanche-backup` | Daily DB backup, installed as a cron job (03:17). |
| `bin/avalanche-install-project`, `bin/avalanche-remove-project` | Add/remove a Project on this host (deployment dir + unit + Caddy route together). |
| `bin/avalanche-reset-adminbot` | Recover adminbot after its state is lost: stop it, run `avalanche-server reset-adminbot` (delete the old adminbot account, clear the one-time superuser claim), move the old state aside, restart (`22`). |

### Install / update contract

**`install.sh`** creates `/opt/avalanche/{deployments,shared}`, builds
`deployments/<RELEASE_TAG>/`, writes the env files and Caddyfile once (using the configure
tool's `REGISTRATION_SHARED_SECRET`, generating `ADMINBOT_DB_KEY`), runs `migrate_env_files`
(which generates `SUPERUSER_BOOTSTRAP_SECRET` on the box and writes it to the server and
adminbot env), installs the units, points `current` at the tag, runs `migrate`, and starts
services.

**`update.sh`** (`avalanche-update [TAG]`, default: latest GitHub release):

1. **Reconcile.** Compare the component subdirectories in `deployments/current/` with the
   installed `avalanche*.service` units. On any mismatch, **halt and print the diff** — never
   start or stop a service to reconcile.
2. **Build alongside.** Download `av-deploy`, `av-server` (if this host has `server/`), and
   the installed bots into a fresh `deployments/<TAG>/`; the live service keeps running. Then
   re-exec the *new* bundle's `update.sh` (self-updating updater).
3. **Migrate.** `deployments/<TAG>/server/avalanche-server migrate`.
4. **Flip.** Run `migrate_env_files`, refresh units, `daemon-reload`, `ln -sfn <TAG> current`.
5. **Restart and verify.** Server first, then Projects; reload Caddy; health-check `/healthz`.
6. **Prune.** Keep the newest 3 deployments (`PRUNE_KEEP`); never touch `shared/`.

### Projects and the client directory

`avalanche-install-project` (and the configure page's testbot toggle) fully configures a web
Project: process, Caddy `/p/<slug>/*` route, and a manifest written to `$SHARED/manifests`.
Adminbot installs every manifest in `ADMINBOT_MANIFEST_DIR` non-interactively at startup,
which publishes the Network-tab directory entry and OAuth registration (docs/22, docs/25).
Removing a Project's directory entry is still a manual adminbot step.

### Environment files

`.env` files are operator-owned: written once at install and never templated or rewritten
wholesale. The one exception is `migrate_env_files` (`lib/common.sh`), run by both
`install.sh` and the updater, which may only **append a missing line** or **drop a retired
secret**. Today it ensures `SUPERUSER_BOOTSTRAP_SECRET` in `avalanche.env` and `adminbot.env`
(reusing an existing value so both agree), adds `ADMINBOT_BOT_SIGNUP_KEY_DIR` and
`TESTBOT_BOT_SIGNUP_KEY_FILE`, removes `REGISTRATION_SHARED_SECRET` from `adminbot.env` and
`testbot.env` (`09` S-01), and creates `$SHARED/bot-signup-keys`. Application code defaults
sensibly for new optional variables; a new value that must be set goes through
`migrate_env_files` rather than a manual step, so the configure tool stays one-paste.

### Bots and Projects: uniform handling

The updater swaps the tree and restarts the service for every bot the same way. It never
backs up, rolls back, or reasons about a Project's local state. Each stateful Project keeps its
own local-store migrations backward-compatible.

## Known gaps

- **No rollback** and **no pre-upgrade DB dump**. A failed upgrade is recovered by hand
  (re-run against a prior tag; the retained deployments make this tractable).
- **No migration-compatibility (N-1) check in CI**, so nothing enforces that the previous
  binary can run against the new schema.
- **New secrets introduced by a release** go through `migrate_env_files` (built for `SUPERUSER_BOOTSTRAP_SECRET`); there is not yet a general declarative mechanism.
- **The push relay is not part of the bundle** (docs/41).
- Upgrades are CLI-only (`ssh` + `avalanche-update`).

## Planned

- **Pre-upgrade DB dumps + retention:** `pg_dump` to `backups/` before `migrate`, pruned in
  lockstep with deployments. Its consumer is rollback, so it ships with rollback.
- **Rollback:** `avalanche-update --rollback` (or to a prior tag) re-points `current` at a
  retained deployment. Paired with backward-compatible (N-1) migrations, rollback is just the
  flip; for a non-reversible migration, restore the pre-upgrade dump.
- **Generalize `migrate_env_files`** into a declarative list of generated secrets (`ensure-secret`), so each release adds one line.
- **In-app upgrade:** an `#admins` `/upgrade [tag]` command via adminbot, which implies a
  `sudo`-gated wrapper adminbot can invoke.

## Rationale and rejected alternatives

- **Prior art: Zulip.** Each version installs under `deployments/<timestamp>/`; upgrades build
  alongside, migrate, atomically flip `current`, and restart; rollback flips back;
  backward-compatible migrations plus pre-upgrade backups cover the database. Battle-tested,
  and it works whether Postgres is local or remote.
- **Rejected: per-file binary swap (`.new`/`.old`).** Doesn't generalize to bot trees, can't
  switch the whole stack atomically, and makes rollback a per-component dance.
- **Rejected (and replaced): an updater baked into cloud-init.** The original
  `/usr/local/sbin/avalanche-update` lived in the cloud-init, took a bare binary URL, was
  server-only, and could never update itself, the units, or the Caddyfile — every box was
  frozen at provision-time logic. That drift is what the in-release bundle eliminates.
- **Rejected: `avalanche-server self-update`.** The server would have to manage other
  processes and its own unit; a server that fails to start can't update itself.
- **Rejected: per-Project upgrade/rollback logic.** Doesn't scale with the number of Projects
  and reintroduces drift. (An adminbot exception is deliberately deferred.)
- **Rejected: independent per-component versions.** Multiplies the compatibility matrix for no
  benefit; same tag means compatible by construction.
- **Rejected: auto-update.** Surprising and risky for a migration-bearing service an operator
  is responsible for.
