// ── Per-account rate limits ─────────────────────────────────────────────────

// Action identifiers stored in rate_limit_counters.action.
pub const ACTION_SEND_MESSAGE: &str = "send_message";
pub const ACTION_UPLOAD_PREKEYS: &str = "upload_prekeys";
pub const ACTION_FETCH_BUNDLE: &str = "fetch_bundle";
pub const ACTION_UPDATE_RECOVERY: &str = "update_recovery";
pub const ACTION_UPDATE_PROFILE: &str = "update_profile";
pub const ACTION_ABUSE_REPORT: &str = "abuse_report";
pub const ACTION_STORAGE_PULL: &str = "storage_pull";
pub const ACTION_STORAGE_PUSH: &str = "storage_push";
pub const ACTION_STORAGE_SNAPSHOT_GET: &str = "storage_snapshot_get";
pub const ACTION_STORAGE_SNAPSHOT_PUT: &str = "storage_snapshot_put";

// Maximum requests per window.
pub const LIMIT_SEND_MESSAGE: i32 = 100;
pub const LIMIT_UPLOAD_PREKEYS: i32 = 10;
pub const LIMIT_FETCH_BUNDLE: i32 = 100;
pub const LIMIT_UPDATE_RECOVERY: i32 = 10;
pub const LIMIT_UPDATE_PROFILE: i32 = 20;
// Abuse reports (docs/12 §3): a daily cap keeps the report signal high-quality
// and bounds weaponized reporting, with comfortable headroom for a user
// cleaning out a spam wave.
pub const LIMIT_ABUSE_REPORT: i32 = 20;
// Storage sync (docs/05 §8): pulls run on every fast-sync nudge, foreground, and
// reconnect; pushes run on every dirty flush. Generous starting values — these
// are steady-state sync traffic, not abuse vectors (records are quota-capped).
pub const LIMIT_STORAGE_PULL: i32 = 600;
pub const LIMIT_STORAGE_PUSH: i32 = 300;
// Snapshots are the passive-backup path (docs/05 §7): pushed periodically by the
// authoritative device and read only on recovery/promotion. Far lower frequency
// than item sync, so tight per-hour limits suffice.
pub const LIMIT_STORAGE_SNAPSHOT_GET: i32 = 60;
pub const LIMIT_STORAGE_SNAPSHOT_PUT: i32 = 60;

// Window sizes in seconds.
pub const WINDOW_SEND_MESSAGE: i64 = 60;
pub const WINDOW_UPLOAD_PREKEYS: i64 = 3600;
pub const WINDOW_FETCH_BUNDLE: i64 = 3600;
pub const WINDOW_UPDATE_RECOVERY: i64 = 3600;
pub const WINDOW_UPDATE_PROFILE: i64 = 3600;
pub const WINDOW_ABUSE_REPORT: i64 = 86400;
pub const WINDOW_STORAGE_PULL: i64 = 60;
pub const WINDOW_STORAGE_PUSH: i64 = 60;
pub const WINDOW_STORAGE_SNAPSHOT_GET: i64 = 3600;
pub const WINDOW_STORAGE_SNAPSHOT_PUT: i64 = 3600;

// ── Per-IP rate limits (unauthenticated endpoints) ──────────────────────────

// Action identifiers stored in ip_rate_limit_counters.action.
pub const ACTION_REGISTER: &str = "register";
pub const ACTION_AUTH_CHALLENGE: &str = "auth_challenge";
pub const ACTION_AUTH_TOKEN: &str = "auth_token";

// Registration is an expensive operation (DID lookup, multiple inserts), and
// every legitimate user only does it a handful of times across their lifetime.
// Keep the limit tight to deter abuse but loose enough that a NAT'd network of
// real new users isn't blocked.
pub const LIMIT_REGISTER: i32 = 10;
pub const WINDOW_REGISTER: i64 = 3600;

// Auth challenge/token pairs run every time a client comes back online to
// refresh a session token. Allow a comfortable margin for retries and for
// multiple devices behind a shared NAT.
pub const LIMIT_AUTH_CHALLENGE: i32 = 60;
pub const WINDOW_AUTH_CHALLENGE: i64 = 3600;
pub const LIMIT_AUTH_TOKEN: i32 = 60;
pub const WINDOW_AUTH_TOKEN: i64 = 3600;

// ── Group endpoints ─────────────────────────────────────────────────────────

// Per-account: group creation is cheap server-side but every legitimate user
// only does it occasionally. Tight to deter abuse, loose enough that an
// admin spinning up an event's worth of project subgroups isn't blocked.
pub const ACTION_CREATE_GROUP: &str = "create_group";
pub const LIMIT_CREATE_GROUP: i32 = 20;
pub const WINDOW_CREATE_GROUP: i64 = 3600;

// Per-account daily: credentials refresh once a day per account; the limit
// has comfortable headroom for retries and multi-device. §3.9 rule 3
// explicitly allows per-DID-per-day counters.
pub const ACTION_ISSUE_GROUP_CREDENTIAL: &str = "issue_group_credential";
pub const LIMIT_ISSUE_GROUP_CREDENTIAL: i32 = 20;
pub const WINDOW_ISSUE_GROUP_CREDENTIAL: i64 = 86400;

// Per-IP on the action-submission endpoint. Presentation-authenticated
// actions don't bind to an account the server can rate-limit by — IP is the
// only handle available. Generous because clients submit changes whenever
// users edit groups, including rapid retry-on-conflict (§3.5).
pub const ACTION_SUBMIT_GROUP_CHANGE: &str = "submit_group_change";
pub const LIMIT_SUBMIT_GROUP_CHANGE: i32 = 240;
pub const WINDOW_SUBMIT_GROUP_CHANGE: i64 = 60;

// Per-IP on push-binding rotation. Clients call this on app launch / device
// rotation, not in steady state.
pub const ACTION_GROUP_PUSH_BINDING: &str = "group_push_binding";
pub const LIMIT_GROUP_PUSH_BINDING: i32 = 60;
pub const WINDOW_GROUP_PUSH_BINDING: i64 = 3600;

// Per-IP on sealed-sender group send. Sender identity is hidden so the only
// rate-limit handle is the originating IP.
pub const ACTION_GROUP_SEND: &str = "group_send";
pub const LIMIT_GROUP_SEND: i32 = 600;
pub const WINDOW_GROUP_SEND: i64 = 60;
