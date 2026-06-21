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

// Surface check for the listGroups() API (full exercise needs a server +
// account, which this offline smoke can't set up).
for (const method of [
  "listGroups",
  "setGroupExpiry",
  "setPendingRequest",
  "fetchAndCacheProfile",
  "leaveGroup",
  "isGroupMember",
  "leaveServer",
  "deleteIdentity",
]) {
  if (typeof AppCore.prototype[method] !== "function") {
    console.error(`unexpected: AppCore.prototype.${method} is not a function`);
    process.exit(1);
  }
}
console.log("contact opt-in + group methods present on AppCore");

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
