#!/usr/bin/env python3
"""Start all actnet dev services: Postgres, homeserver, relay, and project services."""

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
        "package": "testbot",
        "bind_env": "TESTBOT_BIND_ADDR",
        "rust_log": "actnet_testbot=debug,app_core=debug",
    },
]

CORE_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "core")
INFRA_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "infra")
INFRA_COMPOSE = os.path.join(INFRA_DIR, "docker-compose.yml")
MIGRATIONS_DIR = os.path.join(INFRA_DIR, "migrations")
DB_URL = "postgresql://actnet:actnet-dev@localhost:5432/actnet"


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
    """Apply any pending migrations from infra/migrations/."""
    print("Checking migrations...")

    def psql(sql, capture=False):
        cmd = ["psql", DB_URL, "-tAX", "-c", sql]
        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode != 0:
            raise RuntimeError(f"psql failed: {r.stderr.strip()}")
        return r.stdout.strip()

    # Ensure tracking table exists.
    psql("""
        CREATE TABLE IF NOT EXISTS schema_migrations (
            filename TEXT PRIMARY KEY,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
    """)

    # Check if tracking table was just created on an existing DB.
    # If so, seed it with migrations that were already applied via initdb.
    applied = set(psql("SELECT filename FROM schema_migrations").splitlines())
    migration_files = sorted(
        f for f in os.listdir(MIGRATIONS_DIR) if f.endswith(".sql")
    )

    if not applied:
        # Tracking table is empty — check if the DB already has tables from initdb.
        has_tables = psql("SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'accounts')")
        if has_tables == "t":
            # DB was initialized by docker-entrypoint-initdb.d. Seed the
            # tracking table with all migration files and try applying each
            # one — skip those that fail (already applied).
            print("  Existing database detected, syncing migration tracking...")
            for filename in migration_files:
                path = os.path.join(MIGRATIONS_DIR, filename)
                with open(path) as f:
                    sql = f.read()
                wrapped = f"""
                    BEGIN;
                    {sql}
                    INSERT INTO schema_migrations (filename) VALUES ('{filename}');
                    COMMIT;
                """
                cmd = ["psql", DB_URL, "-tAX", "-c", wrapped]
                r = subprocess.run(cmd, capture_output=True, text=True)
                if r.returncode == 0:
                    print(f"  Applied {filename}.")
                else:
                    # Already applied — just record it.
                    psql(f"INSERT INTO schema_migrations (filename) VALUES ('{filename}') ON CONFLICT DO NOTHING")
            print("  Migration tracking synced.")
            return

    pending = [f for f in migration_files if f not in applied]
    if not pending:
        print("  All migrations already applied.")
        return

    for filename in pending:
        path = os.path.join(MIGRATIONS_DIR, filename)
        print(f"  Applying {filename}...")
        with open(path) as f:
            sql = f.read()
        # Apply migration + record it in a single transaction.
        wrapped = f"""
            BEGIN;
            {sql}
            INSERT INTO schema_migrations (filename) VALUES ('{filename}');
            COMMIT;
        """
        psql(wrapped)
        print(f"  Applied {filename}.")

    print(f"  {len(pending)} migration(s) applied.")


def main():
    start_postgres()

    # Build all crates in parallel while Postgres starts up
    print("Building...")
    subprocess.run(["cargo", "build", "-p", "server", "-p", "relay"] +
                   [f"-p{p['package']}" for p in PROJECTS],
                   cwd=CORE_DIR, check=True)

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

    processes.append(subprocess.Popen(
        ["cargo", "run", "-p", "server"],
        cwd=CORE_DIR,
        env={**os.environ, "PROJECTS": json.dumps(projects_json), "RUST_LOG": "tower_http=debug,server=debug", "ACTNET_ALLOW_DEV_DB": "1", "ACTNET_DISABLE_IP_RATE_LIMITS": "1"},
    ))

    processes.append(subprocess.Popen(
        ["cargo", "run", "-p", "relay"],
        cwd=CORE_DIR,
        env={**os.environ, "RUST_LOG": "relay=debug"},
    ))

    for project, port in project_launches:
        print(f"  {project['name']} -> {host}:{port}")
        processes.append(subprocess.Popen(
            ["cargo", "run", "-p", project["package"]],
            cwd=CORE_DIR,
            env={**os.environ, project["bind_env"]: f"0.0.0.0:{port}", "RUST_LOG": project["rust_log"]},
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
