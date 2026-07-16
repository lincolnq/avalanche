# Server upgrades — in-place upgrades for deployed homeservers

How an already-deployed homeserver moves from one release to the next without a
re-provision. The model: install each release into its own immutable
**deployment directory** and switch versions by atomically flipping a `current`
symlink — the same pattern Zulip uses (see *Prior art*). The release also ships
its own deploy/upgrade machinery (a "deploy bundle"), so the updater and systemd
units update along with the binaries instead of being frozen at provision time.
The `.env` files stay operator-owned — written once and never rewritten by the
updater (see *Environment files*).

Status: the deploy bundle is implemented in `infra/deploy/bundle/`
(`install.sh`, `update.sh`, `lib/common.sh`, `systemd/`, `bin/`), wired into
`release.yml` (the `av-deploy-<tag>.tar.gz` asset) and the configure-page
cloud-init — **pending a release that ships `av-deploy` and a real-droplet
validation pass**. The **first cut** implements the forward path only —
build → `migrate` → flip → restart → prune; pre-upgrade DB dumps, on-upgrade
secret injection, and rollback are designed but **deferred** (see *Deferred*).

## What an upgrade has to change

The default simple deployment has four upgradeable things on the filesystem, plus state that must explicitly *not* move:

1. **Server binary** (`avalanche-server`) — gated by a DB migration. Shipped as
   `av-server-<target>.tar.gz`.
2. **Bot/Project bundles** — each first-party Project (adminbot, testbot, and
   more to come) ships as `av-<name>-<target>.tar.gz`: a Node application (in the form of a filesystem tree) that the updater
   swaps and restarts **identically for every one**. The updater has no
   per-Project knowledge.
3. **Server DB schema** — `avalanche-server migrate`, forward-only.
4. **Release-owned glue** — the systemd units and the updater itself.

Co-locating all of this on one box is the simple default the configure page sets
up — **not an assumption**. Each component ships as its own per-arch artifact
(`av-<name>-<target>.tar.gz`), so a Project can instead run on its own host
pointing at a remote server — the recommended posture for sensitive bots like
adminbot (`22-adminbot.md`). The deployment model is therefore **host-agnostic**:
every host (server or Project) has its own `deployments/<tag>/` + `current`
symlink and its own copy of `av-deploy`, and runs the identical install/update
machinery over whatever component(s) it hosts.

**Client directory (Network tab) note.** The install/update scripts still write
the `PROJECTS` env var (and Caddy routes) via `regenerate_projects` when a bot is
installed, but the server now treats `PROJECTS` only as a **one-time seed source**
for the DB-backed directory (`directory_entries`; `GET /v1/projects` reads the DB
— see `22-adminbot.md`). So an installed bot like testbot still appears in the
Network tab (its `PROJECTS` entry is seeded into the DB on first boot), with no
deploy change required. Reconciling the deploy scripts to drive the DB directory
directly (and retire the `PROJECTS`-env half of `regenerate_projects`) is a
tracked follow-up (`02-todos-deferred.md`).

Some Projects keep **local state** (adminbot has a SQLCipher `store.db` +
`state.json`; testbot is stateless). It lives under `shared/<project>/` so it
survives flips and pruning — but beyond keeping it out of the versioned tree,
**the updater does nothing special with Project state**: it never backs it up,
rolls it back, or reasons about a Project's schema. This is deliberate (see *Bots
and Projects: uniform handling*) — per-Project logic doesn't scale as the number
of first-party Projects grows.

## Current state and its gaps

The bootstrap writes `/usr/local/sbin/avalanche-update`, which has the right
bones — pre-migrate `pg_dump`, stop → migrate → swap → health-check → rollback.
But it is stale against the current release scheme:

- it takes a **bare binary URL**, not a release **tag** + per-arch **tarball**
  (`av-server-<target>.tar.gz`, see `.github/workflows/release.yml`);
- it is **server-only** — it does not touch adminbot/testbot;
- it lives **inside the cloud-init**, so it cannot update itself, the systemd
  units, or the Caddyfile.

All first-party artifacts share **one git tag** (the release), so "upgrade" =
"move the whole stack to tag *T*".

## Prior art: Zulip

Zulip runs this exact model and it's worth copying deliberately. Each version is
installed under `/home/zulip/deployments/<timestamp>/` (a complete tree), with
`deployments/current` a symlink to the active one. Upgrading builds the new tree
alongside the running one, runs Django migrations, then **atomically flips the
`current` symlink** and restarts services; old deployments are pruned on a
retention policy. Rollback flips the symlink back. The database is handled by
writing **backward-compatible migrations** (so the previous code can run against
the new schema — rollback is usually just the symlink flip) and taking a backup
before upgrades; a non-reversible migration is rolled back by **restoring from
that backup**. Disciplined migrations + atomic symlink swap + backups is the
battle-tested baseline, and it works whether Postgres is local or remote.

