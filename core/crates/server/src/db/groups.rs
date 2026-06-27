//! Database access for action-bound groups. See `docs/03-groups.md`.
//!
//! All functions take `&mut PgConnection`. Callers reach for `pool.acquire()`
//! when an operation is a single statement and the transactional guarantees
//! don't matter; the change-application path (§3.3) wraps everything in
//! `pool.begin()` because revision bump + history append + membership-table
//! mutation must commit together (§9 invariant 5).
//!
//! Function naming follows the existing crate convention (e.g. `accounts.rs`):
//! plain verbs, no `_group` suffix since the module already establishes the
//! domain.

use sqlx::{PgConnection, Row};

/// Server-side view of a group: routing fields the server needs, plus the
/// opaque encrypted state blob it stores but cannot read.
pub struct Group {
    pub group_id: Vec<u8>,
    pub server_public_params_version: i32,
    pub group_public_params: Vec<u8>,
    pub current_revision: i64,
    pub encrypted_state: Vec<u8>,
    pub policy: Policy,
}

/// Server-readable per-action policy + join policy + invite-link password.
/// Mirrors the `Policy` wire shape in §3.3. Role values: 0 = Member, 1 = Admin.
/// `join_policy`: 0 = Closed, 1 = RequestToJoin, 2 = OpenLink.
pub struct Policy {
    pub invite_members_role: i16,
    pub remove_members_role: i16,
    pub modify_title_role: i16,
    pub modify_description_role: i16,
    pub modify_expiry_role: i16,
    pub join_policy: i16,
    pub invite_link_password: Option<Vec<u8>>,
    pub announcement_only: bool,
}

/// Inputs for a brand-new group. The founder uploads the initial encrypted
/// state, the `GroupPublicParams` derived from their `GroupMasterKey` (so
/// the server can verify future presentations), and their own
/// `encrypted_member_id` + `group_push_pseudonym` so they have an Admin row
/// from the moment the group exists.
pub struct NewGroup<'a> {
    pub group_id: &'a [u8],
    pub server_public_params_version: i32,
    pub group_public_params: &'a [u8],
    pub encrypted_state: &'a [u8],
    pub policy: &'a Policy,
    pub founder_encrypted_member_id: &'a [u8],
    pub founder_group_push_pseudonym: &'a [u8],
}

/// One revision in the encrypted-state history ring buffer.
pub struct StateChange {
    pub revision: i64,
    pub encrypted_state: Vec<u8>,
    pub actions: Vec<u8>,
}

