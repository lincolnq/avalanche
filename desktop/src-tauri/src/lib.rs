// Tauri commands — Avalanche Desktop bridge.
// Each command is a thin delegation to the corresponding app-core method.
// Types are code-generated via tauri-specta → ../src/bindings.ts.
// All FFI types are now derived directly on app-core via the "specta" feature —
// no more manual ffi_types.rs mirror.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use app_core::AppCore;
use tauri::Manager;

// Desktop-specific convenience type (not in app-core).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AccountResult {
    did: String,
    display_name: String,
}

/// Raw OG/meta scrape for a URL (A4). Desktop-specific (not in app-core) — the
/// frontend turns the og:image bytes into an encrypted `AttachmentFfi` via
/// `upload_attachment`, then assembles a `LinkPreviewFfi` for the send path
/// (mirrors iOS `AppState.fetchLinkPreview`). `image_bytes` is empty for a
/// text-only card.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct LinkPreviewMetaFfi {
    url: String,
    title: String,
    description: String,
    date_ms: i64,
    image_bytes: Vec<u8>,
    image_content_type: Option<String>,
}

// ── App state ─────────────────────────────────────────────────────────────────

struct AppState {
    // One live AppCore per signed-in account, keyed by its DID. iOS/Android
    // run the same shared-inbox model (a `cores` map); every per-account command
    // resolves its core via `get_app(&state, &account_id)`.
    cores: Mutex<HashMap<String, Arc<AppCore>>>,
}

/// Transient handle for the *new device* side of device linking (T71). Kept
/// separate from `AppState` because the joining device has no account yet —
/// `device_link_await_step` moves the linked account into `AppState.app` once
/// the provisioning bundle arrives, then clears this. Mirrors iOS keeping a
/// `DeviceLinkNew` alive on the linking view and dropping it on teardown
/// (`LinkNewDeviceView.swift`).
struct DeviceLinkState {
    link: Mutex<Option<Arc<app_core::DeviceLinkNew>>>,
}

fn get_app(
    state: &tauri::State<'_, AppState>,
    account_id: &str,
) -> Result<std::sync::Arc<AppCore>, String> {
    state
        .cores
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?
        .get(account_id)
        .cloned()
        .ok_or_else(|| "no account".to_string())
}

/// Whether a process-arg looks like one of our deep links — the custom
/// `avalanche://` scheme or a universal-link URL on our host. Used to pick the
/// URL out of a second instance's argv (see the single-instance callback).
fn is_deep_link_arg(arg: &str) -> bool {
    arg.starts_with("avalanche://") || arg.contains("go.theavalanche.net")
}

