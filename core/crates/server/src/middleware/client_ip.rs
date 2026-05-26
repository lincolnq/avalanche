//! Client IP extractor used for IP-based rate limiting.
//!
//! Resolves the client address in this order:
//!   1. `X-Forwarded-For` header (first value) — set by a reverse proxy.
//!   2. `ConnectInfo<SocketAddr>` — the immediate TCP peer.
//!   3. `"unknown"` — fallback so handlers never fail purely on missing IP
//!      (e.g. in tests that drive the router via `tower::oneshot`).
//!
//! Deployments behind a proxy MUST configure the proxy to set
//! `X-Forwarded-For`; otherwise every request will be attributed to the proxy
//! and share a single rate-limit bucket.

use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::request::Parts,
};

pub struct ClientIp(pub String);

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if let Some(xff) = parts.headers.get("x-forwarded-for") {
            if let Ok(s) = xff.to_str() {
                if let Some(first) = s.split(',').next() {
                    let trimmed = first.trim();
                    if !trimmed.is_empty() {
                        return Ok(ClientIp(trimmed.to_string()));
                    }
                }
            }
        }

        if let Ok(ConnectInfo(addr)) =
            ConnectInfo::<SocketAddr>::from_request_parts(parts, state).await
        {
            return Ok(ClientIp(addr.ip().to_string()));
        }

        Ok(ClientIp("unknown".to_string()))
    }
}
