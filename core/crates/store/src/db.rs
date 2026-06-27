//! Database handles and key — the entry point for the `store` crate.
//!
//! The local store is split into two SQLCipher databases (docs/06):
//!
//! - [`DeviceStore`] — per-device transport crypto and server-bound caches.
//!   Implements [`crypto::Store`] (the libsignal store traits). Scope: one per
//!   `(device, server)` account context. Never synced; fully rebuildable.
//! - [`IdentityStore`] — durable per-identity state (contacts, groups, profiles,
//!   identity/rotation/storage keys, the trust store, the storage-sync sidecar).
//!   Scope: one logical store per DID; each device holds a replica.
//!
//! Both are `Clone` and wrap a `tokio-rusqlite` [`Connection`] that serializes
//! all SQLite work on a dedicated blocking thread. Cloning shares the underlying
//! connection — load-bearing for libsignal's multi-`&mut` API (see
//! [`DeviceStore`]).
//!
//! A `DeviceStore` carries a handle to its `IdentityStore`
//! ([`DeviceStore::identity`]): the libsignal `IdentityKeyStore` impl needs the
//! identity keypair and trust store, which live in identity.db — the §5
//! "boundary-crossers" made explicit. App-core also reaches durable methods
//! through this field, so a helper that already holds a `DeviceStore` doesn't
//! need a second parameter.
//!
//! [`DatabaseKey`] is intentionally opaque; its internals come from the platform
//! secure enclave in production. The *same* key encrypts both files at rest
//! (docs/06 §7) — the record-level storage key is a separate concern.

use std::path::{Path, PathBuf};
use tokio_rusqlite::Connection;

use crate::error::StoreError;

/// Durable per-identity state (identity.db). See the module docs.
///
/// Cheap to clone; clones share one connection. There is no libsignal
/// multi-`&mut` constraint on this store (that applies to [`DeviceStore`]).
#[derive(Clone)]
pub struct IdentityStore {
    pub(crate) conn: Connection,
}

/// Per-device transport crypto + server-bound caches (device.db). Implements
/// [`crypto::Store`]. See the module docs.
///
/// `Clone` is required so libsignal's session functions — which take separate
/// `&mut dyn SessionStore` and `&mut dyn IdentityKeyStore` parameters — can both
/// be satisfied from a single store by cloning the handle. Clones share the
/// underlying connection (and the same [`identity`](Self::identity) handle).
///
/// # Concurrency safety
///
/// Multiple clones may write the same database; this is safe because
/// `tokio-rusqlite` serializes all operations through one blocking thread. **Do
/// not replace `Connection` with a pool** — concurrent writes could corrupt
/// Double Ratchet state.
#[derive(Clone)]
pub struct DeviceStore {
    pub(crate) conn: Connection,
    /// The identity store this device belongs to. Used by the `IdentityKeyStore`
    /// impl (identity keypair + trust store live in identity.db) and as an
    /// ergonomic path to durable methods for code that already holds a
    /// `DeviceStore`.
    pub identity: IdentityStore,
}

/// `DeviceStore` derefs to its `IdentityStore` so durable per-identity methods
/// (e.g. `load_group`, `touch_contact`) are callable on a `DeviceStore` handle
/// without an explicit `.identity` hop — the ergonomic form of the §5
/// boundary-crosser. The device-cache method names are disjoint from the
/// identity method names, so resolution is unambiguous: device methods bind to
/// `DeviceStore` inherently, durable methods deref to `IdentityStore`. The
/// architectural boundary (two types, two files, two connections) is unaffected;
/// this only sugars method resolution.
impl std::ops::Deref for DeviceStore {
    type Target = IdentityStore;
    fn deref(&self) -> &IdentityStore {
        &self.identity
    }
}

/// Opaque database encryption key. Derived from the device secure enclave in
/// production; both split databases are opened with the same key.
pub struct DatabaseKey(pub(crate) String);