/// Reveal + focus the main window (tray "Open" / left-click, deep-link wake).
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Whether closing the main window should hide it to the tray (keeping the WS +
/// notifications alive) instead of quitting. Read from the same
/// `tauri-plugin-store` file the frontend writes its `closeToTray` toggle to, so
/// the setting survives restarts without a dedicated command/atomic. Defaults to
/// `true` (close-to-tray on) — that is the whole point of the tray: messages and
/// notifications keep arriving after the window is closed (mirrors Signal/Slack
/// desktop). Users opt out via Settings → Developer.
fn close_to_tray_enabled(app: &tauri::AppHandle) -> bool {
    use tauri_plugin_store::StoreExt;
    app.store("avalanche.json")
        .ok()
        .and_then(|store| store.get("closeToTray"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// On the *first* hide-to-tray, fire a one-time OS notification so the user
/// knows the app is still running and how to bring it back — otherwise a window
/// that vanishes on close with no visible exit reads as a bug (Signal/Slack show
/// the same hint). The `trayHintShown` flag persists in the same plugin-store
/// file the close-to-tray toggle uses.
fn maybe_show_first_hide_hint(app: &tauri::AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    use tauri_plugin_store::StoreExt;
    let Ok(store) = app.store("avalanche.json") else {
        return;
    };
    let already = store
        .get("trayHintShown")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if already {
        return;
    }
    let _ = app
        .notification()
        .builder()
        .title("Avalanche is still running")
        .body("The window closed to the system tray so messages keep arriving. Click the tray icon to reopen, or use its menu to Quit.")
        .show();
    store.set("trayHintShown", true);
    let _ = store.save();
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri_specta::Builder::<tauri::Wry>::new()
        // i64 → number is safe: all i64 fields in the FFI surface are
        // timestamps (~1.7e12) or auto-increment IDs that will never
        // approach MAX_SAFE_INTEGER (9e15) in practice.
        .dangerously_cast_bigints_to_number()
        .commands(tauri_specta::collect_commands![
            ping,
            create_account,
            login,
            recover_from_blob,
            send_dm,
            send_group_message,
            next_events,
            save_message,
            load_conversations,
            load_messages,
            mark_messages_read,
            unread_count,
            did,
            device_id,
            own_display_name,
            set_display_name,
            has_recovery,
            update_recovery_blob,
            home_server,
            generate_recovery_phrase,
            recovery_phrase_to_seed,
            derive_did_from_passkey,
            contact_display_name,
            cached_display_names,
            get_account_info,
            refresh_contact_profile,
            list_contacts,
            touch_contact,
            fetch_and_cache_profile,
            prime_contact_profile,
            block_contact,
            unblock_contact,
            leave_server,
            delete_identity,
            clear_session,
            fetch_projects,
            request_project_token,
            validate_invite,
            connection_state,
            wait_for_connection_state_change,
            create_group,
            fetch_group_state,
            cached_group_state,
            invite_member,
            accept_invite,
            decline_invite,
            cancel_join_request,
            approve_join_request,
            deny_join_request,
            remove_member,
            leave_group,
            is_group_member,
            change_member_role,
            set_group_expiry,
            set_group_title,
            group_expiry_seconds,
            apply_pending_group_changes,
            list_groups,
            send_reaction,
            send_edit,
            send_delete,
            load_reactions,
            load_message_revisions,
            // Receipts / requests / safety / timers / recovery (PR1 foundation)
            receive_messages,
            recover_from_phrase,
            send_read_receipt,
            join_via_link,
            accept_request,
            delete_request,
            set_pending_request,
            report_and_block,
            list_blocked,
            get_conversation_timer,
            set_conversation_timer,
            delete_expired_messages,
            // Attachments + link previews + external links (Day-6 A2/A3/A4).
            // download_attachment caches to disk + records the path internally,
            // so set_attachment_downloaded is not a separate JS-facing command.
            upload_attachment,
            download_attachment,
            send_message_with_attachments,
            open_external,
            fetch_link_preview,
            // Device linking (Day-6 B1 / T71). New-device (account-less) side
            // uses DeviceLinkState; existing-device side operates on AppState.
            device_link_create_pairing,
            device_link_accept_pairing,
            device_link_await_step,
            device_link_reset,
            link_create_pairing,
            link_accept_pairing,
            link_send_bundle_step,
            // Foreground gating for the WS keepalive / opportunistic reconnect
            // (Day-6 B2 / T77).
            set_app_active,
            // Manual "Reconnect now" action on the offline banner (Day-6 B3 / T72).
            reconnect_now,
        ]);

    // Codegen path (never compiled into the shipped app): write bindings.ts and
    // exit before launching the GUI, so regeneration runs headless (CI, no
    // display). The export path is relative to the binary's cwd, so this must be
    // run from `desktop/src-tauri` — see the `desktop-bindings` Makefile target.
    #[cfg(feature = "codegen")]
    {
        builder
            .export(
                specta_typescript::Typescript::default(),
                "../src/bindings.ts",
            )
            .expect("failed to export specta bindings");
        return;
    }

    #[allow(unreachable_code)]
    tauri::Builder::default()
        // Single-instance MUST be the first plugin. When a second launch occurs
        // (e.g. opening an avalanche:// link while the app runs) the OS spawns a
        // new process; this callback runs in the *existing* one. We focus the
        // window and forward the deep-link URL from the new process's argv to the
        // frontend (the deep-link plugin's on_open_url only fires for the
        // cold-start launch, not this second-instance path on Windows/Linux).
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            use tauri::Emitter;
            // show() (not just unminimize) so a window hidden to the tray by
            // close-to-tray actually reappears — a hidden window is not minimized.
            show_main_window(app);
            if let Some(url) = argv.iter().find(|a| is_deep_link_arg(a)) {
                eprintln!("[deep-link] second instance: {url}");
                let _ = app.emit("avalanche-deeplink", url.clone());
            }
        }))
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_deep_link::init())
        .setup(|app| {
            use tauri::Emitter;
            use tauri_plugin_deep_link::DeepLinkExt;

            // Forward every opened deep link to the frontend as `avalanche-deeplink`
            // (the raw URL string). AppContext owns parsing/routing — see its
            // handleDeepLink (conversation/<did>, i/<token>). Fires for cold launch
            // and while running. The frontend listener is the single consumer.
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    eprintln!("[deep-link] on_open_url: {url}");
                    // Reveal the window in case it was hidden to the tray — the
                    // user followed a link and expects the app to surface.
                    show_main_window(&handle);
                    let _ = handle.emit("avalanche-deeplink", url.to_string());
                }
            });

            // Best-effort runtime registration of the avalanche:// scheme — needed
            // for dev on Windows/Linux where no installer has registered it. A
            // failure here is non-fatal (e.g. already registered).
            let _ = app.deep_link().register_all();

            // System tray (T72). Lets the app keep running with its window
            // closed so the WebSocket and notifications survive (see the
            // CloseRequested handler below). Menu: Open (reveal window) / Quit
            // (really exit); left-click reveals the window.
            {
                use tauri::menu::{Menu, MenuItem};
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

                let open_i = MenuItem::with_id(app, "open", "Open Avalanche", true, None::<&str>)?;
                let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&open_i, &quit_i])?;

                let mut tray = TrayIconBuilder::with_id("main-tray")
                    .tooltip("Avalanche")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "open" => show_main_window(app),
                        // Real quit: bypasses the close-to-tray intercept.
                        "quit" => app.exit(0),
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_main_window(tray.app_handle());
                        }
                    });
                if let Some(icon) = app.default_window_icon() {
                    tray = tray.icon(icon.clone());
                }
                tray.build(app)?;
            }

            Ok(())
        })
        // Close-to-tray (T72): when enabled, the window's close button hides the
        // window instead of exiting, so the core's WebSocket + notification
        // delivery keep running. The tray "Quit" item (app.exit) is the real
        // exit path. When disabled, close exits normally.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" && close_to_tray_enabled(window.app_handle()) {
                    api.prevent_close();
                    let _ = window.hide();
                    maybe_show_first_hide_hint(window.app_handle());
                }
            }
        })
        .manage(AppState {
            cores: Mutex::new(HashMap::new()),
        })
        .manage(DeviceLinkState {
            link: Mutex::new(None),
        })
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ── Commands ─────────────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn ping() -> String {
    "pong".to_string()
}

// ── Account factory ──────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn create_account(
    state: tauri::State<'_, AppState>,
    server_url: String,
    db_path: String,
    db_key: String,
    prf_output: Vec<u8>,
    display_name: String,
    invite_token: Option<String>,
) -> Result<AccountResult, String> {
    let app =
        AppCore::create_account(server_url, db_path, db_key, prf_output, display_name, invite_token)
            .map_err(|e| e.to_string())?;
    let did = app.did();
    let display_name = app.own_display_name().map_err(|e| e.to_string())?;
    state
        .cores
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?
        .insert(did.clone(), app);
    Ok(AccountResult { did, display_name })
}

