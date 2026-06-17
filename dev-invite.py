#!/usr/bin/env python3
"""Print a fresh invite link for the current dev server, plus a scannable QR.

Reads SERVER_URL (and optionally INVITE_DOMAIN) from .env, falls back to the
same defaults the homeserver uses. Requires `qrencode` on PATH for the QR.
"""

import base64
import json
import os
import pathlib
import shutil
import subprocess
import sys


def load_env(path: pathlib.Path) -> None:
    if not path.exists():
        return
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        os.environ.setdefault(key.strip(), value.strip())


def main() -> int:
    repo_root = pathlib.Path(__file__).resolve().parent
    load_env(repo_root / ".env")

    server_url = os.environ.get("SERVER_URL", "http://localhost:3000")
    invite_domain = os.environ.get("INVITE_DOMAIN", "go.theavalanche.net")
    # The dev server runs CLOSED registration, so the invite must carry the
    # bootstrap secret (default "CHANGEME", matching dev.py / `make dev`).
    shared_secret = os.environ.get("REGISTRATION_SHARED_SECRET", "CHANGEME")

    # Single-char wire keys (s=server_url, k=bootstrap_secret) keep the token /
    # QR compact (docs/24, 51).
    payload_obj = {"s": server_url}
    if shared_secret:
        payload_obj["k"] = shared_secret
    payload = json.dumps(payload_obj, separators=(",", ":")).encode()
    token = base64.urlsafe_b64encode(payload).rstrip(b"=").decode()
    invite_url = f"https://{invite_domain}/i/{token}"

    print(f"Server URL: {server_url}")
    print(f"Invite URL: {invite_url}")
    print()

    if shutil.which("qrencode") is None:
        print("(qrencode not installed; skipping QR. brew install qrencode)")
        return 0

    return subprocess.run(["qrencode", "-t", "ANSIUTF8", invite_url]).returncode


if __name__ == "__main__":
    sys.exit(main())
