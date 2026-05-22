#!/usr/bin/env python3
"""Start all actnet dev services: Postgres, homeserver, relay, and project services."""

import json
import os
import signal
import subprocess
import sys
import time

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
INFRA_COMPOSE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "infra", "docker-compose.yml")


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


def main():
    start_postgres()

    # Build all crates in parallel while Postgres starts up
    print("Building...")
    subprocess.run(["cargo", "build", "-p", "server", "-p", "relay"] +
                   [f"-p{p['package']}" for p in PROJECTS],
                   cwd=CORE_DIR, check=True)

    wait_for_postgres()

    # Assign ports and build PROJECTS JSON for the server
    next_port = 3001
    projects_json = []
    project_launches = []
    for project in PROJECTS:
        port = next_port
        next_port += 1
        projects_json.append({
            "name": project["name"],
            "url": f"http://localhost:{port}",
            "description": project["description"],
        })
        project_launches.append((project, port))

    # Launch all services
    processes = []

    processes.append(subprocess.Popen(
        ["cargo", "run", "-p", "server"],
        cwd=CORE_DIR,
        env={**os.environ, "PROJECTS": json.dumps(projects_json), "RUST_LOG": "tower_http=debug,server=debug"},
    ))

    processes.append(subprocess.Popen(
        ["cargo", "run", "-p", "relay"],
        cwd=CORE_DIR,
        env={**os.environ, "RUST_LOG": "relay=debug"},
    ))

    for project, port in project_launches:
        print(f"  {project['name']} -> localhost:{port}")
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
