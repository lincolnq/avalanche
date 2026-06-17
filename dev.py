#!/usr/bin/env python3
"""Start actnet dev services: Postgres, homeserver, and project services.

The push relay is a separate, single-instance service shared across all
environments (dev + production). It is not launched here — point the
homeserver at the running relay by setting RELAY_URL in .env."""

import json
import os
import pathlib
import signal
import subprocess
import sys
import time
from urllib.parse import urlparse


def load_env(path):
    """Minimal .env loader. Doesn't override values already in os.environ."""
    if not path.exists():
        return
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        os.environ.setdefault(key.strip(), value.strip())


load_env(pathlib.Path(__file__).resolve().parent / ".env")


def project_host():
    """Scheme + hostname clients should use to reach project services. Derived
    from SERVER_URL so e.g. a Tailscale Magic DNS server URL also points
    project webviews at the same reachable host."""
    parsed = urlparse(os.environ.get("SERVER_URL", "http://localhost:3000"))
    return parsed.scheme or "http", parsed.hostname or "localhost"

# ── Project services ────────────────────────────────────────────────────────
# Each project gets a sequential port starting at 3001.
PROJECTS = [
    {
        "name": "Testbot",
        "description": "Chat with an AI bot",
        "dist": "packages/testbot/dist/index.js",
        "bind_env": "TESTBOT_BIND_ADDR",
        "log_env": "TESTBOT_LOG",
    },
]

REPO_DIR = os.path.dirname(os.path.abspath(__file__))
CORE_DIR = os.path.join(REPO_DIR, "core")
NODE_DIR = os.path.join(REPO_DIR, "node")
INFRA_DIR = os.path.join(REPO_DIR, "infra")
INFRA_COMPOSE = os.path.join(INFRA_DIR, "docker-compose.yml")
DB_URL = "postgresql://actnet:actnet-dev@localhost:5432/actnet"
ADMINBOT_STATE_DIR = os.path.join(REPO_DIR, "dev-state", "adminbot")
# Bootstrap secret for dev's closed-registration server. The homeserver accepts
# it; testbot/adminbot present it (as a bootstrap token) to register.
DEV_SHARED_SECRET = os.environ.get("REGISTRATION_SHARED_SECRET", "CHANGEME")


def node_cmd(args):
    """Build a `node ...` argv that runs under the version pinned in
    node/.node-version. We launch subprocesses directly (no shell), so the
    user's shell-side fnm activation doesn't apply — we have to invoke
    `fnm exec` explicitly. Fails loud if the pinned version isn't installed
    so we don't silently fall through to a system node."""
    version_file = os.path.join(NODE_DIR, ".node-version")
    with open(version_file) as f:
        version = f.read().strip()
    return ["fnm", "exec", f"--using={version}", "node", *args]


def start_postgres():
    print("Starting Postgres...")
    subprocess.run(["docker", "compose", "-f", INFRA_COMPOSE, "up", "-d"], check=True)


def wait_for_postgres():
    print("Waiting for Postgres to be healthy...")
    while True:
        result = subprocess.run(
            ["docker", "compose", "-f", INFRA_COMPOSE, "ps", "postgres"],
            capture_output=True, text=True,
        )
        if "healthy" in result.stdout:
            break
        time.sleep(1)


def run_migrations():
    """Apply any pending migrations via the server binary's `migrate` subcommand.

    The server's own `sqlx::migrate!` is the canonical migration path —
    same code production runs, baked into the binary at compile time so
    files and checksums are pinned. dev.py just invokes it; we don't keep
    a parallel tracker here. (Previously we did, and the two trackers
    drifted any time someone ran `make migrate` outside dev.py.)
    """
    print("Applying migrations via server binary...")
    env = {**os.environ, "DATABASE_URL": DB_URL}
    subprocess.run(
        ["cargo", "run", "-q", "-p", "server", "--", "migrate"],
        cwd=CORE_DIR,
        env=env,
        check=True,
    )


def build_node_bots():
    """Build the first-party Node bots (adminbot + testbot). Both depend on the
    shared `node-app-core` binding; a single `make` invocation rebuilds that
    dep just once (and only when the Rust/TS sources changed)."""
    print("Building node bots (adminbot + testbot)...")
    subprocess.run(["make", "node-adminbot-build", "node-testbot-build"], cwd=REPO_DIR, check=True)


