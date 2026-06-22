// Tauri commands — Day 1 scaffold + stubs.
// T03: ping + store plugin.
// T09: all AppCore command stubs (return Err("not implemented") until Day 4 wires app-core).

#[tauri::command]
fn ping() -> String {
    "pong".to_string()
}

// ── Account factory ──────────────────────────────────────────────────────────

#[tauri::command]
async fn create_account(
    server_url: String,
    db_path: String,
    db_key: String,
    display_name: String,
    invite_token: Option<String>,
) -> Result<serde_json::Value, String> {
    let _ = (server_url, db_path, db_key, display_name, invite_token);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn login(db_path: String, db_key: String) -> Result<serde_json::Value, String> {
    let _ = (db_path, db_key);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn recover_from_blob(
    server_url: String,
    did: String,
    db_path: String,
    db_key: String,
    display_name: String,
) -> Result<serde_json::Value, String> {
    let _ = (server_url, did, db_path, db_key, display_name);
    Err("not implemented".to_string())
}

// ── Core messaging ────────────────────────────────────────────────────────────

#[tauri::command]
async fn send_dm(
    recipient_did: String,
    body: String,
    sent_at_ms: i64,
) -> Result<(), String> {
    let _ = (recipient_did, body, sent_at_ms);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn send_group_message(
    group_id: String,
    body: String,
    sent_at_ms: i64,
) -> Result<(), String> {
    let _ = (group_id, body, sent_at_ms);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn receive_messages() -> Result<serde_json::Value, String> {
    Err("not implemented".to_string())
}

#[tauri::command]
async fn next_events() -> Result<serde_json::Value, String> {
    Err("not implemented".to_string())
}

#[tauri::command]
async fn save_message(msg: serde_json::Value) -> Result<(), String> {
    let _ = msg;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn load_conversations() -> Result<serde_json::Value, String> {
    Err("not implemented".to_string())
}

#[tauri::command]
async fn load_messages(conversation_id: String) -> Result<serde_json::Value, String> {
    let _ = conversation_id;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn mark_messages_read(
    conversation_id: String,
    up_to_sent_at_ms: i64,
) -> Result<u64, String> {
    let _ = (conversation_id, up_to_sent_at_ms);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn unread_count(conversation_id: String) -> Result<u64, String> {
    let _ = conversation_id;
    Err("not implemented".to_string())
}

// ── Identity / contacts ───────────────────────────────────────────────────────

#[tauri::command]
async fn did() -> Result<String, String> {
    Err("not implemented".to_string())
}

#[tauri::command]
async fn device_id() -> Result<u32, String> {
    Err("not implemented".to_string())
}

#[tauri::command]
async fn own_display_name() -> Result<String, String> {
    Err("not implemented".to_string())
}

#[tauri::command]
async fn set_display_name(display_name: String) -> Result<(), String> {
    let _ = display_name;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn has_recovery() -> Result<bool, String> {
    Err("not implemented".to_string())
}

#[tauri::command]
async fn contact_display_name(did: String) -> Result<String, String> {
    let _ = did;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn get_account_info(did: String) -> Result<serde_json::Value, String> {
    let _ = did;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn refresh_contact_profile(did: String) -> Result<bool, String> {
    let _ = did;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn list_contacts() -> Result<serde_json::Value, String> {
    Err("not implemented".to_string())
}

#[tauri::command]
async fn touch_contact(did: String, curated: bool) -> Result<(), String> {
    let _ = (did, curated);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn fetch_and_cache_profile(did: String, profile_key: Vec<u8>) -> Result<(), String> {
    let _ = (did, profile_key);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn prime_contact_profile(
    did: String,
    display_name: String,
    profile_key: Vec<u8>,
) -> Result<(), String> {
    let _ = (did, display_name, profile_key);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn block_contact(did: String) -> Result<(), String> {
    let _ = did;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn unblock_contact(did: String) -> Result<(), String> {
    let _ = did;
    Err("not implemented".to_string())
}

// ── Account lifecycle ─────────────────────────────────────────────────────────

#[tauri::command]
async fn leave_server() -> Result<(), String> {
    Err("not implemented".to_string())
}

#[tauri::command]
async fn delete_identity() -> Result<(), String> {
    Err("not implemented".to_string())
}

// ── Projects ──────────────────────────────────────────────────────────────────

#[tauri::command]
async fn fetch_projects() -> Result<serde_json::Value, String> {
    Err("not implemented".to_string())
}

#[tauri::command]
async fn request_project_token(project_url: String) -> Result<String, String> {
    let _ = project_url;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn validate_invite(token: String) -> Result<serde_json::Value, String> {
    let _ = token;
    Err("not implemented".to_string())
}

// ── Connection state ──────────────────────────────────────────────────────────

#[tauri::command]
async fn connection_state() -> Result<serde_json::Value, String> {
    Err("not implemented".to_string())
}

#[tauri::command]
async fn wait_for_connection_state_change(
    last: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let _ = last;
    Err("not implemented".to_string())
}

// ── Groups ────────────────────────────────────────────────────────────────────

#[tauri::command]
async fn create_group(
    title: String,
    description: String,
    expiry_seconds: u32,
) -> Result<serde_json::Value, String> {
    let _ = (title, description, expiry_seconds);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn fetch_group_state(group_id: String) -> Result<serde_json::Value, String> {
    let _ = group_id;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn cached_group_state(group_id: String) -> Result<serde_json::Value, String> {
    let _ = group_id;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn invite_member(
    group_id: String,
    recipient_did: String,
    role: i16,
) -> Result<(), String> {
    let _ = (group_id, recipient_did, role);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn accept_invite(group_id: String) -> Result<(), String> {
    let _ = group_id;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn decline_invite(group_id: String) -> Result<(), String> {
    let _ = group_id;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn cancel_join_request(group_id: String) -> Result<(), String> {
    let _ = group_id;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn approve_join_request(
    group_id: String,
    encrypted_member_id: String,
) -> Result<(), String> {
    let _ = (group_id, encrypted_member_id);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn deny_join_request(
    group_id: String,
    encrypted_member_id: String,
) -> Result<(), String> {
    let _ = (group_id, encrypted_member_id);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn remove_member(
    group_id: String,
    encrypted_member_id: String,
) -> Result<(), String> {
    let _ = (group_id, encrypted_member_id);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn leave_group(group_id: String) -> Result<(), String> {
    let _ = group_id;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn is_group_member(group_id: String) -> Result<bool, String> {
    let _ = group_id;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn change_member_role(
    group_id: String,
    encrypted_member_id: String,
    new_role: i16,
) -> Result<(), String> {
    let _ = (group_id, encrypted_member_id, new_role);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn set_group_expiry(group_id: String, expiry_seconds: u32) -> Result<(), String> {
    let _ = (group_id, expiry_seconds);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn set_group_title(group_id: String, new_title: String) -> Result<(), String> {
    let _ = (group_id, new_title);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn group_expiry_seconds(group_id: String) -> Result<u32, String> {
    let _ = group_id;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn apply_pending_group_changes(group_id: String) -> Result<i64, String> {
    let _ = group_id;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn list_groups() -> Result<serde_json::Value, String> {
    Err("not implemented".to_string())
}

// ── Edit / delete / reactions ─────────────────────────────────────────────────

#[tauri::command]
async fn send_reaction(
    target: serde_json::Value,
    target_author: String,
    target_sent_at_ms: i64,
    emoji: String,
    remove: bool,
    sent_at_ms: i64,
) -> Result<(), String> {
    let _ = (target, target_author, target_sent_at_ms, emoji, remove, sent_at_ms);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn send_edit(
    target: serde_json::Value,
    target_sent_at_ms: i64,
    new_body: String,
    sent_at_ms: i64,
) -> Result<(), String> {
    let _ = (target, target_sent_at_ms, new_body, sent_at_ms);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn send_delete(
    target: serde_json::Value,
    target_author: String,
    target_sent_at_ms: i64,
    for_everyone: bool,
    sent_at_ms: i64,
) -> Result<(), String> {
    let _ = (target, target_author, target_sent_at_ms, for_everyone, sent_at_ms);
    Err("not implemented".to_string())
}

#[tauri::command]
async fn load_reactions(conversation_id: String) -> Result<serde_json::Value, String> {
    let _ = conversation_id;
    Err("not implemented".to_string())
}

#[tauri::command]
async fn load_message_revisions(
    conversation_id: String,
    author: String,
    sent_at_ms: i64,
) -> Result<serde_json::Value, String> {
    let _ = (conversation_id, author, sent_at_ms);
    Err("not implemented".to_string())
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
