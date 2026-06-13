Scaffold a new app-core end-to-end test file for `$ARGUMENTS`.

E2E tests require a running homeserver at `SERVER_URL` (default: `http://localhost:3000`). Run via `make test-e2e`.

## Step 1 — Create the test file

Create `core/crates/app-core/tests/$ARGUMENTS.rs`:

```rust
//! End-to-end tests for $ARGUMENTS.
//!
//! Requires:
//! - A running homeserver at SERVER_URL (default: http://localhost:3000)
//! - The Postgres database backing it
//!
//! Each test creates fresh accounts so they do not interfere with each other.

use app_core::AppCore;

fn server_url() -> String {
    std::env::var("SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

async fn test_store() -> store::Store {
    let store = store::Store::open_in_memory().await.unwrap();
    store.migrate().await.unwrap();
    store
}

/// Filter messages to only those from a specific sender DID.
/// Used to ignore adminbot welcome DMs that arrive on every new account.
fn only_from(
    messages: Vec<app_core::DecryptedMessage>,
    did: &str,
) -> Vec<app_core::DecryptedMessage> {
    messages
        .into_iter()
        .filter(|m| m.sender_did == did)
        .collect()
}

#[tokio::test]
async fn test_$ARGUMENTS() {
    let url = server_url();

    // Create test accounts — each test gets fresh accounts with unique DIDs
    let alice = AppCore::create_account_with_store(
        &url,
        test_store().await,
        None,
        true,
    )
    .await
    .expect("alice create_account");

    // Add more accounts as needed:
    // let bob = AppCore::create_account_with_store(&url, test_store().await, None, true)
    //     .await
    //     .expect("bob create_account");

    // Test logic here
    // Common patterns:
    //
    // Send a DM:
    //   alice.send_dm_async(&bob.did(), b"hello", now_ms()).await.unwrap();
    //
    // Receive messages (filtering out adminbot DMs):
    //   let msgs = only_from(bob.receive_messages_async().await.unwrap(), &alice.did());
    //   assert_eq!(msgs.len(), 1);
    //   assert_eq!(msgs[0].body, "hello");
    //
    // Create a group:
    //   let group_id = alice.create_group_async("Test Group").await.unwrap();
    //
    // Wait for a live WebSocket event (use e2e_group_ws_push.rs as reference):
    //   let mut events = alice.next_events_async().await.unwrap();
}
```

## Step 2 — Register in Cargo.toml

Open `core/crates/app-core/Cargo.toml` and check if there is a `[[test]]` section. If the file needs to be explicitly listed (some setups auto-discover tests, some don't), add:

```toml
[[test]]
name = "$ARGUMENTS"
path = "tests/$ARGUMENTS.rs"
```

If other test files don't have explicit `[[test]]` entries, skip this step — Cargo auto-discovers files in `tests/`.

## Step 3 — Run

```bash
make dev-all   # start homeserver + testbot + relay in another terminal
make test-e2e  # run all e2e tests including the new one
# or run just the new test:
cd core && cargo test -p app-core --test $ARGUMENTS
```

Report: the test function names created and what scenario each covers.
