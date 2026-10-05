//! The SQLCipher database key (S-05, docs/61). One random 256-bit key per
//! install, shared by every account's database (as on iOS/Android), kept in the
//! OS credential store: macOS Keychain, Windows Credential Manager, Linux Secret
//! Service. Format matches mobile: 64 lowercase hex characters.
//!
//! The key never crosses into the webview: the account-opening commands fetch it
//! here, so page script can't read it.
//!
//! Linux without a Secret Service (no keyring daemon) falls back to a 0600 key
//! file in the app-data dir, with a warning — a strong default rather than
//! refusing to start. macOS and Windows never fall back: a credential-store
//! error there (e.g. the user denied Keychain access) is surfaced, because
//! silently minting a new key would make the existing databases unopenable.

use std::sync::Mutex;

use tauri::{AppHandle, Manager};

const SERVICE: &str = "net.avalancheapp.desktop";
const ACCOUNT: &str = "sqlcipher-db-key";

/// Cached for the process lifetime so the credential store (and any macOS
/// access prompt) is hit once per launch, not once per account.
static CACHED: Mutex<Option<String>> = Mutex::new(None);

pub fn db_key(app: &AppHandle) -> Result<String, String> {
    let mut cached = CACHED.lock().map_err(|e| format!("lock poisoned: {e}"))?;
    if let Some(k) = cached.as_ref() {
        return Ok(k.clone());
    }
    let key = load_or_create(app)?;
    *cached = Some(key.clone());
    Ok(key)
}

fn generate() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("random key generation failed: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn load_or_create(app: &AppHandle) -> Result<String, String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string());
    match entry.and_then(|e| keychain_load_or_create(&e)) {
        Ok(key) => Ok(key),
        Err(e) if cfg!(target_os = "linux") => {
            eprintln!(
                "[db-key] no usable Secret Service ({e}); falling back to a key file \
                 in the app-data directory (protected by file permissions only)"
            );
            file_load_or_create(app)
        }
        Err(e) => Err(format!("couldn't access the database key in the credential store: {e}")),
    }
}

fn keychain_load_or_create(entry: &keyring::Entry) -> Result<String, String> {
    match entry.get_password() {
        Ok(key) => Ok(key),
        Err(keyring::Error::NoEntry) => {
            let key = generate()?;
            entry.set_password(&key).map_err(|e| e.to_string())?;
            // Read back so a store that silently drops writes fails here, before
            // any database is created under a key we can't recover.
            match entry.get_password() {
                Ok(stored) if stored == key => Ok(key),
                Ok(_) => Err("credential store returned a different key than was saved".into()),
                Err(e) => Err(e.to_string()),
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn file_load_or_create(app: &AppHandle) -> Result<String, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("db-key");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let existing = existing.trim().to_string();
        if !existing.is_empty() {
            return Ok(existing);
        }
    }
    let key = generate()?;
    write_private(&path, &key)?;
    Ok(key)
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, contents: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(contents.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, contents: &str) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|e| e.to_string())
}

/// Resolve an account database name from the frontend (e.g. `account-ab12cd34.db`)
/// to its path in the app-data directory. Only a bare file name is accepted, so
/// page script can't aim the backend at an arbitrary file. (Databases used to
/// resolve against the process's working directory, which for a packaged app is
/// not writable.)
pub fn db_path(app: &AppHandle, name: &str) -> Result<String, String> {
    if !is_bare_name(name) {
        return Err("invalid database name".into());
    }
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(name).to_string_lossy().into_owned())
}

fn is_bare_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && name != "."
        && name != ".."
        && std::path::Path::new(name).file_name().map(|f| f == name).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::is_bare_name;

    #[test]
    fn only_bare_db_names_are_accepted() {
        assert!(is_bare_name("account-ab12cd34.db"));
        for bad in ["", ".", "..", "../x.db", "/etc/passwd", "a/b.db", "a\\b.db", "..\\x.db"] {
            assert!(!is_bare_name(bad), "{bad:?} should be rejected");
        }
    }
}
