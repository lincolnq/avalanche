# Core API Sketch — `crypto` and `store` crates

This document sketches the public API surface of the two foundational crates in `core/`: `crypto` and `store`. It is a design document, not an implementation — the goal is to nail down the shape and seams before writing code.

---

## The key seam

libsignal's session operations are stateful: the Double Ratchet advances with every message, and that state must be persisted between calls. This creates a dependency between cryptographic operations and storage.

Rather than having `crypto` do its own I/O, it defines a `Store` trait that the caller must provide. The `store` crate implements that trait against SQLCipher. `app-core` holds both and wires them together.

```
app-core
  │
  ├── opens Store (store crate)
  ├── passes &mut store to crypto::encrypt(...)   ← ratchet state saved here
  └── passes &mut store to crypto::decrypt(...)   ← ratchet state advanced here
```

`crypto` never touches a database. `store` never does cryptographic math. The boundary is clean.

---

## `crypto` crate

The `crypto` crate wraps libsignal. All operations are pure logic — no I/O, no async beyond what the `Store` trait requires. This makes it straightforward to unit test and fuzz without any infrastructure.

```rust
// crypto/src/lib.rs
pub mod identity;
pub mod prekeys;
pub mod session;
pub mod groups;  // zkgroup anonymous credentials — added in Stage 4, stubbed initially
```

### Identity

```rust
// crypto/src/identity.rs

/// A local identity — the long-term key pair that identifies a user.
/// Generated once at account creation; the public half is published to the server.
pub struct IdentityKeyPair(/* libsignal internals, opaque */);

impl IdentityKeyPair {
    pub fn generate() -> Self;
    pub fn public_key(&self) -> IdentityKey;
    pub fn serialize(&self) -> Vec<u8>;
    pub fn deserialize(bytes: &[u8]) -> Result<Self, CryptoError>;
}

/// The public identity key — what other users see and what the server stores.
pub struct IdentityKey(/* opaque */);

impl IdentityKey {
    pub fn serialize(&self) -> Vec<u8>;
    pub fn deserialize(bytes: &[u8]) -> Result<Self, CryptoError>;
}
```

### Prekeys

Prekeys are what make asynchronous session initiation possible (X3DH). A user publishes a bundle of prekeys to the server; another user fetches that bundle and uses it to start an encrypted session before the recipient is online.

```rust
// crypto/src/prekeys.rs

/// What we publish to the server so others can initiate sessions with us
/// before we're online (X3DH).
pub struct LocalKeyBundle {
    pub identity_key: IdentityKey,
    pub signed_prekey: SignedPreKey,
    pub one_time_prekeys: Vec<OneTimePreKey>,
}

pub struct SignedPreKey {
    pub id: u32,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,   // signed by the identity key
}

pub struct OneTimePreKey {
    pub id: u32,
    pub public_key: Vec<u8>,
}

pub fn generate_signed_prekey(identity: &IdentityKeyPair, id: u32) -> SignedPreKey;
pub fn generate_one_time_prekeys(start_id: u32, count: usize) -> Vec<OneTimePreKey>;

/// What we fetch from the server when we want to message someone.
/// Their bundle lets us initiate an X3DH session without them being online.
pub struct RecipientKeyBundle {
    pub identity_key: IdentityKey,
    pub signed_prekey: SignedPreKey,
    pub one_time_prekey: Option<OneTimePreKey>,  // may be absent if their pool ran out
}
```

### Sessions

The `Store` trait is defined here. It composes the four libsignal store sub-traits that session operations require. The `store` crate implements this trait; `crypto` just declares it.

```rust
// crypto/src/session.rs

/// The Store trait — crypto defines it, store implements it.
/// Async because the implementation does I/O.
#[async_trait]
pub trait Store:
    libsignal_protocol::IdentityKeyStore
    + libsignal_protocol::SessionStore
    + libsignal_protocol::PreKeyStore
    + libsignal_protocol::SignedPreKeyStore
    + Send
    + Sync
{}

/// An encrypted message, ready to hand to the server.
pub struct EncryptedMessage {
    pub ciphertext: Vec<u8>,
    pub kind: MessageKind,
}

/// PreKey messages are sent to start a new session (X3DH);
/// Whisper messages use the established Double Ratchet session.
pub enum MessageKind {
    PreKey,
    Whisper,
}

/// Opaque address of a remote device.
pub struct DeviceAddress {
    pub account_id: AccountId,  // from types crate
    pub device_id: DeviceId,    // from types crate
}

/// Process a recipient's key bundle and establish a session.
/// Called once per (local identity, recipient device) pair before the first message.
pub async fn initiate_session(
    store: &mut impl Store,
    recipient: &DeviceAddress,
    bundle: &RecipientKeyBundle,
) -> Result<(), CryptoError>;

/// Encrypt a plaintext for a recipient we have an established session with.
/// Advances the sending ratchet; the updated session state is saved to store.
pub async fn encrypt(
    store: &mut impl Store,
    recipient: &DeviceAddress,
    plaintext: &[u8],
) -> Result<EncryptedMessage, CryptoError>;

/// Decrypt a message from a sender, advancing the receiving ratchet.
/// The updated session state is saved to store.
pub async fn decrypt(
    store: &mut impl Store,
    sender: &DeviceAddress,
    message: &EncryptedMessage,
) -> Result<Vec<u8>, CryptoError>;
```

---

## `store` crate

`store` has two responsibilities:

1. **Implement `crypto::Store`** so that session state survives across app launches.
2. **Provide its own API** for things `crypto` doesn't know about: account state, the prekey pool, the outbound message queue, and (in Stage 4) group state.

