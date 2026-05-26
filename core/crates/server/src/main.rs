//! Homeserver binary entry point.
//!
//! Connects to PostgreSQL, runs schema migrations, spawns background cleanup
//! tasks, and starts the Axum HTTP/WebSocket server. Configuration is read
//! from environment variables (see [`server::config::Config`]).

use axum::http::Request as HttpRequest;
use sqlx::postgres::PgPoolOptions;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use server::{config::Config, routes, state::AppState, tasks};

#[tokio::main]
async fn main() {
    // Load `.env` so devs can configure SERVER_URL, DATABASE_URL, etc.
    // without setting them in their shell each time. Tries the current
    // working dir first, then the repo root (when run from `core/`).
    // Same loading pattern as testbot.
    if dotenvy::dotenv().is_err() {
        let _ = dotenvy::from_filename("../.env");
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = Config::from_env();

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&config.database_url)
        .await
        .expect("failed to connect to database");

    tracing::info!(bind = %config.bind_addr, "starting server");

    let state = AppState::new(pool, config.clone());
    tasks::spawn_all(state.clone());
    let app = routes::router()
        .with_state(state)
        .layer(
            // Custom span so the request URI logged at DEBUG is path-only.
            // The default span logs `uri = <full URI>` which includes query
            // strings — and `/v1/ws?token=<session_token>` would write the
            // session token into every log line. Individual handlers can
            // log specific request details under their own spans if needed.
            TraceLayer::new_for_http().make_span_with(|req: &HttpRequest<_>| {
                // Match tower-http's default span target so existing
                // `RUST_LOG=tower_http=...` filters keep working.
                tracing::debug_span!(
                    target: "tower_http::trace::make_span",
                    "request",
                    method = %req.method(),
                    uri = req.uri().path(),
                    version = ?req.version(),
                )
            }),
        );

    let listener = tokio::net::TcpListener::bind(&config.bind_addr)
        .await
        .expect("failed to bind");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .expect("server error");
}
