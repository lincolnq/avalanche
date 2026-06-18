# Avalanche deploy bundle

The release-owned deploy/upgrade machinery. This directory is packaged by
`.github/workflows/release.yml` into the arch-independent
`av-deploy-<tag>.tar.gz` release asset, and unpacked into each host's
`/opt/avalanche/deployments/<tag>/deploy/`.

Design and rationale: **`docs/42-server-upgrades.md`**.

## Contents

| Path | Purpose |
|---|---|
| `install.sh` | First-time provision; called by cloud-init. |
| `update.sh` | In-place upgrade to a tag (`avalanche-update [TAG]`). |
| `lib/common.sh` | Shared helpers: arch detection, fetch, reconcile, per-bot env. |
| `systemd/*.service` | Unit files; `ExecStart` points at `deployments/current/`. |
| `bin/avalanche-status` | Health/inventory readout. |
| `bin/avalanche-backup` | Daily operational DB backup (cron). |
| `bin/avalanche-install-project <name>` | Add a Project to this host (dir + unit together). |
| `bin/avalanche-remove-project <name>` | Remove a Project from this host (dir + unit together). |
| `VERSION` | Stamped to the tag at release time (`0.0.0-dev` in-repo). |

## Model (summary)

- Each release installs into an immutable `/opt/avalanche/deployments/<tag>/`
  tree; `deployments/current` is a symlink, and switching versions is an atomic
  flip. `shared/` holds per-Project state and is never touched by the updater.
- `.env` files and the Caddyfile are written once at install and are
  operator-owned; the updater never rewrites them.
- An upgrade rebuilds the **same component set** already present in
  `current/`; on any mismatch between the on-disk set and the installed systemd
  units it **halts** rather than guessing.
- Forward path only in this cut (build → migrate → flip → restart → prune);
  pre-upgrade dumps, secret injection on upgrade, and rollback are deferred
  (see docs/42).
