Scaffold a new server DB module for the entity `$ARGUMENTS`.

## Step 1 — Create the module file

Create `core/crates/server/src/db/$ARGUMENTS.rs` using this pattern:

```rust
//! $ARGUMENTS persistence: <one-line description of what this module manages>.

use sqlx::{PgConnection, Row};

/// <description of the primary record type, if any>
pub struct $ArgumentsPascalCase {
    pub id: i64,
    // add fields here
}

/// <description>
pub async fn create(
    conn: &mut PgConnection,
    // parameters
) -> Result<$ArgumentsPascalCase, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO $ARGUMENTS (...) VALUES (...) RETURNING id, ...",
    )
    // .bind(param)
    .fetch_one(&mut *conn)
    .await?;
    Ok($ArgumentsPascalCase {
        id: row.get("id"),
        // map fields
    })
}

/// <description>
pub async fn find_by_id(
    conn: &mut PgConnection,
    id: i64,
) -> Result<Option<$ArgumentsPascalCase>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM $ARGUMENTS WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.map(|r| $ArgumentsPascalCase {
        id: r.get("id"),
        // map fields
    }))
}
```

Conventions:
- All functions take `&mut PgConnection` as the first parameter (not `&PgPool`) — this enables transaction-rollback testing
- Return `Result<T, sqlx::Error>` — never unwrap
- Use `.bind()` for all query parameters — never interpolate values into SQL strings
- Use `.fetch_one()` / `.fetch_optional()` / `.fetch_all()` / `.execute()` as appropriate

## Step 2 — Register the module

Open `core/crates/server/src/db/mod.rs` and add:

```rust
pub mod $ARGUMENTS;
```

in alphabetical order with the other module declarations.

## Step 3 — Verify

Run `cd core && cargo check -p server` and fix any errors.

Report: the full file path and the public function signatures created.