#[tauri::command]
#[specta::specta]
fn login(
    state: tauri::State<'_, AppState>,
    db_path: String,
    db_key: String,
) -> Result<AccountResult, String> {
    let app = AppCore::login(db_path, db_key).map_err(|e| e.to_string())?;
    let did = app.did();
    let display_name = app.own_display_name().map_err(|e| e.to_string())?;
    state
        .cores
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?
        .insert(did.clone(), app);
    Ok(AccountResult { did, display_name })
}

#[tauri::command]
#[specta::specta]
fn recover_from_blob(
    state: tauri::State<'_, AppState>,
    server_url: String,
    did: String,
    prf_output: Vec<u8>,
    db_path: String,
    db_key: String,
    display_name: String,
) -> Result<AccountResult, String> {
    let app = AppCore::recover_from_blob(server_url, did, prf_output, db_path, db_key, display_name)
        .map_err(|e| e.to_string())?;
    let did = app.did();
    let display_name = app.own_display_name().map_err(|e| e.to_string())?;
    state
        .cores
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?
        .insert(did.clone(), app);
    Ok(AccountResult { did, display_name })
}

/// Recover an account from a BIP39 recovery phrase. Mirrors `recover_from_blob`
/// but derives the 32-byte recovery seed from the phrase first
/// (`recovery_phrase_to_seed` returns exactly 32 bytes, satisfying
/// `recover_from_blob`'s `len() == 32` check). The seed plays the role of
/// `prf_output` in the blob recovery path.
#[tauri::command]
#[specta::specta]
fn recover_from_phrase(
    state: tauri::State<'_, AppState>,
    phrase: String,
    server_url: String,
    did: String,
    db_path: String,
    db_key: String,
    display_name: String,
) -> Result<AccountResult, String> {
    let seed = app_core::recovery_phrase_to_seed(phrase).map_err(|e| e.to_string())?;
    let app = AppCore::recover_from_blob(server_url, did, seed, db_path, db_key, display_name)
        .map_err(|e| e.to_string())?;
    let did = app.did();
    let display_name = app.own_display_name().map_err(|e| e.to_string())?;
    state
        .cores
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?
        .insert(did.clone(), app);
    Ok(AccountResult { did, display_name })
}

// ── Device linking (T71) ──────────────────────────────────────────────────────
//
// Two sides, both driven by a TS poll loop (1s interval, 180s timeout — see
// AppContext.completeDeviceLink / linkSendBundle, mirroring iOS):
//   * New device (account-less): `DeviceLinkState` holds a `DeviceLinkNew`.
//     create/accept a pairing code, then poll `device_link_await_step` until the
//     linked account arrives and is moved into `AppState.app`.
//   * Existing device (signed in): operates on the live `AppCore` in `AppState`.
//     create/accept a pairing code, then poll `link_send_bundle_step` until true.
// All steps do network I/O against the mailbox, so they run on the blocking pool
// (`spawn_blocking`) rather than the WebView thread — same rule as `next_events`.

