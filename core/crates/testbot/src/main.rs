//! Testbot — the first Project on the actnet platform.
//!
//! A standalone Axum service that serves a web UI and manages bot accounts.
//! Each bot is a full Signal protocol participant: it registers on the
//! homeserver, holds its own identity keys, and sends/receives encrypted DMs.
//!
//! Bots use Claude Haiku for conversation. All bot state lives in-memory;
//! bots die when the service restarts.
//!
//! # Threading model
//!
//! Each bot runs its message loop on a dedicated blocking thread via
//! `tokio::task::spawn_blocking`. This is required because libsignal's
//! futures are not Send (they use `dyn Future` without a Send bound), so
//! they cannot be used inside `tokio::spawn`. The bot uses app-core's
//! synchronous FFI methods, which internally block on a global tokio runtime.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Json, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

// ── Types ───────────────────────────────────────────────────────────────────

struct BotHandle {
    did: String,
    device_id: u32,
}

struct AppState {
    homeserver_url: String,
    anthropic_api_key: Option<String>,
    /// user DID -> list of bot handles (metadata only; the bot itself runs on its own thread)
    bots: RwLock<HashMap<String, Vec<BotHandle>>>,
}

#[derive(Serialize)]
struct BotInfo {
    did: String,
    device_id: u32,
}

#[derive(Deserialize)]
struct TextMeRequest {}

#[derive(Serialize)]
struct TextMeResponse {
    bot: BotInfo,
}

#[derive(Serialize)]
struct BotListResponse {
    bots: Vec<BotInfo>,
}

// ── Auth helper ─────────────────────────────────────────────────────────────

/// Extract and verify the Project token from the Authorization header.
/// Returns the user's DID on success.
async fn verify_token(
    state: &AppState,
    auth_header: Option<&str>,
) -> Result<String, StatusCode> {
    let token = auth_header
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or_else(|| {
            tracing::warn!("[auth] missing or malformed Authorization header: {:?}", auth_header);
            StatusCode::UNAUTHORIZED
        })?;

    tracing::debug!("[auth] verifying token with homeserver: {}...{}", &token[..8.min(token.len())], &token[token.len().saturating_sub(4)..]);

    let resp = reqwest::Client::new()
        .get(format!(
            "{}/v1/project-token/verify?token={}",
            state.homeserver_url, token
        ))
        .send()
        .await
        .map_err(|e| {
            tracing::error!("[auth] homeserver request failed: {}", e);
            StatusCode::BAD_GATEWAY
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!("[auth] homeserver rejected token: {} {}", status, body);
        return Err(StatusCode::UNAUTHORIZED);
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| {
        tracing::error!("[auth] failed to parse homeserver response: {}", e);
        StatusCode::BAD_GATEWAY
    })?;
    let did = body["did"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| {
            tracing::error!("[auth] homeserver response missing 'did': {}", body);
            StatusCode::BAD_GATEWAY
        })?;

    tracing::info!("[auth] verified token for did={}", did);
    Ok(did)
}

// ── Routes ──────────────────────────────────────────────────────────────────

async fn index() -> impl IntoResponse {
    Html(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Testbot</title>
    <style>
        body { font-family: -apple-system, system-ui, sans-serif; max-width: 480px; margin: 40px auto; padding: 0 20px; }
        h1 { font-size: 24px; }
        button { font-size: 18px; padding: 12px 24px; cursor: pointer; background: #007AFF; color: white; border: none; border-radius: 8px; }
        button:disabled { background: #999; }
        #status { margin-top: 16px; color: #666; }
    </style>
</head>
<body>
    <h1>Testbot</h1>
    <p>Tap below to start a conversation with an AI chatbot. The bot will send you an encrypted DM.</p>
    <button id="textme" onclick="textMe()">Text Me</button>
    <div id="status"></div>
    <script>
        const params = new URLSearchParams(window.location.search);
        const token = params.get('token');

        async function textMe() {
            const btn = document.getElementById('textme');
            const status = document.getElementById('status');
            btn.disabled = true;
            status.textContent = 'Creating bot...';

            try {
                const resp = await fetch('/api/text-me', {
                    method: 'POST',
                    headers: {
                        'Authorization': 'Bearer ' + token,
                        'Content-Type': 'application/json',
                    },
                    body: '{}',
                });
                if (!resp.ok) {
                    status.textContent = 'Error: ' + resp.status;
                    btn.disabled = false;
                    return;
                }
                const data = await resp.json();
                status.textContent = 'Bot created! Opening conversation...';
                window.location.href = 'https://go.theavalanche.net/conversation/' + data.bot.did;
            } catch (e) {
                status.textContent = 'Error: ' + e.message;
                btn.disabled = false;
            }
        }
    </script>
</body>
</html>"#,
    )
}

async fn text_me(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(_req): Json<TextMeRequest>,
) -> Result<Json<TextMeResponse>, StatusCode> {
    let auth = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok());
    let user_did = verify_token(&state, auth).await?;

    tracing::info!("[text-me] user_did={}", user_did);

    let homeserver_url = state.homeserver_url.clone();
    let anthropic_key = state.anthropic_api_key.clone();
    let user_did_clone = user_did.clone();

    // All bot creation and crypto happens on a dedicated thread because
    // libsignal futures are not Send.
    let (bot_did, bot_device_id, bot) = tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create bot runtime");

        rt.block_on(async {
            let bot_store = store::Store::open_in_memory()
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, "failed to create bot store");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            let bot = app_core::AppCore::create_account_with_store(
                &homeserver_url,
                bot_store,
                Some("Actbot".to_string()),
                true,
            )
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "failed to create bot account");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            let bot_did = bot.did_async().await;
            let bot_device_id = bot.device_id_async().await;

            let opening = "Hey! I'm a testbot. Ask me anything.";
            tracing::info!("[bot {}] created, sending opening DM to {}: {:?}", bot_did, user_did_clone, opening);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            bot.send_dm_async(&user_did_clone, opening.as_bytes(), now_ms)
                .await
                .map_err(|e| {
                    tracing::error!("[bot {}] failed to send opening message: {}", bot_did, e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            Ok::<_, StatusCode>((bot_did, bot_device_id, bot))
        })
    })
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "spawn_blocking panicked");
        StatusCode::INTERNAL_SERVER_ERROR
    })??;

    let bot_info = BotInfo {
        did: bot_did.clone(),
        device_id: bot_device_id,
    };

    // Register bot handle.
    {
        let mut bots = state.bots.write().await;
        bots.entry(user_did.clone()).or_default().push(BotHandle {
            did: bot_did.clone(),
            device_id: bot_device_id,
        });
    }

    // Wrap bot in Arc<Mutex> so the message loop thread owns it.
    let bot = Arc::new(Mutex::new(BotRunner {
        app_core: bot,
        conversation: vec![ConversationMessage {
            role: "assistant".into(),
            content: "Hey! I'm a testbot. Ask me anything.".into(),
        }],
    }));

    // Spawn the bot's message loop on a dedicated blocking thread.
    let bot_did_for_loop = bot_info.did.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create bot runtime");

        rt.block_on(bot_message_loop(bot, bot_did_for_loop, anthropic_key));
    });

    Ok(Json(TextMeResponse { bot: bot_info }))
}