```rust
// store/src/lib.rs
pub mod db;       // SQLCipher connection setup, key derivation
pub mod session;  // implements crypto::Store traits
pub mod account;  // local identity, device registration state
pub mod prekeys;  // prekey pool management
pub mod messages; // outbound message queue
pub mod groups;   // group state, member lists (Stage 4)
```

### Top-level handle

```rust
// store/src/db.rs

/// The top-level store handle. Everything goes through this.
/// Holds a SQLCipher connection pool; cheap to clone.
#[derive(Clone)]
pub struct Store { /* pool: sqlx::SqlitePool */ }

impl Store {
    /// Open (or create) the encrypted local database at the given path.
    /// `key` is a placeholder in Stage 1; wired to the platform secure enclave
    /// in Stage 3.
    pub async fn open(path: &Path, key: &DatabaseKey) -> Result<Self, StoreError>;

    /// Run any pending schema migrations.
    pub async fn migrate(&self) -> Result<(), StoreError>;
}

/// Opaque database encryption key.
///
/// Stage 1: derived from a constant or environment variable.
/// Stage 3: derived from a secret held in the iOS Secure Enclave or
///          Android Keystore, so the database is useless without the device.
pub struct DatabaseKey(/* opaque */);
```

`DatabaseKey` is opaque from day one so that the Stage 3 secure enclave wiring is a change to how the key is derived, not a refactor of the API.

### Trait implementation

```rust
// store/src/session.rs

// Store implements all four libsignal store traits,
// satisfying the crypto::Store bound.
impl libsignal_protocol::SessionStore for Store { ... }
impl libsignal_protocol::IdentityKeyStore for Store { ... }
impl libsignal_protocol::PreKeyStore for Store { ... }
impl libsignal_protocol::SignedPreKeyStore for Store { ... }

// Blanket impl — no extra methods needed beyond the four above.
impl crypto::Store for Store {}
```

### Account state

```rust
// store/src/account.rs

impl Store {
    pub async fn save_identity(&self, keypair: &IdentityKeyPair) -> Result<(), StoreError>;
    pub async fn load_identity(&self) -> Result<Option<IdentityKeyPair>, StoreError>;
    pub async fn save_registration(&self, info: &RegistrationInfo) -> Result<(), StoreError>;
    pub async fn load_registration(&self) -> Result<Option<RegistrationInfo>, StoreError>;
}
```

### Prekey pool

The prekey pool needs active management: one-time prekeys are consumed as other users initiate sessions, and the pool must be refilled before it runs dry. The store tracks what's available; the refill logic lives in `app-core`.

```rust
// store/src/prekeys.rs

impl Store {
    pub async fn save_one_time_prekeys(&self, keys: &[OneTimePreKey]) -> Result<(), StoreError>;
    pub async fn remaining_one_time_prekey_count(&self) -> Result<usize, StoreError>;
    pub async fn save_signed_prekey(&self, key: &SignedPreKey) -> Result<(), StoreError>;
}
```

### Outbound message queue

Encrypted messages that couldn't be delivered immediately (e.g. the server was unreachable) are held here and drained when connectivity returns.

```rust
// store/src/messages.rs

impl Store {
    pub async fn enqueue(&self, msg: &QueuedMessage) -> Result<(), StoreError>;
    pub async fn drain(&self) -> Result<Vec<QueuedMessage>, StoreError>;
    pub async fn mark_delivered(&self, id: MessageId) -> Result<(), StoreError>;
}

pub struct QueuedMessage {
    pub id: MessageId,
    pub recipient: DeviceAddress,
    pub encrypted: EncryptedMessage,
    pub enqueued_at: Timestamp,
}
```

---

## Design notes

**`crypto::Store` re-exports libsignal traits directly.** This means both `crypto` and `store` have a direct libsignal dependency and must stay on the same version. The workspace `Cargo.toml` enforces this — libsignal appears once, at the workspace level.

**`Store` is a single struct, not split by concern.** You could imagine `SessionStore`, `AccountStore`, etc. as separate types. A single `Store` handle with distinct impl blocks is simpler and avoids duplicating the connection pool.

**`Store` clones share a single connection, serialized by `tokio-rusqlite`.** libsignal's session functions require separate `&mut` references for different store sub-traits. We satisfy this by cloning the `Arc`-backed `Store` handle. This is safe because `tokio-rusqlite` serializes all operations through one blocking thread — there is never concurrent SQLite access. Do not replace the `Connection` with a connection pool without revisiting this invariant.

**`crypto` functions take `&mut impl Store`, not `&mut dyn Store`.** Monomorphization rather than dynamic dispatch — no vtable overhead on the hot encrypt/decrypt path.

**Multi-device: `app-core` encrypts per recipient device.** The session functions operate on a single `(AccountId, DeviceId)` pair. When sending a message, `app-core` is responsible for looking up all of the recipient's registered devices and calling `encrypt` once per device. The server fans out the resulting ciphertexts.

**Message envelope is protobuf.** The plaintext inside `EncryptedMessage.ciphertext` is a serialized `ContentMessage` protobuf (defined in `proto/content.proto`). The `crypto` crate treats plaintext as opaque `&[u8]`; `app-core` handles serialization/deserialization of the envelope.

**Group state is stubbed in Stage 1.** The `groups` module in `store` exists as an empty file. The schema will be designed alongside the zkgroup integration in Stage 4, but the module boundary is established early so Stage 4 doesn't require reorganizing the crate.
