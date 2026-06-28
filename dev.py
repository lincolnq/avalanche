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


# Keep the Job Object handle alive for the whole process lifetime. The job is
# configured to kill all member processes when its last handle closes, so this
# handle must NOT be garbage-collected / closed early.
_job_handle = None


def enable_kill_on_close():
    """Make every descendant process die when dev.py does — even if the terminal
    window is closed and Python never runs its signal handlers.

    On Windows, `Popen.terminate()` only kills the *direct* child, but our workers
    are grandchildren (cargo -> avalanche-server.exe, fnm -> node) so they orphan.
    Fix: put dev.py itself into a Job Object with KILL_ON_JOB_CLOSE. Child
    processes automatically inherit the job, so the whole tree is terminated by
    the OS when dev.py exits for any reason (Ctrl-C, terminate, or window close).
    No-op on non-Windows, where killing the process group handles this instead."""
    global _job_handle
    if sys.platform != "win32":
        return

    import ctypes
    from ctypes import wintypes

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

    class JOBOBJECT_BASIC_LIMIT_INFORMATION(ctypes.Structure):
        _fields_ = [
            ("PerProcessUserTimeLimit", wintypes.LARGE_INTEGER),
            ("PerJobUserTimeLimit", wintypes.LARGE_INTEGER),
            ("LimitFlags", wintypes.DWORD),
            ("MinimumWorkingSetSize", ctypes.c_size_t),
            ("MaximumWorkingSetSize", ctypes.c_size_t),
            ("ActiveProcessLimit", wintypes.DWORD),
            ("Affinity", ctypes.c_size_t),
            ("PriorityClass", wintypes.DWORD),
            ("SchedulingClass", wintypes.DWORD),
        ]

    class IO_COUNTERS(ctypes.Structure):
        _fields_ = [(n, ctypes.c_ulonglong) for n in (
            "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
            "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]

    class JOBOBJECT_EXTENDED_LIMIT_INFORMATION(ctypes.Structure):
        _fields_ = [
            ("BasicLimitInformation", JOBOBJECT_BASIC_LIMIT_INFORMATION),
            ("IoInfo", IO_COUNTERS),
            ("ProcessMemoryLimit", ctypes.c_size_t),
            ("JobMemoryLimit", ctypes.c_size_t),
            ("PeakProcessMemoryUsed", ctypes.c_size_t),
            ("PeakJobMemoryUsed", ctypes.c_size_t),
        ]

    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000
    JobObjectExtendedLimitInformation = 9

    kernel32.CreateJobObjectW.restype = wintypes.HANDLE
    kernel32.CreateJobObjectW.argtypes = [wintypes.LPVOID, wintypes.LPCWSTR]
    kernel32.SetInformationJobObject.restype = wintypes.BOOL
    kernel32.SetInformationJobObject.argtypes = [
        wintypes.HANDLE, ctypes.c_int, wintypes.LPVOID, wintypes.DWORD]
    kernel32.AssignProcessToJobObject.restype = wintypes.BOOL
    kernel32.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
    kernel32.GetCurrentProcess.restype = wintypes.HANDLE

    job = kernel32.CreateJobObjectW(None, None)
    if not job:
        print("warning: CreateJobObject failed; orphaned processes may persist")
        return

    info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION()
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    if not kernel32.SetInformationJobObject(
        job, JobObjectExtendedLimitInformation, ctypes.byref(info), ctypes.sizeof(info)
    ):
        print("warning: SetInformationJobObject failed; orphaned processes may persist")
        return

    # Nested jobs (Win8+) let this succeed even if the terminal already placed us
    # in a job; descendants join this inner job and inherit kill-on-close.
    if not kernel32.AssignProcessToJobObject(job, kernel32.GetCurrentProcess()):
        print("warning: AssignProcessToJobObject failed; orphaned processes may persist")
        return

    _job_handle = job


# On POSIX, give each child its own process group (start_new_session) so cleanup
# can signal the whole subtree — including grandchildren like the cargo-spawned
# server binary and the fnm-spawned node bots — with killpg. On Windows the Job
# Object (enable_kill_on_close) provides the equivalent kill-the-tree guarantee.
_POPEN_KW = {} if sys.platform == "win32" else {"start_new_session": True}


def _killpg(proc, sig):
    """Best-effort: signal a child's entire process group (POSIX only). Ignores
    a process that already exited or that we can't signal."""
    try:
        os.killpg(os.getpgid(proc.pid), sig)
    except (ProcessLookupError, PermissionError):
        pass


def free_port(port):
    """Kill whatever is LISTENing on `port` so a restart doesn't collide with an
    orphaned service from a previous dev run.

    dev.py reaps its own process tree on exit (Job Object on Windows, killpg on
    POSIX), but a force-killed dev.py — or one whose children were reparented
    before the job closed — can leave a server bound to 3000, which then makes
    the next `make dev-all` die with AddrInUse. Clearing the port up front makes
    the restart self-heal. Best-effort and cross-platform; a no-op when the port
    is already free or the lookup tool (netstat / lsof) is unavailable."""
    pids = set()
    try:
        if sys.platform == "win32":
            out = subprocess.run(
                ["netstat", "-ano", "-p", "tcp"],
                capture_output=True, text=True,
            ).stdout
            for line in out.splitlines():
                if "LISTENING" not in line:
                    continue
                cols = line.split()
                # Columns: Proto  Local-Address  Foreign-Address  State  PID.
                # The leading colon anchors the match so ":3000" can't hit
                # ":30000"; works for IPv4 and "[::1]:3000" alike.
                if len(cols) >= 5 and cols[1].endswith(f":{port}"):
                    pid = cols[-1]
                    if pid.isdigit() and pid != "0":
                        pids.add(pid)
        else:
            out = subprocess.run(
                ["lsof", "-ti", f"tcp:{port}", "-sTCP:LISTEN"],
                capture_output=True, text=True,
            ).stdout
            pids.update(p for p in out.split() if p.isdigit())
    except FileNotFoundError:
        return  # netstat / lsof not on PATH; nothing we can do

    for pid in pids:
        try:
            if sys.platform == "win32":
                subprocess.run(
                    ["taskkill", "/PID", pid, "/F"],
                    capture_output=True, check=False,
                )
            else:
                os.kill(int(pid), signal.SIGKILL)
            print(f"  Freed port {port}: killed stale process {pid}")
        except (ProcessLookupError, PermissionError, ValueError):
            pass


def main():
    enable_kill_on_close()
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

    # Free any port left bound by an orphaned service from a previous dev run
    # (see free_port) so `make dev-all` self-heals instead of dying with
    # AddrInUse. The server port comes from SERVER_URL (default 3000); project
    # ports are the ones assigned above.
    server_port = urlparse(os.environ.get("SERVER_URL", "http://localhost:3000")).port or 3000
    for port in [server_port, *(port for _project, port in project_launches)]:
        free_port(port)

    # Launch all services
    processes = []

    relay_url = os.environ.get("RELAY_URL")
    if relay_url:
        print(f"  Homeserver → relay: {relay_url}")
    else:
        print("  RELAY_URL not set — homeserver will not send push wakeups")

    # Point the signup privacy-policy link at the hosted demo-server policy so
    # the onboarding flow can be exercised locally. Override PRIVACY_POLICY_URL
    # in .env to test against a different hosted policy.
    privacy_policy_url = os.environ.get(
        "PRIVACY_POLICY_URL", "https://theavalanche.net/avdemo-privacy/"
    )

    processes.append(subprocess.Popen(
        ["cargo", "run", "-p", "server"],
        cwd=CORE_DIR,
        **_POPEN_KW,
        # Dev runs CLOSED registration (the prod default) so the dev clients
        # exercise the same admission path prod uses. Clients present the shared
        # secret below; testbot/adminbot read DEV_SHARED_SECRET to build a
        # bootstrap token. Override REGISTRATION_MODE=open for quick hacking.
        # Attachment blobs (docs/35) land under the repo-root dev-state/ tree
        # (gitignored, wiped by `make db-reset`), alongside the bots' stores.
        env={**os.environ, "PROJECTS": json.dumps(projects_json), "RUST_LOG": "tower_http=debug,server=debug", "ACTNET_ALLOW_DEV_DB": "1", "ACTNET_DISABLE_IP_RATE_LIMITS": "1", "REGISTRATION_SHARED_SECRET": DEV_SHARED_SECRET, "PRIVACY_POLICY_URL": privacy_policy_url, "ATTACHMENT_BLOB_DIR": os.path.join(REPO_DIR, "dev-state", "attachments")},
    ))

    for project, port in project_launches:
        print(f"  {project['name']} -> {host}:{port}")
        processes.append(subprocess.Popen(
            node_cmd([project["dist"]]),
            cwd=NODE_DIR,
            **_POPEN_KW,
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
        **_POPEN_KW,
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

    # Tear down the whole tree on Ctrl-C, terminal close, or if any child exits.
    # POSIX: signal each child's process group so grandchildren (cargo's server
    # binary, fnm's node bots) die too. Windows: terminate the direct children
    # for a prompt shutdown; the Job Object kills any grandchildren when we exit.
    def cleanup(*_args):
        if sys.platform == "win32":
            for p in processes:
                try:
                    p.terminate()
                except Exception:
                    pass
            for p in processes:
                try:
                    p.wait(timeout=5)
                except Exception:
                    pass
            sys.exit(0)
        for p in processes:
            _killpg(p, signal.SIGTERM)
        for p in processes:
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                _killpg(p, signal.SIGKILL)
        sys.exit(0)

    signal.signal(signal.SIGINT, cleanup)
    signal.signal(signal.SIGTERM, cleanup)
    # Terminal close: with start_new_session the children no longer receive the
    # terminal's SIGHUP directly, so dev.py catches it and tears the tree down.
    if hasattr(signal, "SIGHUP"):
        signal.signal(signal.SIGHUP, cleanup)

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