## On-disk layout

```
/opt/avalanche/
  deployments/
    <tag>/                          # one complete, immutable release tree
      server/avalanche-server       # from av-server-<target>.tar.gz
      adminbot/                     # from av-adminbot-<target>.tar.gz (if installed)
      testbot/                      # from av-testbot-<target>.tar.gz  (if installed)
      deploy/                       # from av-deploy-<tag>.tar.gz: install.sh, update.sh, units, templates
      VERSION                       # = <tag>
    current -> <tag>                # the single atomic switch
  shared/                           # per-Project local state — survives flips; the updater never touches it
    adminbot-state/                 #   e.g. adminbot's store.db + state.json (testbot keeps none)
```

Config the units reference lives outside the deployment trees and is
**operator-owned**: `/etc/avalanche/*.env` and `/etc/caddy/Caddyfile` are written
once at install and never rewritten by the updater (see *Environment files*).

systemd units (in `/etc/systemd/system`, refreshed from `current/deploy/systemd`
on upgrade) point at the symlink, so a flip + restart switches versions:

```
# avalanche.service
ExecStart=/opt/avalanche/deployments/current/server/avalanche-server
# avalanche-adminbot.service
ExecStart=/usr/bin/node /opt/avalanche/deployments/current/adminbot/node_modules/.bin/adminbot
Environment=ADMINBOT_STATE_DIR=/opt/avalanche/shared/adminbot-state   # state lives in shared/, not the deployment
```

## The deploy bundle

`av-deploy-<tag>.tar.gz` — arch-independent (scripts only, no compiled code),
unpacked into each deployment's `deploy/`:

```
deploy/
  install.sh                 # first-time provision (called by cloud-init)
  update.sh                  # build + activate this tag
  lib/common.sh              # arch detection, release base URL, helpers
  systemd/                   # the unit files (point at current/)
  bin/                       # avalanche-status, avalanche-backup → /usr/local
  VERSION
```

No env or Caddyfile templates — `install.sh` writes those once, directly from the
operator's inputs (see *Environment files*).

Because the deploy/upgrade logic and the unit files ship *inside each release*,
every upgrade re-lays them: a bug in the updater is fixed by the next release, and
a unit change rolls out like any other release. This kills the recurring "the
cloud-init and the updater drifted" class of bug — the bundle is the single
source of truth for release-owned glue.

## The install / update contract

`install.sh` (first boot, idempotent): create `/opt/avalanche/{deployments,shared}`,
build `deployments/<RELEASE_TAG>/` (download `av-deploy`, `av-server`, and the
selected Projects; unpack into the tree), **write `/etc/avalanche/*.env` and the
Caddyfile once** from the configure-page inputs (plain files, no templates),
generating the required secrets at this point, install the symlink-referencing
units, point `current` at the tag, `migrate`, enable + start services. Installs
whatever set of Projects the configure page selected — uniformly, with no
per-Project branches.

`update.sh` (operator-facing `avalanche-update [TAG]`): upgrade to *TAG* (default
= latest GitHub release, resolved with the same `/releases` lookup the configure
page uses — see `web/assets/configure/configure.js`). The running deployment is
untouched until the atomic flip:

1. **Resolve & build alongside.** Determine `TAG` and `TARGET` (arch). Build a
   fresh `deployments/<TAG>/` next to `current` — download `av-deploy-<TAG>`,
   `av-server-<TARGET>`, and the installed bots, unpack into the tree. The live
   service keeps running on `current` the whole time. Then re-exec
   `deployments/<TAG>/deploy/update.sh` so the *new* bundle's logic drives the
   rest (self-updating updater).
2. **Migrate.** Run `deployments/<TAG>/server/avalanche-server migrate`.
   Migrations run *only* here — never on service startup.
3. **Flip.** Refresh the systemd units from `deployments/<TAG>/deploy/systemd`;
   `systemctl daemon-reload`; then `ln -sfn <TAG> current` — the single atomic
   switch. The updater does **not** rewrite `/etc/avalanche/*.env` or the
   Caddyfile.
4. **Restart & verify.** Restart the server, then the Projects (`Restart=always`
   absorbs the brief skew); reload Caddy; health-check `/healthz`.