async fn list_bots(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<BotListResponse>, StatusCode> {
    let auth = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok());
    let user_did = verify_token(&state, auth).await?;

    let bots = state.bots.read().await;
    let user_bots = bots.get(&user_did);

    let bot_list: Vec<BotInfo> = match user_bots {
        Some(handles) => handles
            .iter()
            .map(|h| BotInfo {
                did: h.did.clone(),
                device_id: h.device_id,
            })
            .collect(),
        None => vec![],
    };

    Ok(Json(BotListResponse { bots: bot_list }))
}

// ── Bot message loop ────────────────────────────────────────────────────────

struct BotRunner {
    app_core: app_core::AppCore,
    conversation: Vec<ConversationMessage>,
}

#[derive(Clone)]
struct ConversationMessage {
    role: String,    // "user" or "assistant"
    content: String,
}

async fn bot_message_loop(
    bot: Arc<Mutex<BotRunner>>,
    bot_did: String,
    anthropic_key: Option<String>,
) {
    tracing::info!("[bot {}] starting message loop (WebSocket)", bot_did);

    loop {
        // Wait for the next message via WebSocket. This blocks until a
        // message arrives or the connection drops.
        let messages = {
            let runner = bot.lock().await;
            match runner.app_core.receive_messages_ws_async().await {
                Ok(msgs) => msgs,
                Err(e) => {
                    tracing::warn!("[bot {}] WS receive failed: {}, reconnecting in 2s", bot_did, e);
                    drop(runner);
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    continue;
                }
            }
        };

        if messages.is_empty() {
            tracing::info!("[bot {}] WS connection closed, reconnecting in 1s", bot_did);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            continue;
        }

        for msg in messages {
            let plaintext = match String::from_utf8(msg.plaintext) {
                Ok(s) => s,
                Err(_) => {
                    tracing::warn!("[bot {}] received non-UTF8 message from {}, skipping", bot_did, msg.sender_did);
                    continue;
                }
            };

            tracing::info!("[bot {}] <<< from {}: {:?}", bot_did, msg.sender_did, plaintext);

            // Pause briefly before sending read receipt, like a human reading.
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

            // Send a read receipt back to the sender using their sent_at timestamp.
            if let Some(sender_ts) = msg.sent_at_ms {
                let runner = bot.lock().await;
                if let Err(e) = runner.app_core.send_read_receipt_async(
                    &msg.sender_did, vec![sender_ts],
                ).await {
                    tracing::warn!("[bot {}] failed to send read receipt: {}", bot_did, e);
                }
            }

            // Generate reply after the read receipt.
            let mut runner = bot.lock().await;

            // Look up the sender's display name from the local cache. The
            // profile_key was attached to this very message, so app-core has
            // already fetched and decrypted the profile blob in handle_inbound_profile_key.
            let user_display_name = runner.app_core.contact_display_name_async(&msg.sender_did).await;

            runner.conversation.push(ConversationMessage {
                role: "user".into(),
                content: plaintext,
            });

            let response =
                generate_response(&anthropic_key, &runner.conversation, user_display_name.as_deref()).await;

            tracing::info!("[bot {}] >>> to {}: {:?}", bot_did, msg.sender_did, response);

            runner.conversation.push(ConversationMessage {
                role: "assistant".into(),
                content: response.clone(),
            });

            let reply_ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            if let Err(e) = runner
                .app_core
                .send_dm_async(&msg.sender_did, response.as_bytes(), reply_ts)
                .await
            {
                tracing::error!("[bot {}] failed to send to {}: {}", bot_did, msg.sender_did, e);
            }
        }
    }
}

