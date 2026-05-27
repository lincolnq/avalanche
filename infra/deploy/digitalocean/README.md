# DigitalOcean deployment

Bootstrap files for running an actnet homeserver on a DigitalOcean droplet
with managed Postgres. Walkthrough lives in
[`docs/40-deployment.md`](../../../docs/40-deployment.md).

| File              | Purpose                                                  |
|-------------------|----------------------------------------------------------|
| `setup.sh`        | First-boot script. Paste into DO's "user data" field.    |
| `actnet.service`  | systemd unit for the homeserver binary.                  |
| `Caddyfile`       | Caddy reverse-proxy + auto-TLS config template.          |
| `actnet.env`      | Environment file template. Operator fills in real values.|

These files are also usable on Hetzner, Linode, or any Ubuntu 24.04 host —
they don't depend on DO-specific APIs beyond user-data (which is a
standard cloud-init feature).