5. **Prune.** Keep the last *N* deployments; remove older trees. Never touch
   `shared/`. (Retaining prior deployments is what makes rollback cheap to add
   later — see *Deferred*.)

**Which components to act on — reconcile, don't guess.** Two facts describe a
host: the component subdirectories under `deployments/current/` (what's installed
on disk) and the `avalanche*.service` units (what's actually run). They map 1:1 —
`server/` ↔ `avalanche.service`, `<name>/` ↔ `avalanche-<name>.service` — and on a
healthy host they agree. The updater treats them as a **cross-check, not two
interchangeable sources**: at the start of every install/update it compares them
and, on any mismatch, **halts and prints the diff** rather than picking a winner.
It never starts or stops a service to reconcile.

So an upgrade acts only on the set both agree on and rebuilds it at the new tag —
the right thing on any host, with no separate manifest to maintain:

- server-only host → updates `av-server` (and runs `migrate`);
- a Project host → updates just that `av-<name>`;
- a co-located box → updates each.

It **never implicitly starts a new service or stops an existing one** — it swaps
and restarts exactly what's already running (`av-deploy` is always refreshed; the
`migrate` step keys off the presence of a `server/` component). Pulling a Project
onto this box or pushing it off is a separate, explicit operation. 
Reconcile-and-halt is the backstop for a hand-edit that touched only one side: the next upgrade refuses and asks you to resolve it, so you can never accidentally start or stop the wrong service.

## Environment files

`.env` files are **operator-owned config, written once and never rewritten by the
updater** — no templates, no merge, no re-render. `install.sh` writes
`/etc/avalanche/*.env` (and the Caddyfile) from the configure-page inputs at
provision; after that they're the operator's to edit, and upgrades leave them
exactly as found.

Secrets (`REGISTRATION_SHARED_SECRET`, `ADMINBOT_DB_KEY`, …) are generated **once
at install** and written into these files then; the updater never regenerates or
rewrites them. Injecting a *new* secret that a later release introduces is
deferred (see *Deferred*); until then, a release that needs a new secret handles
it as a documented manual step.

Consequence: the updater adds no new config of any kind on upgrade. Application
code defaults sensibly for new optional vars, and a rare new *required* var is a
one-line manual edit called out in release notes — operators who've tuned their
`.env` are never surprised by a rewrite.

## Bots and Projects: uniform handling

The updater treats every bot/Project the same: swap the deployment tree, restart
the service, done. It has **no per-Project knowledge** — no per-Project backup,
no per-Project rollback, no awareness of which Projects are stateful. Local state,
if any, lives in `shared/<project>/` and the updater never touches it.

This is a deliberate scaling decision. There will be many first-party Projects; an
updater that encodes each one's backup/rollback behavior becomes a maintenance
sink and a fresh source of drift (the bug class this whole design exists to kill).
The cost lands where it scales: each stateful Project keeps its **local store
migrations backward-compatible**, so a code rollback is survivable — exactly as
the server does for Postgres. The updater guarantees nothing about Project state
across a rollback.

adminbot is the Project most deployments will lean on, so it's the obvious
candidate for an exception. We're explicitly **not** making one for now — if its
real-world resilience ever justifies special handling (e.g. an automatic
pre-upgrade copy of its store), add it then as a documented exception, not the
default.

## Versioning & coordination

- **One tag for the whole stack.** Server and Projects are separate processes, so
  an upgrade has a brief window where versions differ; moving everything to the
  same tag keeps the Project↔server contract simple (same-tag = compatible by
  construction). No mixed-tag stacks.
- The wire contract should tolerate the short skew during a rolling restart
  (server first, then Projects). Projects already reconnect/retry, so a few
  seconds of 5xx is absorbed.

## `release.yml` changes

Add `av-deploy` as a single, arch-independent artifact alongside `RUST_BINS` /
`NODE_BOTS`: tar `infra/deploy/bundle/` (the new home for the templates +
scripts) as `av-deploy-<tag>.tar.gz` and attach it. No compiled code, so it's
built once, not per-target.

## cloud-init / configure-page changes

`web/assets/configure/cloudinit-template.yaml` shrinks to: install prereqs (Node,
Postgres, Caddy, qrencode, ufw, persistent journald), `curl` the
`av-deploy-<RELEASE_TAG>` tarball, run `install.sh` with the form-provided values
(`SERVER_URL`, `SERVER_NAME`, `RELEASE_TAG`, adminbot/testbot opt-ins) passed via
`bootstrap.env`. The per-component region-stripping the configure page does today
(adminbot/testbot/node) moves into `install.sh` as flags, so the configure page
toggles env values rather than editing YAML. The ASCII guard in
`layouts/_default/configure.html` still applies to whatever inline cloud-init
remains.

