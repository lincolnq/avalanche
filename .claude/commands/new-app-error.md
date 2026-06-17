Add a new error variant named `$ARGUMENTS` to `AppError` and `AppErrorFfi`.

## Step 1 — Edit `core/crates/app-core/src/error.rs`

Read the file first. Add the new variant to both enums and the `From` impl:

**In `AppError`:**
```rust
#[error("<description of what went wrong>")]
$ArgumentsPascalCase,
// or with a payload:
#[error("<description>: {0}")]
$ArgumentsPascalCase(String),
```

**In `AppErrorFfi`:**
```rust
#[error("<description>")]
$ArgumentsPascalCase,
// or with a reason string (required if the variant carries a payload):
#[error("{reason}")]
$ArgumentsPascalCase { reason: String },
```

**In `From<AppError> for AppErrorFfi`:**
```rust
AppError::$ArgumentsPascalCase => AppErrorFfi::$ArgumentsPascalCase,
// or:
AppError::$ArgumentsPascalCase(msg) => AppErrorFfi::$ArgumentsPascalCase { reason: msg },
```

## Step 2 — Use the new variant

Return it at the appropriate site:
```rust
return Err(AppError::$ArgumentsPascalCase);
// or:
return Err(AppError::$ArgumentsPascalCase("details".into()));
```

## Step 3 — Verify

Run `cd core && cargo check -p app-core`. The compiler will point out any `match` arms on `AppError` or `AppErrorFfi` that need updating — fix them all (exhaustive match is enforced).

Report: the variant definition, where it is returned, and any match arms updated.
