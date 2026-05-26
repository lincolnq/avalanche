// ── Per-account rate limits ─────────────────────────────────────────────────

// Action identifiers stored in rate_limit_counters.action.
pub const ACTION_SEND_MESSAGE: &str = "send_message";
pub const ACTION_UPLOAD_PREKEYS: &str = "upload_prekeys";
pub const ACTION_FETCH_BUNDLE: &str = "fetch_bundle";
pub const ACTION_UPDATE_RECOVERY: &str = "update_recovery";
pub const ACTION_UPDATE_PROFILE: &str = "update_profile";

// Maximum requests per window.
pub const LIMIT_SEND_MESSAGE: i32 = 100;
pub const LIMIT_UPLOAD_PREKEYS: i32 = 10;
pub const LIMIT_FETCH_BUNDLE: i32 = 100;
pub const LIMIT_UPDATE_RECOVERY: i32 = 10;
pub const LIMIT_UPDATE_PROFILE: i32 = 20;

// Window sizes in seconds.
pub const WINDOW_SEND_MESSAGE: i64 = 60;
pub const WINDOW_UPLOAD_PREKEYS: i64 = 3600;
pub const WINDOW_FETCH_BUNDLE: i64 = 3600;
pub const WINDOW_UPDATE_RECOVERY: i64 = 3600;
pub const WINDOW_UPDATE_PROFILE: i64 = 3600;

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
