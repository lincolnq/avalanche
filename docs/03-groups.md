# Groups: detailed design

Status: **in-progress design doc.** This is the working document for Stage 5
(action-bound groups) and Stage 9 (cross-server casual groups). Sections
marked **OPEN** are unresolved; sections marked **DECIDED** are committed.

Background reading:

- `docs/00-design.md` §"Two kinds of groups" — product framing.
- `docs/01-technical-implementation.md` §"Cross-server casual group encryption" — Sender Keys sketch.
- Chase, Perrin, Zaverucha 2019, *The Signal Private Group System*.
- Signal blog, *Technology preview: Private contact discovery for Signal*, and the *Private Groups* writeup.
- libsignal source at pinned commit `4c460615`, crates `rust/zkgroup` and `rust/zkcredential`.

## 1. Scope of this document

Two questions drive the doc:

1. **Can we use libsignal's zkgroup as-is?** What does the API give us, what does it not, and where do we have to do new work?
2. **What is the encrypted group state model?** What does "the group" actually consist of, where does it live, who can change it, and how do members fetch and apply changes?

Out of scope (covered elsewhere or deferred):

- Abuse handling and sender-cert disclosure flow — sketched in §3.11; deferred to `docs/12-abuse-handling.md` for the full design.
- Federated guest credentials — separate doc; depends on this one.
- Cross-server Sender Keys protocol details — Stage 9, separate doc; this doc only restates the chosen scheme.
- Group calls (LiveKit + Insertable Streams) — deferred to Stage 12.

## 2. zkgroup in our pinned libsignal

### 2.1 What's there

`libsignal-zkgroup` is a separate crate in the libsignal monorepo (`rust/zkgroup`, AGPL-3.0). We do not currently depend on it — only `libsignal-protocol`. Adding it is a one-line addition to the workspace `Cargo.toml`:

```toml
libsignal-zkgroup = { git = "https://github.com/signalapp/libsignal", rev = "4c460615cdbe4ed53b23a5d1bf71493514bf2a80" }
```

The public surface we care about:

- **Server params.** `ServerSecretParams::generate(randomness)` / `ServerPublicParams`. The homeserver holds the secret, publishes the public. One pair per homeserver (not per group) — the secret is the credential-signing key.
- **Group params.** `GroupMasterKey` (32 bytes) → `GroupSecretParams::derive_from_master_key(...)` → `GroupPublicParams`. The master key is created by the group founder and shared E2E with members; it is the long-term group key. From it: `get_group_identifier()` (public, used by server to route), `encrypt_service_id` / `decrypt_service_id` (members ↔ pseudonymous ciphertexts the server stores), `encrypt_blob` / `decrypt_blob` (encrypted group state, AES-256-GCM-SIV).
- **Auth credentials.** `AuthCredentialWithPniZkc` is the current ZKC-based credential. Flow: server issues with `AuthCredentialWithPniZkcResponse::issue_credential(aci, pni, redemption_time, server_secret, randomness)`; client `receive`s it; client `present`s tied to a specific `GroupSecretParams`; server `verify`s the presentation. The presentation carries the *encrypted* `aci`/`pni` under the group key — server can verify it's a valid credential but can't link to identity.
- **Group send endorsements.** `GroupSendEndorsement` and friends: per-recipient anonymous tokens that let a member prove "I am allowed to send to recipient X in this group" without revealing who they are. This is the post-2023 replacement for the old sealed-sender-with-auth-credential path. Critical for the multi-recipient sealed-sender send path.
- **Lower-level zkcredential.** `zkcredential` crate underneath (`rust/zkcredential`) is the generic anonymous-credential framework. We may need it (see §2.3).

### 2.2 What works directly

- Group identifier derivation, group blob encryption/decryption, member-ID encryption — these are pure crypto over a `GroupMasterKey`. We use them as-is. They have nothing Signal-server-specific.
- Server params generation and storage. The homeserver gets one `ServerSecretParams` at first boot, stored in the DB, and publishes `ServerPublicParams` via a new endpoint.
- Group send endorsements. The mechanism is generic; the issuer is identified by `ServerSecretParams`, so any server can issue them for groups it hosts.

### 2.3 What does NOT work directly: the identity attribute mismatch

`AuthCredentialWithPniZkc` is hardcoded to `(Aci, Pni)`. These are Signal's identity types: ACI = 16-byte UUID for the account, PNI = 16-byte UUID for the phone-number-bound identity. We don't have either; we have **DIDs** (variable-length, `did:plc:...`).

Options:

1. **Use a UUID derived from the DID.** Define `UUID(did) := SHA-256(b"actnet-did-to-uuid-v1" || did)[..16]`. All zkgroup machinery works as designed. For the second attribute we pass `Pni::from(UUID(did))` — same UUID bytes, distinct UidStruct because of the service-id-type tag. Clients still map EMI ↔ DID via the cleartext members list in the encrypted state blob (which we already need for display anyway).
2. **Build our own credential on `zkcredential`.** Same generic anonymous-credential framework underneath zkgroup; mirror `auth_credential_with_pni/zkc.rs` with a DID-shaped attribute (`DidStruct` = DID hashed to two Ristretto points).
3. **Skip zkgroup entirely, use a simpler scheme.** Signed credentials with blind signatures, or rotated bearer tokens. Strictly worse anonymity guarantees.

**DECIDED:** Option 1. (See §2.4 for the full reasoning — option 2 was the original choice, implemented and shipped, then superseded after the same identity-attribute problem began to repeat for every zkgroup primitive we wanted to adopt.)

