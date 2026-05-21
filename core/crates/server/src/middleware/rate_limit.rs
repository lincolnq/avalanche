// Action identifiers stored in rate_limit_counters.action.
pub const ACTION_SEND_MESSAGE: &str = "send_message";
pub const ACTION_UPLOAD_PREKEYS: &str = "upload_prekeys";
pub const ACTION_FETCH_BUNDLE: &str = "fetch_bundle";

// Maximum requests per window.
pub const LIMIT_SEND_MESSAGE: i32 = 100;
pub const LIMIT_UPLOAD_PREKEYS: i32 = 10;
pub const LIMIT_FETCH_BUNDLE: i32 = 100;

// Window sizes in seconds.
pub const WINDOW_SEND_MESSAGE: i64 = 60;
pub const WINDOW_UPLOAD_PREKEYS: i64 = 3600;
pub const WINDOW_FETCH_BUNDLE: i64 = 3600;
