//! Dev-only debugging surface (debug builds only; see `desktop/CLAUDE.md`
//! "Debugging the running app"). Lets a local tool run JavaScript in the main
//! webview and read the result, so the running app can be navigated and
//! inspected from the command line (`desktop/scripts/devctl`).
//!
//! - Listens on `127.0.0.1:$AVALANCHE_DEBUG_PORT` (default 17890).
//! - Writes `{port, token}` to `/tmp/avalanche-desktop-debug.json` (mode 0600).
//!   `POST /eval` needs the `x-debug-token` header; the body is a JavaScript
//!   function body (`return ...` to produce a value; `await` allowed).
//! - The page posts the result back to `POST /report` (the token travels in the
//!   body), allowed by `devCsp` only; the production CSP is untouched.
//!
//! Compiled only with `debug_assertions`, so release builds have no listener.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Manager};

const DEFAULT_PORT: u16 = 17890;
const INFO_FILE: &str = "/tmp/avalanche-desktop-debug.json";
const EVAL_TIMEOUT: Duration = Duration::from_secs(15);

struct Bridge {
    app: AppHandle,
    port: u16,
    token: String,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, mpsc::Sender<String>>>,
}

pub fn start(app: AppHandle) {
    let port = std::env::var("AVALANCHE_DEBUG_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[debug-bridge] not started: can't bind 127.0.0.1:{port}: {e}");
            return;
        }
    };
    let token = random_token();
    if let Err(e) = write_info_file(port, &token) {
        eprintln!("[debug-bridge] not started: can't write {INFO_FILE}: {e}");
        return;
    }
    let bridge = Arc::new(Bridge {
        app,
        port,
        token,
        next_id: AtomicU64::new(1),
        pending: Mutex::new(HashMap::new()),
    });
    eprintln!("[debug-bridge] listening on 127.0.0.1:{port} (token in {INFO_FILE})");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let bridge = bridge.clone();
            std::thread::spawn(move || {
                let _ = handle(&bridge, stream);
            });
        }
    });
}

fn random_token() -> String {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .expect("read /dev/urandom");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn write_info_file(port: u16, token: &str) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(INFO_FILE)?;
    write!(f, "{}", serde_json::json!({ "port": port, "token": token }))
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

fn read_request(stream: &TcpStream) -> std::io::Result<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h)?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Request { method, path, headers, body: String::from_utf8_lossy(&body).into_owned() })
}

fn respond(mut stream: &TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn handle(bridge: &Bridge, stream: TcpStream) -> std::io::Result<()> {
    let req = read_request(&stream)?;
    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/eval") => {
            if req.headers.get("x-debug-token") != Some(&bridge.token) {
                return respond(&stream, "401 Unauthorized", r#"{"error":"bad token"}"#);
            }
            let body = eval(bridge, &req.body);
            respond(&stream, "200 OK", &body)
        }
        ("POST", "/report") => {
            // From the page: {id, token, ok, value|error}. Text/plain, so no
            // CORS preflight; the token in the body authenticates it.
            let v: serde_json::Value = serde_json::from_str(&req.body).unwrap_or_default();
            if v.get("token").and_then(|t| t.as_str()) == Some(bridge.token.as_str()) {
                if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
                    if let Some(tx) = bridge.pending.lock().unwrap().remove(&id) {
                        let _ = tx.send(req.body.clone());
                    }
                }
            }
            respond(&stream, "204 No Content", "")
        }
        _ => respond(&stream, "404 Not Found", r#"{"error":"not found"}"#),
    }
}

fn eval(bridge: &Bridge, js: &str) -> String {
    let Some(window) = bridge.app.get_webview_window("main") else {
        return r#"{"ok":false,"error":"no main window"}"#.into();
    };
    let id = bridge.next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    bridge.pending.lock().unwrap().insert(id, tx);
    // Run the snippet as an async function body; serialize its return value
    // (or error) and post it back. The snippet is spliced in as code: `webview.eval`
    // injects the script directly (not subject to CSP), whereas building a
    // function from a string would be `eval`, which the CSP forbids. A syntax
    // error in the snippet therefore surfaces as a timeout.
    let wrapped = format!(
        r#"(async () => {{
  const report = (r) => fetch("http://127.0.0.1:{port}/report", {{
    method: "POST", headers: {{ "Content-Type": "text/plain" }},
    body: JSON.stringify(Object.assign({{ id: {id}, token: "{token}" }}, r)) }});
  try {{
    const value = await (async () => {{
{src}
    }})();
    await report({{ ok: true, value: value === undefined ? null : value }});
  }} catch (e) {{
    await report({{ ok: false, error: String(e && e.stack || e) }});
  }}
}})();"#,
        port = bridge.port,
        token = bridge.token,
        src = js,
    );
    if let Err(e) = window.eval(&wrapped) {
        bridge.pending.lock().unwrap().remove(&id);
        return serde_json::json!({ "ok": false, "error": format!("eval failed: {e}") }).to_string();
    }
    match rx.recv_timeout(EVAL_TIMEOUT) {
        Ok(report) => {
            // Strip the token before handing the result to the caller.
            let mut v: serde_json::Value = serde_json::from_str(&report).unwrap_or_default();
            if let Some(o) = v.as_object_mut() {
                o.remove("token");
                o.remove("id");
            }
            v.to_string()
        }
        Err(_) => {
            bridge.pending.lock().unwrap().remove(&id);
            r#"{"ok":false,"error":"timed out waiting for the page"}"#.into()
        }
    }
}