/// Insert a freshly-created group along with its founder's Admin row.
/// Caller passes a transaction so the two writes commit together.
pub async fn create(conn: &mut PgConnection, ng: &NewGroup<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO groups (
            group_id, server_public_params_version, group_public_params,
            current_revision, encrypted_state,
            policy_invite_members_role, policy_remove_members_role,
            policy_modify_title_role, policy_modify_description_role,
            policy_modify_expiry_role, policy_join_policy,
            policy_invite_link_password, policy_announcement_only
         ) VALUES ($1,$2,$3, 0, $4, $5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(ng.group_id)
    .bind(ng.server_public_params_version)
    .bind(ng.group_public_params)
    .bind(ng.encrypted_state)
    .bind(ng.policy.invite_members_role)
    .bind(ng.policy.remove_members_role)
    .bind(ng.policy.modify_title_role)
    .bind(ng.policy.modify_description_role)
    .bind(ng.policy.modify_expiry_role)
    .bind(ng.policy.join_policy)
    .bind(ng.policy.invite_link_password.as_deref())
    .bind(ng.policy.announcement_only)
    .execute(&mut *conn)
    .await?;

    sqlx::query(
        "INSERT INTO member_credentials (group_id, encrypted_member_id, role)
         VALUES ($1, $2, 1)",
    )
    .bind(ng.group_id)
    .bind(ng.founder_encrypted_member_id)
    .execute(&mut *conn)
    .await?;
    // The founder's first device registers its routing pseudonym (docs/04).
    insert_member_pseudonym(
        conn,
        ng.group_id,
        ng.founder_encrypted_member_id,
        ng.founder_group_push_pseudonym,
    )
    .await?;
    Ok(())
}

pub async fn get(conn: &mut PgConnection, group_id: &[u8]) -> Result<Option<Group>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT group_id, server_public_params_version, group_public_params,
                current_revision, encrypted_state,
                policy_invite_members_role, policy_remove_members_role,
                policy_modify_title_role, policy_modify_description_role,
                policy_modify_expiry_role, policy_join_policy,
                policy_invite_link_password, policy_announcement_only
         FROM groups WHERE group_id = $1",
    )
    .bind(group_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|r| Group {
        group_id: r.get("group_id"),
        server_public_params_version: r.get("server_public_params_version"),
        group_public_params: r.get("group_public_params"),
        current_revision: r.get("current_revision"),
        encrypted_state: r.get("encrypted_state"),
        policy: Policy {
            invite_members_role: r.get("policy_invite_members_role"),
            remove_members_role: r.get("policy_remove_members_role"),
            modify_title_role: r.get("policy_modify_title_role"),
            modify_description_role: r.get("policy_modify_description_role"),
            modify_expiry_role: r.get("policy_modify_expiry_role"),
            join_policy: r.get("policy_join_policy"),
            invite_link_password: r.get("policy_invite_link_password"),
            announcement_only: r.get("policy_announcement_only"),
        },
    }))
}

/// Apply a new revision: bump the counter, replace the encrypted blob, and
/// append the previous (now-historical) snapshot + actions to the ring
/// buffer. Caller is in a transaction along with whatever membership-table
/// mutations the actions imply.
///
/// `expected_revision` is the *current* revision the caller observed; the
/// update succeeds only if the row still has that revision. Otherwise
/// another submitter beat us and the caller should retry (§3.5).
pub async fn apply_revision(
    conn: &mut PgConnection,
    group_id: &[u8],
    expected_revision: i64,
    new_encrypted_state: &[u8],
    actions: &[u8],
) -> Result<bool, sqlx::Error> {
    // Append the *outgoing* state to history (the one we're replacing), so
    // a client at revision N can read history rows for revisions N..current
    // to reconstruct.
    let prev = sqlx::query(
        "SELECT encrypted_state FROM groups
         WHERE group_id = $1 AND current_revision = $2",
    )
    .bind(group_id)
    .bind(expected_revision)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(prev) = prev else { return Ok(false) };
    let prev_state: Vec<u8> = prev.get("encrypted_state");

    let new_revision = expected_revision + 1;
    sqlx::query(
        "INSERT INTO group_state_history (group_id, revision, encrypted_state, actions)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(group_id)
    .bind(expected_revision)
    .bind(&prev_state)
    .bind(actions)
    .execute(&mut *conn)
    .await?;

    let result = sqlx::query(
        "UPDATE groups
            SET current_revision = $1,
                encrypted_state  = $2
          WHERE group_id = $3 AND current_revision = $4",
    )
    .bind(new_revision)
    .bind(new_encrypted_state)
    .bind(group_id)
    .bind(expected_revision)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Fetch history rows in the half-open range `(from_revision, current]`.
/// Used by GET /v1/groups/{id}/changes to deliver deltas to a client
/// catching up from a known revision.
pub async fn get_changes_since(
    conn: &mut PgConnection,
    group_id: &[u8],
    from_revision: i64,
    limit: i64,
) -> Result<Vec<StateChange>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT revision, encrypted_state, actions
         FROM group_state_history
         WHERE group_id = $1 AND revision >= $2
         ORDER BY revision ASC
         LIMIT $3",
    )
    .bind(group_id)
    .bind(from_revision)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| StateChange {
            revision: r.get("revision"),
            encrypted_state: r.get("encrypted_state"),
            actions: r.get("actions"),
        })
        .collect())
}

