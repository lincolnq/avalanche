# Deploying a homeserver on DigitalOcean

This guide walks through running your own Avalanche homeserver for a single
organization, on the cheapest DigitalOcean setup that's realistic for
production. End state: a domain like `av.example.org` that your members'
mobile apps connect to.

This is technical, but accessible to anyone who can use a terminal. 
We'll walk you through it all.

**What you'll need before starting:**
- A credit card (DigitalOcean charges hourly; the all-in cost is ~$20/mo).
- A domain name you control (e.g. via Namecheap, Cloudflare, etc.). A
  subdomain of an existing domain is fine.
- About 30 minutes.

**What you'll get:**
- A single homeserver, dedicated to your org's accounts, messages, and
  device records.
- Daily database backups (DigitalOcean handles them).
- Control over your org's digital life :)

---

## Cost summary

| Component                          | Cost      |
|------------------------------------|-----------|
| Droplet (`s-1vcpu-512mb-10gb`)     | $4/mo     |
| Managed PostgreSQL (smallest tier) | $15/mo    |
| **Total**                          | **$19/mo**|

---

## Step 1 — Create the managed database

1. In the [DigitalOcean control panel](https://cloud.digitalocean.com),
   click **Create → Databases**.
2. Choose **PostgreSQL 16**.
3. Datacenter: pick the region geographically closest to your members
   (e.g. NYC3 for the US east coast, AMS3 for Europe). **Write this region
   down — your droplet has to live in the same region.**
4. Cluster configuration: **Basic** plan, **1 GB RAM / 1 vCPU / 10 GB**.
   That's the $15/mo tier.
5. Choose a name: e.g. `actnet-db`.
6. Click **Create Database Cluster**. Provisioning takes ~5 minutes.

When the cluster is ready, you'll see a **Connection Details** panel. Switch
the dropdown to **Connection string** and copy the value — it looks like:

```
postgresql://doadmin:abc123XYZ...@db-postgresql-nyc3-12345-do-user-...ondigitalocean.com:25060/defaultdb?sslmode=require
```

Keep this open in a tab; you'll paste it into the droplet in Step 4.

---

## Step 2 — Create the droplet

1. **Create → Droplets**.
2. Region: **same as the database** (this matters — it puts them on a
   shared private network with no egress cost).
3. Image: **Ubuntu 24.04 LTS x64**.
4. Size: **Basic → Regular SSD → $4/mo** (512 MB RAM, 1 vCPU, 10 GB disk).
   That's enough — the database lives elsewhere.
5. **Authentication: SSH key** (not password). If you've never set up an
   SSH key, see [DigitalOcean's 2-minute
   guide](https://docs.digitalocean.com/products/droplets/how-to/add-ssh-keys/).
6. Hostname: something memorable, e.g. `actnet-yourorg`.
7. **Advanced options → Add Initial Scripts (user data)**: paste the
   contents of [`infra/deploy/digitalocean/setup.sh`](../infra/deploy/digitalocean/setup.sh)
   into the text box. This script installs the server binary, Caddy (for
   HTTPS), and a systemd service. It runs as root on first boot.
8. Click **Create Droplet**.

Wait ~2 minutes for the droplet to boot. The setup script takes another
~30 seconds after that. The droplet's public IP shows in the control panel.

---

## Step 3 — Point your domain at the droplet

In your DNS provider (Cloudflare, Namecheap, etc.), create an **A record**:

| Type | Name              | Value (the droplet's IP) | TTL |
|------|-------------------|--------------------------|-----|
| A    | `av`              | `192.0.113.42`           | 300 (or leave default) |

(You can use any subdomain you want. Replace the IP with
your actual droplet IP. TTL 300 = 5 minutes; pick lower if your provider
allows it, just for the initial setup.)

If you're using Cloudflare, **turn off the orange-cloud proxy** for this
record (click the cloud icon so it goes grey). Cloudflare's proxy
interferes with WebSocket connections and TLS renewals.

Verify the DNS propagated:

```bash
dig av.example.org +short
# Should print your droplet's IP.
```

---

## Step 4 — SSH in and configure

[SSH](https://en.wikipedia.org/wiki/Secure_Shell) is how you remote into the
droplet from your laptop's terminal:

```bash
ssh root@203.0.113.42
```

(Use your droplet's IP, not `203.0.113.42`.) The first connection asks you
to confirm a fingerprint — type `yes`. You should land at a prompt like
`root@av-yourorg:~#`.

Now edit the config file. We'll use `nano`, a beginner-friendly text
editor:

```bash
nano /etc/actnet/actnet.env
```

You'll see a file with placeholders. Fill in three values:

```bash
# Your homeserver's public URL — must match what's in DNS and what
# you'll put in invite links.
SERVER_URL=https://actnet.your-org.org

# The connection string you copied from Step 1.
DATABASE_URL=postgresql://doadmin:abc123XYZ...@db-postgresql-...

# A human-readable name members see during onboarding.
SERVER_NAME=Your Org's Homeserver
```

Save (Ctrl+O, Enter) and exit (Ctrl+X).

Then tell Caddy your domain so it can fetch a TLS certificate:

```bash
nano /etc/caddy/Caddyfile
```

Change the placeholder `your-domain.example.com` to your actual domain
(e.g. `actnet.your-org.org`). Save and exit.

Now run the database migrations and start everything:

```bash
actnet-init           # creates the schema in your managed PG
systemctl restart caddy actnet
```

Check it's running:

```bash
systemctl status actnet
# Look for: Active: active (running)

curl https://actnet.your-org.org/healthz
# Should print: ok
```

If the `curl` succeeds with HTTPS, you're done — Let's Encrypt issued a
certificate, the server is reachable, and your org's members can now
sign up.

---

## Step 5 — Generate invite codes

Your members need an invite link to join your homeserver. Generate one
from the droplet:

```bash
actnet-invite create --name "Alice"
# Prints: https://go.theavalanche.net/i/abc123def456
```

Send that URL to Alice via any channel (Signal, email, in person — it's a
one-time token). She opens it on her phone with the actnet app installed,
and she's signed up against your homeserver.

Each invite is single-use and expires after 24 hours by default.

---

## Day-2 operations

### Watching logs

```bash
journalctl -u actnet -f
```

This streams the server's logs. Ctrl+C to exit (the server keeps running).

### Updating to a new server version

```bash
actnet-update
```

This fetches the latest release binary, swaps it in, and restarts the
service. Downtime: a few seconds.

### Backups

DigitalOcean's managed Postgres takes a daily backup automatically, kept
for 7 days, with one-click point-in-time restore. Use the DigitalOcean 
console to manage this.

---

## Threat model notes

- The server stores **metadata**, not message content. Message bodies are
  end-to-end encrypted between devices using the Signal protocol; the
  server never sees plaintext.
- The metadata it does store: account DIDs, device identity keys, prekey
  bundles, ciphertext blobs (until delivered), push pseudonyms. The
  social graph (who talks to whom, when) is visible to the server.
- DigitalOcean is a US company, subject to US legal process. If your
  threat model includes the US government, see [§ EU
  deployment](#eu-deployment-hetzner) below for a Hetzner-based
  alternative.
- The droplet's root SSH key controls everything. Treat it like a
  long-lived secret: don't share it, rotate it if a laptop is lost, and
  consider hardware-backed keys (YubiKey).

### EU deployment (Hetzner)

For EU jurisdiction, the equivalent stack is:

- Hetzner Cloud CX11 droplet (€4.51/mo, Falkenstein or Helsinki).
- Hetzner Managed PG (€19/mo, smallest tier).
- Same `setup.sh`, same flow — Hetzner accepts user-data scripts the
  same way DO does.

The deployment scripts in `infra/deploy/digitalocean/` work unchanged on
Hetzner. We'll add a `hetzner/` folder once someone has actually
deployed there end-to-end and can validate the steps.

---

## Open work

This doc describes the target deployment experience. Some pieces are not
yet built — tracking here:

- **Prebuilt release binaries.** No GitHub Release publishes
  `actnet-server-linux-x86_64` yet. Until that exists, the setup script
  has to build from source on the droplet (slow on 512 MB — bump to the
  1 GB droplet for the initial bootstrap, then resize down).
- **`actnet-init` / `actnet-invite` / `actnet-update` /
  `actnet-backup-setup` / `actnet-restore` CLIs.** Today these are
  manual steps (run `psql` against the cluster with the migration files;
  use `dev-invite.py`). The deploy folder will get small shell wrappers
  for each.
- **Migrations runner.** Currently `infra/migrations/*.sql` is applied
  manually. Plan: embed `sqlx::migrate!()` into the server binary so it
  runs on startup, removing the `actnet-init` step entirely.
- **Healthcheck endpoint.** `/healthz` doesn't exist yet on the server.
  Easy add — returns 200 if the DB pool is reachable.

See `docs/02-todos-deferred.md` for the full backlog.
