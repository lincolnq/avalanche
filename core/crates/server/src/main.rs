//! Homeserver binary entry point.
//!
//! Connects to PostgreSQL, runs schema migrations, spawns background cleanup
//! tasks, and starts the Axum HTTP/WebSocket server. Configuration is read
//! from environment variables (see [`server::config::Config`]).

use sqlx::postgres::PgPoolOptions;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use server::{config::Config, routes, state::AppState, tasks};

#[tokio::main]
async fn main() {
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
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&config.bind_addr)
        .await
        .expect("failed to bind");

    axum::serve(listener, app).await.expect("server error");
}