The 128-bit collision space matches zkgroup's `Aci` and Signal's deployed assumption; the attack surface is restricted to DIDs that already exist in the system (not the full 2^128 space); the cleartext members list inside the encrypted state already lets a client cross-check `UUID(did) == claimed_did's hash`. Collision-via-pre-image gains an attacker no useful capability (they'd have to also control a DID with a colliding hash, and convince an admin to invite that DID).

**Encoded as UidStruct + UidEncryptionDomain.** `encrypted_member_id` = stock `zkgroup::groups::UuidCiphertext` = `Ciphertext<UidEncryptionDomain>` over `Aci::from(UUID(did))`. Deterministic in `(did, group_key)`, opaque to the server, exactly the shape every zkgroup primitive expects.

### 2.4 History: why we switched from option 2 to option 1

**Status:** the original §2.3 decision was option 2 (build a DID-shaped credential on `zkcredential`); that scheme shipped in step 5. While planning step 6 PR 2 the same identity-attribute problem began to surface for `GroupSendEndorsement`, prompting a re-evaluation. This section records the reasoning. The actual migration code lives in steps 2–3 of §5's implementation plan.

**The pattern §2.3 missed.** The DID-versus-UUID mismatch isn't specific to `AuthCredentialWithPniZkc`. Every zkgroup primitive that touches an identity attribute uses the UUID-shaped encryption domain (`UidEncryptionDomain`, `UidStruct`, `UuidCiphertext`): `AuthCredential`, `ProfileKeyCredential`, `GroupSendEndorsementsResponse`'s `issue` / `receive_with_service_ids`, and the `Aci`-typed fields on `SenderCertificate`'s peers. §2.3 chose option 2 ("build our own credential on `zkcredential`") for `AuthCredential`. Carrying that choice through means a parallel reimplementation of *every* zkgroup primitive we eventually use — endorsements being the next one.

**The cost we accepted in §2.3.** A DID-shaped `crypto::groups::credentials` module on top of `zkcredential`. ~500 lines, all security-sensitive. Plus a custom `DidEncryptionDomain`, a custom `DidStruct`, custom one-way encoding for `EncryptedMemberId`.

**The cost we're about to accept for step 6 PR 2 if we keep going.** A parallel `crypto::groups::endorsements` module mirroring zkgroup's `GroupSendEndorsement`, on top of `zkcredential::endorsements`. Same shape, same risks. Then again for whatever the next zkgroup primitive turns out to be.

**What the DID-shaped scheme actually buys us.** I claimed in §2.3 that it gives a tighter binding ("the credential cryptographically commits to the DID") and rules out hash-collision impersonation. Re-examining:

- **Server opacity is unchanged.** The server stores `encrypted_member_id` = `encrypt_under_group_key(identity_attribute)`. Whether the inner thing is `DidStruct(did)` (two Ristretto points hashed from the DID) or `UUID(did) := SHA-256(did)[:16]` (16-byte hash), the server can't reverse it without the group key in either case. §3.9 rule 2 still holds; §3.4's 404-not-403 rule still holds.
- **Cross-group linkability is unchanged.** Both schemes embed a deterministic per-DID quantity. `DidStruct::from_did(did)` produces the same two points every time; `UUID(did)` produces the same 16 bytes every time. A leak of either across groups gives the same linkage. The actual unlinkability property comes from the *per-group encryption key* (`did_enc_key_pair` in our scheme, `uid_enc_key_pair` in zkgroup's), and that's the same shape in both.
- **Collision resistance.** §2.3 worried that hashing a DID to 16 bytes risks impersonation via collision. 128 bits of collision resistance against a chosen-prefix attack is borderline for a long-lived global identifier — but: zkgroup's own `Aci` is also 16 bytes, and Signal treats that as sufficient; the cleartext members list inside the encrypted state blob already lets a client cross-check `UUID(did) == claimed_did's hash`, so client-side detection is trivial; and the attacker would need to find a collision *within the closed set of DIDs already in the system*, not against the whole 2^128 space. The collision-resistance worry was overstated.
- **The DID itself is still not on the server.** A `did` value appearing in a credential request (which is session-authenticated and identified, §3.11) is unchanged. The `UUID(did)` value is *derived from* the DID; it appears in EMIs and credentials but not in routing tables.

**What we lose by switching.** One quantifiable thing: in our current scheme the credential's `verify` step cryptographically attests to the exact DID string (via the `DidStruct` points), so a server that has been compromised to issue fake credentials still has to commit to a specific DID at issuance time. With `UUID(did)`, the server commits to `UUID(did)` instead — to forge against a victim DID, the attacker would have to either (a) pre-image the hash (infeasible) or (b) find a colliding DID (1-in-2^128, and the new DID would have to be valid for some account in PLC). Practically identical security; theoretically a hair weaker.

**What we gain by switching.** Stock zkgroup primitives work as designed. Step 6 PR 2 becomes "wire libsignal's `GroupSendEndorsements`" instead of "build a parallel endorsement scheme." We delete `crypto::groups::credentials`, `DidStruct`, `DidEncryptionDomain`, our custom `EncryptedMemberId`. Server-side, `zkgroup_server_params` swaps the `auth_credential_key` field for stock `zkgroup::ServerSecretParams` usage (which already covers auth credentials, endorsements, and profile credentials in one shot). Less custom crypto to audit and maintain; easier to track upstream libsignal changes.

**Migration cost.** Schema: the `encrypted_member_id` byte shape changes (current: ~64-byte `Ciphertext<DidEncryptionDomain>` bincode; new: 32-byte `UuidCiphertext`). Either a rebuild migration (acceptable pre-launch — no production data) or version both shapes side-by-side during transition. Code: `crypto::groups::credentials` deletes (~500 lines), `crypto::groups::group_key` swaps `DidEncryptionDomain` for zkgroup's `uid_enc_key_pair` (~50 lines net), `server::routes::groups` swaps presentation verification to call zkgroup, `app-core::groups` swaps `GroupKey::encrypt_member_id(did)` to `encrypt_member_id(uuid_of(did))` with the same call sites. App-core gets a small new helper `did_to_uuid(did) -> Uuid := SHA-256(b"actnet-did-to-uuid-v1" || did)[..16]` (same domain-separated SHA-256 pattern we use for `distribution_id_for`). Clients re-fetch credentials on first request after the swap.

**Outcome.** Switched. §2.3 now records option 1 as DECIDED, with this section as the rationale. The migration is enumerated in §5 (implementation plan), which marks the now-deleted `crypto::groups::credentials` work as superseded.

### 2.5 API surface for `crypto::groups`

Per §2.3, identities are carried as `Aci::from(UUID(did))` and the credential type is stock `zkgroup::auth::AuthCredentialWithPniZkc`. The Rust interface is mostly a thin wrapper / re-export of zkgroup types:

```rust
// Newtype around zkgroup::GroupSecretParams / zkgroup::GroupPublicParams.
pub struct GroupKey { ... }

impl GroupKey {
    pub fn generate() -> Self;
    pub fn from_bytes(bytes: [u8; 32]) -> Self;
    pub fn to_bytes(&self) -> [u8; 32];
    pub fn group_id(&self) -> GroupId;          // == GroupPublicParams::get_group_identifier()
    pub fn encrypt_state(&self, plaintext: &[u8]) -> Vec<u8>;
    pub fn decrypt_state(&self, ciphertext: &[u8]) -> Result<Vec<u8>>;
    pub fn encrypt_member_id(&self, did: &str) -> UuidCiphertext;  // via Aci::from(UUID(did))
    pub fn public_params(&self) -> GroupPublicParams;
}

// 16-byte UUID derived from the DID; the value passed into zkgroup as Aci.
// Lives in `crypto::groups` so server + client agree on the derivation.
pub fn did_to_uuid(did: &str) -> uuid::Uuid;
//     == SHA-256(b"actnet-did-to-uuid-v1" || did)[..16]

// Server params: thin wrapper around zkgroup::ServerSecretParams / ServerPublicParams.
// No custom auth_credential_key any more — credentials use zkgroup's
// generic_credential_key_pair via the public AuthCredentialWithPniZkcResponse API.
pub struct ServerSecretParams(zkgroup::ServerSecretParams);
pub struct ServerPublicParams(zkgroup::ServerPublicParams);

// Credentials are stock zkgroup types, re-exported:
pub use zkgroup::auth::{
    AuthCredentialWithPniZkc,
    AuthCredentialWithPniZkcResponse,
    AuthCredentialWithPniZkcPresentation,
};
pub use zkgroup::groups::UuidCiphertext as EncryptedMemberId;
```

Sealed-sender-related modules added in step 6 PR 2 (see §3.11 for the
threat model and §5 step 6 for the full landed surface):

```rust
// Canonical libsignal ServiceId string for a DID — used everywhere we
// need a `ProtocolAddress.name()`. Migration: identity + session store
// are keyed on this string, not on the DID. The sealed-sender wire
// format requires it (libsignal parses the name as a ServiceId).
pub fn did_to_service_id_string(did: &str) -> String;
//     == Aci::from(did_to_uuid(did)).service_id_string()

// Server-side sender-certificate chain: trust_root + ServerCertificate
// (single-key shape: trust_root == ServerCertificate leaf, key_id=1).
// Generated once at first boot, persisted alongside ServerSecretParams
// in the `GroupCryptoBundle` (version 3) row. Used by the homeserver to
// mint per-(DID, day) SenderCertificates at credential-refresh time.
pub mod sender_cert {
    pub struct SenderCertChain { /* private */ }
    impl SenderCertChain {
        pub fn generate() -> Result<Self>;
        pub fn to_bytes(&self) -> Vec<u8>;
        pub fn from_bytes(b: &[u8]) -> Result<Self>;
        pub fn trust_root_public_bytes(&self) -> Vec<u8>;
        pub fn issue_sender_cert(
            &self, did: &str, device_id: u32,
            identity_key: &[u8], expiration_unix_millis: u64,
        ) -> Result<Vec<u8>>;
    }
    pub struct SenderCertInfo {
        pub sender_did: String, pub sender_device_id: u32,
        pub identity_key_pub: Vec<u8>, pub expiration_unix_millis: u64,
    }
    pub fn validate_sender_cert(
        cert_bytes: &[u8], trust_root_pub: &[u8],
        validation_time_unix_millis: u64,
    ) -> Result<SenderCertInfo>;
}

// SSv2 multi-recipient sealed-sender envelope wrappers.
pub mod sealed_sender {
    pub async fn encrypt_group_envelope<S: Store>(
        store: &mut S, sender_cert_bytes: &[u8],
        group_id: Option<Vec<u8>>, sender_key_ciphertext: &[u8],
        destinations: &[ProtocolAddress],   // names = ServiceId strings
    ) -> Result<Vec<u8>>;
    pub fn parse_sent_message(bytes: &[u8]) -> Result<Vec<RecipientFanout>>;
    pub async fn decrypt_envelope_to_usmc<S: Store>(
        store: &mut S, envelope: &[u8],
    ) -> Result<DecryptedEnvelope>;
}

// Group send endorsements (zkgroup::groups::GroupSend* wrappers).
pub mod groups::endorsements {
    pub fn default_expiration_unix_seconds(now_unix_seconds: u64) -> u64;
    pub fn issue_endorsements(
        server_secret: &ServerSecretParams,
        member_ciphertext_bytes: &[Vec<u8>], expiration_unix_seconds: u64,
    ) -> Result<Vec<u8>>;
    pub fn receive_endorsements(
        response_bytes: &[u8], member_dids: &[String],
        group_key: &GroupKey, server_public: &ServerPublicParams,
        now_unix_seconds: u64,
    ) -> Result<Vec<Vec<u8>>>;
    pub fn token_for_recipients(
        endorsement_bytes: &[Vec<u8>], group_key: &GroupKey,
        expiration_unix_seconds: u64,
    ) -> Result<Vec<u8>>;
    pub fn verify_token_for_service_ids(
        token_bytes: &[u8], recipient_service_ids: &[ServiceId],
        server_secret: &ServerSecretParams, now_unix_seconds: u64,
    ) -> Result<()>;
}
```

Server side, presentation verification uses `presentation.verify(&server_secret, redemption_time)` and reads `presentation.aci_ciphertext()` to get the `UuidCiphertext` for membership lookup.

The `groups` module's interface to the rest of the system stays scheme-agnostic at the application layer (`encrypt_member_id` takes a `&str`, not an `Aci`), so a later MLS swap stays possible.

`AuthCredentialDid` internals stay private to the module. `app-core` and `server` see only this scheme-agnostic API.

## 3. Encrypted group state

### 3.1 What is "group state"

Group state lives in two layers — distinguishing them matters for the storage model in §3.2 and the change-authorization model in §3.3.

**Encrypted state blob** (opaque to the server; the source of truth for clients):

- **Group identity:** `group_id` (derived, public), `created_at`, `creator_did`.
- **Members:** list of `(did, encrypted_member_id, role, joined_at, profile_key_ciphertext)`. Roles: `Admin`, `Member`. (Guest is a Stage-6+ extension; defer.) The `did` is carried in cleartext *inside the encrypted blob* so members can render names and so that the one-way `encrypt_member` map (§2.3) is invertible to clients-who-hold-the-master-key. The server, which doesn't have the master key, sees neither.
- **Metadata:** title, description, avatar reference + key.
- **Policy:** expiry timer (seconds), announcement-only flag, join policy (`InviteOnly`, `RequestToJoin`, `OpenLink(token)`).
- **Revision:** monotonically increasing `u64`. Each accepted change increments by 1.

This is small — a v1 group with 50 members easily fits in <16 KB encrypted.

**Server-visible routing subset** (the minimum the server needs to enforce membership, validate actions, and route pushes):

- `member_credentials` per full member: `(group_id, encrypted_member_id, role, group_push_pseudonym)`.
- `members_pending` per invited-but-not-accepted user: `(group_id, encrypted_member_id, role, jittered_timestamp)`. Following Signal's `MemberPendingProfileKey` state. Invitee accepts via `promote_pending_members` action to graduate into `member_credentials`; or declines via `decline_invite`. (Signal's equivalent table also carries `addedByUserId`, but that field lives in the encrypted state blob for client display — the server has no use for it and storing it here would weaken §3.9 for no benefit.)
- `members_pending_approval` per join-request: `(group_id, encrypted_member_id, group_push_pseudonym, jittered_timestamp)`. Following Signal's `MemberPendingAdminApproval` state. An admin approves via `approve_join_request` to graduate the requester into `member_credentials`; or denies via `deny_join_request`. The requester can `cancel_join_request`. The pseudonym is supplied by the requester at request time so the admin's approval doesn't need to know it.
- `group_policy` per group: a `Policy` record covering per-action-type minimum role, join policy, and invite link password. See §3.3 for the wire shape. Following Signal's group access control: low-entropy enum values, doesn't reveal members, content, or identity. Two action types are protocol-fixed Admin-only regardless of policy (see §3.3): `modify_policy` and `modify_member_role`.

The four views are kept consistent by the change-application logic (§3.3): every `GroupChange` updates both the encrypted state (via the new blob the client uploads) and the server's routing subsets (via the structured actions the server parses). A client reconstructing group state always trusts the encrypted state blob, not the server's subsets.

### 3.2 Where it lives

**DECIDED:** server stores the encrypted blob, indexed by `group_id`. Server cannot read it. Server stores additionally:

- `group_id` (public)
- `server_public_params_version` (which credential issuance key was active when group was created)
- `current_revision` (server can see the revision number — it's a counter, not secret)
- `encrypted_state` (bytea, latest revision)
- `encrypted_state_history` (last 256 revisions; server keeps a ring buffer so clients catching up from an old revision can fetch deltas — see §3.4)
- `member_credentials` (table: `group_id`, `encrypted_member_id`, `role`, `group_push_pseudonym`) — server uses this to enforce membership, validate roles (§3.3), and route pushes without learning who members are. The `encrypted_member_id` is a fixed-size (~64 byte) ciphertext produced by encrypting the member's DID under a key derived from the group master key; see §2.3 for the encoding and `crypto::groups` for the implementation. The server doesn't hold the master key and §3.9 rule 2 prevents any server-side `(encrypted_member_id → did)` index, so membership opacity is structural. `role` is `Admin` or `Member`; the encrypted state blob is authoritative for clients but the server's copy is sufficient for the server's enforcement role.
- `members_pending` (table: `group_id`, `encrypted_member_id`, `role`, `jittered_timestamp`) — invited users awaiting acceptance. See §3.10. (Inviter identity lives in the encrypted state blob for client display; not stored server-side.)
- `members_pending_approval` (table: `group_id`, `encrypted_member_id`, `group_push_pseudonym`, `jittered_timestamp`) — join requesters awaiting admin approval. See §3.10. Requester provides their pseudonym at request time so admin approval doesn't need it.
- `group_policy` (record on the group row, or a small joined table) — per-action-type minimum role, join policy (`Closed | RequestToJoin | OpenLink`), invite link password (`Option<Bytes>`), announcement-only flag. See §3.1 for the rationale and §3.3 for the wire shape and how it's used. Updated atomically when a `modify_policy` action is applied.

**Critically, none of these tables carry a DID column.** The server cannot answer "which groups is DID X in?" or "which DID is encrypted_member_id E?" from its own storage. The `group_push_pseudonym` column lives in `member_credentials`, not in the existing DM `push_pseudonyms` table — the two have no shared identifier the server can join on. The relay holds `(group_push_pseudonym → device_token)` independently; only the relay knows the device link, and only the server knows the group↔pseudonym link.

This mirrors Signal's storage-service model: server is a dumb encrypted-blob store with a revision counter. See §3.9 for the full discipline rules that keep this property load-bearing.

### 3.3 How updates are authorized

A group change is a structured, signed mutation. The wire format:

```
GroupChange {
  revision: u64,                  // new revision after applying
  actions: Actions,               // structured; some sub-fields encrypted under group key
  presentation: AuthPresentation, // proves submitter has a valid credential for this group
}

Actions {
  // Membership: invite/accept/decline (admin invites; invitee accepts to graduate)
  invite_members:             [ InviteMember ],          // each: encrypted_member_id, role
  promote_pending_members:    Option<PromoteSelf>,        // self-action: encrypted_profile_key, group_push_pseudonym
  decline_invite:             Option<EncryptedMemberId>,  // self-action
  remove_members:             [ EncryptedMemberId ],
  modify_member_role:         [ (EncryptedMemberId, Role) ],

  // Membership: link-based self-join (server picks immediate-add vs pending based on join_policy)
  join_via_link:              Option<JoinViaLink>,        // self-action: encrypted_profile_key, group_push_pseudonym, invite_link_password
  cancel_join_request:        Option<EncryptedMemberId>,  // self-action
  approve_join_request:       [ EncryptedMemberId ],      // admin-action
  deny_join_request:          [ EncryptedMemberId ],      // admin-action

  // Group metadata
  modify_title:               Option<EncryptedBlob>,      // sub-encrypted under group key
  modify_description:         Option<EncryptedBlob>,      // sub-encrypted under group key
  modify_expiry:              Option<EncryptedBlob>,      // sub-encrypted under group key
  modify_policy:              Option<Policy>,             // server-visible; updates group_policy
  // (more action types added as the protocol grows)
}

Policy {
  // Per-action-type minimum role
  invite_members:     Admin | Member,
  remove_members:     Admin | Member,
  modify_title:       Admin | Member,
  modify_description: Admin | Member,
  modify_expiry:      Admin | Member,
  // modify_member_role and modify_policy are protocol-fixed Admin; not configurable.

  // Join behavior
  join_policy:            Closed | RequestToJoin | OpenLink,
  invite_link_password:   Option<Bytes>,
  announcement_only:      bool,
}
```

The `Actions` envelope is **partly visible to the server**, partly sub-encrypted. The server can see the *structure* (which operations, which `encrypted_member_id`s, which roles, the policy values) but not the content of sensitive sub-fields (title, description, expiry timer, encrypted_profile_keys). Note that `encrypted_member_id` itself is still opaque to the server (it's encrypted under the group key the server doesn't have), so visibility of the structure does not de-anonymize members.

**Self-actions vs. admin actions.** Actions are split into two classes:
- **Admin-class actions** (`invite_members`, `remove_members`, `modify_*`, `approve_join_request`, `deny_join_request`, etc.) are submitted by an existing member with sufficient role. Multiple admin-class actions may be batched in a single `GroupChange`.
- **Self-class actions** (`promote_pending_members`, `decline_invite`, `join_via_link`, `cancel_join_request`) are submitted by the user the action operates on, and **must be the sole action in a `GroupChange`**. This keeps actor-authentication unambiguous (the credential presentation identifies the self-action's actor; mixing with admin-class actions would create a multi-actor change).

**Server-side check on submission:**

1. `presentation` verifies under `server_secret_params` and the group's `GroupPublicParams`. Extract the actor's `encrypted_member_id` from the presentation.
2. **Actor-eligibility check**, by action class:
   - Admin-class actions: actor's `encrypted_member_id` appears in `member_credentials` for this `group_id`.
   - `promote_pending_members` / `decline_invite` (self-actions): actor's `encrypted_member_id` appears in `members_pending` for this `group_id`.
   - `cancel_join_request` (self-action): actor's `encrypted_member_id` appears in `members_pending_approval`.
   - `join_via_link` (self-action): actor does **not** need to be in any membership table. The auth credential validates "some valid user with a credential for this group key"; the supplied `invite_link_password` must match `group_policy.invite_link_password` (constant-time compare); `group_policy.join_policy` must be `OpenLink` or `RequestToJoin` (a `Closed` policy rejects). The server picks the resulting path based on `join_policy` — see step 5.
3. `revision == current_revision + 1`. Concurrent updates lose — see §3.5.
4. **Role check** (admin-class actions only; self-class actions skip this step). Server reads the actor's role from `member_credentials.role` and the group's `group_policy`. For each action, server checks the actor's role meets the minimum required for that action type per the group's policy. Two action types are protocol-fixed Admin-only regardless of what policy says, because making them configurable would allow privilege escalation:
   - `modify_policy` — if Members could change the policy, they could grant themselves any other right. Always Admin.
   - `modify_member_role` — if Members could change roles, they could promote themselves. Always Admin.
   `approve_join_request`, `deny_join_request`, and `remove_members` default to Admin-only but are configurable per group; `invite_members` likewise defaults to Admin-only but is configurable. Clients re-verify on apply (see §3.6).
5. **Apply structural changes.** Each action updates one or more server-visible tables atomically with the revision bump:
   - `invite_members [InviteMember]` (each `InviteMember`: `encrypted_member_id`, `role`): for each, insert `(group_id, encrypted_member_id, role, jittered_timestamp)` into `members_pending`.
   - `promote_pending_members` (self-action; `PromoteSelf`: `encrypted_profile_key`, `group_push_pseudonym`): delete actor's row from `members_pending` (using the role preserved there); insert `(group_id, actor_encrypted_member_id, role_from_pending, group_push_pseudonym)` into `member_credentials`. `encrypted_profile_key` is broadcast in the actions for clients but not persisted server-side.
   - `decline_invite` (self-action): delete actor's row from `members_pending`.
   - `remove_members [EncryptedMemberId]`: for each, delete from `member_credentials`. Also delete any matching row in `members_pending` / `members_pending_approval` to handle pending-when-removed races.
   - `modify_member_role [(EncryptedMemberId, Role)]`: for each, update `member_credentials.role`.
   - `join_via_link` (self-action; `JoinViaLink`: `encrypted_profile_key`, `group_push_pseudonym`, `invite_link_password`): server branches on `group_policy.join_policy`:
     - **`OpenLink`** → insert `(group_id, actor_encrypted_member_id, role=Member, group_push_pseudonym)` directly into `member_credentials`. Response: `200 { member: true }`.
     - **`RequestToJoin`** → insert `(group_id, actor_encrypted_member_id, group_push_pseudonym, jittered_timestamp)` into `members_pending_approval`. Response: `202 { pending: true }`.
     - **`Closed`** → reject with `403 { reason: "closed" }` (also caught earlier at step 2).
     `encrypted_profile_key` is broadcast in the actions for clients but not persisted server-side.
   - `cancel_join_request` (self-action): delete actor's row from `members_pending_approval`.
   - `approve_join_request [EncryptedMemberId]`: for each, move the row from `members_pending_approval` to `member_credentials` (preserving `group_push_pseudonym`; setting `role = Member`).
   - `deny_join_request [EncryptedMemberId]`: for each, delete from `members_pending_approval`.
   - `modify_policy`: replace `group_policy` for this group with the new record.
   - Sub-encrypted actions (`modify_title`, `modify_description`, `modify_expiry`) are not applied to any server-visible state — they're present only so the server can validate role permission against `group_policy` and broadcast the change to other clients.
6. On success: store new encrypted_state blob, increment `current_revision`, append previous blob to history, push-notify members (including any newly-graduated members, whose `group_push_pseudonym` was registered as part of the action).

There is no separate cleartext diff for the server to validate against the encrypted actions — the actions *are* the diff, and they're structured exactly so the server can apply them directly. This is how Signal handles group changes.

**What the server learns from a change:**

- That a change occurred in group `G` at jittered time `T` (§3.9 rule 5).
- The actor's `encrypted_member_id` (already in some membership table; opaque to server).
- For invite/promote/decline/remove/role-change/approve/deny actions: the affected `encrypted_member_id`s, any new role assignments, and (for promote/join/request) `group_push_pseudonym` values. All opaque components from the server's perspective — the pseudonym is a routing label, not an identifier the server can correlate back to anything.
- That a `modify_title` (or description / expiry) operation occurred, but not the new value.
- For `modify_policy`: the new policy record (low-entropy enums + the `invite_link_password` bytes). No identities.

What the server still does not learn: any DID, the actual title / description / expiry values, profile keys, or any link from `encrypted_member_id` to any other identifier on the server. All §3.9 discipline rules continue to hold.

### 3.4 Fetching state

A new member, or a member coming back after time offline, fetches:

```
GET /v1/groups/{group_id}
Headers: X-Group-Auth: <auth-presentation>
```

Response:

```
{
  revision: u64,
  encrypted_state: bytes,
  group_public_params: bytes,  // so a client joining via invite link can derive the encryption key
  policy: Policy                // server-readable, see §3.3
}
```

The homeserver's `ServerPublicParams` (used to verify credential issuance) lives at the separate `GET /v1/groups/server-params` endpoint — clients fetch it once at first contact and cache it.

**Fetch is membership-gated.** Server verifies the presentation, extracts the actor's `encrypted_member_id`, and checks it appears in `member_credentials[group_id]` **or** `members_pending[group_id]` (admin-invited but not yet promoted — §3.10 step 3 needs them to read state in order to construct an accurate `new_encrypted_state` blob on promotion). Everyone else is rejected with **404** (not 403): the response is intentionally indistinguishable from "no such group", so an attacker holding a valid credential but no membership can't probe for the existence of arbitrary groups. This matches the info-flow leak rule in §3.11 ("Error responses don't leak existence") applied to read endpoints. It is the load-bearing reason the master key on its own (without server cooperation) cannot read group state — see §3.10 "Security note on what the master key grants." Pending-approval (link-join) requesters are *not* allowed to fetch: they don't graduate themselves (an admin does via `approve_join_request`), so they don't need pre-approval state, and exposing it would leak membership before approval.

Members fetching incrementally:

```
GET /v1/groups/{group_id}/changes?from_revision=N
```

Response: list of `GroupChange` messages from revision `N+1` to current, if all still in history; otherwise a snapshot pointer ("history truncated at revision M, fetch full state"). Same membership check applies.

#### Why retain changes at all?

The server could in principle hold only the current snapshot and force a full re-fetch on every catch-up. Retaining a change-log ring buffer serves three purposes, in increasing order of importance:

1. **Bandwidth for catch-up.** A client at revision N that reconnects to a group at revision M can pull the M−N deltas (typically a few hundred bytes each) instead of re-downloading the encrypted state blob (16 KB for a 50-member group, megabytes for a large one). Modest saving for small groups; meaningful for large ones.
2. **Historical UI.** Clients display chat-timeline entries like "Alice added Bob" and "Carol renamed the group to X." That information lives in the *changes*, not in the snapshot (which only shows the current end-state). For changes a client observed live, it caches locally; for changes that landed during an offline window, the server-side history is what backfills the timeline.
3. **Tamper detection.** The encrypted state is opaque to the server, but a malicious server (or admin who briefly seized control) could substitute a forged snapshot. Each `GroupChange` carries both the actions and the resulting encrypted state, so a client walking the chain `(state_N, change_{N+1}, state_{N+1}, ...)` can verify each state is the legitimate continuation of the prior one — the actions must produce the new state when applied to the old. Without the change log, a client receiving a snapshot at revision M has to take it on faith. This is the load-bearing reason to retain history at all. (Detection is enabled by retention; what a client *does* on detected tampering — warn, suspend sends, offer to fork the group, migrate homeservers — is out of scope for v1 and will be designed when the rest of the abuse-response surface is.)

#### Sizing

**DECIDED: keep last 256 changes per group.** Sized for "active group with ~8 changes a day for a month." Past that, a full re-fetch is fine (tamper detection within the last month covers any realistic detection window — a group whose members haven't been online in 30+ days isn't in active use). Storage cost is trivial (~128 KB per group at avg 500 bytes per change).

### 3.5 Conflict resolution on concurrent updates

Two admins simultaneously kick the same problem user, or one admin promotes member X while another admin removes X. With monotonic revision numbering, exactly one of the concurrent updates wins server-side; the loser gets `409 Conflict { current_revision: M }`.

Client behavior on 409:

1. Fetch revisions from `local_revision+1` to `M`.
2. Apply them locally.
3. Reconcile the user's intended change against the new state (e.g. "X is already removed → no-op; otherwise re-submit").
4. Retry with revision `M+1`.

This is good enough — group admin changes are rare and the conflict window is small. The alternative (CRDT or operational transform) is overkill.

**DECIDED:** actions express intent declaratively, not imperatively. `remove_members [E1, E2]` means "ensure these members are not present" — re-applying on a state where they're already absent is a no-op, not an error. `modify_title(T)` means "set the title to T" — re-applying when the title already equals T is a no-op. This makes 409-retry idempotent: after fetching the new state and re-submitting, the action either silently succeeds (intent already satisfied or freshly applied) or has clean semantics (e.g. role demotion when concurrent promote landed). Imperative semantics ("decrement X's role") would surface confusing errors on retry; declarative semantics absorb the concurrency.

### 3.6 Role enforcement is layered

The server does role enforcement at submission time (§3.3 step 4) using `member_credentials.role` and `group_policy`. This catches every misconfigured or malicious change before it's broadcast: a non-admin attempting an Admin-only action gets a 403, the revision is never created, no other client sees the attempt.

Clients re-verify on apply. When a member fetches a `GroupChange` and walks the actions, they confirm the actor's role and the group's policy (both from the authoritative encrypted state blob) permit each action. This catches:

- Server bugs or compromise — a malicious server that decides to accept an unauthorized change still can't make clients converge on the resulting state. The encrypted blob is the source of truth.
- Race conditions where the server's `member_credentials.role` or `group_policy` is briefly out of sync with the encrypted state (e.g. mid-`modify_policy`).
- Action semantics that depend on sub-encrypted state the server can't read (e.g. an announcement-only flag that further restricts who may post — the policy field is server-visible, but per-message restrictions inside the announcement-only mode are enforced by clients).

If a client detects a role or policy violation on apply, it rejects the revision and reports it; the actor is identifiable by their `encrypted_member_id` and can be kicked by group admins.

This is Signal's model: the server enforces what it can see (membership, role, policy); the encrypted state blob remains authoritative for clients. Layered enforcement is strictly stronger than either alone.

### 3.7 Fan-out, delivery, and notification

Delivery to a group recipient takes one of two paths depending on whether the recipient's client is currently connected over a WebSocket: **online delivery** pushes the envelope directly down the WebSocket; **offline delivery** wakes the device via APNs/FCM through the relay, and the device fetches on wake. The same pseudonym (`group_push_pseudonym`) is the routing key for both paths.

**DECIDED.** Each `member_credentials` row carries a `group_push_pseudonym` distinct from any DM pseudonym. The client registers `(group_push_pseudonym, device_token)` directly with the relay via the existing `/v1/register` endpoint — the relay can't tell it apart from any other pseudonym registration. The server only ever holds `(group_id, encrypted_member_id, group_push_pseudonym)`; it never sees the device.

#### Delivery to online clients (WebSocket subscription)

The challenge: under §3.9, the server has no persistent `(account_id → groups joined)` link, so it can't ask "is this group recipient currently connected?" when it has a message for `encrypted_member_id E`. The solution is **explicit, ephemeral subscription** by the client at WebSocket connect time:

1. Client opens its authenticated WebSocket (existing DM-delivery WebSocket — no new connection).
2. Client sends a `subscribe { pseudonyms: [P1, P2, ...] }` frame listing each `group_push_pseudonym` it wants to receive deliveries for in this session.
3. Server holds an **in-memory** map `(pseudonym → ws_session)` for the duration of the session. The map is per-process, never written to disk, and re-built from `subscribe` frames on every reconnect.
4. On group message arrival, for each recipient: look up `group_push_pseudonym P` from `member_credentials`. Check the in-memory map for `P → ws`. **Hit:** push the envelope slot down the WebSocket directly. **Miss:** fall through to the offline path (relay wakeup, below).

The client may resubscribe / unsubscribe at any time during the session (e.g. when joining or leaving groups). On WebSocket disconnect, all of that client's subscriptions are dropped from the map.

**Live-memory caveat.** While the WebSocket is connected, the server's in-memory state has `(ws_session → account_id)` (from auth) and `(pseudonym → ws_session)` (from the subscription). Composed: server effectively knows "account A is interested in pseudonym P, which routes to group(s) G" for the duration of the session. The link is **never persisted**: cold seizure still yields nothing per §3.9; only an attacker reading live process memory recovers it. This is the same operational/live-memory tradeoff Signal accepts, and is consistent with the project-wide decision to avoid at-rest key infrastructure (recorded in `docs/00-design.md` line 81: "your homeserver knows your social graph"). High-risk users can opt out of WebSocket subscription per-group, accepting the APNs/FCM round-trip latency in exchange for never registering the subscription.

**State changes use the same path.** A `GroupChange` revision bump generates the same per-recipient delivery decisions: WS push for online subscribers, relay wakeup for everyone else. State-change notifications are typically content-free pointers ("new revision N exists"); the recipient fetches §3.4.

#### Delivery to offline clients (relay path)

This is the original §3.7 design. When the in-memory subscription map misses, or when the recipient never connected this session at all:

1. Server forwards a content-free wakeup to the relay, addressed to `group_push_pseudonym P`.
2. Relay maps `P → device_token` and fires APNs/FCM.
3. Member device wakes, fetches the new revision or queued message envelope.

#### Rotation

Group push pseudonyms rotate every 7 days, mirroring the device pseudonym cadence (`PUSH_ROTATION_MAX_AGE_SECONDS` in `core/crates/app-core/src/lib.rs`). To avoid correlated bursts where the relay sees N pseudonyms rotate together per device, the per-group rotation time is offset deterministically: `next_rotation = registered_at + 7d + (hash(group_id) mod 7d)`. Rotation does:

1. Client generates a new 32-byte random pseudonym.
2. Client calls the existing `register_push_with_relay` with the new pseudonym + the device's current `device_token`.
3. Client calls a new server endpoint `POST /v1/groups/{group_id}/push_binding`, authorized by an `AuthPresentation`, which atomically updates the row in `member_credentials`.
4. Client best-effort unregisters the old pseudonym from the relay.
5. Client updates its WebSocket subscription: `unsubscribe(old_P) + subscribe(new_P)` so online delivery routes to the new pseudonym.

The same flow runs at join (no old pseudonym to unregister) and at device_token change (re-register all group pseudonyms against the new token, no new pseudonym needed). When a member is removed from a group, their row is deleted by the change-application logic — they keep receiving wakeups from the relay until their pseudonym registration at the relay is unregistered (best-effort) or expires (relay-side TTL).

#### Trust topology

Client↔relay channel already exists for DMs (`app-core/src/lib.rs:1039`); group pseudonyms reuse it. The homeserver never learns any device_token, consistent with `docs/00-design.md` line 183. The WebSocket subscription map is in-process state on the homeserver; it does not extend the relay's knowledge or persist any new linkage on disk.

### 3.8 Message expiry

Expiry timer is a field in the encrypted group state. Server learns nothing about it. Clients enforce the per-group expiry on their local copies.

Independently of the encrypted timer, the server has its own retention policy for undelivered message slots: each per-recipient slot is deleted on delivery acknowledgment, or after **30 days** queued without delivery, whichever comes first. **DECIDED.** This is a backstop against dead-device queues accumulating indefinitely (lost phones, abandoned accounts), not a privacy lever and not extra retention buffer beyond delivery. Matches Signal's number.

A malicious server can't extend retention beyond 30 days; a malicious client can ignore the per-group timer for its local copy (same caveat as Signal).

### 3.9 Schema discipline for membership opacity

The design property "a seized server does not yield group memberships" (per `docs/00-design.md` line 44) holds *for free* because `encrypted_member_id` is a ciphertext under the group key the server never has — *as long as* the server doesn't maintain auxiliary tables that link DIDs to group state. No at-rest encryption is required; the property is structural.

The following rules are load-bearing. A reviewer or future contributor adding schema, logging, or caching should check against this list. If a proposed change violates a rule, either the change must be rejected or the design property must be explicitly relaxed in `docs/00-design.md` first.

1. **No `(did → groups_joined)` table or cache on the server, ever.** Why: it's exactly the table a seized server would yield. Even a transient in-memory cache rebuilt at runtime is fine; a persisted one is not.
2. **No `(encrypted_member_id → did)` map.** Why: trivially defeats the opacity. The transformation goes one way only (DID + group key → encrypted_member_id, computed by clients).
3. **Credential issuance is not logged with credential identifier.** Rate-limit counters per DID per day are fine. A row that records "DID X received credential C on day D" is not, because the same `C` could surface later in a presentation log. Why: linking issuance to presentation reconstructs the DID↔encrypted_member_id chain.
4. **Presentation verification logs security events only, never credential identifier or `encrypted_member_id` correlation.** Failed verifications can be counted; successful ones can be counted; the actual identifiers must not be persisted. Why: same as rule 3, but for the presentation side.
5. **`member_credentials` row timestamps are jittered (or omitted).** A precise `joined_at` correlates with the account's login or registration timestamp, de-anonymizing the row. Use day-aligned or coarsely-jittered timestamps if a timestamp is needed at all. Why: temporal correlation is the easiest deanonymization attack against opaque-id storage.

Operational tooling that requires DID↔group lookup (e.g., "remove this DID from all groups on the server") is **not available server-side** under this discipline. Such operations must be driven by clients holding the relevant group keys. See open question on account deletion in §4.

### 3.10 Invite and link-based join flows

Following Signal's `MemberPendingProfileKey` / `MemberPendingAdminApproval` model, joining a group is a **two-step** process — the action that "adds" a user only puts them in a pending state; a separate self-action (or, for join requests, an admin-action) graduates them into `member_credentials`. The two steps are decoupled because:

- The inviter often does not have the invitee's profile_key (a per-(user, contact) secret), and certainly doesn't have the invitee's fresh `group_push_pseudonym` for this group. The invitee supplies both at acceptance time.
- The invitee may decline (or simply ignore) the invitation. They should not appear as a sender-cert-authenticated member until they've explicitly opted in.
- First-contact (admin and invitee have never DM'd before) needs to work without an extra round-trip session setup.

Three flows: **invite (admin-initiated)**, **request-to-join (user-initiated)**, **open-link self-join**.

#### Invite flow (admin-initiated)

1. **Admin submits `invite_members`** with the invitee's `encrypted_member_id` and intended role. Server inserts a row into `members_pending`. Revision bumps.
2. **Admin's client sends the invitee a substrate DM** carrying the group context: `{group_id, group_master_key, hosting_server_url, inviter_did, invited_at}`. Transport details below.
3. **Invitee receives, decrypts, stores the group key locally.** If the invitee is not yet a member of `hosting_server_url`, their client first guides them through joining that server (per `docs/13-federation.md` — multi-homing membership), establishing per-server prekeys and a session. Then the client fetches the group state (§3.4) and sees the invitee in the pending list.
4. **Invitee submits `promote_pending_members`** with their `encrypted_profile_key` and a freshly-generated `group_push_pseudonym` (already registered with the relay via `register_push_with_relay`). Server moves the row from `members_pending` to `member_credentials`. Revision bumps. The invitee is now an operating member.
   - Or invitee submits `decline_invite`, which deletes the pending row. Done.

#### Link-based join flow (user-initiated)

Used when the requester has obtained an invite link (encoding `group_master_key` and `invite_link_password`) out-of-band — e.g. from the channel directory Project, a shared link, or a QR code. The single `join_via_link` action covers both `OpenLink` and `RequestToJoin` policies; the server picks the resulting path based on the group's current `join_policy`. The client doesn't need to know the policy ahead of time (and can't easily learn it — §3.4 fetch is membership-gated).

1. **Requester submits `join_via_link`** with `encrypted_profile_key`, a freshly-generated and relay-registered `group_push_pseudonym`, and the `invite_link_password` from the link.
2. **Server branches on `group_policy.join_policy`**:
   - **`OpenLink`** → server inserts directly into `member_credentials`. Response: `200 { member: true }`. Requester is an operating member immediately.
   - **`RequestToJoin`** → server inserts into `members_pending_approval`. Response: `202 { pending: true }`. Requester sees "waiting for admin approval" in UI.
   - **`Closed`** → reject `403 { reason: "closed" }`. UI surfaces "this group no longer accepts new joins; ask an admin to invite you directly."
3. **If pending: existing admins are notified** via the standard group push fan-out (the `join_via_link` action creates a revision that all current members fetch). Admins see "X is requesting to join" in their app — X is an `encrypted_member_id` they can decrypt to a DID under the group key.
4. **An admin submits `approve_join_request`** (or `deny_join_request`) with the requester's `encrypted_member_id`. Server moves the row from `members_pending_approval` to `member_credentials`, preserving the `group_push_pseudonym` the requester supplied. Requester is now an operating member.
   - `deny_join_request` deletes the pending row. Optionally accompanied by a brief substrate DM to the requester so they know.
   - Or requester submits `cancel_join_request` to withdraw before any admin acts.

A consequence of the unified action: the requester's UI shouldn't pre-commit to "request to join" vs "join immediately" before submitting — the response from the server is what tells the UI which path applies. Display something neutral like "Join" on the button, render the outcome based on the response.

#### Group-context delivery for the invite flow (step 2 above)

The admin's per-recipient delivery is a **normal substrate DM** carrying the group context as its payload. PreKey first-contact (X3DH session establishment for parties with no prior Signal session) is the existing substrate mechanism and works regardless of whether the envelope is sealed-sender-wrapped.

```
ContentMessage {
  kind: GroupContext {
    group_id:             GroupId,
    group_master_key:     GroupMasterKey,   // 32 bytes; bootstraps the invitee
    hosting_server_url:   Url,              // where the group lives; invitee may need to join this server (see §3.12)
    inviter_did:          Did,              // display only
    invited_at:           Timestamp,
  }
  ...
}
```

**First-contact works without a round-trip.** If admin and invitee have no prior Signal session, the admin's client fetches the invitee's prekey bundle, performs X3DH, and sends the `GroupContext` as the initial Signal message (PreKey message type). This is the same path used for any first DM; no new infrastructure.

**Sender anonymity is not a goal of this envelope.** The admin's identity is already known to the server — they had to authenticate to submit `invite_members`. Wrapping the `GroupContext` in a sealed-sender envelope would add nothing: the server already knows the same admin sent both the action and the envelope. Sender opacity for *group message sends* (where it carries real weight) is tracked separately in §4 open question #4.

**Compatibility with §3.9.** The server routes the DM via the invitee's account, not via any group-aware path. Server learns "admin DM'd invitee" — the same metadata it already sees for any DM. No new DID↔group link is created on the server.

#### Atomicity

The admin client must submit both the `invite_members` action and the per-recipient `GroupContext` envelopes. If only one half lands, the system degrades:

- If `invite_members` lands but the envelope fails: invitee appears as pending in the group state but never learns. Other members can see "X has been invited but hasn't accepted" and re-trigger the invite, or kick the pending row via a follow-up `remove_members`.
- If the envelope lands but `invite_members` does not: invitee receives the group key, fetches the group, sees they're *not* in `members_pending`, and their client surfaces "this invite appears stale; ask the sender to re-add you."

Recommended client ordering: submit `invite_members` first (cheap revision bump on the group), then send the envelopes (best-effort, with retry). The pending state acts as the recoverable receipt — failed envelopes don't lose state, they just delay the invitee's notification.

#### Edge cases

- **Multiple admins inviting the same user concurrently.** Only one `invite_members` lands (revision conflict, §3.5); the losers can resolve client-side ("already invited, no-op") on retry. Multiple envelopes may still arrive; the invitee's client dedups by `(group_id, group_master_key)`.
- **Inviting someone who doesn't have an account yet.** The `invite_members` action requires the target's `encrypted_member_id`, which requires their DID — so admins can't pre-invite a not-yet-existent account that way. Instead, the invite link itself carries the group info: an extended invite token (`docs/51-invite-tokens.md`) can include a `group_invitations` array of `{group_master_key, invite_link_password}` entries. (`group_id` is derived client-side via `GroupKey::group_id()` from the master key — same as Signal's invite links; no need to ship it in the token.) After the new user finishes registration on the server, their client iterates the array and submits `join_via_link` for each. The server picks the resulting path (immediate member for `OpenLink`, pending approval for `RequestToJoin`, rejection for `Closed`) — see the link-based join flow above. No admin DM is involved; the token carries the bootstrap info directly. Admin-side UI for generating these tokens is a Project concern (see `docs/51-invite-tokens.md` §"Projects (future)").

  **Security note on what the master key grants.** The `GroupMasterKey` does dramatically less on its own than the name suggests. A holder of just the master key, without other server cooperation or pairwise key exchange with a member, can:

  - Compute `GroupSecretParams` / `GroupPublicParams` for the group.
  - Construct their own `encrypted_member_id` for the group (necessary for joining; useless for anything else without joining).
  - In principle, decrypt the encrypted state blob — *but only if they can obtain a copy of it*. Obtaining the blob from the server requires a membership-gated fetch (§3.4); the server rejects non-member fetches with 403. So in practice the master key alone doesn't read state.
  - Identify which mesh packets belong to this group **only under one variant** of §7's tag derivation. The current default (`group_tag = HMAC(sender_key, ...)`, per-sender) doesn't leak; the alternative (`group_tag = HMAC(group_master_key, ...)`, per-group) would. §7 currently leans toward per-sender for exactly this reason.

  The master key cannot:

  - Decrypt message content (mesh or server). Messages use Sender Keys, distributed per-member via pairwise Signal sessions. Without explicit onboarding by a member, no Sender Keys are obtainable.
  - Forge credentials (issued by the homeserver from `ServerSecretParams`).
  - Bypass the `invite_link_password` server check for joining.

  Combined with `invite_link_password` and an admitted join (`join_via_link` — server picks immediate-add or pending-approval based on policy), the holder becomes a full member and gets the rest — that's the point.

  **Why we still include `group_master_key` in invite tokens for both `OpenLink` and `RequestToJoin`.** Signal does this and the property analysis above shows it's safe: the master key in a leaked link gives a passive observer essentially nothing (no content read, no state read without membership-gated server cooperation, no Sender Key access). The widened distribution channel (SMS, QR, etc.) is fine because what's being distributed isn't a "read everything" key. `Closed` groups still use the admin-direct invite flow (E2E DM) because they don't have an invite link in the first place, not because tokens would be unsafe for them.

  **Recovery from leaked links.** The `invite_link_password` is rotatable via `modify_policy`, invalidating any previously-distributed links. The master key itself isn't realistically rotatable for an existing group (would require re-distributing all per-member crypto), but as established above, master-key-only leakage doesn't grant content or state access.
- **Re-invite after decline or removal.** Same flow as a fresh invite. The invitee's client recognizes the `group_id` from local state and skips redundant key storage, but still goes through the pending-state transition.
- **Invitee already has the group key locally** (e.g. they accepted on another device first). Client sees they're already in `member_credentials` for this group, skips `promote_pending_members`. The lingering pending row (if any from a slow propagation) will be deduplicated when the corresponding `promote_pending_members` from the other device lands.
- **Invitee is offline for a long time.** Pending rows accumulate but cost almost nothing — opaque per-group rows. Optional cleanup: pending rows older than 30 days (jittered) auto-expire server-side. Configurable per group.

This section will be expanded when the `proto/content.proto` message-type registry is formalized; for now, `GroupContext` is the only group-related substrate message type.

### 3.11 Sender opacity for group message sends

When Alice sends a message to a group, we want the server to validate "this is a legitimate group message" without learning *which* member sent it. Database snapshots, log analysis, and subpoenas for "who sent message X" should all fail. This is Signal's sealed-sender property generalized to groups. It requires three layers; the envelope alone is insufficient.

#### Threat model

- **Strong defense against:** database snapshots, log analysis, subpoenas for "who sent message X," casual operator inspection of message queues, server compromise at the storage layer.
- **Weak defense against:** real-time network correlation by a server with active monitoring capability — the unauthenticated send request and the user's other authenticated traffic can be linked via source IP. Mitigation is Tor/VPN on the user side. This matches Signal's stated sealed-sender threat model and is the price of not building cover traffic / mix-net infrastructure at this stage.

#### Layer 1: the envelope

The wire-level message is a **multi-recipient sealed-sender envelope** built with libsignal's `sealed_sender_multi_recipient_encrypt`. Construction:

1. Sender encrypts the plaintext once with the group's **Sender Key** (see §6 for the Sender Keys mechanism — used both for casual cross-server groups and for action-bound groups' message content).
2. Sender wraps the Sender Key ciphertext inside an `UnidentifiedSenderMessage` that contains a **sender certificate** (libsignal's `SenderCertificate`). The cert is signed by the homeserver's **sender-cert chain** — a long-term trust-root keypair plus a self-signed `ServerCertificate` (single-key shape: `trust_root == ServerCertificate` leaf, `key_id = 1`) generated once at first boot and persisted alongside `ServerSecretParams` in the v3 `GroupCryptoBundle` row. The trust-root *public* half is published on `GET /v1/groups/server-params` (field `sender_cert_trust_root`), pinned by every client on first contact. Each cert binds `(did, device_id, identity_key, expiration)` and is minted at credential-refresh time (§3.11 "Daily credential refresh"); expiration is `redemption_time + 2 days`.
3. Sender calls `sealed_sender_multi_recipient_encrypt` with one `ProtocolAddress` per (recipient, device). The `ProtocolAddress` name **must** be a parseable `ServiceId` string, so all identity + session-store entries are keyed on `Aci::from(did_to_uuid(did)).service_id_string()` rather than the raw DID — this is a project-wide convention enforced in app-core at every `DeviceAddress` / `ProtocolAddress` construction site. Output: a single packed envelope with one slot per recipient device. Each slot is encrypted to that device's identity key; only that device can decrypt to obtain the sender cert + Sender Key ciphertext inside.

The server can route the envelope and apply per-slot delivery without ever decrypting it. Recipients verify the sender cert chains to the pinned trust root (`crypto::sender_cert::validate_sender_cert`), then `sender_keys::group_decrypt` the inner ciphertext (proves the sender is a member of the group, because they hold the Sender Key).

#### Layer 2: the request endpoint

Group sends use a **dedicated endpoint that does not accept session credentials**. The request authenticates with a single `GroupSendFullToken` covering the entire recipient set (not one per recipient — combining endorsements via `endorsements::token_for_recipients` produces a single token over the sum) — not with the user's session bearer:

```
POST /v1/groups/{group_id}/send
Headers:
  Content-Type: application/json
  (no Authorization header)
Body:
  envelope:   base64                  // SSv2 SentMessage from layer 1
  token:      base64                  // GroupSendFullToken over the recipient ServiceId set
  recipients: [{
    service_id_fixed_width: base64    // 17-byte fixed-width-binary, as embedded in `envelope`
    encrypted_member_id:    base64    // sender computes from recipient DID + group key
  }, ...]
  expiry_secs: int64 (optional)
```

Server processing:

1. **Parse the envelope** with `SealedSenderV2SentMessage::parse` (via `crypto::sealed_sender::parse_sent_message`) into per-recipient fanout slices keyed by ServiceId.
2. **Resolve recipients.** For each entry in `recipients`: confirm the supplied `service_id_fixed_width` matches a fanout in the envelope, then look up `encrypted_member_id → group_push_pseudonym` in `member_credentials` for this group. A wrong EMI produces an undeliverable envelope (recipient's keys won't match the ServiceId in the slot) — no security risk, just dropped delivery. EMI not in the group's `member_credentials` ⇒ 400.
3. **Verify the token.** Call `endorsements::verify_token_for_service_ids` against the collected ServiceId set. Note: the server never sees DIDs at this step — verification works on ServiceIds throughout. Failure ⇒ 401.
4. **Enqueue per recipient** into `group_message_queue` (a dedicated table, schema in `migrations/011_group_messages.sql`; separate from the DM `message_queue` because routing semantics, auth model, and wire envelope all differ). The row carries `(recipient_group_pseudonym, group_id, ciphertext, expires_at)`. Live-push immediately to any subscribed pseudonym (§3.11 "Delivery channel" below); otherwise the row waits for either WS subscribe-time drain or HTTP offline pickup.
5. **Discard request metadata.** Do not log source IP, TLS session ID, or any other connection identifier beyond what's needed for the request's own processing. The endpoint's access log records only the group ID, recipient count, and response status — never anything that could identify the sender. Rate limiting is per-IP only (`rate_limit::ACTION_GROUP_SEND`); IPs are kept in a transient counter, never linked to a sender identity.

The user's authenticated WebSocket (if any) is **not used** for this endpoint — the send goes over a separate HTTP POST. The handler never sees the sender.

#### Delivery channel

Two-stage delivery, both keyed on the recipient's per-(group, member) `group_push_pseudonym`:

- **Live WS push.** Devices on the homeserver's authenticated WebSocket send a `SubscribeGroupPseudonyms` frame at connect time carrying the full active set of pseudonyms they want pushes for (one per group they're in). The server maintains a `pseudonym → ws sender` map (`AppState::group_subscriptions`); on subscribe it replaces any prior subscriptions for that socket and **drains queued rows for newly-added pseudonyms**. Outbound `GroupDeliverRequest` frames carry `(message_id, group_id, ciphertext, recipient_group_pseudonym, enqueued_at)`; clients reply with `GroupDeliverAck` and the server deletes the row. Server-tracked acks distinguish DM-vs-group via a small `PendingAck` sum so the right queue row is freed.
- **HTTP offline pickup.** `GET /v1/groups/{id}/messages` and `DELETE /v1/groups/{id}/messages` (both presentation-auth, member-only) let a device drain whatever queued up while it was offline. Pseudonym is resolved server-side from the presentation's `EncryptedMemberId` via `member_credentials`, so the device never has to claim a pseudonym it doesn't own.

A background sweeper (`tasks::group_message_expiry`) deletes rows past `expires_at`.

#### Layer 3: the network layer (out of scope)

Source-IP correlation between this send POST and the user's other authenticated traffic remains possible. Documented limitation. Mitigation is user-side network anonymization (Tor / VPN). Cover-traffic infrastructure is not part of Stage 5; revisit if a use case demands it.

#### Daily credential refresh

To send sealed-sender group messages anonymously, the client needs three credential artifacts. The shape split across **two identified endpoints** (one identity-level, one per-group):

```
// Identity-level: auth credential + per-device sender cert.
POST /v1/groups/credentials
Headers: Authorization: <session bearer>     // identified — server knows the DID
Body:   { did, redemption_time }             // server verifies `did` matches the session's account
Response: {
  response:                 base64,          // zkgroup AuthCredentialWithPniZkcResponse
  redemption_time:          u64,
  sender_cert:              base64,          // libsignal SenderCertificate, signed by the trust-root chain
  sender_cert_expires_at:   u64,             // unix millis; `redemption_time + 2 days`
}
```

```
// Per-group: endorsement bundle covering every active member.
GET /v1/groups/{group_id}/endorsements
Headers: X-Group-Auth: <presentation>        // member-only; pending-invitees rejected
Response: {
  response:                base64,           // zkgroup GroupSendEndorsementsResponse
  expiration_unix_seconds: u64,              // day-aligned (~25–49h ahead per zkgroup's default)
}
```

Why split: the auth credential + sender cert are identity-scoped (one per DID per day) and require the session bearer. The endorsement bundle is group-scoped (one per group per day, MAC'd over every member's EMI) and uses presentation auth so the server doesn't see DIDs at this step. Both refresh independently. Client-side caching: `store::groups` carries the credential and sender cert in the same `group_credentials` row (same expiration class); endorsements are fetched fresh per send for now (caching them locally is a future optimization, gated on profiling).

The refresh is identifiable on the identity-level endpoint (server knows which DID is fetching credentials at which time). Anonymity kicks in at *send time*, not at *refresh time*. This is the same trade Signal makes: daily credential issuance is identified, daily send activity is anonymous within the issuance window.

The client stores the credentials locally and uses them throughout the day. On day rollover (or just before, with some clock skew margin), the client refreshes again. If the client misses a refresh window, sends from that point require either (a) a fresh refresh — which is identifiable — or (b) waiting until the next window. The typical case is a background refresh on app launch / periodically while online.

#### Rate limiting under anonymous auth

The standard "N requests per user per minute" pattern doesn't work — the server can't identify the user. Layered approach:

1. **Per-group rate limit.** Configured in `group_policy.rate_limit_per_minute` (server-visible). Catches a single attacker spamming a group. Permissive enough to accommodate legitimate burst traffic from N members.
2. **Per-recipient capacity via endorsement.** `GroupSendFullToken`s are issued once daily and encode a usage budget — e.g. "valid for at most N sends to this recipient before expiry." Floods a single recipient by one attacker exhaust the token, forcing them to wait for the next daily refresh. Mirrors Signal's approach. The budget is configurable (likely high enough that legitimate users never notice).
3. **IP-based rate limit on the endpoint itself.** Coarse anti-abuse. The server sees source IPs (network-layer caveat above) but uses them only for transient rate-limit counters — never persisted, never linked to a sender identity in any stored record.

The combination defends against most spam scenarios while preserving sender anonymity. Sophisticated attackers can rotate IPs and endorsements; the system isn't claimed to be DoS-resistant against motivated actors — only "not trivially abusable."

#### Abuse handling

Spam reporting requires *selective* sender disclosure: the recipient consents to reveal the sender's identity for one specific message. The flow:

1. Recipient's client decrypts the offending message, extracts the embedded `SenderCertificate` (which contains the sender's identity key, signed by `ServerSecretParams`).
2. Recipient submits a report: `{group_id, original_envelope, sender_certificate, send_token_used, recipient_attestation}`. The report can verify itself: the cert and token are server-signed; the recipient attests the message was delivered to them via these.
3. Server (or group admins; venue TBD) verifies the bundle. The sender is identified *only* for this one report. No retroactive de-anonymization of other messages.

Full design deferred to `docs/12-abuse-handling.md`. Stage 5 needs the report-shape sketch above; the rest can land later.

#### What §3.11 doesn't change elsewhere in the doc

- **State operations** (§3.3 group changes) continue to use identified auth — the actor is identified by `encrypted_member_id` in the `AuthPresentation`. State changes are infrequent enough that per-action sender opacity doesn't carry as much weight as for message sends, and admin / policy enforcement actively needs to know the actor's role.
- **Group push pseudonyms** (§3.7) still route opaquely via the relay; the send endpoint enqueues per recipient via `group_push_pseudonym`, same as state-change notifications.
- **§3.9 discipline rules** are unaffected. The server learns no new identifying information at send time.

### 3.12 Federation interaction

Action-bound groups are **single-server** — a group lives on exactly one homeserver, called the *hosting server*. Cross-server participation works in one of two ways:

**Multi-homing (the primary path; covered by Stage 5).** Per `docs/13-federation.md`, a user has accounts on potentially multiple servers — one *discovery server* (in PLC) and zero or more additional *member servers*. To participate in a group on hosting server X, the user becomes a member of X (acquires a per-server account, prekey bundle, queue, WebSocket). From the group's perspective, all members are local to X — the design in §3.1–§3.11 applies without modification. Group messages, state mutations, push routing, and credentials are all server-local on X.

The elegant consequence (per `docs/13-federation.md` line 51): **same-community group conversations never federate.** All members of a group hosted on `safe-haven.org` have accounts on `safe-haven.org` and exchange traffic entirely within it. Federation enters the picture only at *account-creation time* (the join-server step) and for *DMs that cross server boundaries* (e.g. an invite DM where the inviter and invitee don't yet share a server).

This means the federation surface in §3.10's invite flow is concentrated at one place: **if the invitee is not yet a member of the hosting server, their client first walks them through joining it.** The `GroupContext` DM carries `hosting_server_url` so the client knows where to direct them. Once they're a member of the hosting server, the §3.10 flow proceeds normally.

**Guest access (deferred to a future doc; tracked as §4 open question on federated guest credentials).** Per `docs/00-design.md` §"Guests across federation," a user on server Y can participate in a group on server X *without* registering a full membership on X — Y issues a scoped identity claim, X accepts it and issues a time-limited guest credential. The group operates on X; the guest's client interacts with X directly using the guest credential. Stage 5 does not include guest credentials; the design depends on §2.3 landing first and is out of scope for this doc.

What's the same in both flavors:

- Group state lives on the hosting server. There is no replication, no cross-server group fork, no consensus protocol.
- Group push pseudonyms register with the relay (which is global, not per-server); push delivery works for members regardless of how many servers they're on.
- Sender opacity (§3.11) and membership opacity (§3.9) properties hold per the design — the hosting server doesn't learn additional identifying info because a member is multi-homed or a guest.
- Mesh fallback (§7) doesn't care about server identity; Sender Key ciphertext flows the same way regardless of where the group "lives."

What changes in the guest flavor (when designed):

- A new credential type (`AuthCredentialGuest` or similar) issued by X against a Y-signed claim.
- Likely a separate column or flag in `member_credentials` distinguishing full from guest (or a sibling `member_credentials_guests` table).
- Guest scope rules: probably no admin role, no inviting further members, time-limited expiry that requires renewal via Y.
- A federated trust mechanism between X and Y (peering, signature verification, abuse signals).

These are real design work but are not blockers for Stage 5's multi-homing case.

## 4. Open questions and blockers

Tracked here for visibility; each needs an answer before Stage 5 is unblocked.

1. **Account deletion under strict opacity.** §3.9. When a DID deletes its account, the server can't walk the user's group memberships to clean up. Options: (a) client-driven cleanup before delete (multi-step flow; failure leaves opaque blobs lingering); (b) best-effort tombstone (delete proceeds immediately, client attempts cleanup in background, lingering memberships GC'd when other members notice); (c) hybrid (try cleanup first, tombstone on failure). Needs decision before Stage 5.
2. **Abuse handling for sealed-sender group sends.** §3.11 specifies envelope, request endpoint, daily credential refresh, and rate limiting. The abuse-handling sub-question (where reports are routed — server, group admins, or a hybrid; how reporter identity is protected; how a sender-disclosure release plays with the membership-opacity discipline) is sketched in §3.11 "Abuse handling" and deferred to `docs/12-abuse-handling.md` for the full design.
3. **Federated guest credentials.** Out of scope here; depends on §2.3 landing first.

## 5. Sketch of the Stage 5 implementation path

Rough order, assuming the open questions above are answered:

1. **[done]** Add `zkgroup` and `zkcredential` to the workspace; the workspace `[patch.crates-io]` rewrites `curve25519-dalek` 4.1.3 to Signal's fork (zkgroup requires it). `crypto::groups::ServerSecretParams` / `ServerPublicParams` are thin newtypes around `zkgroup::ServerSecretParams` / `ServerPublicParams` — no custom credential key, since per §2.3 we use stock zkgroup auth credentials. Persisted on the homeserver via migration `010_groups.sql`'s `zkgroup_server_params` table, served at `GET /v1/groups/server-params`.
2. **[done]** Auth credential issuance and verification use stock `zkgroup::auth::AuthCredentialWithPniZkc{Response, Presentation}`. The carried identity is `Aci::from(UUID(did))` for the primary attribute and `Pni::from(UUID(did))` for the secondary — same UUID bytes, distinct UidStructs because of the service-id-type tag. `crypto::groups::did_to_uuid(did) -> uuid::Uuid` provides the deterministic `SHA-256(b"actnet-did-to-uuid-v1" || did)[..16]` derivation; server and client agree on it. (History: an earlier `crypto::groups::credentials` module built a parallel DID-shaped scheme on `zkcredential` primitives; superseded by §2.3 option 1. See §2.4 for rationale.)
3. **[done]** `crypto::groups::GroupKey` wraps `zkgroup::GroupSecretParams` directly. Exposes `generate`/`from_bytes`/`to_bytes`/`group_id`/`encrypt_state`/`decrypt_state`/`public_params`/`encrypt_member_id(did)`. `encrypt_member_id` returns a stock `zkgroup::groups::UuidCiphertext` (computed as `secret_params.encrypt_service_id(Aci::from(UUID(did)).into())`). `decrypt_member_id` is not exposed; clients resolve EMI → DID via the cleartext members list inside the encrypted state blob. `GroupPublicParams` is a thin wrapper around `zkgroup::GroupPublicParams` — the routing `group_id` plus uid-encryption public material, uploaded at create-group time and used server-side to verify presentations.
4. **[done]** Server endpoints:
   - `GET /v1/groups/server-params` (public) — publish `ServerPublicParams`.
   - `POST /v1/groups` (session-auth) — founder uploads `GroupPublicParams`, initial encrypted state, their own admin row. 409 on duplicate `group_id`.
   - `POST /v1/groups/credentials` (session-auth) — daily credential refresh. Server verifies the requested DID matches the session's account before issuing.
   - `GET /v1/groups/{id}` and `GET /v1/groups/{id}/changes?from_revision=` (presentation-auth, membership-gated; non-members get 404 to hide existence per §3.11 info-flow leak rule, NOT 403 as originally sketched in §3.4).
   - `POST /v1/groups/{id}/changes` (presentation-auth) — full §3.3 action handler: classify self-vs-admin, revision freshness, per-class eligibility, role check against `group_policy`, transactional apply of all 11 action types (invite/promote/decline/remove/role-change/join-via-link with OpenLink vs RequestToJoin branching/cancel/approve/deny/modify_policy/sub-encrypted modify_*), revision bump + history append.
   - `POST /v1/groups/{id}/push_binding` (presentation-auth) — rotate `group_push_pseudonym`.

   Schema: migration `010_groups.sql` with `groups`, `group_state_history` (256-revision ring buffer), `member_credentials`, `members_pending`, `members_pending_approval`. Every column annotated per §9 invariant 1; pending tables use day-aligned timestamps per §3.9 rule 5. Rate limiting: per-account on session-authed endpoints (create / credentials issuance); per-IP on presentation-authed write endpoints (changes / push_binding), the only handle available since presentation auth doesn't bind to an account. Per-group rate limiting is deferred — the existing `rate_limit_counters` table is keyed on `account_id`; per-group needs a parallel table.

   **Convention pinned during step 4:** group endpoints speak URL-safe-no-pad base64 everywhere (URL paths, headers, JSON bodies, response bodies). One alphabet, one decoder. Other endpoints in the server keep their existing standard base64.
5. **[done]** `app-core`: `create_group`, `invite_member`, `accept_invite`, `decline_invite`, `join_via_link`, `cancel_join_request`, `approve_join_request`, `deny_join_request`, `remove_member`, `change_member_role`, `fetch_group_state`, `apply_pending_group_changes`, `rotate_group_pseudonym`. `invite_member` internally submits `invite_members` and sends the per-recipient `GroupContext` DM (§3.10); inbound `GroupContext` is handled in both the WS path (`process_decrypted`) and the HTTP path (`receive_messages`), persisting the master key locally so a follow-up `fetch_group_state` shows the pending row. `join_via_link` returns an enum indicating immediate-member vs pending-approval (server-decided) so the UI renders the right state.

   Plumbing: encrypted-state plaintext is `proto::groups::GroupState` (new `core/proto/groups.proto`); `GroupContext` is a new variant on `ContentMessage` in `core/proto/content.proto`. `store::groups` adds tables for the per-group row (master key, cached state plaintext, policy mirror, push pseudonym), daily-cached credentials, and server-params cache. `net::groups` wraps all seven `/v1/groups/*` endpoints with URL-safe-no-pad base64 and the `X-Group-Auth` header for presentation-auth routes. `app-core::groups` exposes helpers `ensure_server_params` / `ensure_credential` / `build_presentation_bytes` over the cached state, plus the 13 sync FFI methods. Async equivalents (`*_async`) exposed for tests / testbot. Sync FFI, blocking on the global tokio runtime, per existing convention.

   **Not yet wired up:** scheduled credential refresh / pseudonym rotation. `rotate_group_pseudonym` exists as a callable FFI method but nothing on a timer drives it yet.

   E2E test in `core/crates/app-core/tests/e2e_groups.rs` covers create → invite (with GroupContext DM) → fetch → accept → re-fetch. Compiles; needs a running homeserver (`make test-e2e`).
6. Group send. Split into two PRs.
   - **[done] PR 1: Sender Keys content-encryption over the existing DM transport.** `store::Store` implements libsignal's `SenderKeyStore` against a new `sender_keys` table. `crypto::sender_keys` wraps `create_skdm` / `process_skdm` / `group_encrypt` / `group_decrypt`. Two new `ContentMessage` body variants — `SenderKeyDistribution` and `GroupMessage` — carry SKDMs and Sender-Keys-encrypted content respectively. `app-core::groups`: deterministic `distribution_id_for(master_key)` (UUIDv5 under a fixed namespace), `seed_own_sender_key` at create / accept, automatic SKDM exchange piggybacked on the invite (admin→invitee) and broadcast on accept (invitee→all current members). FFI `send_group_message(group_id, plaintext)` encrypts under our Sender Key and fans out as DMs to every other member, including the same `proto::GroupMessage` body. Inbound `SenderKeyDistribution` installs the sender's key; inbound `GroupMessage` decrypts and surfaces as a normal message event with `sender_did` from the envelope. e2e test verifies the full create→invite→accept→send→reply roundtrip.
   - **[done] PR 2: sealed-sender + dedicated send endpoint.** Group messages are now wrapped in `sealed_sender_multi_recipient_encrypt` on a dedicated endpoint `POST /v1/groups/{id}/send` authenticated with a `GroupSendFullToken` over the full recipient ServiceId set (not session credentials). See §3.11 for the protocol-level description; landed surface:

     **Crypto wrappers** (`core/crates/crypto/src/`):
     - `sender_cert.rs` — `SenderCertChain::generate / to_bytes / from_bytes / trust_root_public_bytes / issue_sender_cert` and `validate_sender_cert` (single-key trust-root chain, `key_id=1`).
     - `sealed_sender.rs` — `encrypt_group_envelope`, server-side `parse_sent_message`, recipient-side `decrypt_envelope_to_usmc`.
     - `groups/endorsements.rs` — `default_expiration_unix_seconds`, `issue_endorsements`, `receive_endorsements`, `token_for_recipients`, `verify_token` / `verify_token_for_service_ids`.

     **ServiceId migration.** Identity + session-store entries are keyed on `Aci::from(did_to_uuid(did)).service_id_string()` — required by SSv2 (libsignal parses `ProtocolAddress.name()` as a `ServiceId`). New helper `crypto::groups::did_to_service_id_string`; every `DeviceAddress` / `ProtocolAddress` construction site in `app-core` now uses it. DM tests still pass; pre-launch DBs can be wiped.

     **Server**:
     - `GroupCryptoBundle` (zkgroup secret + sender-cert chain) persisted in `zkgroup_server_params` row; `CURRENT_VERSION` bumped to 3.
     - `GET /v1/groups/server-params` now publishes `sender_cert_trust_root`.
     - `POST /v1/groups/credentials` response carries a per-device `sender_cert` (+ `sender_cert_expires_at`). The `devices.identity_key` uploaded at registration is the signing input.
     - `GET /v1/groups/{id}/endorsements` (presentation-auth, member-only) returns the daily `GroupSendEndorsementsResponse`.
     - `POST /v1/groups/{id}/send` (no auth header) — parses the envelope, resolves each `(encrypted_member_id → group_push_pseudonym)` via `member_credentials`, verifies the token against the envelope's ServiceIds, enqueues per recipient. IP rate-limited only.
     - `GET` / `DELETE /v1/groups/{id}/messages` (presentation-auth, member-only) — HTTP offline pickup; pseudonym resolved server-side from the presentation's EMI.
     - WS: new `SubscribeGroupPseudonyms` / `GroupDeliverRequest` / `GroupDeliverAck` frames (`ws.proto`); `AppState::group_subscriptions` map (`pseudonym → ws sender`); per-socket subscription tracking; drain-on-subscribe; `PendingAck` sum distinguishes DM vs group acks.
     - Schema: `migrations/011_group_messages.sql` (`group_message_queue` table — kept separate from the DM queue because auth model, wire envelope, and routing semantics all differ).
     - Background sweeper task `group_message_expiry`.

     **Client** (`net::Client` + `app-core::groups`):
     - `net::Client::send_group_message / fetch_group_messages / ack_group_messages / get_group_endorsements`.
     - `groups::send_group_message` free function (also wired into the existing FFI `send_group_message` and `send_group_message_async`): loads cached credential + sender cert, fetches endorsements, builds SSv2 envelope + recipient mapping (each entry carries `service_id_fixed_width` and the EMI we compute from recipient DID + group key), POSTs.
     - `groups::fetch_group_messages` (+ `fetch_group_messages_async`): drains via HTTP, runs each envelope through `decrypt_envelope_to_usmc → validate_sender_cert → group_decrypt`, returns `ReceivedGroupMessage { message_id, group_id_b64, sender_did, sender_device_id, plaintext }` and acks server-side.

     **e2e tests** (`core/crates/app-core/tests/e2e_groups.rs`):
     - `create_invite_accept_promote_remove_roundtrip` — 2-member group, exercises the sealed-sender round-trip.
     - `three_member_fanout_roundtrip` — 3-member group; one send fans out to two recipients (exercises SSv2 multi-recipient parsing and per-recipient routing); bob and carol decrypt the same plaintext from alice, then carol's reply fans out to alice and bob. Both green; full `make test-e2e` still passes.

     **Not yet wired up:**
     - **WS push on the client side.** `net::ws::WsConnection` exposes `subscribe_group_pseudonyms / next_group_message / group_ack`, but no app-core code drives them yet — the e2e uses HTTP fetch. Production receive path needs to wire WS subscribe at connect and dispatch `GroupDeliverRequest` through the same `process_decrypted` pipeline as DMs.
     - **Sync FFI receive surface.** Only the `_async` flavor exists.
     - **Endorsement caching.** Fetched fresh per send; trivial to cache if profiling shows it matters.
     - **Coverage gaps in e2e.** No tests for expired sender cert, mismatched token, or recipient-not-in-group rejection paths (these are server-side branches with zero coverage).
7. Push: per-group pseudonym registration at join via the existing `register_push_with_relay` (`core/crates/net/src/lib.rs:519`) — **no new relay endpoint needed**, the relay's `/v1/register` is reused. Wakeups on new revisions and new messages forwarded to relay by group pseudonym.
8. **[partial]** Mobile UI: create group, list members, simple roles, expiry timer setting, send/receive.

   **[done]**:
   - Compose 2+ recipients fans out `create_group` + per-recipient
     `invite_members` and tail-sends the first message
     (`AppState.createGroupAndOpen` in `mobile/ios/Actnet/Sources/App/AppState.swift`).
   - `Conversation` model carries `groupId` and an `isGroup` flag;
     `groupConversationId(_:)` derives the persistence key (prefix
     `group-<groupIdB64>`).
   - `ConversationView` dispatches `sendGroupMessage` for group threads
     and exposes a toolbar link to a new `GroupDetailView`.
   - `GroupDetailView` (`mobile/ios/Actnet/Sources/Views/Chats/GroupDetailView.swift`)
     renders title, description, revision, member list with admin badge,
     pending-invite count, and a `Leave group` action that calls
     `remove_member` on the current user's `encrypted_member_id`.
   - Inbound `proto::GroupMessage` carries the `group_id` (newly added
     `Option<String>` field on `DecryptedMessage`) so the iOS event loop
     routes group messages into the right thread instead of inferring
     from sender DID.
   - Lazy title refresh: `AppState.refreshGroupTitle` runs on conversation
     open and on first inbound group message; cached titles survive an
     app restart via `groupTitleCache`.

   **Not yet implemented**:
   - Admin actions in the detail screen (add member, change role, remove
     other members, edit title/description/expiry, invite-link toggle,
     announcement-only).
   - Per-member tap to view profile / block / report.
   - Pending-approval UI for `RequestToJoin` groups (approve / deny).
   - Render the sender's display name above a message bubble in a group
     thread (currently bubbles render without an author label).
   - Group icon mosaic (placeholder icon only).
   - WS push receive path for group messages on the client — the e2e
     uses HTTP fetch (matches the gap recorded in step 6 PR 2).
9. Multi-client integration test (20 clients) per Stage 5's existing test plan.

## 6. Cross-server casual groups (Stage 9): not in this doc

The Sender Keys scheme is described in `docs/01-technical-implementation.md` §"Cross-server casual group encryption" and isn't expanded here. The membership-churn protocol (who rotates keys when, how stragglers recover) deserves its own doc before Stage 9. The interface in `crypto::groups` should be designed so that Sender Keys groups and zkgroup groups expose the same `encrypt`/`decrypt`/`add_member`/`remove_member` shape to `app-core`, with the scheme picked per-group at creation. MLS as a future replacement for either or both stays an option behind that interface.

## 7. Action-bound groups over bitchat mesh

When the homeserver is unreachable and the user enables bitchat (see `docs/14-bitchat-fallback.md`), action-bound groups operate in a degraded but useful mode. The interaction is more compatible than the bitchat doc's framing suggests, because zkgroup is the *authorization* layer for groups while the actual on-the-wire ciphertext is Sender Keys — and Sender Key traffic flows over bitchat unchanged.

**What works on mesh in steady state:**

- **Sending and receiving messages** in groups you're already a member of. The Sender Key for the group was distributed at join time over the regular DM/server path; mesh just transports the ciphertext. Recipients decrypt as normal.
- **Sender authentication.** Sender Key distribution messages are signed by the sender's identity key, so receivers know which member each message came from without needing a server-issued credential.

**What does NOT work on mesh:**

- **State mutations.** Add/remove member, role changes, title/description edits, expiry-timer changes all go through `POST /v1/groups/{id}/changes`. Mesh has no server, so these are queued client-side until reconnect.
- **Daily credential refresh.** Credentials gate server-side operations (presentation for state fetch, push-binding rotation). Mesh-only operation doesn't need a fresh credential — but on reconnect, the client must fetch a new one before any server-side action.
- **GroupSendEndorsements.** These are the server's enforcement primitive ("token X proves the sender is allowed to send to recipient R in this group"). With no server in the loop on mesh, no token is checked; the Sender Key itself proves group membership to recipients.
- **Sealed sender envelope.** The anonymity property is "hide sender identity from the server." On mesh there's no server, so the envelope is redundant; we can either strip it (saves packet bytes) or leave it for code-path uniformity. Implementation choice; security implication is nil.

**Membership opacity from the server is unchanged.** Mesh traffic doesn't reach the homeserver, so server-side opacity (§3.9) is unaffected by what happens on mesh. Membership-observability to *adjacent BLE peers* is the standard bitchat trade — see `docs/14-bitchat-fallback.md` §2.5's forced-mesh-activation threat. zkgroup-based action-bound groups are no better or worse than casual Sender Keys groups in this respect, because the relevant ciphertext shape is the same.

The summary rule: **mesh inherits exactly the steady-state operation of a zkgroup-based group, minus state mutations.**

## 8. Threat checklist (PR review gate)

This section is the standing review checklist for any group-related PR. The reviewer walks through it before approving. Each item names the attack and points to the design section that documents the defense; the reviewer's job is to verify the implementation actually realizes the documented defense, not just claim to.

A PR is "group-related" if it touches: any file under `core/crates/crypto/src/groups*`, any `core/crates/server/src/db/group*` or `core/crates/server/src/routes/group*`, any `core/crates/app-core/src/lib.rs` method named `*_group*` / `*_member*` / `join_*` / `invite_*` / `approve_*` / `deny_*`, any group-related migration, or any change to `proto/content.proto` adding group-related message types.

### Authorization at submission

For each submitted `GroupChange` action, verify the server:

- [ ] Verifies the `AuthPresentation` under `server_secret_params` and the group's `GroupPublicParams` (§3.3 step 1). Rejected presentations never proceed.
- [ ] For admin-class actions: confirms `presented_encrypted_member_id ∈ member_credentials[group_id]` (§3.3 step 2).
- [ ] For `promote_pending_members` / `decline_invite`: confirms `presented_encrypted_member_id ∈ members_pending[group_id]` (§3.3 step 2).
- [ ] For `cancel_join_request`: confirms `presented_encrypted_member_id ∈ members_pending_approval[group_id]` (§3.3 step 2).
- [ ] For `join_via_link`: confirms `invite_link_password` matches `group_policy.invite_link_password` with **constant-time compare** (§3.3 step 2). Branches on `join_policy` and rejects `Closed`.
- [ ] Enforces revision freshness: `submitted_revision == current_revision + 1` (§3.3 step 3). Concurrent submissions return 409.
- [ ] Self-class actions are the **sole** action in their `GroupChange` (§3.3 "Self-actions vs. admin actions"). Batches containing a self-action with anything else are rejected.
- [ ] Role check uses both `member_credentials.role` for the actor and `group_policy` for the per-action minimum (§3.3 step 4). `modify_policy` and `modify_member_role` are protocol-fixed Admin regardless of policy.

### Authorization at fetch and delivery

- [ ] `GET /v1/groups/{id}` rejects non-members with 403 even when presentation is otherwise valid (§3.4 "Fetch is membership-gated"). Same for `GET /v1/groups/{id}/changes`.
- [ ] WebSocket `subscribe` frames are checked against the connected account's actual memberships *only at the point of message delivery*, not eagerly: server doesn't pre-populate the in-memory map with anything the client didn't explicitly subscribe to, and refuses to deliver to a pseudonym the client didn't claim (§3.7 "Delivery to online clients"). A client claiming a pseudonym they don't actually own results in the legitimate owner not receiving WS deliveries (because the in-memory map has the wrong entry); the legitimate owner's later subscribe will overwrite, but for a moment messages might be silently lost. **Audit:** how do we prevent claim-squatting? Two compatible mitigations: (a) reject `subscribe` for a pseudonym not in this account's `member_credentials` rows — but that requires the server to know "is this account a member of any group with this pseudonym?", which we want to avoid (§3.9). (b) allow concurrent subscriptions; deliver to *all* claimers and rely on at-recipient decryption failure to be no-op for impostors. Option (b) is the right answer; (a) violates §3.9. Verify the implementation does (b).
- [ ] Send endpoint `POST /v1/groups/{id}/send` does NOT accept a session-bearer Authorization header; only `GroupSendFullToken` per-recipient (§3.11 Layer 2). Code that processes this endpoint must never read the session context.
- [ ] Send endpoint validates each `GroupSendFullToken` for: signed by `ServerSecretParams`, not expired, bound to this group, bound to the specific `encrypted_member_id` recipient (§3.11 Layer 2 step 1). Rejected slots don't fail the whole envelope.

### §3.9 schema discipline

For each new table, column, or cache:

- [ ] No `(did → groups_joined)` table or cache, even ephemeral on disk. In-memory ephemeral state (e.g. the WebSocket subscription map, §3.7) is acceptable if explicitly documented as such.
- [ ] No `(encrypted_member_id → did)` map, ever.
- [ ] No persisted log of credential issuance with credential identifier. Rate-limit counters per DID per day only.
- [ ] No persisted log of presentation verification with credential ID or `encrypted_member_id`. Counts only.
- [ ] Timestamps on `member_credentials` / `members_pending` / `members_pending_approval` are jittered or omitted (rule 5).
- [ ] Send endpoint logs do not include source IP, TLS info, or any other connection identifier (§3.11 Layer 2 step 4). Verify the request handler's logging path.
- [ ] Any new column on a group-related table is annotated in the migration with `-- public | -- opaque | -- ephemeral | -- exempt` (see §9 invariant 1).

### Wire-format subencryption boundaries

For each action type:

- [ ] Fields the server must read (`encrypted_member_id`, `role`, `group_push_pseudonym`, `invite_link_password`, `Policy`) are **plaintext to the server**, opaque only in the sense of being encrypted_member_id-style ciphertexts.
- [ ] Fields the server must NOT read (`encrypted_profile_key`, `modify_title`, `modify_description`, `modify_expiry`) are **sub-encrypted under the group key** and present in the actions blob only for client broadcast — never persisted server-side. Re-read §3.3 "Apply structural changes" for the exhaustive list.

### Atomicity and consistency

- [ ] State updates (encrypted_state blob bump, `current_revision` increment, history append, table mutations) are in a single database transaction (§3.3 step 6). Partial application is impossible.
- [ ] Cross-table operations (`approve_join_request` moves a row from `members_pending_approval` to `member_credentials`) are within the same transaction.
- [ ] Invite flow atomicity: client submits `invite_members` first, then sends the per-recipient `GroupContext` DMs. If the DMs fail, the pending row is recoverable evidence (§3.10 "Atomicity").

### Information-flow leaks

- [ ] Error responses don't leak existence: a `request_to_join` for a `Closed` group, and a `request_to_join` with a wrong password to a `RequestToJoin` group, both return identical 403 responses to an unauthenticated probe. (Defense against "is group X open to join?" reconnaissance.)
- [ ] Timing: membership-check rejection paths take constant time relative to the membership table size (e.g. use indexed lookup, not linear scan). Otherwise an attacker can infer membership-table size from response timing.
- [ ] Error messages for invalid `AuthPresentation` don't reveal whether the credential was issued by this server, expired, malformed, or just signed wrong. Single "invalid presentation" response.

### Mesh-specific (§7)

- [ ] On bitchat mesh, the group tag derivation uses **per-sender** Sender Keys, not the per-group master key (§7 / §3.10 security note). Verify the tag-derivation function takes `sender_key`, not `group_master_key`.
- [ ] State mutations submitted on mesh (if the mesh code surfaces this) are queued client-side and replayed to the homeserver on reconnect, not delivered via mesh broadcast.

### Rate limiting

- [ ] Per-group rate limit applied to `POST /v1/groups/{id}/send` based on `group_policy.rate_limit_per_minute` (§3.11 "Rate limiting").
- [ ] Per-recipient capacity enforced via `GroupSendFullToken` budget (§3.11).
- [ ] IP-based rate limit applied to the send endpoint as anti-abuse, transient counters only, not persisted (§3.11).
- [ ] Rate limits applied to all group state-change endpoints, especially `join_via_link` (anti-spam-request defense).

### Forbidden code patterns (caught by §9 invariant tests)

The §9 invariant tests catch the following automatically; reviewers can sanity-check during review but the tests are the binding enforcement:

- [ ] No `tracing::*` call in group-related modules passes a `Did`, `account_id`, or unencrypted DID alongside any `encrypted_member_id` or `group_id`.
- [ ] No `pub fn` in `crypto::groups` accepts an `EncryptedMemberId` and returns a `Did` without taking a `GroupKey` parameter (decryption requires the key, in the type system).
- [ ] No SQL in `server/src/db/group*` joins `member_credentials` (or `members_pending*`) with `accounts` or `push_pseudonyms` (the DM table).

## 9. Implementation invariants (test-enforced)

The properties below are invariants the implementation must hold. Each names a test (or lint) that enforces it, with the file path where the test will live. CI runs all of these. New invariants get added here when discovered; never remove an invariant without explicit design-doc review.

### Invariant 1: schema annotations

**Property.** Every column in every migration file for group-related tables carries one of the annotations `-- public`, `-- opaque`, `-- ephemeral`, or `-- exempt` as a trailing line comment. Annotations are also required for new columns added to existing tables.

**Why.** Forces the migration author to consciously categorize each column. `public` is "server uses freely, no privacy weight"; `opaque` is "server stores but cannot decrypt" (e.g. `encrypted_member_id`, `encrypted_state`); `ephemeral` is "row deleted when the resource resolves" (e.g. members_pending entries); `exempt` is "this is a known-leak we've accepted, explain in the comment."

**Test.** `core/crates/server/tests/migration_schema_audit.rs`. Walks `core/crates/server/migrations/*.sql`, finds all `CREATE TABLE` / `ALTER TABLE ADD COLUMN` for tables matching `groups|member_credentials|members_pending|members_pending_approval|group_policy|group_*`, and verifies each column has an annotation in the trailing comment. Test fails with the offending file path and column name.

### Invariant 2: no DID-bearing columns in group routing tables

**Property.** No column in `member_credentials`, `members_pending`, `members_pending_approval`, or `group_policy` is named `did`, `account_id`, contains `_did` as a substring, or is typed as a foreign key to `accounts(id)` / `accounts(did)`. (Exception explicitly written: `members_pending_approval.requester_did` if we ever revert to the DID-in-pending design from the membership-opacity discussion; would require a §9 amendment.)

**Why.** Direct enforcement of §3.9 rule 1.

**Test.** `core/crates/server/tests/migration_schema_audit.rs`. Same walk as invariant 1; for each `CREATE TABLE` / `ALTER TABLE` on the named tables, regex against forbidden column names and foreign-key declarations. Test fails with the offending column.

### Invariant 3: no DID + encrypted_member_id in the same log line

**Property.** No `tracing::*` call in `core/crates/server/src/db/group*.rs`, `core/crates/server/src/routes/group*.rs`, or `core/crates/crypto/src/groups*.rs` has both a `Did` (or `did:` literal, or anything typed `&str` named `did`) and an `EncryptedMemberId` / `encrypted_member_id` in the same call.

**Why.** Direct enforcement of §3.9 rule 2 and 4. A log line correlating the two is exactly the artifact that defeats opacity.

**Test.** `core/crates/server/tests/logging_audit.rs`. Walks the listed source paths, parses each `tracing::info!`, `tracing::warn!`, etc. macro call, and inspects the arguments. Implementation: simple AST walk via `syn` is sufficient (these macros' arg lists are visible at parse time). Test fails with file:line of the offending call.

### Invariant 4: send endpoint rejects session credentials

**Property.** The handler for `POST /v1/groups/{id}/send` does not have the standard session-auth middleware applied, and the handler function signature does not accept an authenticated-user extractor.

**Why.** Direct enforcement of §3.11 Layer 2. If the handler accepts a session credential, the server's request context will contain the sender's identity and the sender opacity property is defeated.

**Test.** `core/crates/server/tests/send_endpoint_unauthenticated.rs`. (a) Compile-time: a static-typed test that the handler function does not accept the auth-extractor type. (b) Runtime: a test that sends a valid envelope WITH a session bearer and verifies the server ignores the bearer (and returns the same response as without it).

### Invariant 5: state updates are transactional

**Property.** Every code path that mutates `groups`, `member_credentials`, `members_pending`, `members_pending_approval`, or `group_policy` does so within a `pool.begin()` transaction, not a `pool.acquire()`. The transaction commits exactly once at the end of the request handler. Partial commits are impossible.

**Why.** §3.3 step 6 atomicity. A partial state update leaves the group inconsistent and may grant unintended access.

**Test.** `core/crates/server/tests/transactional_writes.rs`. AST walk via `syn` over `core/crates/server/src/routes/group*.rs` and `core/crates/server/src/db/group*.rs`. For each function that writes to a group table, verify it takes `&mut Transaction<'_, Postgres>` (not `&mut PgConnection` directly). Functions that take `&mut PgConnection` and are not called from a transactional caller fail the test.

### Invariant 6: send endpoint discards connection metadata

**Property.** The send endpoint's handler does not call `tracing::*` (or any logging function) with the request's source IP, TLS session, or any header value other than the request body fields.

**Why.** §3.11 Layer 2 step 4. Logged IPs combined with anonymous endorsements defeat sender opacity for the database adversary.

**Test.** `core/crates/server/tests/send_endpoint_no_metadata_logging.rs`. AST walk: for the send endpoint handler and any functions it calls, verify no expression of type `IpAddr`, `SocketAddr`, `&Request<_>`, or `HeaderMap` is passed to a logging macro.

### Invariant 7: forbidden joins

**Property.** No SQL query in `core/crates/server/src/db/group*.rs` joins `member_credentials` (or `members_pending*`) with `accounts` or with the DM `push_pseudonyms` table. (Group push pseudonyms live in `member_credentials` directly; no join is needed for routing.)

**Why.** §3.7 / §3.9 — the join would expose the link the design specifically structures to avoid.

**Test.** `core/crates/server/tests/forbidden_joins.rs`. Regex over `db/group*.rs` SQL string literals; flag any query containing both `member_credentials` (or `members_pending*`) and `accounts` or `push_pseudonyms`. False positives can be whitelisted with an `#[allow(forbidden_join, reason = "...")]` annotation on the surrounding function.

### Invariant 8: §3.9 discipline coverage

**Property.** Every rule in §3.9 ("Schema discipline for membership opacity") is enforced by at least one of the invariants above. If a §3.9 rule isn't covered by a test, this invariant fails.

**Why.** Forces the §9 invariants and §3.9 rules to stay in sync. If we add a discipline rule and forget the test, this catches it.

**Test.** `core/crates/server/tests/discipline_coverage.rs`. Parses §3.9 of `docs/03-groups.md`, extracts the numbered rules, and verifies each rule is referenced by a comment in at least one test file. The reference format: a comment like `// Enforces §3.9 rule N` somewhere in the test source. Test fails listing any unenforced rule number.

### Adding new invariants

When a new design property emerges (e.g. from §3.9 amendment, new action types, new endpoint), the contributor adds:

1. A line in this §9 listing the invariant, with the test path.
2. The test itself in the listed file.
3. A `// Enforces invariant N` comment in any related production code (optional but recommended).

The §8 threat checklist should also be reviewed for any new items implied by the new invariant.

### What these don't catch

These invariants enforce structural properties at the schema, logging, and type-system level. They do NOT catch:

- Logic bugs in handlers (wrong condition, off-by-one in revision check, etc.) — covered by the adversarial test suite (§5 step 9 plus expansion).
- Race conditions between concurrent requests — covered by integration tests with concurrent clients.
- Crypto-layer mistakes — bounded by trust in libsignal and `crypto::groups::credentials` unit tests.
- Operational mistakes (wrong key loaded, server misconfiguration) — covered by deployment runbooks, not source-code tests.

The invariants are a floor, not a ceiling. They reduce one specific class of bug — discipline drift over time, where individual PRs each look fine but the cumulative state violates a property — to near zero. Other bug classes need their own defenses.
