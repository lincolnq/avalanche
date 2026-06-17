//! Shared helpers for the app-core e2e tests.
//!
//! `tests/common/mod.rs` is a submodule (not its own test binary), included via
//! `mod common;` in each e2e file.

use base64::prelude::*;

/// A bootstrap registration token carrying the shared secret, for registering
/// against a closed-registration dev server (the default). The secret comes
/// from `REGISTRATION_SHARED_SECRET` (default `"CHANGEME"`, matching `make dev`).
/// Against an open server the token is simply ignored. `server_url` is a
/// placeholder — the server validates only the secret on the bootstrap path.
pub fn invite_token() -> Option<String> {
    let secret = std::env::var("REGISTRATION_SHARED_SECRET").unwrap_or_else(|_| "CHANGEME".into());
    // Single-char wire keys: s=server_url, k=bootstrap_secret (docs/24, 51).
    let payload = serde_json::json!({ "s": "dev", "k": secret });
    Some(BASE64_URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap()))
}