impl DatabaseKey {
    /// A fixed development key. Never use in production.
    pub fn dev_key() -> Self {
        Self("actnet-dev-placeholder-key".to_string())
    }

    /// Construct from an arbitrary passphrase (e.g. derived from secure enclave).
    pub fn from_passphrase(passphrase: impl Into<String>) -> Self {
        Self(passphrase.into())
    }
}

/// Apply the SQLCipher key to a freshly opened connection.
async fn apply_key(conn: &Connection, key: &DatabaseKey) -> Result<(), StoreError> {
    let passphrase = key.0.clone();
    conn.call(move |conn| {
        // Never interpolate the passphrase into SQL — pragma_update binds it.
        conn.pragma_update(None, "key", &passphrase)?;
        Ok(())
    })
    .await
    .map_err(StoreError::Db)
}

/// Add `column` to `table` with `decl` (type + constraints) if it isn't already
/// present. SQLite lacks `ADD COLUMN IF NOT EXISTS`, so we probe `table_info`
/// first. Synchronous: runs inside the `migrate` connection closure.
fn add_column_if_missing(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
    decl: &str,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let exists = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|c| c == column);
    if !exists {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
    }
    Ok(())
}

/// True if a table with `name` exists in the database behind `conn`.
async fn table_exists(conn: &Connection, name: &'static str) -> Result<bool, StoreError> {
    conn.call(move |conn| {
        let n: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get(0),
        )?;
        Ok(n > 0)
    })
    .await
    .map_err(StoreError::Db)
}

impl IdentityStore {
    /// Open (or create) the identity database at `path`, applying the key and
    /// running [`schema::IDENTITY_MIGRATIONS`](crate::schema::IDENTITY_MIGRATIONS).
    pub async fn open(path: &Path, key: &DatabaseKey) -> Result<Self, StoreError> {
        let conn = Connection::open(path).await?;
        apply_key(&conn, key).await?;
        let store = Self { conn };
        store.migrate().await?;
        Ok(store)
    }

