Add a property-based test for `$ARGUMENTS` using proptest.

Property-based tests generate hundreds of random inputs and verify an invariant holds for all of them. Use for: serialization round-trips, state machine reachability, crypto correctness, encoding edge cases.

## Step 1 — Identify the invariant

State the property as: "For any valid [input], [operation] always produces [expected outcome]."

Examples:
- "For any sequence of messages, encrypt then decrypt always returns the original plaintext."
- "For any group state and sequence of valid actions, the resulting state is always consistent."
- "For any byte sequence, base64-encode then base64-decode always returns the original bytes."

## Step 2 — Add the test

Add to the `#[cfg(test)]` module in the relevant source file, or to a `tests/` file:

```rust
#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    // Define a strategy for generating test inputs
    // Simple types: any::<bool>(), any::<u8>(), 0u32..100u32
    // Collections: prop::collection::vec(any::<u8>(), 1..64)
    // Custom types: use .prop_map() to transform simpler strategies

    proptest! {
        #[test]
        fn $ARGUMENTS_invariant_name(
            // input: strategy_type
            input in prop::collection::vec(any::<u8>(), 1..256),
        ) {
            // The test body must be synchronous.
            // For async logic, use a runtime:
            //   let rt = tokio::runtime::Runtime::new().unwrap();
            //   let result = rt.block_on(async { ... });

            // Assert the invariant
            // prop_assert!(condition, "message if failed: {:?}", input);
            // prop_assert_eq!(actual, expected);
        }
    }
}
```

For the async pattern used elsewhere in this codebase (see `core/crates/store/tests/integration.rs`):
```rust
fn run<F: std::future::Future<Output = Result<(), String>>>(f: F) -> Result<(), String> {
    tokio::runtime::Runtime::new().unwrap().block_on(f)
}

proptest! {
    #[test]
    fn $ARGUMENTS_round_trips(
        data in prop::collection::vec(any::<u8>(), 0..1024)
    ) {
        let result = run(async move {
            // async test logic
            Ok(())
        });
        prop_assert!(result.is_ok(), "{}", result.unwrap_err());
    }
}
```

## Step 3 — Ensure proptest is in Cargo.toml

Check `core/crates/<crate>/Cargo.toml` for:
```toml
[dev-dependencies]
proptest = "1"
```

If missing, add it. Check `core/Cargo.toml` (workspace) first — it may already be in `[workspace.dev-dependencies]`.

## Step 4 — Run

```bash
cd core && cargo test -p <crate> -- $ARGUMENTS_invariant_name
# proptest runs 256 cases by default; increase with:
# PROPTEST_CASES=10000 cargo test -p <crate> -- $ARGUMENTS_invariant_name
```

Report: the invariant being tested, the input strategy, and the assertion.