## Triggering

- **Now / near-term:** manual `ssh` + `avalanche-update [TAG]`.
- **Target:** an `#admins` chat command — `/upgrade [tag]` — so operators upgrade
  from inside the app without shelling in, gated by `#admins` membership. Fits
  adminbot's superuser model (`22-adminbot.md`): adminbot is already the trusted
  bridge for privileged operations. Implies the updater must be invokable by
  adminbot (e.g. a `sudo`-gated wrapper), not only interactively.

## Staging

1. **Phase 1 (unblock):** rewrite `avalanche-update` to be **tag + tarball**
   based and cover the **bots** — for the *current* flat layout (in-place swap of
   the installed components, migrate, restart). Stamp the tag into
   `bootstrap.env`; surface it in `avalanche-status`. Smallest change that makes
   existing boxes upgradeable; unit/Caddyfile changes still need a manual step.
2. **Phase 2 (deployments + bundle):** introduce `av-deploy-<tag>.tar.gz`, the
   `/opt/avalanche/deployments/<tag>` + `current` layout, `shared/` for state,
   and `install.sh`/`update.sh`. Thin out the cloud-init. This is the model
   above; it resolves unit/config drift and the frozen-updater problem, and lays
   down the forward upgrade path.
3. **Phase 3:** the *Deferred* items — pre-upgrade DB dumps + retention,
   `ensure-secret`, and rollback (which the deployments layout already makes a
   pointer flip).
4. **Phase 4 (in-app):** the `/upgrade` `#admins` command.

## Deferred

Designed above but intentionally out of the first cut. The forward path
(build → `migrate` → flip → restart → prune) ships first; these follow.

- **Pre-upgrade DB dumps + retention.** A `pg_dump` to a `backups/` snapshot
  before `migrate`, as the restore point for a non-reversible migration, pruned in
  lockstep with deployments. Its only consumer is DB rollback, so it's deferred
  alongside rollback. (Independent of the separate daily `avalanche-backup` cron,
  and of DO's own backups in the managed-PG deployment.)
- **`ensure-secret` helper.** Append-only, generate-once injection of a *new*
  secret on upgrade (for secrets a future release introduces). Until then all
  secrets are generated once at install; a new one is a documented manual step.
  Env files stay operator-owned and untouched by the updater regardless.
- **Rollback.** `avalanche-update --rollback` (or to a prior tag): re-point
  `current` to a retained deployment, refresh its units, restart — near-instant,
  because the prune step keeps prior trees on disk. The DB side pairs with
  backward-compatible (N-1) migrations (the flip alone suffices) or the
  pre-upgrade dump above for a non-reversible migration — so migrations should be
  written backward-compatible to keep this a pointer flip. Until it's built, a
  failed upgrade is recovered manually (re-run against a prior tag), which the
  retained deployments make tractable.

## Rejected alternatives

- **Per-file binary swap (`.new`/`.old` in place).** Works for a single binary
  but doesn't generalize cleanly to the bot trees, leaves rollback as a
  per-component dance, and can't atomically switch the whole stack. The
  whole-tree `deployments/<tag>` + `current` flip subsumes it: one switch for
  binary + bots + (refreshed) units, with the old tree retained for instant
  rollback.
- **Updater baked into cloud-init only (status quo).** Can't update itself, the
  units, or the Caddyfile; every box is frozen at provision-time logic. This is
  what motivates shipping the bundle inside each deployment.
- **`avalanche-server self-update` subcommand.** Keeps upgrade logic in Rust and
  versioned with the binary, but the server would have to manage *other*
  processes (bots) and *its own* unit/Caddyfile — awkward, and a server failing
  to start can't update itself.
- **Per-Project upgrade/rollback logic.** Hard-coding each bot/Project's state
  backup + rollback in the updater doesn't scale past a handful of Projects and
  re-introduces the drift this design exists to kill. Projects own their own
  store forward/backward compatibility; the updater stays uniform. (adminbot
  exception deliberately deferred — see *Bots and Projects*.)
- **Independent per-component versions.** Lets server and Projects drift;
  multiplies the compatibility matrix for no benefit when all artifacts are cut
  from one tag.
- **Auto-update (unattended-upgrades style).** Surprising and risky for a
  stateful, migration-bearing service an operator is responsible for. Upgrades
  stay operator-triggered (CLI now, `#admins` later).
