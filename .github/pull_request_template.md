## Summary

<!-- What changed and why? 1–3 sentences. -->


## Test plan

<!-- How was this tested? List specific test commands run and any manual steps performed. -->

- [ ] `make ci` passed locally


## Checklist

### All PRs
- [ ] If this implements an item in `docs/02-todos-deferred.md`, that line has been deleted in this PR
- [ ] If any planned work was cut from scope, a new entry has been added to `docs/02-todos-deferred.md`
- [ ] New tests added for non-trivial logic (or explained why none are needed)

### Server PRs
- [ ] Migration file included under `infra/migrations/` (or confirmed no schema change needed)
- [ ] New writable or fetchable endpoints have rate limiting (see `middleware/rate_limit.rs`)
- [ ] Errors follow conventions in `CLAUDE.md`: no DB details or internal state exposed to the client
- [ ] `GET` endpoints that return user data are authenticated (no accidental public exposure)

### Mobile / FFI PRs
- [ ] `make bindings` was run after any changes to `core/crates/app-core/src/lib.rs`
- [ ] New FFI methods added to `AppCoreProtocol` in `ActnetService.swift`
- [ ] New FFI methods stubbed in `MockActnetService.swift`
- [ ] FFI exports are synchronous (no async FFI boundary crossings)

### Crypto / Protocol PRs
- [ ] No I/O introduced into the `crypto` crate
- [ ] `Store` trait changes are reflected in both the `store` crate implementation and any mock implementations
- [ ] Signal Protocol invariants preserved (no double-ratchet state sharing, no session reuse)
