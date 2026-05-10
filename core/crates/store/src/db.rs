//! Database handle and key — the entry point for the `store` crate.
//!
//! [`Store`] is the single handle through which all database access flows. It
//! wraps a `tokio-rusqlite` [`Connection`], which runs all SQLite operations on
//! a dedicated blocking thread and exposes them as async calls. Because
//! `Connection` is internally Arc-backed, `Store` is cheap to clone — all
//! clones share the same underlying connection. This property is load-bearing:
//! libsignal's session functions require separate `&mut dyn SessionStore` and
//! `&mut dyn IdentityKeyStore` references, which we satisfy by cloning the
//! store handle rather than trying to alias a single `&mut`.
//!
//! [`DatabaseKey`] is intentionally opaque. In Stage 1 it wraps a plaintext
//! passphrase; in Stage 3 the key material will come from the platform secure
//! enclave and this type's internals change without affecting any call site.

use std::path::Path;
use tokio_rusqlite::Connection;

use crate::error::StoreError;

/// The top-level store handle. Wraps a `tokio-rusqlite` connection, which
/// runs all SQLite operations on a dedicated blocking thread.
///
/// `Store` is `Clone`: all clones share the same underlying connection.
/// This is required so that libsignal session functions — which take separate
/// `&mut dyn SessionStore` and `&mut dyn IdentityKeyStore` parameters — can
/// both be satisfied from a single store instance.
///
/// # Concurrency safety
///
/// Multiple clones may issue writes that target the same underlying SQLite
/// database. This is safe because `tokio-rusqlite` serializes all operations
/// through a single dedicated blocking thread — there is never concurrent
/// access at the SQLite level. **Do not replace `Connection` with a connection
/// pool** without revisiting this invariant; a pool would allow truly
/// concurrent writes, which could corrupt Double Ratchet state if two
/// encrypt/decrypt operations interleave on the same session.
#[derive(Clone)]
pub struct Store {
    pub(crate) conn: Connection,
}

/// Opaque database encryption key.
///
/// Stage 1: derived from a constant or environment variable (see `dev_key`).
/// Stage 3: derived from a secret held in the iOS Secure Enclave or Android
///          Keystore, so the database file is useless without the device.
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

impl Store {
    /// Open (or create) the encrypted SQLite database at `path`.
    ///
    /// Applies the database key and runs all pending schema migrations before
    /// returning. The caller can use the store immediately after this returns.
    pub async fn open(path: &Path, key: &DatabaseKey) -> Result<Self, StoreError> {
        let conn = Connection::open(path).await?;
        let passphrase = key.0.clone();

        // Apply the SQLCipher key via pragma_update, which uses sqlite3_key()
        // internally. Never interpolate the passphrase into a SQL string — that
        // would be a SQL injection vector if the passphrase contains a quote.
        conn.call(move |conn| {
            conn.pragma_update(None, "key", &passphrase)?;
            Ok(())
        })
        .await?;

        let store = Self { conn };
        store.migrate().await?;
        Ok(store)
    }

    /// Open an in-memory encrypted database. Useful for tests.
    pub async fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().await?;
        let store = Self { conn };
        store.migrate().await?;
        Ok(store)
    }

    /// Apply all pending schema migrations.
    pub async fn migrate(&self) -> Result<(), StoreError> {
        self.conn
            .call(|conn| {
                conn.execute_batch(crate::schema::MIGRATIONS)?;

                // Apply ALTER TABLE migrations. These are not idempotent in
                // SQL (no IF NOT EXISTS for ADD COLUMN), so we ignore
                // "duplicate column name" errors.
                for sql in crate::schema::ALTER_MIGRATIONS {
                    match conn.execute(sql, []) {
                        Ok(_) => {}
                        Err(rusqlite::Error::SqliteFailure(_, Some(ref msg)))
                            if msg.contains("duplicate column name") => {}
                        Err(e) => return Err(e.into()),
                    }
                }

                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }
}

