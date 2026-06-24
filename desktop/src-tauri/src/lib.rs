// Tauri commands — Avalanche Desktop bridge.
// Each command is a thin delegation to the corresponding app-core method.
// Types are code-generated via tauri-specta → ../src/bindings.ts.
// All FFI types live in ffi_types.rs — the single source of truth for the contract.
// See desktop/BRIDGE-CODEGEN-SPEC.md for the migration plan once app-core is wired.

mod ffi_types;

use std::sync::Mutex;

use app_core::AppCore;
use ffi_types::{
    AccountInfoFfi, AccountResult, ConnectionState, ContactRowFfi, CreatedGroupFfi,
    ConversationSummaryFfi, DecryptedMessage, GroupSummaryFfi, GroupTarget, IncomingEvent,
    InviteInfo, MessageRevisionFfi, MessageTarget, ProjectInfoFfi, ReactionFfi,
    StoredMessageFfi,
};

/// Map a vec of app-core types to ffi_types via `Into`.
fn map_vec<T, U: From<T>>(v: Vec<T>) -> Vec<U> {
    v.into_iter().map(Into::into).collect()
}

// ── App state ─────────────────────────────────────────────────────────────────

struct AppState {
    app: Mutex<Option<std::sync::Arc<AppCore>>>,
}

