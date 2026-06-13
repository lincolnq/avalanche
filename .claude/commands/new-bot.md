Scaffold a new Node.js bot package named `$ARGUMENTS` under `node/packages/`.

## Step 1 — Create the package directory and files

**`node/packages/$ARGUMENTS/package.json`:**
```json
{
  "name": "@actnet/$ARGUMENTS",
  "version": "0.1.0",
  "description": "<one-line description of what this bot does>",
  "private": true,
  "license": "AGPL-3.0",
  "type": "module",
  "bin": {
    "$ARGUMENTS": "dist/index.js"
  },
  "main": "dist/index.js",
  "scripts": {
    "build": "tsc",
    "start": "node dist/index.js"
  },
  "dependencies": {
    "@actnet/app-core": "*"
  },
  "devDependencies": {
    "@types/node": "^22.19.19",
    "typescript": "^5.9.3"
  },
  "engines": {
    "node": ">=26"
  }
}
```

**`node/packages/$ARGUMENTS/tsconfig.json`:**
```json
{
  "extends": "../../tsconfig.base.json",
  "compilerOptions": {
    "outDir": "dist",
    "rootDir": "src"
  },
  "include": ["src"]
}
```

**`node/packages/$ARGUMENTS/src/index.ts`:**
```typescript
// $ARGUMENTS bot — <one-line description>
//
// Persistent state:
//   - SQLCipher DB at STATE_DIR/store.db — owned by app-core
//
// Environment variables:
//   $ARGUMENTS_SERVER_URL  — homeserver to register against (required)
//   $ARGUMENTS_STATE_DIR   — SQLCipher DB location (default: node/$ARGUMENTS-state)
//   $ARGUMENTS_DB_KEY      — SQLCipher passphrase (default: empty)

import { mkdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { AppCore, initLogging } from "@actnet/app-core";

const BOT_DID_SUFFIX = "$ARGUMENTS";
const BOT_DID = `did:local:${BOT_DID_SUFFIX}`;

interface Env {
  serverUrl: string;
  stateDir: string;
  dbPath: string;
  dbKey: string;
}

function readEnv(): Env {
  const serverUrl = process.env.$ARGUMENTS_UPPER_SERVER_URL;
  if (!serverUrl) throw new Error("$ARGUMENTS_UPPER_SERVER_URL is required");
  const stateDir = process.env.$ARGUMENTS_UPPER_STATE_DIR
    ?? join(import.meta.dirname, "../../$ARGUMENTS-state");
  return {
    serverUrl,
    stateDir,
    dbPath: join(stateDir, "store.db"),
    dbKey: process.env.$ARGUMENTS_UPPER_DB_KEY ?? "",
  };
}

async function main() {
  const env = readEnv();
  initLogging("info");
  mkdirSync(env.stateDir, { recursive: true });

  // Register or re-open existing account
  let core: AppCore;
  if (existsSync(env.dbPath)) {
    core = await AppCore.login(env.dbPath, env.dbKey);
    console.log("logged in as", core.did());
  } else {
    core = await AppCore.createAccount(
      env.serverUrl,
      env.dbPath,
      env.dbKey,
      new Uint8Array(0),
      "$ARGUMENTS",
      BOT_DID_SUFFIX,
    );
    console.log("registered as", core.did());
  }

  // Main event loop
  for await (const event of core.events()) {
    if (event.kind === "message") {
      const { senderDid, body, groupId } = event.message;
      console.log(`message from ${senderDid}:`, body);
      // TODO: handle the message
    }
  }
}

main().catch((err) => {
  console.error("fatal:", err);
  process.exit(1);
});
```

## Step 2 — Register in the workspace

Open `node/package.json` and add the new package to the `workspaces` array:
```json
"workspaces": [
  "packages/app-core",
  "packages/adminbot",
  "packages/$ARGUMENTS"
]
```

Add a Makefile target in the root `Makefile` following the `adminbot` pattern:
```makefile
$ARGUMENTS:
	cd node && [ -d node_modules ] || npm install
	cd node && npm run build:native -w @actnet/app-core
	cd node && npm run build -w @actnet/$ARGUMENTS
	cd node && node --env-file=.env packages/$ARGUMENTS/dist/index.js
.PHONY: $ARGUMENTS
```

## Step 3 — Verify

Run `cd node && npm install && npm run build -w @actnet/$ARGUMENTS` and fix any TypeScript errors.

Report: the bot's DID suffix, the environment variables it reads, and the event types it handles.
