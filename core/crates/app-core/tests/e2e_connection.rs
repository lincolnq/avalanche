//! End-to-end tests for the connection-lifecycle controls added alongside the
//! reconnect-hang fix: `reconnect_now()` and `set_app_active()`.
//!
//! These can't deterministically exercise the *recovery* path (forcing a
//! silently-dead socket, then waiting out `IDLE_TIMEOUT`/`PROBE_DEADLINE`, is
//! slow and flaky) — that's covered by the pure-logic unit tests in `net` and
//! manual verification. What they *can* guarantee fast is the regression that
//! matters here: probing/toggling a **healthy** connection must not tear it
//! down, and live WS delivery keeps working afterward.
//!
//! Requires a homeserver at `SERVER_URL` (default `http://localhost:3000`).
//! Run via `make test-e2e`.

mod common;

use app_core::{AppCore, ConnectionState, IncomingEvent};
use std::sync::Arc;
use std::time::Duration;

fn server_url() -> String {
    std::env::var("SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

async fn test_store() -> store::DeviceStore {
    store::DeviceStore::open_in_memory().await.unwrap()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Create a bot account and start its reconnect task — mirrors the production
/// FFI constructors: an `Arc<AppCore>` with a running WS loop.
async fn live_account(url: &str) -> Arc<AppCore> {
    let core = AppCore::create_account_with_store(url, test_store().await, None, true, common::invite_token())
        .await
        .unwrap();
    let arc = Arc::new(core);
    arc.start_reconnect_task();
    arc
}

/// Poll the (non-blocking) connection-state snapshot until `Connected` or the
/// timeout elapses.
async fn wait_connected(core: &AppCore, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if matches!(core.connection_state(), ConnectionState::Connected) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Drain `next_events_async` until an event matches `pred`, or `timeout`.
async fn wait_for_event(
    core: &AppCore,
    pred: impl Fn(&IncomingEvent) -> bool,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        let batch = match tokio::time::timeout(remaining, core.next_events_async()).await {
            Ok(Ok(b)) => b,
            Ok(Err(_)) | Err(_) => return false,
        };
        if batch.iter().any(&pred) {
            return true;
        }
    }
}

/// `reconnect_now()` and `set_app_active()` on a live connection are harmless:
/// the foreground liveness probe must see the server's keepalive echo and keep
/// the socket open, and live WS delivery must still work after the toggles.
#[tokio::test]
async fn reconnect_now_and_app_active_keep_live_connection() {
    let url = server_url();

    let alice = live_account(&url).await;
    let bob = live_account(&url).await;
    assert!(wait_connected(&alice, Duration::from_secs(10)).await, "alice should connect");
    assert!(wait_connected(&bob, Duration::from_secs(10)).await, "bob should connect");

    let alice_did = alice.did_async().await;
    let bob_did = bob.did_async().await;

    // Foreground probe on a healthy connection: must not tear it down.
    alice.reconnect_now();
    // Background then foreground (the latter also probes via reconnect_now).
    bob.set_app_active(false);
    bob.set_app_active(true);
    // Give the probe round-trip (keepalive → echo) a moment to complete.
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(
        matches!(alice.connection_state(), ConnectionState::Connected),
        "alice's connection should survive a foreground probe, got {:?}",
        alice.connection_state()
    );
    assert!(
        matches!(bob.connection_state(), ConnectionState::Connected),
        "bob's connection should survive an active toggle, got {:?}",
        bob.connection_state()
    );

    // And the socket is genuinely still usable: a DM fans out live to bob's
    // `next_events` (WS push), not just a stale "Connected" snapshot.
    let payload = b"alive after probe";
    alice.send_dm_async(&bob_did, payload, now_ms()).await.unwrap();
    let got = wait_for_event(
        &bob,
        |ev| matches!(ev, IncomingEvent::Message { msg }
            if msg.sender_did == alice_did && msg.plaintext == payload),
        Duration::from_secs(5),
    )
    .await;
    assert!(got, "bob should receive alice's DM via live WS push after the probe/toggle");
}
