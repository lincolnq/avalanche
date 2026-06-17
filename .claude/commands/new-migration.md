Create a new SQL migration file for `$ARGUMENTS`.

## Step 1 — Find the next sequence number

List all files in `infra/migrations/`. The filename format is `NNN_description.sql` (three-digit zero-padded number). Find the highest existing number and add 1 to get the next sequence number.

## Step 2 — Create the file

Create `infra/migrations/NNN_$ARGUMENTS.sql` where:
- `NNN` is the next sequence number, zero-padded to 3 digits
- `$ARGUMENTS` is the migration name, with spaces replaced by underscores

Use this template:

```sql
-- Migration NNN: $ARGUMENTS
-- <one-line description of what this migration does>

-- TODO: add your SQL here

-- Example patterns:
--
-- New table:
-- CREATE TABLE foo (
--     id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
--     account_id BIGINT NOT NULL REFERENCES accounts(id),
--     created_at TIMESTAMPTZ NOT NULL DEFAULT now()
-- );
-- CREATE INDEX idx_foo_account ON foo (account_id);
--
-- New column:
-- ALTER TABLE foo ADD COLUMN bar TEXT;
--
-- New index:
-- CREATE INDEX idx_foo_bar ON foo (bar);
```

## Step 3 — Report

Show the full file path and confirm the sequence number is correct (no gaps, no duplicates). Remind the user to add a DOWN migration comment if they want rollback support, and to run `make db-up` before testing with `sqlx migrate run`.
