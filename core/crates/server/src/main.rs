//! Homeserver binary entry point.
//!
//! Subcommands:
//!   - (none)   start the HTTP/WebSocket server
//!   - migrate  apply pending schema migrations and exit
//!
//! Configuration is read from environment variables (see
//! [`server::config::Config`]).

use axum::http::Request as HttpRequest;
use sqlx::postgres::PgPoolOptions;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crypto::groups::ServerSecretParams;
use crypto::sender_cert::SenderCertChain;
use server::{config::Config, db, routes, state::AppState, tasks};

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

    // Subcommands are handled before loading the full Config so they don't
    // trip the prod-safety check on dev-default credentials.
    match std::env::args().nth(1).as_deref() {
        Some("migrate") => {
            let url = std::env::var("DATABASE_URL")
                .expect("DATABASE_URL must be set to run migrations");
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(std::time::Duration::from_secs(5))
                .connect(&url)
                .await
                .expect("failed to connect to database");
            server::migrate::run(&pool).await.expect("migration failed");
            tracing::info!("migrations applied");
            return;
        }
        Some(other) => {
            eprintln!("unknown subcommand: {other}");
            eprintln!("usage: avalanche-server [migrate]");
            std::process::exit(2);
        }
        None => {}
    }

    let config = Config::from_env();

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&config.database_url)
        .await
        .expect("failed to connect to database");

    tracing::info!(bind = %config.bind_addr, "starting server");

    // Load (or generate-and-persist on first boot) the homeserver's group
    // crypto bundle (zkgroup signing key + sender-cert chain). Failure here
    // is fatal: without it we can't issue group credentials or sign sender
    // certs. See docs/03-groups.md §2.1.
    let (zkgroup_secret, sender_cert_chain) = {
        let mut conn = pool.acquire().await.expect("failed to acquire db connection");
        let bytes = db::zkgroup_params::load_or_init(
            &mut conn,
            db::zkgroup_params::CURRENT_VERSION,
            || {
                let bundle = db::zkgroup_params::GroupCryptoBundle {
                    zkgroup_secret: ServerSecretParams::generate().to_bytes(),
                    sender_cert_chain: SenderCertChain::generate()
                        .expect("failed to generate sender cert chain")
                        .to_bytes(),
                };
                bundle.to_bytes()
            },
        )
        .await
        .expect("failed to load group crypto bundle");
        let bundle = db::zkgroup_params::GroupCryptoBundle::from_bytes(&bytes)
            .expect("stored group crypto bundle is corrupt");
        let zk = ServerSecretParams::from_bytes(&bundle.zkgroup_secret)
            .expect("stored zkgroup params are corrupt");
        let chain = SenderCertChain::from_bytes(&bundle.sender_cert_chain)
            .expect("stored sender cert chain is corrupt");
        (zk, chain)
    };

    // Seed the pinned superuser Project (the anchor for adminbot authority).
    // Seeded empty: a bot becomes superuser by registering with a bootstrap
    // token that names this Project's slug while the shared secret is active
    // (see routes::registration). Idempotent.
    {
        let mut conn = pool.acquire().await.expect("failed to acquire db connection");
        db::projects::ensure_adminbot_project(&mut conn, server::config::ADMINBOT_PROJECT_SLUG)
            .await
            .expect("failed to seed superuser project");
    }

    // One-time migration of the legacy PROJECTS env directory into the DB
    // (docs/22): the client directory is now DB-backed. Seeds only when the
    // table is empty, preserving each entry's operator-set official flag +
    // OAuth client id. Once seeded, later PROJECTS edits are ignored — the
    // directory is DB-owned and managed via adminbot from here on.
    {
        #[derive(serde::Deserialize)]
        struct SeedProject {
            name: String,
            url: String,
            #[serde(default)]
            description: String,
            #[serde(default)]
            client_id: Option<String>,
            #[serde(default)]
            official: bool,
        }
        let mut conn = pool.acquire().await.expect("failed to acquire db connection");
        if db::directory::is_empty(&mut conn)
            .await
            .expect("failed to check directory")
        {
            match serde_json::from_str::<Vec<SeedProject>>(&config.projects_json) {
                Ok(seed) if !seed.is_empty() => {
                    let entries: Vec<_> = seed
                        .into_iter()
                        .map(|p| db::directory::SeedEntry {
                            name: p.name,
                            url: p.url,
                            description: p.description,
                            client_id: p.client_id,
                            official: p.official,
                        })
                        .collect();
                    let n = entries.len();
                    db::directory::seed(&mut conn, &entries)
                        .await
                        .expect("failed to seed directory from PROJECTS env");
                    tracing::info!(count = n, "seeded project directory from PROJECTS env");
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "invalid PROJECTS env; skipping directory seed"),
            }
        }
    }

    // Warn loudly if registration is closed but no admission path is
    // configured — otherwise no one (not even the first admin) can register.
    if config.registration_mode == server::config::RegistrationMode::Closed
        && config.registration_shared_secret.is_none()
    {
        let mut conn = pool.acquire().await.expect("failed to acquire db connection");
        let has_gatekeeper = db::capabilities::any_gatekeeper_exists(&mut conn)
            .await
            .unwrap_or(false);
        if !has_gatekeeper {
            tracing::warn!(
                "registration is CLOSED and no REGISTRATION_SHARED_SECRET is set and no \
                 gatekeeper is installed — no one can register. Set REGISTRATION_SHARED_SECRET \
                 (or REGISTRATION_MODE=open for local dev) to admit accounts."
            );
        }
    }

    let state = AppState::new(pool, config.clone(), zkgroup_secret, sender_cert_chain);
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
