//! Sends a single APNs push to a device token, for smoke-testing the APNs
//! auth key + bundle ID + entitlement, and for verifying that silent
//! pushes wake the app's didReceiveRemoteNotification handler.
//!
//! Usage:
//!   APNS_KEY_PATH=./AuthKey_3WMG978DSL.p8 \
//!   APNS_KEY_ID=3WMG978DSL \
//!   APNS_TEAM_ID=7FVK3RR3TV \
//!   APNS_BUNDLE_ID=net.theavalanche.app \
//!   APNS_ENVIRONMENT=sandbox \
//!   cargo run -p relay --example send_test_push -- <device_token_hex> [alert|silent]
//!
//! `alert` (default) produces a visible banner — verifies the credentials chain.
//! `silent` mimics the relay's wakeup payload — verifies the background-fetch
//! handler runs (look for `[PushHandler] silent push received` in the device log).

use a2::{
    Client, ClientConfig, DefaultNotificationBuilder, Endpoint, NotificationBuilder,
    NotificationOptions, Priority, PushType,
};
use std::env;
use std::fs::File;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = env::args().nth(1).expect("usage: send_test_push <device_token_hex> [alert|silent]");
    let mode = env::args().nth(2).unwrap_or_else(|| "alert".into());

    let key_path = env::var("APNS_KEY_PATH").expect("APNS_KEY_PATH");
    let key_id = env::var("APNS_KEY_ID").expect("APNS_KEY_ID");
    let team_id = env::var("APNS_TEAM_ID").expect("APNS_TEAM_ID");
    let bundle_id = env::var("APNS_BUNDLE_ID").expect("APNS_BUNDLE_ID");
    let endpoint = match env::var("APNS_ENVIRONMENT").as_deref() {
        Ok("production") => Endpoint::Production,
        _ => Endpoint::Sandbox,
    };

    let mut key = File::open(&key_path)?;
    let client = Client::token(&mut key, &key_id, &team_id, ClientConfig::new(endpoint))?;

    let (payload, push_type, priority) = match mode.as_str() {
        "silent" => (
            DefaultNotificationBuilder::new().set_content_available().build(
                &token,
                NotificationOptions {
                    apns_topic: Some(&bundle_id),
                    apns_push_type: Some(PushType::Background),
                    apns_priority: Some(Priority::Normal),
                    ..Default::default()
                },
            ),
            "background",
            "normal",
        ),
        _ => (
            DefaultNotificationBuilder::new()
                .set_title("actnet test")
                .set_body("hello from send_test_push")
                .set_sound("default")
                .build(
                    &token,
                    NotificationOptions {
                        apns_topic: Some(&bundle_id),
                        apns_push_type: Some(PushType::Alert),
                        apns_priority: Some(Priority::High),
                        ..Default::default()
                    },
                ),
            "alert",
            "high",
        ),
    };

    println!("sending {push_type}/{priority} topic={bundle_id} token={token}");
    let response = client.send(payload).await?;
    println!("APNs response: {response:?}");
    Ok(())
}