    /// Open an in-memory identity database. Useful for tests.
    pub async fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().await?;
        let store = Self { conn };
        store.migrate().await?;
        Ok(store)
    }

    /// Apply the identity schema (idempotent).
    pub async fn migrate(&self) -> Result<(), StoreError> {
        self.conn
            .call(|conn| {
                conn.execute_batch(crate::schema::IDENTITY_MIGRATIONS)?;
                // Column additions to existing tables: the schema batch above
                // only `CREATE TABLE IF NOT EXISTS`, so a database created
                // before a column was added needs an explicit `ADD COLUMN`.
                // SQLite has no `ADD COLUMN IF NOT EXISTS`, so add only what's
                // missing (per `PRAGMA table_info`). Keep this list append-only.
                add_column_if_missing(
                    conn,
                    "contacts",
                    "is_blocked",
                    "INTEGER NOT NULL DEFAULT 0",
                )?;
                add_column_if_missing(
                    conn,
                    "contacts",
                    "has_pending_request",
                    "INTEGER NOT NULL DEFAULT 0",
                )?;
                // docs/03 §3.6 group metadata/system timeline entries.
                add_column_if_missing(
                    conn,
                    "message_history",
                    "kind",
                    "INTEGER NOT NULL DEFAULT 0",
                )?;
                add_column_if_missing(conn, "message_history", "metadata", "TEXT")?;
                // docs/03 §5 disappearing-messages enforcement.
                add_column_if_missing(
                    conn,
                    "message_history",
                    "expire_timer_secs",
                    "INTEGER NOT NULL DEFAULT 0",
                )?;
                add_column_if_missing(conn, "message_history", "expire_at", "INTEGER")?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// One-time migration from a pre-split single-file database (docs/06 §11).
    ///
    /// The identity database *is* the original combined file, so all identity
    /// tables are already in place. This copies the DID out of the legacy
    /// `account` row into `account_identity`, then drops the device-owned tables
    /// (and the legacy `account` table) that no longer belong here. A no-op if
    /// there is no legacy `account` table.
    async fn migrate_from_legacy(&self) -> Result<(), StoreError> {
        if !table_exists(&self.conn, "account").await? {
            return Ok(());
        }
        self.conn
            .call(|conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO account_identity (id, did, registered_at) \
                     SELECT 1, account_id, registered_at FROM account WHERE id = 1",
                    [],
                )?;
                // Normalize the legacy `identity_keypair` (it carried a
                // per-device `registration_id NOT NULL` column that has moved to
                // device.db). Rebuild without it so keypair re-saves don't hit a
                // missing-default NOT NULL.
                conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS identity_keypair_new ( \
                       id INTEGER PRIMARY KEY CHECK (id = 1), keypair_bytes BLOB NOT NULL); \
                     INSERT OR IGNORE INTO identity_keypair_new (id, keypair_bytes) \
                       SELECT id, keypair_bytes FROM identity_keypair; \
                     DROP TABLE identity_keypair; \
                     ALTER TABLE identity_keypair_new RENAME TO identity_keypair;",
                )?;
                for t in crate::schema::DEVICE_TABLES {
                    conn.execute(&format!("DROP TABLE IF EXISTS {t}"), [])?;
                }
                conn.execute("DROP TABLE IF EXISTS account", [])?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }
}

impl DeviceStore {
    /// Open (or create) the device database at `path`, applying the key and
    /// running [`schema::DEVICE_MIGRATIONS`](crate::schema::DEVICE_MIGRATIONS).
    /// `identity` is the already-opened identity store this device belongs to.
    pub async fn open(
        path: &Path,
        key: &DatabaseKey,
        identity: IdentityStore,
    ) -> Result<Self, StoreError> {
        let conn = Connection::open(path).await?;
        apply_key(&conn, key).await?;
        let store = Self { conn, identity };
        store.migrate().await?;
        Ok(store)
    }

    /// Open an in-memory device database, bundling a fresh in-memory identity
    /// store. Useful for device-crypto tests that don't need a shared identity.
    pub async fn open_in_memory() -> Result<Self, StoreError> {
        let identity = IdentityStore::open_in_memory().await?;
        let conn = Connection::open_in_memory().await?;
        let store = Self { conn, identity };
        store.migrate().await?;
        Ok(store)
    }

    /// Apply the device schema (idempotent).
    pub async fn migrate(&self) -> Result<(), StoreError> {
        self.conn
            .call(|conn| {
                conn.execute_batch(crate::schema::DEVICE_MIGRATIONS)?;
                // `sender_key_shared` gained `recipient_device_id` in its primary
                // key for multi-device groups (docs/04). It is a rebuildable
                // cache, so a database created with the old per-DID shape is just
                // dropped and recreated — the next group send re-distributes
                // SKDMs idempotently. `CREATE TABLE IF NOT EXISTS` above won't
                // alter an existing table, hence this explicit recreate.
                let has_device_col = {
                    let mut stmt = conn.prepare("PRAGMA table_info(sender_key_shared)")?;
                    let found = stmt
                        .query_map([], |row| row.get::<_, String>(1))?
                        .filter_map(Result::ok)
                        .any(|c| c == "recipient_device_id");
                    found
                };
                if !has_device_col {
                    conn.execute_batch(
                        "DROP TABLE IF EXISTS sender_key_shared;
                         CREATE TABLE sender_key_shared (
                             group_id            TEXT NOT NULL,
                             recipient_did       TEXT NOT NULL,
                             recipient_device_id INTEGER NOT NULL,
                             PRIMARY KEY (group_id, recipient_did, recipient_device_id)
                         );",
                    )?;
                }
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// One-time migration from a pre-split single-file database (docs/06 §11).
    ///
    /// The device database is a byte copy of the legacy combined file, so it
    /// holds every legacy table. This reconstructs the `device_account` row from
    /// the legacy `account` row + `identity_keypair.registration_id`, then drops
    /// the identity-owned tables (and the legacy `account` table). A no-op if
    /// there is no legacy `account` table.
    async fn migrate_from_legacy(&self) -> Result<(), StoreError> {
        if !table_exists(&self.conn, "account").await? {
            return Ok(());
        }
        self.conn
            .call(|conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO device_account \
                       (id, server_url, device_id, registered_at, registration_id) \
                     SELECT 1, a.server_url, a.device_id, a.registered_at, k.registration_id \
                     FROM account a, identity_keypair k WHERE a.id = 1 AND k.id = 1",
                    [],
                )?;
                for t in crate::schema::IDENTITY_TABLES {
                    conn.execute(&format!("DROP TABLE IF EXISTS {t}"), [])?;
                }
                conn.execute("DROP TABLE IF EXISTS account", [])?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

}

/// Derive the device-database path that sits beside the identity database path.
/// `/foo/actnet.db` → `/foo/actnet.db.device`.
fn device_path_for(identity_path: &Path) -> PathBuf {
    let mut s = identity_path.as_os_str().to_owned();
    s.push(".device");
    PathBuf::from(s)
}

/// Open the split store at `db_path`, migrating a pre-split single-file database
/// in place on first run (docs/06 §11).
///
/// `db_path` names the **identity** database; the device database is a sibling
/// at `<db_path>.device`. On the first open after the split, when only the
/// single legacy file exists, it is byte-copied to the device path and each side
/// then prunes the tables it doesn't own — preserving all data without any
/// cross-file `INSERT…SELECT`.
///
/// Returns `(identity, device)`, where `device.identity` is a clone of the
/// returned identity (they share the identity connection).
pub async fn open_split(
    db_path: &Path,
    key: &DatabaseKey,
) -> Result<(IdentityStore, DeviceStore), StoreError> {
    let device_path = device_path_for(db_path);

    // Legacy single-file DB present, no device sibling yet → migrate. (A fresh
    // install has neither file; an already-split install has both.)
    let legacy = db_path.exists() && !device_path.exists();
    if legacy {
        copy_db_files(db_path, &device_path)?;
    }

    let identity = IdentityStore::open(db_path, key).await?;
    let device = DeviceStore::open(&device_path, key, identity.clone()).await?;

    if legacy {
        identity.migrate_from_legacy().await?;
        device.migrate_from_legacy().await?;
    }

    Ok((identity, device))
}

/// Open an in-memory split store: two independent in-memory databases sharing no
/// state with anything else, with `device.identity` pointing at the returned
/// identity. Useful for app-core tests.
pub async fn open_in_memory_split() -> Result<(IdentityStore, DeviceStore), StoreError> {
    let identity = IdentityStore::open_in_memory().await?;
    let conn = Connection::open_in_memory().await?;
    let device = DeviceStore { conn, identity: identity.clone() };
    device.migrate().await?;
    Ok((identity, device))
}

/// Copy a SQLCipher database file (and its WAL/SHM sidecars, if present) from
/// `src` to `dst`. Both files share the same key, so a byte copy is a valid
/// encrypted database. Done before any connection is opened on either path.
fn copy_db_files(src: &Path, dst: &Path) -> Result<(), StoreError> {
    std::fs::copy(src, dst).map_err(StoreError::Io)?;
    for ext in ["-wal", "-shm"] {
        let mut from = src.as_os_str().to_owned();
        from.push(ext);
        let from = PathBuf::from(from);
        if from.exists() {
            let mut to = dst.as_os_str().to_owned();
            to.push(ext);
            std::fs::copy(&from, PathBuf::from(to)).map_err(StoreError::Io)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod migration_tests {
    use super::*;

    /// Build a pre-split single-file database with the legacy combined schema
    /// (a representative subset) and a row in each scope, then assert
    /// `open_split` migrates it: DID → `account_identity`; server binding +
    /// registration_id → `device_account`; identity rows stay in identity.db and
    /// device rows move to device.db; each side drops the other's tables.
    #[tokio::test]
    async fn open_split_migrates_legacy_single_file() {
        // Unique temp path (test-only timestamp; not a workflow script).
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir();
        let id_path = dir.join(format!("actnet-legacy-{nanos}.db"));
        let dev_path = device_path_for(&id_path);
        let key = DatabaseKey::dev_key();

        // 1. Fabricate the legacy combined file with the old schema + data.
        {
            let conn = Connection::open(&id_path).await.unwrap();
            apply_key(&conn, &key).await.unwrap();
            conn.call(|conn| {
                conn.execute_batch(
                    "CREATE TABLE identity_keypair (id INTEGER PRIMARY KEY CHECK (id=1), \
                       keypair_bytes BLOB NOT NULL, registration_id INTEGER NOT NULL);
                     CREATE TABLE account (id INTEGER PRIMARY KEY CHECK (id=1), \
                       account_id TEXT NOT NULL, server_url TEXT NOT NULL, \
                       registered_at INTEGER NOT NULL, device_id INTEGER NOT NULL DEFAULT 1);
                     CREATE TABLE sessions (address TEXT PRIMARY KEY, record BLOB NOT NULL);
                     CREATE TABLE contacts (did TEXT PRIMARY KEY, is_curated INTEGER NOT NULL \
                       DEFAULT 0, last_interaction_at INTEGER NOT NULL DEFAULT 0);
                     INSERT INTO identity_keypair (id, keypair_bytes, registration_id) \
                       VALUES (1, x'0102', 4242);
                     INSERT INTO account (id, account_id, server_url, registered_at, device_id) \
                       VALUES (1, 'did:plc:legacy', 'https://hs.example', 111, 3);
                     INSERT INTO sessions (address, record) VALUES ('peer.1', x'aabb');
                     INSERT INTO contacts (did, is_curated, last_interaction_at) \
                       VALUES ('did:plc:friend', 1, 99);",
                )?;
                Ok(())
            })
            .await
            .unwrap();
        }
        assert!(id_path.exists() && !dev_path.exists());

        // 2. Open split → triggers the one-time migration.
        let (identity, device) = open_split(&id_path, &key).await.unwrap();

        // 3. DID + identity rows landed in identity.db.
        let (did, _) = identity.load_did().await.unwrap().expect("did migrated");
        assert_eq!(did, "did:plc:legacy");
        let contact = identity.load_contact("did:plc:friend").await.unwrap();
        assert!(contact.is_some(), "identity-scoped contact preserved");

        // 4. Server binding + registration_id landed in device.db.
        let dev = device
            .load_device_account()
            .await
            .unwrap()
            .expect("device_account migrated");
        assert_eq!(dev.server_url, "https://hs.example");
        assert_eq!(dev.device_id, 3);
        assert_eq!(dev.registration_id, 4242);

        // 5. Each side dropped the other's tables.
        assert!(!table_exists(&identity.conn, "sessions").await.unwrap());
        assert!(!table_exists(&identity.conn, "account").await.unwrap());
        assert!(!table_exists(&device.conn, "contacts").await.unwrap());
        assert!(table_exists(&device.conn, "sessions").await.unwrap());

        // 6. Re-opening is a no-op (both files now exist → not legacy).
        drop((identity, device));
        let (identity2, _device2) = open_split(&id_path, &key).await.unwrap();
        assert_eq!(
            identity2.load_did().await.unwrap().unwrap().0,
            "did:plc:legacy"
        );

        let _ = std::fs::remove_file(&id_path);
        let _ = std::fs::remove_file(&dev_path);
    }
}