// ── Claude API ──────────────────────────────────────────────────────────────

async fn generate_response(
    api_key: &Option<String>,
    conversation: &[ConversationMessage],
    user_display_name: Option<&str>,
) -> String {
    let Some(api_key) = api_key else {
        return echo_response(conversation);
    };

    let messages: Vec<serde_json::Value> = conversation
        .iter()
        .map(|m| {
            serde_json::json!({
                "role": m.role,
                "content": m.content,
            })
        })
        .collect();

    let mut system_prompt = String::from(
        "You are a friendly chatbot on the actnet platform. Keep your responses \
         concise and conversational. You're chatting with an activist — be \
         supportive and helpful."
    );
    if let Some(name) = user_display_name {
        system_prompt.push_str(&format!(" The user's display name is {name}."));
    }

    let body = serde_json::json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 1024,
        "system": system_prompt,
        "messages": messages,
    });

    tracing::info!("[claude] sending {} messages to API", messages.len());

    let resp = match reqwest::Client::new()
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("[claude] API request failed: {}", e);
            return echo_response(conversation);
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        tracing::error!("[claude] API error {}: {}", status, text);
        return echo_response(conversation);
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("[claude] failed to parse response: {}", e);
            return echo_response(conversation);
        }
    };

    body["content"][0]["text"]
        .as_str()
        .unwrap_or("I'm having trouble thinking right now. Try again?")
        .to_string()
}

/// Fallback when no API key is configured: just echo the last message.
fn echo_response(conversation: &[ConversationMessage]) -> String {
    let last_user_msg = conversation
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or("...");
    format!("(echo) You said: {last_user_msg}")
}

// ── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // Load .env file — try current dir first, then parent (repo root when run from core/)
    if dotenvy::dotenv().is_err() {
        let _ = dotenvy::from_filename("../.env");
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let homeserver_url = std::env::var("HOMESERVER_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    let anthropic_api_key = std::env::var("ANTHROPIC_API_KEY").ok();
    let bind_addr = std::env::var("TESTBOT_BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3001".to_string());

    if anthropic_api_key.is_none() {
        tracing::warn!("ANTHROPIC_API_KEY not set — bot will echo messages instead of using Claude");
    }

    let state = Arc::new(AppState {
        homeserver_url,
        anthropic_api_key,
        bots: RwLock::new(HashMap::new()),
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/api/text-me", post(text_me))
        .route("/api/bots", get(list_bots))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    tracing::info!(bind = %bind_addr, "starting testbot service");

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("failed to bind");

    axum::serve(listener, app).await.expect("server error");
}
