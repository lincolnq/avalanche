Add a new HTTP integration test for `$ARGUMENTS` to the server test suite.

HTTP tests use `tower::oneshot` to drive Axum handlers in-process. They require `TEST_DATABASE_URL`. Run via `make test-server`.

## Step 1 — Add to the appropriate test file

Open `core/crates/server/tests/http_tests.rs` (for general endpoints) or `core/crates/server/tests/group_tests.rs` (for group endpoints). Add the test at the bottom:

```rust
#[tokio::test]
async fn test_$ARGUMENTS() {
    ensure_setup().await;
    let state = test_state().await;
    let app = routes::router().with_state(state.clone());

    // Register a test account (generates a unique DID via nanosecond entropy)
    let (did, token) = register(&state).await;

    // Build the request
    let req = Request::builder()
        .method("GET")  // or POST, DELETE, etc.
        .uri("/v1/your-endpoint")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                // request body if needed
            })).unwrap()
        ))
        .unwrap();

    // Drive the handler
    let resp = app.oneshot(req).await.unwrap();

    // Assert response
    assert_eq!(resp.status(), StatusCode::OK);

    // Parse and assert body if needed:
    // let body = resp.into_body().collect().await.unwrap().to_bytes();
    // let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    // assert_eq!(json["field"], "expected_value");
}
```

Common status codes to assert:
- `StatusCode::OK` (200) — successful GET
- `StatusCode::CREATED` (201) — successful POST that creates a resource
- `StatusCode::NO_CONTENT` (204) — successful DELETE or write with no body
- `StatusCode::UNAUTHORIZED` (401) — missing/invalid auth (test this too)
- `StatusCode::NOT_FOUND` (404) — resource not found
- `StatusCode::BAD_REQUEST` (400) — invalid input

## Step 2 — Test the error cases too

Add a second test for the unhappy path:

```rust
#[tokio::test]
async fn test_$ARGUMENTS_unauthorized() {
    ensure_setup().await;
    let state = test_state().await;
    let app = routes::router().with_state(state.clone());

    let req = Request::builder()
        .method("GET")
        .uri("/v1/your-endpoint")
        // No Authorization header
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
```

## Step 3 — Run

```bash
make db-up     # ensure Postgres is running
make test-server
# or run just the new test:
cd core && TEST_DATABASE_URL=postgres://actnet:actnet-dev@localhost/actnet cargo test -p server -- test_$ARGUMENTS
```

Report: the endpoint tested, the happy-path assertion, and the error cases covered.
