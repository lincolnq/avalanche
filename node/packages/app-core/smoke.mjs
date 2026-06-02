// Smoke test: load the wrapper, exercise initLogging, and confirm
// AppCore.login() rejects with NoAccount on a fresh DB.
import { rmSync } from "node:fs";
import { initLogging, AppCore } from "./dist/index.js";

initLogging("info");

try { rmSync("/tmp/actnet-node-smoke.db"); } catch {}

try {
  const core = await AppCore.login("/tmp/actnet-node-smoke.db", "wrong");
  console.error("unexpected: login returned", core?.constructor?.name, "did=", core.did());
  process.exit(1);
} catch (e) {
  console.log("login error (expected):", e.message);
}

if (typeof globalThis.Temporal === "undefined") {
  console.warn(
    "warning: globalThis.Temporal is undefined. The wrapper expects native " +
    "Temporal (Node 26+ binaries compiled with Rust toolchain present at " +
    "build time). Calls that pass or return Temporal.Instant will fail.",
  );
} else {
  console.log("Temporal.Now.instant():", Temporal.Now.instant().toString());
}

console.log("smoke ok");
