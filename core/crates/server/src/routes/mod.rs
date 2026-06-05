//! HTTP and WebSocket route definitions.
//!
//! Each submodule defines routes for one API domain. The top-level
//! [`router()`] function merges them into a single Axum router that
//! `main.rs` serves.
//!
//! All authenticated endpoints use the [`crate::middleware::auth::AuthDevice`]
//! extractor, which validates the `Authorization: Bearer <token>` header and
//! resolves it to the device's internal PK before the handler runs.

use axum::Router;

use crate::state::AppState;

mod accounts;
mod admin;
mod auth;
mod devices;
mod did;
mod groups;
mod health;
mod invites;
mod key_rotation;
pub(crate) mod messages;
mod prekeys;
mod profile;
mod projects;
mod push;
mod recovery;
mod registration;
mod websocket;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(registration::routes())
        .merge(auth::routes())
        .merge(prekeys::routes())
        .merge(messages::routes())
        .merge(websocket::routes())
        .merge(did::routes())
        .merge(projects::routes())
        .merge(push::routes())
        .merge(accounts::routes())
        .merge(recovery::routes())
        .merge(devices::routes())
        .merge(invites::routes())
        .merge(key_rotation::routes())
        .merge(profile::routes())
        .merge(groups::routes())
        .merge(health::routes())
        .merge(admin::routes())
}
