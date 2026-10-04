# 42 — Server deployment and upgrades

> **Status:** Partial — provisioning and the forward upgrade path are built and shipping
> (`av-deploy` since the v0.5 releases); rollback, pre-upgrade dumps, and in-app upgrades are
> not built.
> **Last verified against code:** 2026-10-03

## Summary

A homeserver is provisioned from the configure page on the website and upgraded in place by
the **deploy bundle** (`infra/deploy/bundle/`), which ships inside every release. Each
release installs into its own immutable deployment directory, and switching versions is an
atomic flip of a `current` symlink — the model Zulip uses. Because the updater and systemd
units ship inside each release, they upgrade along with the binaries. `.env` files stay
operator-owned and are never rewritten. The push relay is deployed separately (docs/41).

## Current design

### Provisioning

The organizer tutorial and configure page live on the website:
<https://theavalanche.net/getting-started/organizer/>, sourced from
`web/layouts/_default/configure.html` and `web/assets/configure/*`. The page produces a
cloud-init (`web/assets/configure/cloudinit-template.yaml`) that installs prerequisites
(Node, Postgres, Caddy, qrencode, ufw, persistent journald), downloads
`av-deploy-<RELEASE_TAG>.tar.gz`, and hands off to its `install.sh` with the operator's
inputs (`SERVER_URL`, `SERVER_NAME`, `RELEASE_TAG`, `RELAY_URL`, Project opt-ins).

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
| `lib/common.sh` | Arch detection, fetch, reconcile, per-bot env. |
| `systemd/` | `avalanche.service`, `avalanche-adminbot.service`, `avalanche-testbot.service`. |
| `bin/avalanche-status` | Health and inventory readout. |
| `bin/avalanche-backup` | Daily DB backup, installed as a cron job (03:17). |
| `bin/avalanche-install-project`, `bin/avalanche-remove-project` | Add/remove a Project on this host (deployment dir + unit + Caddy route together). |

### Install / update contract

**`install.sh`** creates `/opt/avalanche/{deployments,shared}`, builds
`deployments/<RELEASE_TAG>/`, writes the env files and Caddyfile once (generating secrets
such as `REGISTRATION_SHARED_SECRET` and `ADMINBOT_DB_KEY`), installs the units, points
`current` at the tag, runs `migrate`, and starts services.

**`update.sh`** (`avalanche-update [TAG]`, default: latest GitHub release):

1. **Reconcile.** Compare the component subdirectories in `deployments/current/` with the
   installed `avalanche*.service` units. On any mismatch, **halt and print the diff** — never
   start or stop a service to reconcile.
2. **Build alongside.** Download `av-deploy`, `av-server` (if this host has `server/`), and
   the installed bots into a fresh `deployments/<TAG>/`; the live service keeps running. Then
   re-exec the *new* bundle's `update.sh` (self-updating updater).
3. **Migrate.** `deployments/<TAG>/server/avalanche-server migrate`.
4. **Flip.** Refresh units, `daemon-reload`, `ln -sfn <TAG> current`.
5. **Restart and verify.** Server first, then Projects; reload Caddy; health-check `/healthz`.
6. **Prune.** Keep the newest 3 deployments (`PRUNE_KEEP`); never touch `shared/`.

### Projects and the client directory

`avalanche-install-project` (and the configure page's testbot toggle) fully configures a web
Project: process, Caddy `/p/<slug>/*` route, and a manifest written to `$SHARED/manifests`.
Adminbot installs every manifest in `ADMINBOT_MANIFEST_DIR` non-interactively at startup,
which publishes the Network-tab directory entry and OAuth registration (docs/22, docs/25).
Removing a Project's directory entry is still a manual adminbot step.

### Environment files

`.env` files are operator-owned: written once at install, never rewritten, templated, or
merged by the updater. Application code defaults sensibly for new optional variables; a rare
new *required* variable is a manual step called out in release notes.

### Bots and Projects: uniform handling

The updater swaps the tree and restarts the service for every bot the same way. It never
backs up, rolls back, or reasons about a Project's local state. Each stateful Project keeps its
own local-store migrations backward-compatible.

## Known gaps

- **No rollback** and **no pre-upgrade DB dump**. A failed upgrade is recovered by hand
  (re-run against a prior tag; the retained deployments make this tractable).
- **No migration-compatibility (N-1) check in CI**, so nothing enforces that the previous
  binary can run against the new schema.
- **New secrets introduced by a release** must be added by hand.
- **The push relay is not part of the bundle** (docs/41).
- Upgrades are CLI-only (`ssh` + `avalanche-update`).

## Planned

- **Pre-upgrade DB dumps + retention:** `pg_dump` to `backups/` before `migrate`, pruned in
  lockstep with deployments. Its consumer is rollback, so it ships with rollback.
- **Rollback:** `avalanche-update --rollback` (or to a prior tag) re-points `current` at a
  retained deployment. Paired with backward-compatible (N-1) migrations, rollback is just the
  flip; for a non-reversible migration, restore the pre-upgrade dump.
- **`ensure-secret`:** append-only, generate-once injection of a secret a new release needs.
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
