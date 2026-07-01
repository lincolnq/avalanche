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

mod abuse;
mod accounts;
mod attachments;
mod admin;
mod auth;
mod devices;
mod did;
mod groups;
mod health;
mod info;
mod invites;
pub(crate) mod messages;
mod oauth;
mod prekeys;
mod profile;
mod projects;
mod provisioning;
mod push;
mod recovery;
mod registration;
mod storage;
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
        .merge(oauth::routes())
        .merge(push::routes())
        .merge(accounts::routes())
        .merge(recovery::routes())
        .merge(storage::routes())
        .merge(devices::routes())
        .merge(provisioning::routes())
        .merge(invites::routes())
        .merge(profile::routes())
        .merge(groups::routes())
        .merge(info::routes())
        .merge(health::routes())
        .merge(admin::routes())
        .merge(abuse::routes())
        .merge(attachments::routes())
}