/// Update the policy fields on a group. Called from the `modify_policy`
/// action handler. Doesn't bump the revision — caller does that via
/// `apply_revision` in the same transaction.
pub async fn update_policy(
    conn: &mut PgConnection,
    group_id: &[u8],
    policy: &Policy,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE groups SET
            policy_invite_members_role     = $1,
            policy_remove_members_role     = $2,
            policy_modify_title_role       = $3,
            policy_modify_description_role = $4,
            policy_modify_expiry_role      = $5,
            policy_join_policy             = $6,
            policy_invite_link_password    = $7,
            policy_announcement_only       = $8
         WHERE group_id = $9",
    )
    .bind(policy.invite_members_role)
    .bind(policy.remove_members_role)
    .bind(policy.modify_title_role)
    .bind(policy.modify_description_role)
    .bind(policy.modify_expiry_role)
    .bind(policy.join_policy)
    .bind(policy.invite_link_password.as_deref())
    .bind(policy.announcement_only)
    .bind(group_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

// ---------- membership tables ----------

/// Look up an actor's role within a group. Used at submission time to verify
/// the actor is a member and to enforce per-action role minimums.
/// Returns `None` if the encrypted_member_id is not in this group.
pub async fn member_role(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
) -> Result<Option<i16>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT role FROM member_credentials
         WHERE group_id = $1 AND encrypted_member_id = $2",
    )
    .bind(group_id)
    .bind(encrypted_member_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|r| r.get::<i16, _>("role")))
}

/// List every active member's encrypted_member_id for `group_id`. Used by
/// the endorsement-issuance endpoint, which MACs the whole member set into
/// the `GroupSendEndorsementsResponse`. Order is unspecified — zkgroup
/// canonicalizes internally — but stable per call (sorted by EMI bytes).
pub async fn list_member_encrypted_ids(
    conn: &mut PgConnection,
    group_id: &[u8],
) -> Result<Vec<Vec<u8>>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT encrypted_member_id FROM member_credentials \
         WHERE group_id = $1 ORDER BY encrypted_member_id",
    )
    .bind(group_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| r.get::<Vec<u8>, _>("encrypted_member_id"))
        .collect())
}

/// All push pseudonyms registered for a member — one per device (docs/04
/// multi-device groups). Used by the websocket subscribe path and by group
/// send fan-out, which delivers one copy per pseudonym.
pub async fn member_pseudonyms(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
) -> Result<Vec<Vec<u8>>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT group_push_pseudonym FROM group_member_pseudonyms
         WHERE group_id = $1 AND encrypted_member_id = $2
         ORDER BY created_at, group_push_pseudonym",
    )
    .bind(group_id)
    .bind(encrypted_member_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| r.get::<Vec<u8>, _>("group_push_pseudonym"))
        .collect())
}

/// Register a device's push pseudonym for a member (additive — a member may
/// hold several, one per device). Idempotent on the pseudonym primary key.
pub async fn insert_member_pseudonym(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
    group_push_pseudonym: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO group_member_pseudonyms
            (group_id, encrypted_member_id, group_push_pseudonym)
         VALUES ($1, $2, $3)
         ON CONFLICT (group_id, group_push_pseudonym) DO NOTHING",
    )
    .bind(group_id)
    .bind(encrypted_member_id)
    .bind(group_push_pseudonym)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Whether `pseudonym` is registered to this member in this group. Used to
/// authorize a device draining/acking its own offline queue (docs/04).
pub async fn pseudonym_belongs_to_member(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
    pseudonym: &[u8],
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        "SELECT 1 FROM group_member_pseudonyms
         WHERE group_id = $1 AND encrypted_member_id = $2 AND group_push_pseudonym = $3",
    )
    .bind(group_id)
    .bind(encrypted_member_id)
    .bind(pseudonym)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.is_some())
}

