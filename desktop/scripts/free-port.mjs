// Frees the Vite dev port before `vite` binds to it.
//
// Vite is configured with `strictPort: true` (vite.config.ts) because Tauri's
// webview points at a fixed port, so Vite errors out instead of falling back
// when the port is taken. A `tauri dev` whose parent dies without reaping its
// Vite child leaves that child squatting on the port, which then breaks the
// next start. Run via the `predev` npm hook so it fires on every dev launch.
//
// Dependency-free and cross-platform (Windows / macOS / Linux).

import { execFileSync } from "node:child_process";

const PORT = 1420;

function pidsOnPort(port) {
  try {
    if (process.platform === "win32") {
      // netstat lists one line per connection; the PID is the last column.
      // Match LISTENING sockets on the exact local port (":1420").
      const out = execFileSync("netstat", ["-ano", "-p", "tcp"], {
        encoding: "utf8",
      });
      const pids = new Set();
      for (const line of out.split(/\r?\n/)) {
        if (!/LISTENING/i.test(line)) continue;
        const cols = line.trim().split(/\s+/);
        const local = cols[1] ?? "";
        if (local.endsWith(`:${port}`)) {
          const pid = cols[cols.length - 1];
          if (/^\d+$/.test(pid) && pid !== "0") pids.add(pid);
        }
      }
      return [...pids];
    }
    // macOS / Linux: lsof prints one PID per line for the listening socket.
    const out = execFileSync("lsof", [`-ti`, `tcp:${port}`, "-sTCP:LISTEN"], {
      encoding: "utf8",
    });
    return out.split(/\r?\n/).filter((p) => /^\d+$/.test(p));
  } catch {
    // No process on the port (lsof exits non-zero when nothing matches), or
    // the lookup tool is unavailable — nothing to free either way.
    return [];
  }
}

function kill(pid) {
  try {
    if (process.platform === "win32") {
      execFileSync("taskkill", ["/PID", pid, "/F"], { stdio: "ignore" });
    } else {
      execFileSync("kill", ["-9", pid], { stdio: "ignore" });
    }
    return true;
  } catch {
    return false;
  }
}

const pids = pidsOnPort(PORT);
if (pids.length === 0) {
  process.exit(0);
}
for (const pid of pids) {
  if (kill(pid)) {
    console.log(`free-port: killed stale process ${pid} on port ${PORT}`);
  } else {
    console.warn(`free-port: could not kill process ${pid} on port ${PORT}`);
  }
}
