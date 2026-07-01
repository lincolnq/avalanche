// Smoke test: load the wrapper, exercise initLogging, and confirm
// AppCore.login() rejects with NoAccount on a fresh DB.
import { rmSync } from "node:fs";
import { initLogging, AppCore, DeviceLinkNew } from "./dist/index.js";

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
  "reconnectNow",
  "setAppActive",
  "linkCreatePairing",
  "linkAcceptPairing",
  "linkSendBundle",
  "uploadAttachment",
  "downloadAttachment",
  "sendWithAttachments",
  "setAttachmentDownloaded",
  "oauthIssueCode",
  "oauthApproveDevice",
]) {
  if (typeof AppCore.prototype[method] !== "function") {
    console.error(`unexpected: AppCore.prototype.${method} is not a function`);
    process.exit(1);
  }
}
console.log("contact opt-in + group + device-link methods present on AppCore");

// DeviceLinkNew (new-device side of device linking, docs/04 §4). A full
// round-trip needs a server + an existing device; this offline smoke only
// confirms the napi object is constructable and its methods are wired.
{
  const link = new DeviceLinkNew();
  for (const method of ["createPairing", "acceptPairing", "awaitLink"]) {
    if (typeof link[method] !== "function") {
      console.error(`unexpected: DeviceLinkNew.prototype.${method} is not a function`);
      process.exit(1);
    }
  }
  console.log("DeviceLinkNew constructable with createPairing/acceptPairing/awaitLink");
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
