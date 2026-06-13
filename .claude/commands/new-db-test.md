Add a new database integration test for `$ARGUMENTS` using the transaction-rollback isolation pattern.

DB tests hit a real Postgres instance via `TEST_DATABASE_URL`. Each test runs inside a transaction that is rolled back on drop, so tests are isolated without truncating tables. Run via `make test-server`.

## Step 1 — Add to db_tests.rs

Open `core/crates/server/tests/db_tests.rs` and add at the bottom:

```rust
#[tokio::test]
async fn test_$ARGUMENTS() {
    let pool = test_pool().await;
    let mut tx = begin_tx(&pool).await;

    // All DB calls go through &mut tx — changes are visible within this
    // transaction but rolled back automatically when tx is dropped.

    // Example: create prerequisite data
    // let account_id = db::accounts::create(&mut tx, "did:plc:test123", false, None)
    //     .await
    //     .expect("create account");

    // Call the function under test
    // let result = db::$ARGUMENTS::create(&mut tx, ...)
    //     .await
    //     .expect("$ARGUMENTS create");

    // Assert
    // assert_eq!(result.field, expected_value);

    // Verify with a fetch
    // let fetched = db::$ARGUMENTS::find_by_id(&mut tx, result.id)
    //     .await
    //     .expect("find")
    //     .expect("should exist");
    // assert_eq!(fetched.field, expected_value);

    // tx drops here → transaction rolls back → no leftover rows
}
```

Common setup helpers available in `db_tests.rs`:
- `test_pool()` — connects to `TEST_DATABASE_URL`, applies migrations once via `OnceCell`
- `begin_tx(&pool)` — starts a transaction, returns a `PgConnection` that rolls back on drop
- Look at existing tests for helpers like `create_test_account()` if they exist

## Step 2 — Test edge cases

Add tests for:
- Not-found: `find_by_id` with a nonexistent ID returns `Ok(None)`
- Constraint violation: inserting a duplicate unique key returns an error
- Cascade behavior: deleting a parent record removes child records (if applicable)

## Step 3 — Run

```bash
make db-up    # ensure Postgres is running
make test-server
# or run just the new test:
cd core && TEST_DATABASE_URL=postgres://actnet:actnet-dev@localhost/actnet cargo test -p server -- test_$ARGUMENTS
```

Report: the DB functions tested and the assertions made.