def main():
    start_postgres()

    # Build the server while Postgres starts up. Project services are Node
    # packages, built alongside the bots below.
    print("Building...")
    subprocess.run(["cargo", "build", "-p", "server"], cwd=CORE_DIR, check=True)
    build_node_bots()

    wait_for_postgres()
    run_migrations()

    # Assign ports and build PROJECTS JSON for the server.
    # Project URLs use the same host clients use to reach the homeserver
    # (from SERVER_URL), so a Tailscale / LAN dev setup also gives phones
    # a reachable URL for project webviews.
    scheme, host = project_host()
    next_port = 3001
    projects_json = []
    project_launches = []
    for project in PROJECTS:
        port = next_port
        next_port += 1
        projects_json.append({
            "name": project["name"],
            "url": f"{scheme}://{host}:{port}",
            "description": project["description"],
        })
        project_launches.append((project, port))

    # Launch all services
    processes = []

    relay_url = os.environ.get("RELAY_URL")
    if relay_url:
        print(f"  Homeserver → relay: {relay_url}")
    else:
        print("  RELAY_URL not set — homeserver will not send push wakeups")

    processes.append(subprocess.Popen(
        ["cargo", "run", "-p", "server"],
        cwd=CORE_DIR,
        # Dev runs CLOSED registration (the prod default) so the dev clients
        # exercise the same admission path prod uses. Clients present the shared
        # secret below; testbot/adminbot read DEV_SHARED_SECRET to build a
        # bootstrap token. Override REGISTRATION_MODE=open for quick hacking.
        env={**os.environ, "PROJECTS": json.dumps(projects_json), "RUST_LOG": "tower_http=debug,server=debug", "ACTNET_ALLOW_DEV_DB": "1", "ACTNET_DISABLE_IP_RATE_LIMITS": "1", "REGISTRATION_SHARED_SECRET": DEV_SHARED_SECRET},
    ))

    for project, port in project_launches:
        print(f"  {project['name']} -> {host}:{port}")
        processes.append(subprocess.Popen(
            node_cmd([project["dist"]]),
            cwd=NODE_DIR,
            env={
                **os.environ,
                project["bind_env"]: f"0.0.0.0:{port}",
                "HOMESERVER_URL": os.environ.get("HOMESERVER_URL", "http://localhost:3000"),
                project["log_env"]: os.environ.get(project["log_env"], "info"),
                # Present the bootstrap secret so the bot can register against
                # the closed-registration dev server.
                "REGISTRATION_SHARED_SECRET": DEV_SHARED_SECRET,
            },
        ))

    # Adminbot — auto-registers as did:local:adminbot on first launch (matches
    # the server's default ADMINBOT_DID). Retries connect against the
    # homeserver internally, so the launch order with the server is not
    # load-bearing.
    pathlib.Path(ADMINBOT_STATE_DIR).mkdir(parents=True, exist_ok=True)
    adminbot_log_path = os.path.join(ADMINBOT_STATE_DIR, "adminbot.log")
    adminbot_log = open(adminbot_log_path, "a", buffering=1)
    print(f"  Adminbot -> state {ADMINBOT_STATE_DIR} (log: {adminbot_log_path})")
    processes.append(subprocess.Popen(
        node_cmd(["packages/adminbot/dist/index.js"]),
        cwd=NODE_DIR,
        stdout=adminbot_log,
        stderr=subprocess.STDOUT,
        env={
            **os.environ,
            "ADMINBOT_SERVER_URL": os.environ.get(
                "ADMINBOT_SERVER_URL", "http://localhost:3000"
            ),
            "ADMINBOT_STATE_DIR": ADMINBOT_STATE_DIR,
            "ADMINBOT_DB_KEY": os.environ.get("ADMINBOT_DB_KEY", "dev-adminbot-key"),
            "ADMINBOT_LOG": os.environ.get("ADMINBOT_LOG", "info"),
            # Bootstrap secret: registers adminbot against the closed dev server
            # and links it into the superuser Project (it names that Project).
            "REGISTRATION_SHARED_SECRET": DEV_SHARED_SECRET,
        },
    ))

    # Wait for all processes; kill them all on Ctrl-C or if any exits
    def cleanup(*_args):
        for p in processes:
            p.terminate()
        for p in processes:
            p.wait()
        sys.exit(0)

    signal.signal(signal.SIGINT, cleanup)
    signal.signal(signal.SIGTERM, cleanup)

    try:
        while True:
            for p in processes:
                if p.poll() is not None:
                    print(f"Process {p.args} exited with code {p.returncode}, shutting down...")
                    cleanup()
            time.sleep(0.5)
    except KeyboardInterrupt:
        cleanup()


if __name__ == "__main__":
    main()