fn get_app(state: &tauri::State<'_, AppState>) -> Result<std::sync::Arc<AppCore>, String> {
    state
        .app
        .lock()
        .map_err(|e| format!("lock poisoned: {}", e))?
        .clone()
        .ok_or_else(|| "no account".to_string())
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
            receive_messages,
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
            contact_display_name,
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
        ]);

    #[cfg(debug_assertions)]
    {
        builder
            .export(
                specta_typescript::Typescript::default(),
                "../src/bindings.ts",
            )
            .expect("failed to export specta bindings");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(AppState {
            app: Mutex::new(None),
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
    *state.app.lock().map_err(|e| format!("lock poisoned: {}", e))? = Some(app);
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
    *state.app.lock().map_err(|e| format!("lock poisoned: {}", e))? = Some(app);
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
    *state.app.lock().map_err(|e| format!("lock poisoned: {}", e))? = Some(app);
    Ok(AccountResult { did, display_name })
}

// ── Core messaging ────────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn send_dm(
    state: tauri::State<'_, AppState>,
    recipient_did: String,
    plaintext: Vec<u8>,
    sent_at_ms: i64,
) -> Result<(), String> {
    get_app(&state)?
        .send_dm(recipient_did, plaintext, sent_at_ms)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn send_group_message(
    state: tauri::State<'_, AppState>,
    group_id: String,
    plaintext: Vec<u8>,
    sent_at_ms: i64,
) -> Result<(), String> {
    get_app(&state)?
        .send_message(MessageTarget::Group(GroupTarget { group_id }).into_app_core(), plaintext, sent_at_ms)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn receive_messages(state: tauri::State<'_, AppState>) -> Result<Vec<DecryptedMessage>, String> {
    get_app(&state)?
        .receive_messages()
        .map(map_vec)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn next_events(state: tauri::State<'_, AppState>) -> Result<Vec<IncomingEvent>, String> {
    get_app(&state)?
        .next_events()
        .map(map_vec)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn save_message(
    state: tauri::State<'_, AppState>,
    msg: StoredMessageFfi,
) -> Result<(), String> {
    get_app(&state)?
        .save_message(msg.into_app_core())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn load_conversations(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ConversationSummaryFfi>, String> {
    get_app(&state)?
        .load_conversations()
        .map(map_vec)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn load_messages(
    state: tauri::State<'_, AppState>,
    conversation_id: String,
) -> Result<Vec<StoredMessageFfi>, String> {
    get_app(&state)?
        .load_messages(conversation_id)
        .map(map_vec)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn mark_messages_read(
    state: tauri::State<'_, AppState>,
    conversation_id: String,
    up_to_sent_at_ms: i64,
) -> Result<u64, String> {
    get_app(&state)?
        .mark_messages_read(conversation_id, up_to_sent_at_ms)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn unread_count(
    state: tauri::State<'_, AppState>,
    conversation_id: String,
) -> Result<u64, String> {
    get_app(&state)?
        .unread_count(conversation_id)
        .map_err(|e| e.to_string())
}

// ── Identity / contacts ───────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn did(state: tauri::State<'_, AppState>) -> Result<String, String> {
    Ok(get_app(&state)?.did())
}

#[tauri::command]
#[specta::specta]
fn device_id(state: tauri::State<'_, AppState>) -> Result<u32, String> {
    Ok(get_app(&state)?.device_id())
}

#[tauri::command]
#[specta::specta]
fn own_display_name(state: tauri::State<'_, AppState>) -> Result<String, String> {
    get_app(&state)?
        .own_display_name()
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn set_display_name(
    state: tauri::State<'_, AppState>,
    display_name: String,
) -> Result<(), String> {
    get_app(&state)?
        .set_display_name(display_name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn has_recovery(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    Ok(get_app(&state)?.has_recovery())
}

#[tauri::command]
#[specta::specta]
fn contact_display_name(
    state: tauri::State<'_, AppState>,
    did: String,
) -> Result<String, String> {
    get_app(&state)?
        .contact_display_name(did)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn get_account_info(
    state: tauri::State<'_, AppState>,
    did: String,
) -> Result<AccountInfoFfi, String> {
    get_app(&state)?
        .get_account_info(did)
        .map(Into::into)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn refresh_contact_profile(
    state: tauri::State<'_, AppState>,
    did: String,
) -> Result<bool, String> {
    get_app(&state)?
        .refresh_contact_profile(did)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn list_contacts(state: tauri::State<'_, AppState>) -> Result<Vec<ContactRowFfi>, String> {
    get_app(&state)?
        .list_contacts()
        .map(map_vec)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn touch_contact(
    state: tauri::State<'_, AppState>,
    did: String,
    curated: bool,
) -> Result<(), String> {
    get_app(&state)?
        .touch_contact(did, curated)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn fetch_and_cache_profile(
    state: tauri::State<'_, AppState>,
    did: String,
    profile_key: Vec<u8>,
) -> Result<(), String> {
    get_app(&state)?
        .fetch_and_cache_profile(did, profile_key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn prime_contact_profile(
    state: tauri::State<'_, AppState>,
    did: String,
    display_name: String,
    profile_key: Vec<u8>,
) -> Result<(), String> {
    get_app(&state)?
        .prime_contact_profile(did, display_name, profile_key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn block_contact(state: tauri::State<'_, AppState>, did: String) -> Result<(), String> {
    get_app(&state)?
        .block_contact(did)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn unblock_contact(state: tauri::State<'_, AppState>, did: String) -> Result<(), String> {
    get_app(&state)?
        .unblock_contact(did)
        .map_err(|e| e.to_string())
}

// ── Account lifecycle ─────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn leave_server(state: tauri::State<'_, AppState>) -> Result<(), String> {
    get_app(&state)?
        .leave_server()
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn delete_identity(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let result = get_app(&state)?.delete_identity().map_err(|e| e.to_string());
    // Clear session state regardless of result — identity is gone either way.
    *state.app.lock().map_err(|e| format!("lock poisoned: {}", e))? = None;
    result
}

// ── Projects ──────────────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn fetch_projects(state: tauri::State<'_, AppState>) -> Result<Vec<ProjectInfoFfi>, String> {
    get_app(&state)?
        .fetch_projects()
        .map(map_vec)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn request_project_token(
    state: tauri::State<'_, AppState>,
    project_url: String,
) -> Result<String, String> {
    get_app(&state)?
        .request_project_token(project_url)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn validate_invite(token: String) -> Result<InviteInfo, String> {
    app_core::validate_invite(token)
        .map(Into::into)
        .map_err(|e| e.to_string())
}

// ── Connection state ──────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn connection_state(state: tauri::State<'_, AppState>) -> Result<ConnectionState, String> {
    Ok(get_app(&state)?.connection_state().into())
}

#[tauri::command]
#[specta::specta]
fn wait_for_connection_state_change(
    state: tauri::State<'_, AppState>,
    last: ConnectionState,
) -> Result<ConnectionState, String> {
    get_app(&state)?
        .wait_for_connection_state_change(last.into_app_core())
        .map(Into::into)
        .map_err(|e| e.to_string())
}

// ── Groups ────────────────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn create_group(
    state: tauri::State<'_, AppState>,
    title: String,
    description: String,
    expiry_seconds: u32,
) -> Result<CreatedGroupFfi, String> {
    get_app(&state)?
        .create_group(title, description, expiry_seconds)
        .map(Into::into)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn fetch_group_state(
    state: tauri::State<'_, AppState>,
    group_id: String,
) -> Result<GroupSummaryFfi, String> {
    get_app(&state)?
        .fetch_group_state(group_id)
        .map(Into::into)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn cached_group_state(
    state: tauri::State<'_, AppState>,
    group_id: String,
) -> Result<Option<GroupSummaryFfi>, String> {
    get_app(&state)?
        .cached_group_state(group_id)
        .map(|opt| opt.map(Into::into))
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn invite_member(
    state: tauri::State<'_, AppState>,
    group_id: String,
    recipient_did: String,
    role: i16,
) -> Result<(), String> {
    get_app(&state)?
        .invite_member(group_id, recipient_did, role)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn accept_invite(
    state: tauri::State<'_, AppState>,
    group_id: String,
) -> Result<(), String> {
    get_app(&state)?
        .accept_invite(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn decline_invite(
    state: tauri::State<'_, AppState>,
    group_id: String,
) -> Result<(), String> {
    get_app(&state)?
        .decline_invite(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn cancel_join_request(
    state: tauri::State<'_, AppState>,
    group_id: String,
) -> Result<(), String> {
    get_app(&state)?
        .cancel_join_request(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn approve_join_request(
    state: tauri::State<'_, AppState>,
    group_id: String,
    encrypted_member_id: String,
) -> Result<(), String> {
    get_app(&state)?
        .approve_join_request(group_id, encrypted_member_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn deny_join_request(
    state: tauri::State<'_, AppState>,
    group_id: String,
    encrypted_member_id: String,
) -> Result<(), String> {
    get_app(&state)?
        .deny_join_request(group_id, encrypted_member_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn remove_member(
    state: tauri::State<'_, AppState>,
    group_id: String,
    encrypted_member_id: String,
) -> Result<(), String> {
    get_app(&state)?
        .remove_member(group_id, encrypted_member_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn leave_group(state: tauri::State<'_, AppState>, group_id: String) -> Result<(), String> {
    get_app(&state)?
        .leave_group(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn is_group_member(
    state: tauri::State<'_, AppState>,
    group_id: String,
) -> Result<bool, String> {
    get_app(&state)?
        .is_group_member(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn change_member_role(
    state: tauri::State<'_, AppState>,
    group_id: String,
    encrypted_member_id: String,
    new_role: i16,
) -> Result<(), String> {
    get_app(&state)?
        .change_member_role(group_id, encrypted_member_id, new_role)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn set_group_expiry(
    state: tauri::State<'_, AppState>,
    group_id: String,
    expiry_seconds: u32,
) -> Result<(), String> {
    get_app(&state)?
        .set_group_expiry(group_id, expiry_seconds)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn set_group_title(
    state: tauri::State<'_, AppState>,
    group_id: String,
    new_title: String,
) -> Result<(), String> {
    get_app(&state)?
        .set_group_title(group_id, new_title)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn group_expiry_seconds(
    state: tauri::State<'_, AppState>,
    group_id: String,
) -> Result<u32, String> {
    get_app(&state)?
        .group_expiry_seconds(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn apply_pending_group_changes(
    state: tauri::State<'_, AppState>,
    group_id: String,
) -> Result<i64, String> {
    get_app(&state)?
        .apply_pending_group_changes(group_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn list_groups(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    get_app(&state)?
        .list_groups()
        .map_err(|e| e.to_string())
}

// ── Edit / delete / reactions ─────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn send_reaction(
    state: tauri::State<'_, AppState>,
    target: MessageTarget,
    target_author: String,
    target_sent_at_ms: i64,
    emoji: String,
    remove: bool,
    sent_at_ms: i64,
) -> Result<(), String> {
    get_app(&state)?
        .send_reaction(target.into_app_core(), target_author, target_sent_at_ms, emoji, remove, sent_at_ms)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn send_edit(
    state: tauri::State<'_, AppState>,
    target: MessageTarget,
    target_sent_at_ms: i64,
    new_body: String,
    sent_at_ms: i64,
) -> Result<(), String> {
    get_app(&state)?
        .send_edit(target.into_app_core(), target_sent_at_ms, new_body, sent_at_ms)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn send_delete(
    state: tauri::State<'_, AppState>,
    target: MessageTarget,
    target_author: String,
    target_sent_at_ms: i64,
    for_everyone: bool,
    sent_at_ms: i64,
) -> Result<(), String> {
    get_app(&state)?
        .send_delete(target.into_app_core(), target_author, target_sent_at_ms, for_everyone, sent_at_ms)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn load_reactions(
    state: tauri::State<'_, AppState>,
    conversation_id: String,
) -> Result<Vec<ReactionFfi>, String> {
    get_app(&state)?
        .load_reactions(conversation_id)
        .map(map_vec)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn load_message_revisions(
    state: tauri::State<'_, AppState>,
    conversation_id: String,
    author: String,
    sent_at_ms: i64,
) -> Result<Vec<MessageRevisionFfi>, String> {
    get_app(&state)?
        .load_message_revisions(conversation_id, author, sent_at_ms)
        .map(map_vec)
        .map_err(|e| e.to_string())
}
