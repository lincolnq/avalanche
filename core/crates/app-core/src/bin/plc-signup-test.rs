//! Reproduce the signup PLC submission against the real plc.directory.
//!
//! Mirrors what `PreparedAccountState::prepare` + `create_inner` do:
//!   1. Generate a fresh random P-256 rotation key
//!   2. Generate a fresh Ed25519 identity key
//!   3. Build + sign genesis op, derive DID
//!   4. Build + sign identity-update op with `prev = CID(genesis)`
//!   5. POST genesis to plc.directory
//!   6. POST identity-update to plc.directory
//!
//! Every step is printed; on rejection the response status + body are shown.
//!
//! Usage:
//!     cargo run -p app-core --bin plc-signup-test -- <server-url>
//!
//! `<server-url>` defaults to https://example.invalid and is only used as the
//! AvalancheHomeserver service endpoint inside the op — it isn't contacted.

use app_core::plc::{
    build_genesis_op, build_identity_update_op, derive_did, ensure_op_submitted, plc_op_cid,
    sign_plc_op, PlcOperation,
};
use app_core::recovery::generate_rotation_key;

#[tokio::main]
async fn main() {
    let server_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://example.invalid".to_string());

    // Fresh random rotation key — guarantees a fresh DID each run so we never
    // collide with a previous signup. Pass --deterministic to derive from a
    // fixed seed (reproduces the "same passkey twice" simulator failure).
    let deterministic = std::env::args().any(|a| a == "--deterministic");
    let (rot_priv, rot_pub) = if deterministic {
        app_core::recovery::derive_rotation_key_from_seed(&[7u8; 32])
    } else {
        generate_rotation_key()
    };
    // Fresh Ed25519 identity key (32 bytes of randomness — we don't need a
    // real libsignal IdentityKey for the PLC submission, just a key blob).
    let identity_pub: [u8; 32] = rand::random();

    println!("server_url            = {server_url}");
    println!("rotation_pub (hex)    = {}", hex(&rot_pub));
    println!("identity_pub (hex)    = {}", hex(&identity_pub));

    let genesis = build_genesis_op(&rot_pub, &server_url);
    let signed_genesis = sign_plc_op(&genesis, &rot_priv).expect("sign genesis");
    let did = derive_did(&signed_genesis).expect("derive did");
    let cid = plc_op_cid(&signed_genesis).expect("genesis cid");

    println!("did                   = {did}");
    println!("genesis cid           = {cid}");
    dump("signed_genesis", &signed_genesis);

    let update = build_identity_update_op(&rot_pub, &identity_pub, &server_url, &cid);
    let signed_update = sign_plc_op(&update, &rot_priv).expect("sign update");
    dump("signed_identity_update", &signed_update);

    println!("\n--- ensure genesis ---");
    match ensure_op_submitted(&did, &signed_genesis, 0).await {
        Ok(o) => println!("genesis: {o:?}"),
        Err(e) => {
            println!("genesis FAILED: {e}");
            std::process::exit(1);
        }
    }

    println!("\n--- ensure identity update ---");
    match ensure_op_submitted(&did, &signed_update, 1).await {
        Ok(o) => println!("identity update: {o:?}"),
        Err(e) => {
            println!("identity update FAILED: {e}");
            std::process::exit(1);
        }
    }

    println!("\nSuccess. Resolve at: https://plc.directory/{did}");
}

fn dump(label: &str, op: &PlcOperation) {
    let json = serde_json::to_string_pretty(op).unwrap();
    println!("\n{label} =\n{json}");
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