/// Remove every push pseudonym for a member (called when the member is removed
/// or leaves). The membership row in `member_credentials` is deleted separately.
pub async fn delete_member_pseudonyms(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM group_member_pseudonyms
         WHERE group_id = $1 AND encrypted_member_id = $2",
    )
    .bind(group_id)
    .bind(encrypted_member_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn insert_member(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
    role: i16,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO member_credentials
            (group_id, encrypted_member_id, role)
         VALUES ($1, $2, $3)",
    )
    .bind(group_id)
    .bind(encrypted_member_id)
    .bind(role)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Delete a member's credential row and all their device pseudonyms.
pub async fn delete_member(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM member_credentials
         WHERE group_id = $1 AND encrypted_member_id = $2",
    )
    .bind(group_id)
    .bind(encrypted_member_id)
    .execute(&mut *conn)
    .await?;
    delete_member_pseudonyms(conn, group_id, encrypted_member_id).await?;
    Ok(())
}

pub async fn set_member_role(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
    role: i16,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE member_credentials SET role = $1
         WHERE group_id = $2 AND encrypted_member_id = $3",
    )
    .bind(role)
    .bind(group_id)
    .bind(encrypted_member_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Rotate one device's push pseudonym old → new for a member (docs/04). Only
/// the row matching `old_pseudonym` is replaced, so sibling devices' bindings
/// are untouched. Returns true if a row changed. Called from
/// POST /v1/groups/{id}/push_binding when the client supplies its prior
/// pseudonym; a first-time device registration uses `insert_member_pseudonym`.
pub async fn rotate_member_pseudonym(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
    old_pseudonym: &[u8],
    new_pseudonym: &[u8],
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE group_member_pseudonyms
            SET group_push_pseudonym = $1, created_at = now()
          WHERE group_id = $2 AND encrypted_member_id = $3 AND group_push_pseudonym = $4",
    )
    .bind(new_pseudonym)
    .bind(group_id)
    .bind(encrypted_member_id)
    .bind(old_pseudonym)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

// ---------- pending invites and join-requests ----------

/// `(role, day_aligned_invited_at)` row for a pending invite.
pub struct PendingInvite {
    pub role: i16,
}

pub async fn get_pending_invite(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
) -> Result<Option<PendingInvite>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT role FROM members_pending
         WHERE group_id = $1 AND encrypted_member_id = $2",
    )
    .bind(group_id)
    .bind(encrypted_member_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|r| PendingInvite { role: r.get("role") }))
}

pub async fn insert_pending_invite(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
    role: i16,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO members_pending
            (group_id, encrypted_member_id, role, day_aligned_invited_at)
         VALUES ($1, $2, $3, date_trunc('day', now()))",
    )
    .bind(group_id)
    .bind(encrypted_member_id)
    .bind(role)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn delete_pending_invite(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM members_pending
         WHERE group_id = $1 AND encrypted_member_id = $2",
    )
    .bind(group_id)
    .bind(encrypted_member_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// `(group_push_pseudonym,)` row for a pending join request.
pub struct PendingApproval {
    pub group_push_pseudonym: Vec<u8>,
}

pub async fn get_pending_approval(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
) -> Result<Option<PendingApproval>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT group_push_pseudonym FROM members_pending_approval
         WHERE group_id = $1 AND encrypted_member_id = $2",
    )
    .bind(group_id)
    .bind(encrypted_member_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|r| PendingApproval {
        group_push_pseudonym: r.get("group_push_pseudonym"),
    }))
}

pub async fn insert_pending_approval(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
    group_push_pseudonym: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO members_pending_approval
            (group_id, encrypted_member_id, group_push_pseudonym, day_aligned_requested_at)
         VALUES ($1, $2, $3, date_trunc('day', now()))",
    )
    .bind(group_id)
    .bind(encrypted_member_id)
    .bind(group_push_pseudonym)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn delete_pending_approval(
    conn: &mut PgConnection,
    group_id: &[u8],
    encrypted_member_id: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM members_pending_approval
         WHERE group_id = $1 AND encrypted_member_id = $2",
    )
    .bind(group_id)
    .bind(encrypted_member_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}