/// New device: generate this device's pairing code (show mode). Creates the
/// handshake handle and stores it for the subsequent `device_link_await_step`.
#[tauri::command]
#[specta::specta]
async fn device_link_create_pairing(
    link_state: tauri::State<'_, DeviceLinkState>,
    mailbox_server: Option<String>,
) -> Result<String, String> {
    let link = app_core::DeviceLinkNew::new();
    let handle = link.clone();
    let code = tauri::async_runtime::spawn_blocking(move || {
        handle.create_pairing(mailbox_server).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    *link_state.link.lock().map_err(|e| format!("lock poisoned: {}", e))? = Some(link);
    Ok(code)
}

/// New device: accept (paste) the existing device's pairing code (scan mode).
/// Stores the handshake handle for the subsequent `device_link_await_step`.
#[tauri::command]
#[specta::specta]
async fn device_link_accept_pairing(
    link_state: tauri::State<'_, DeviceLinkState>,
    code: String,
) -> Result<(), String> {
    let link = app_core::DeviceLinkNew::new();
    let handle = link.clone();
    tauri::async_runtime::spawn_blocking(move || {
        handle.accept_pairing(code).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    *link_state.link.lock().map_err(|e| format!("lock poisoned: {}", e))? = Some(link);
    Ok(())
}

/// New device: one non-blocking step toward completing the link. Returns the
/// linked account (and installs it as the active account in `AppState`) once the
/// bundle has arrived; `None` while still waiting. Requires a prior
/// `device_link_create_pairing` / `device_link_accept_pairing`.
#[tauri::command]
#[specta::specta]
async fn device_link_await_step(
    link_state: tauri::State<'_, DeviceLinkState>,
    app_state: tauri::State<'_, AppState>,
    db_path: String,
    db_key: String,
) -> Result<Option<AccountResult>, String> {
    let link = link_state
        .link
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?
        .clone()
        .ok_or_else(|| "no pairing in progress".to_string())?;
    let linked = tauri::async_runtime::spawn_blocking(move || {
        link.await_link_step(db_path, db_key).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    match linked {
        Some(app) => {
            let did = app.did();
            let display_name = app.own_display_name().map_err(|e| e.to_string())?;
            app_state
                .cores
                .lock()
                .map_err(|e| format!("lock poisoned: {}", e))?
                .insert(did.clone(), app);
            *link_state.link.lock().map_err(|e| format!("lock poisoned: {}", e))? = None;
            Ok(Some(AccountResult { did, display_name }))
        }
        None => Ok(None),
    }
}

/// New device: abandon an in-progress pairing (cancel / view teardown).
#[tauri::command]
#[specta::specta]
fn device_link_reset(link_state: tauri::State<'_, DeviceLinkState>) -> Result<(), String> {
    *link_state.link.lock().map_err(|e| format!("lock poisoned: {}", e))? = None;
    Ok(())
}

/// Existing device: generate this device's pairing code for a new device to
/// enter (show mode). Follow with `link_send_bundle_step` polling.
#[tauri::command]
#[specta::specta]
async fn link_create_pairing(
    state: tauri::State<'_, AppState>,
    account_id: String,
    mailbox_server: Option<String>,
) -> Result<String, String> {
    let app = get_app(&state, &account_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        app.link_create_pairing(mailbox_server).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Existing device: accept (paste) the new device's pairing code (scan mode).
/// Follow with `link_send_bundle_step` polling.
#[tauri::command]
#[specta::specta]
async fn link_accept_pairing(
    state: tauri::State<'_, AppState>,
    account_id: String,
    code: String,
) -> Result<(), String> {
    let app = get_app(&state, &account_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        app.link_accept_pairing(code).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Existing device: one non-blocking step of sealing + sending the provisioning
/// bundle. Returns true when done; the TS layer polls this.
#[tauri::command]
#[specta::specta]
async fn link_send_bundle_step(state: tauri::State<'_, AppState>, account_id: String) -> Result<bool, String> {
    let app = get_app(&state, &account_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        app.link_send_bundle_step().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── Foreground / connection (T77) ───────────────────────────────────────────────

/// Tell the core whether the desktop window is foreground-active (focused).
/// Gates the WS keepalive (foreground-only) and, on a transition to active,
/// triggers an opportunistic reconnect + liveness probe so a socket that died
/// while the window was hidden/blurred recovers promptly — the reconnect path
/// resyncs durable storage via the core connection loop (no separate
/// `sync_storage` call needed, matching iOS, which only drives `setAppActive`
/// from `scenePhase`). Sync + infallible. No-op before sign-in (no core yet):
/// focus events can fire on the onboarding screens, where there's nothing to
/// gate.
#[tauri::command]
#[specta::specta]
fn set_app_active(state: tauri::State<'_, AppState>, account_id: String, active: bool) -> Result<(), String> {
    if let Ok(app) = get_app(&state, &account_id) {
        app.set_app_active(active);
    }
    Ok(())
}

/// Opportunistically retry/validate connectivity now (the "Reconnect now"
/// action on the offline banner — T72). Wakes the reconnect loop if it's backing
/// off and probes an open socket's liveness. Sync, infallible, cheap. No-op
/// before sign-in.
#[tauri::command]
#[specta::specta]
fn reconnect_now(state: tauri::State<'_, AppState>, account_id: String) -> Result<(), String> {
    if let Ok(app) = get_app(&state, &account_id) {
        app.reconnect_now();
    }
    Ok(())
}

// ── Core messaging ────────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn send_dm(
    state: tauri::State<'_, AppState>,
    account_id: String,
    recipient_did: String,
    plaintext: Vec<u8>,
    sent_at_ms: i64,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .send_dm(recipient_did, plaintext, sent_at_ms)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn send_group_message(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
    plaintext: Vec<u8>,
    sent_at_ms: i64,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .send_message(app_core::MessageTarget::Group { group_id }, plaintext, sent_at_ms)
        .map_err(|e| e.to_string())
}

/// Async so it runs off the main thread. `app_core.next_events()` blocks until
/// decrypted events arrive (WebSocket push via app-core's MPSC channel), so it
/// must not run on the main thread — that would freeze the WebView. We clone the
/// `Arc<AppCore>` out of `State` *before* awaiting (a `State` reference cannot be
/// held across an await point) and run the blocking call on the blocking pool.
#[tauri::command]
#[specta::specta]
async fn next_events(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<Vec<app_core::IncomingEvent>, String> {
    let app = get_app(&state, &account_id)?;
    tauri::async_runtime::spawn_blocking(move || app.next_events().map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
fn save_message(
    state: tauri::State<'_, AppState>,
    account_id: String,
    msg: app_core::StoredMessageFfi,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .save_message(msg)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn load_conversations(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<Vec<app_core::ConversationSummaryFfi>, String> {
    get_app(&state, &account_id)?
        .load_conversations()
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn load_messages(
    state: tauri::State<'_, AppState>,
    account_id: String,
    conversation_id: String,
) -> Result<Vec<app_core::StoredMessageFfi>, String> {
    get_app(&state, &account_id)?
        .load_messages(conversation_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn mark_messages_read(
    state: tauri::State<'_, AppState>,
    account_id: String,
    conversation_id: String,
    up_to_sent_at_ms: i64,
) -> Result<u64, String> {
    get_app(&state, &account_id)?
        .mark_messages_read(conversation_id, up_to_sent_at_ms)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn unread_count(
    state: tauri::State<'_, AppState>,
    account_id: String,
    conversation_id: String,
) -> Result<u64, String> {
    get_app(&state, &account_id)?
        .unread_count(conversation_id)
        .map_err(|e| e.to_string())
}

// ── Identity / contacts ───────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn did(state: tauri::State<'_, AppState>, account_id: String) -> Result<String, String> {
    Ok(get_app(&state, &account_id)?.did())
}

#[tauri::command]
#[specta::specta]
fn device_id(state: tauri::State<'_, AppState>, account_id: String) -> Result<u32, String> {
    Ok(get_app(&state, &account_id)?.device_id())
}

#[tauri::command]
#[specta::specta]
fn own_display_name(state: tauri::State<'_, AppState>, account_id: String) -> Result<String, String> {
    get_app(&state, &account_id)?
        .own_display_name()
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn set_display_name(
    state: tauri::State<'_, AppState>,
    account_id: String,
    display_name: String,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .set_display_name(display_name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn has_recovery(state: tauri::State<'_, AppState>, account_id: String) -> Result<bool, String> {
    Ok(get_app(&state, &account_id)?.has_recovery())
}

/// Re-encrypt and upload this account's recovery blob for the given PRF output
/// and server list. The PRF output must be exactly 32 bytes — desktop has no
/// passkey/PRF authenticator, so the only caller is the recovery-phrase setup
/// flow, which feeds the 32-byte seed derived from the phrase
/// (`recovery_phrase_to_seed`). See `desktop/CLAUDE.md` (passkey divergence).
#[tauri::command]
#[specta::specta]
fn update_recovery_blob(
    state: tauri::State<'_, AppState>,
    account_id: String,
    prf_output: Vec<u8>,
    servers: Vec<String>,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .update_recovery_blob(prf_output, servers)
        .map_err(|e| e.to_string())
}

/// This account's home (primary) server URL.
#[tauri::command]
#[specta::specta]
fn home_server(state: tauri::State<'_, AppState>, account_id: String) -> Result<String, String> {
    Ok(get_app(&state, &account_id)?.home_server())
}

/// Generate a fresh 12-word BIP39 recovery phrase. Stateless — drives the
/// recovery-phrase *setup* flow (desktop has no passkey/PRF path).
#[tauri::command]
#[specta::specta]
fn generate_recovery_phrase() -> Result<String, String> {
    app_core::generate_recovery_phrase().map_err(|e| e.to_string())
}

/// Validate a BIP39 recovery phrase and derive its 32-byte seed (the PRF-output
/// stand-in for `update_recovery_blob` / `derive_did_from_passkey`).
#[tauri::command]
#[specta::specta]
fn recovery_phrase_to_seed(phrase: String) -> Result<Vec<u8>, String> {
    app_core::recovery_phrase_to_seed(phrase).map_err(|e| e.to_string())
}

/// Recompute the DID a given seed + signup server URL would produce, without
/// fetching anything. The recovery-phrase restore flow needs the DID before it
/// can download the recovery blob (the phrase carries no DID, unlike a passkey
/// userHandle).
#[tauri::command]
#[specta::specta]
fn derive_did_from_passkey(prf_output: Vec<u8>, signup_server_url: String) -> Result<String, String> {
    app_core::derive_did_from_passkey(prf_output, signup_server_url).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn contact_display_name(
    state: tauri::State<'_, AppState>,
    account_id: String,
    did: String,
) -> Result<String, String> {
    get_app(&state, &account_id)?
        .contact_display_name(did)
        .map_err(|e| e.to_string())
}

/// Batch-resolve display names from local storage only (no network) for a set
/// of DIDs — used to warm the name cache on conversation load so chat-list rows
/// render real names immediately instead of flashing raw DIDs (T78). Returns
/// only DIDs with a non-empty cached name.
#[tauri::command]
#[specta::specta]
fn cached_display_names(
    state: tauri::State<'_, AppState>,
    account_id: String,
    dids: Vec<String>,
) -> Result<std::collections::HashMap<String, String>, String> {
    get_app(&state, &account_id)?
        .cached_display_names(dids)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn get_account_info(
    state: tauri::State<'_, AppState>,
    account_id: String,
    did: String,
) -> Result<app_core::AccountInfoFfi, String> {
    get_app(&state, &account_id)?
        .get_account_info(did)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn refresh_contact_profile(
    state: tauri::State<'_, AppState>,
    account_id: String,
    did: String,
) -> Result<bool, String> {
    get_app(&state, &account_id)?
        .refresh_contact_profile(did)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn list_contacts(state: tauri::State<'_, AppState>, account_id: String) -> Result<Vec<app_core::ContactRowFfi>, String> {
    get_app(&state, &account_id)?
        .list_contacts()
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn touch_contact(
    state: tauri::State<'_, AppState>,
    account_id: String,
    did: String,
    curated: bool,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .touch_contact(did, curated)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn fetch_and_cache_profile(
    state: tauri::State<'_, AppState>,
    account_id: String,
    did: String,
    profile_key: Vec<u8>,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .fetch_and_cache_profile(did, profile_key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn prime_contact_profile(
    state: tauri::State<'_, AppState>,
    account_id: String,
    did: String,
    display_name: String,
    profile_key: Vec<u8>,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .prime_contact_profile(did, display_name, profile_key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn block_contact(state: tauri::State<'_, AppState>, account_id: String, did: String) -> Result<(), String> {
    get_app(&state, &account_id)?
        .block_contact(did)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn unblock_contact(state: tauri::State<'_, AppState>, account_id: String, did: String) -> Result<(), String> {
    get_app(&state, &account_id)?
        .unblock_contact(did)
        .map_err(|e| e.to_string())
}

// ── Account lifecycle ─────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn leave_server(state: tauri::State<'_, AppState>, account_id: String) -> Result<(), String> {
    get_app(&state, &account_id)?
        .leave_server()
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn delete_identity(state: tauri::State<'_, AppState>, account_id: String) -> Result<(), String> {
    let result = get_app(&state, &account_id)?.delete_identity().map_err(|e| e.to_string());
    // Clear session state regardless of result — identity is gone either way.
    state
        .cores
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?
        .remove(&account_id);
    result
}

// ── Session management ─────────────────────────────────────────────────────────

/// Drops the `Arc<AppCore>` handle so `get_app` returns "no account". Called by
/// the frontend on logout / mode-switch. The TS-owned polling loop has already
/// been stopped, so there is no background thread to cancel — this just releases
/// the core so the old reconnect task + WS connection die on drop.
#[tauri::command]
#[specta::specta]
fn clear_session(state: tauri::State<'_, AppState>, account_id: String) -> Result<(), String> {
    state
        .cores
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?
        .remove(&account_id);
    Ok(())
}

// ── Projects ──────────────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn fetch_projects(state: tauri::State<'_, AppState>, account_id: String) -> Result<Vec<app_core::ProjectInfoFfi>, String> {
    get_app(&state, &account_id)?
        .fetch_projects()
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn request_project_token(
    state: tauri::State<'_, AppState>,
    account_id: String,
    project_url: String,
) -> Result<String, String> {
    get_app(&state, &account_id)?
        .request_project_token(project_url)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn validate_invite(token: String) -> Result<app_core::InviteInfo, String> {
    app_core::validate_invite(token)
        .map_err(|e| e.to_string())
}

// ── Connection state ──────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn connection_state(state: tauri::State<'_, AppState>, account_id: String) -> Result<app_core::ConnectionState, String> {
    Ok(get_app(&state, &account_id)?.connection_state())
}

/// Async + `spawn_blocking` for the same reason as `next_events`: this parks on
/// `ffi_runtime().block_on(rx.changed().await)` until the connection state
/// changes, so it must not run on the main thread or it freezes the WebView.
/// `startConnectionLoop` long-polls this concurrently with the event loop.
#[tauri::command]
#[specta::specta]
async fn wait_for_connection_state_change(
    state: tauri::State<'_, AppState>,
    account_id: String,
    last: app_core::ConnectionState,
) -> Result<app_core::ConnectionState, String> {
    let app = get_app(&state, &account_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        app.wait_for_connection_state_change(last)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── Groups ────────────────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn create_group(
    state: tauri::State<'_, AppState>,
    account_id: String,
    title: String,
    description: String,
    expiry_seconds: u32,
) -> Result<app_core::CreatedGroupFfi, String> {
    get_app(&state, &account_id)?
        .create_group(title, description, expiry_seconds)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn fetch_group_state(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
) -> Result<app_core::GroupSummaryFfi, String> {
    get_app(&state, &account_id)?
        .fetch_group_state(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn cached_group_state(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
) -> Result<Option<app_core::GroupSummaryFfi>, String> {
    get_app(&state, &account_id)?
        .cached_group_state(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn invite_member(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
    recipient_did: String,
    role: i16,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .invite_member(group_id, recipient_did, role)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn accept_invite(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .accept_invite(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn decline_invite(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .decline_invite(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn cancel_join_request(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .cancel_join_request(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn approve_join_request(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
    encrypted_member_id: String,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .approve_join_request(group_id, encrypted_member_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn deny_join_request(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
    encrypted_member_id: String,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .deny_join_request(group_id, encrypted_member_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn remove_member(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
    encrypted_member_id: String,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .remove_member(group_id, encrypted_member_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn leave_group(state: tauri::State<'_, AppState>, account_id: String, group_id: String) -> Result<(), String> {
    get_app(&state, &account_id)?
        .leave_group(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn is_group_member(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
) -> Result<bool, String> {
    get_app(&state, &account_id)?
        .is_group_member(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn change_member_role(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
    encrypted_member_id: String,
    new_role: i16,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .change_member_role(group_id, encrypted_member_id, new_role)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn set_group_expiry(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
    expiry_seconds: u32,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .set_group_expiry(group_id, expiry_seconds)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn set_group_title(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
    new_title: String,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .set_group_title(group_id, new_title)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn group_expiry_seconds(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
) -> Result<u32, String> {
    get_app(&state, &account_id)?
        .group_expiry_seconds(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn apply_pending_group_changes(
    state: tauri::State<'_, AppState>,
    account_id: String,
    group_id: String,
) -> Result<i64, String> {
    get_app(&state, &account_id)?
        .apply_pending_group_changes(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn list_groups(state: tauri::State<'_, AppState>, account_id: String) -> Result<Vec<String>, String> {
    get_app(&state, &account_id)?
        .list_groups()
        .map_err(|e| e.to_string())
}

// ── Edit / delete / reactions ─────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)] // gained account_id for multi-account routing
fn send_reaction(
    state: tauri::State<'_, AppState>,
    account_id: String,
    target: app_core::MessageTarget,
    target_author: String,
    target_sent_at_ms: i64,
    emoji: String,
    remove: bool,
    sent_at_ms: i64,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .send_reaction(target, target_author, target_sent_at_ms, emoji, remove, sent_at_ms)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn send_edit(
    state: tauri::State<'_, AppState>,
    account_id: String,
    target: app_core::MessageTarget,
    target_sent_at_ms: i64,
    new_body: String,
    sent_at_ms: i64,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .send_edit(target, target_sent_at_ms, new_body, sent_at_ms)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn send_delete(
    state: tauri::State<'_, AppState>,
    account_id: String,
    target: app_core::MessageTarget,
    target_author: String,
    target_sent_at_ms: i64,
    for_everyone: bool,
    sent_at_ms: i64,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .send_delete(target, target_author, target_sent_at_ms, for_everyone, sent_at_ms)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn load_reactions(
    state: tauri::State<'_, AppState>,
    account_id: String,
    conversation_id: String,
) -> Result<Vec<app_core::ReactionFfi>, String> {
    get_app(&state, &account_id)?
        .load_reactions(conversation_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn load_message_revisions(
    state: tauri::State<'_, AppState>,
    account_id: String,
    conversation_id: String,
    author: String,
    sent_at_ms: i64,
) -> Result<Vec<app_core::MessageRevisionFfi>, String> {
    get_app(&state, &account_id)?
        .load_message_revisions(conversation_id, author, sent_at_ms)
        .map_err(|e| e.to_string())
}

// ── Read receipts ─────────────────────────────────────────────────────────────

/// Drain decrypted messages from app-core's queue. The Desktop event loop polls
/// `next_events`; this lower-level call is plumbed for parity with iOS/Android.
#[tauri::command]
#[specta::specta]
fn receive_messages(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<Vec<app_core::DecryptedMessage>, String> {
    get_app(&state, &account_id)?
        .receive_messages()
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn send_read_receipt(
    state: tauri::State<'_, AppState>,
    account_id: String,
    recipient_did: String,
    timestamps: Vec<i64>,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .send_read_receipt(recipient_did, timestamps)
        .map_err(|e| e.to_string())
}

// ── Message requests / safety ─────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn accept_request(state: tauri::State<'_, AppState>, account_id: String, did: String) -> Result<(), String> {
    get_app(&state, &account_id)?
        .accept_request(did)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn delete_request(state: tauri::State<'_, AppState>, account_id: String, did: String) -> Result<(), String> {
    get_app(&state, &account_id)?
        .delete_request(did)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn set_pending_request(
    state: tauri::State<'_, AppState>,
    account_id: String,
    did: String,
    pending: bool,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .set_pending_request(did, pending)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn report_and_block(
    state: tauri::State<'_, AppState>,
    account_id: String,
    did: String,
    reason: String,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .report_and_block(did, reason)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn list_blocked(state: tauri::State<'_, AppState>, account_id: String) -> Result<Vec<app_core::ContactRowFfi>, String> {
    get_app(&state, &account_id)?
        .list_blocked()
        .map_err(|e| e.to_string())
}

// ── Disappearing-message timers ───────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn get_conversation_timer(
    state: tauri::State<'_, AppState>,
    account_id: String,
    conversation_id: String,
) -> Result<Option<u32>, String> {
    get_app(&state, &account_id)?
        .get_conversation_timer(conversation_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn set_conversation_timer(
    state: tauri::State<'_, AppState>,
    account_id: String,
    recipient_did: String,
    expiry_secs: Option<u32>,
) -> Result<(), String> {
    get_app(&state, &account_id)?
        .set_conversation_timer(recipient_did, expiry_secs)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn delete_expired_messages(
    state: tauri::State<'_, AppState>,
    account_id: String,
) -> Result<Vec<String>, String> {
    get_app(&state, &account_id)?
        .delete_expired_messages()
        .map_err(|e| e.to_string())
}

// ── Group join via link ───────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn join_via_link(
    state: tauri::State<'_, AppState>,
    account_id: String,
    master_key: Vec<u8>,
    hosting_server_url: String,
    password: Vec<u8>,
) -> Result<app_core::JoinResultFfi, String> {
    get_app(&state, &account_id)?
        .join_via_link(master_key, hosting_server_url, password)
        .map_err(|e| e.to_string())
}

// ── Attachments ────────────────────────────────────────────────────────────────

/// Encrypt and upload an attachment blob, returning the pointer to send. Async +
/// `spawn_blocking` because the upload does network I/O (mirrors `next_events`'s
/// off-thread pattern so the WebView never freezes).
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
async fn upload_attachment(
    state: tauri::State<'_, AppState>,
    account_id: String,
    plaintext: Vec<u8>,
    content_type: String,
    file_name: Option<String>,
    width: i32,
    height: i32,
    duration_ms: i32,
    thumbnail: Vec<u8>,
    flags: i32,
) -> Result<app_core::AttachmentFfi, String> {
    let app = get_app(&state, &account_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        app.upload_attachment(
            plaintext, content_type, file_name, width, height, duration_ms, thumbnail, flags,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Deterministic on-disk cache path for an attachment id, under the app cache
/// dir. `None` for an unsaved pointer (empty id) — those aren't cached.
fn attachment_cache_path(app_handle: &tauri::AppHandle, id: &str) -> Option<std::path::PathBuf> {
    if id.is_empty() {
        return None;
    }
    let dir = app_handle.path().app_cache_dir().ok()?.join("attachments");
    // Sanitize the id into a safe filename (ids are local row ids / uuids).
    let safe: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    Some(dir.join(safe))
}

/// Download, verify, and decrypt an attachment blob; returns the plaintext bytes.
/// Caches the decrypted blob on disk and records the path via app-core's
/// `set_attachment_downloaded`, so re-opening a transcript (or restarting) reads
/// from disk instead of re-fetching + re-decrypting (mirrors iOS). Async +
/// `spawn_blocking` (network + filesystem I/O).
#[tauri::command]
#[specta::specta]
async fn download_attachment(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    account_id: String,
    attachment: app_core::AttachmentFfi,
) -> Result<Vec<u8>, String> {
    let app = get_app(&state, &account_id)?;
    let cache_path = attachment_cache_path(&app_handle, &attachment.id);
    tauri::async_runtime::spawn_blocking(move || {
        // Disk-cache hit: skip the network fetch + decryption entirely.
        if let Some(path) = &cache_path {
            if let Ok(bytes) = std::fs::read(path) {
                if !bytes.is_empty() {
                    return Ok(bytes);
                }
            }
        }
        let bytes = app
            .download_attachment(attachment.clone())
            .map_err(|e| e.to_string())?;
        // Best-effort: persist to the disk cache and record the path so later
        // loads (this session and after restart) hit the cache. A failure here
        // never fails the download — the caller still gets the bytes.
        if let Some(path) = &cache_path {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if std::fs::write(path, &bytes).is_ok() {
                let _ = app.set_attachment_downloaded(
                    attachment.id.clone(),
                    path.to_string_lossy().into_owned(),
                );
            }
        }
        Ok(bytes)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Send a message carrying attachments and/or link previews to a DM or group.
/// One path for both targets (the `MessageTarget` fork lives in app-core). Async
/// + `spawn_blocking` (network I/O).
#[tauri::command]
#[specta::specta]
async fn send_message_with_attachments(
    state: tauri::State<'_, AppState>,
    account_id: String,
    target: app_core::MessageTarget,
    body: String,
    attachments: Vec<app_core::AttachmentFfi>,
    previews: Vec<app_core::LinkPreviewFfi>,
    sent_at_ms: i64,
) -> Result<(), String> {
    let app = get_app(&state, &account_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        app.send_message_with_attachments(target, body, attachments, previews, sent_at_ms)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── External browser ───────────────────────────────────────────────────────────

/// Whether `url` is an `http`/`https` URL we'll hand to the OS browser. Rejects
/// every other scheme (`file:`, `javascript:`, `data:`, custom schemes, …) so a
/// crafted message body can't open a local file or a dangerous handler. Matches
/// iOS, which only linkifies http(s) (NSDataDetector `.link`).
fn is_web_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Open a validated http(s) URL in the OS default browser. The WebView must never
/// navigate to message URLs in-app (A2); link clicks and link-preview card taps
/// route here. Non-web schemes are rejected.
#[tauri::command]
#[specta::specta]
fn open_external(url: String) -> Result<(), String> {
    if !is_web_url(&url) {
        return Err("refusing to open non-http(s) url".to_string());
    }
    open::that(&url).map_err(|e| e.to_string())
}

// ── Link-preview OG fetch (A4) ───────────────────────────────────────────────

const PREVIEW_MAX_HTML_BYTES: usize = 1024 * 1024; // 1 MiB of HTML is plenty for <head>
const PREVIEW_MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024; // 5 MiB og:image cap
const PREVIEW_TIMEOUT_SECS: u64 = 10;
// A normal-looking UA — some sites serve no OG tags to obvious bots.
const PREVIEW_USER_AGENT: &str =
    "Mozilla/5.0 (compatible; AvalancheLinkPreview/1.0; +https://theavalanche.net)";

/// Read a response body but stop once `cap` bytes have been buffered, so a
/// hostile or huge URL can't exhaust memory.
async fn read_body_capped(mut resp: reqwest::Response, cap: usize) -> Result<Vec<u8>, String> {
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        buf.extend_from_slice(&chunk);
        if buf.len() >= cap {
            buf.truncate(cap);
            break;
        }
    }
    Ok(buf)
}

/// Extract `(title, description, image_url)` from OG / Twitter-card / standard
/// `<meta>` tags, falling back to `<title>`. `image_url` may be relative — the
/// caller resolves it against the fetched page URL.
fn parse_preview_meta(html: &str) -> (String, String, Option<String>) {
    let doc = scraper::Html::parse_document(html);
    let meta_sel = scraper::Selector::parse("meta").expect("valid meta selector");
    let mut title: Option<String> = None;
    let mut description: Option<String> = None;
    let mut image: Option<String> = None;
    for el in doc.select(&meta_sel) {
        let key = el
            .value()
            .attr("property")
            .or_else(|| el.value().attr("name"));
        let content = el.value().attr("content");
        if let (Some(key), Some(content)) = (key, content) {
            if content.trim().is_empty() {
                continue;
            }
            match key.trim().to_ascii_lowercase().as_str() {
                "og:title" | "twitter:title" if title.is_none() => {
                    title = Some(content.to_string())
                }
                "og:description" | "twitter:description" | "description"
                    if description.is_none() =>
                {
                    description = Some(content.to_string())
                }
                "og:image" | "og:image:url" | "og:image:secure_url" | "twitter:image"
                    if image.is_none() =>
                {
                    image = Some(content.to_string())
                }
                _ => {}
            }
        }
    }
    if title.is_none() {
        let title_sel = scraper::Selector::parse("title").expect("valid title selector");
        if let Some(t) = doc.select(&title_sel).next() {
            let txt = t.text().collect::<String>().trim().to_string();
            if !txt.is_empty() {
                title = Some(txt);
            }
        }
    }
    (title.unwrap_or_default(), description.unwrap_or_default(), image)
}

/// Fetch a URL and scrape its OG/meta tags + og:image bytes (A4). Lives in Rust
/// because the WebView's CSP forbids external fetches (`connect-src ipc:`).
/// Size-capped, timed out, and http(s)-only. Returns a text-only card when no
/// image is found or the image fetch fails — never errors on a missing image.
#[tauri::command]
#[specta::specta]
async fn fetch_link_preview(url: String) -> Result<LinkPreviewMetaFfi, String> {
    if !is_web_url(&url) {
        return Err("refusing to fetch non-http(s) url".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(PREVIEW_TIMEOUT_SECS))
        .user_agent(PREVIEW_USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    // The post-redirect URL is the base for resolving a relative og:image.
    let base_url = resp.url().clone();
    let html_bytes = read_body_capped(resp, PREVIEW_MAX_HTML_BYTES).await?;
    let html = String::from_utf8_lossy(&html_bytes).into_owned();
    let (title, description, image_ref) = parse_preview_meta(&html);

    let mut image_bytes = Vec::new();
    let mut image_content_type = None;
    if let Some(image_ref) = image_ref {
        if let Ok(image_url) = base_url.join(&image_ref) {
            if is_web_url(image_url.as_str()) {
                if let Ok(img_resp) = client.get(image_url).send().await {
                    let ct = img_resp
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string());
                    if let Ok(bytes) = read_body_capped(img_resp, PREVIEW_MAX_IMAGE_BYTES).await {
                        if !bytes.is_empty() {
                            image_bytes = bytes;
                            image_content_type = ct;
                        }
                    }
                }
            }
        }
    }

    Ok(LinkPreviewMetaFfi {
        url,
        title,
        description,
        // No reliable published-date source without a date parser; iOS's
        // LPMetadataProvider doesn't surface one either. 0 = unknown.
        date_ms: 0,
        image_bytes,
        image_content_type,
    })
}

