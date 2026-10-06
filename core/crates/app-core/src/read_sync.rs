//! Read-state sync to my own other devices (docs/04 §5.4, docs/31).
//!
//! `mark_messages_read` records a pending read mark here (never touching the
//! network, so marking stays fast and lock-free); a background task coalesces
//! a burst of marks into one `SyncRead` and sends it as a self-DM, which fans
//! out to my other devices only. The receive side is `messaging::apply_sync_read`.
//!
//! Marks are high-water marks ("everything sent at/below T is read"), so
//! merging keeps the max and a retry after a failed send is idempotent.
//! Pending marks are in memory only: a mark not yet sent when the process
//! exits simply doesn't sync (best-effort, like the Sent transcript).

use std::collections::HashMap;
use std::sync::Weak;
use std::time::Duration;

use prost::Message as _;
use types::Timestamp;

use crate::error::AppError;
use crate::groups;
use crate::proto::{content_message::Body, read_mark, ContentMessage, ReadMark, SyncRead};
use crate::AppCore;

/// How long to wait after the first queued mark before sending, so a scroll
/// (one mark per visibility change) coalesces into a single `SyncRead`.
const DEBOUNCE: Duration = Duration::from_secs(1);
/// Retry backoff after a failed send (e.g. offline), doubling up to the max.
const RETRY_INITIAL: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(300);
/// Idle park bound, so the task re-checks `weak` and exits after the last
/// `Arc<AppCore>` drops.
const IDLE_RECHECK: Duration = Duration::from_secs(60);

/// Record that `conversation_id` is read up to `up_to` (keeping the highest
/// mark per conversation).
pub(crate) fn merge_mark(pending: &mut HashMap<String, u64>, conversation_id: &str, up_to: u64) {
    let entry = pending.entry(conversation_id.to_string()).or_insert(up_to);
    *entry = (*entry).max(up_to);
}

/// The wire `ReadMark` for a local conversation id — the inverse of the
/// mapping in `messaging::apply_sync_read`. `dm-{me}-{peer}` names the peer
/// (note-to-self names me); `group-{b64}` carries the raw group id bytes.
/// `None` for an id that isn't one of mine or doesn't parse.
pub(crate) fn read_mark_for_conversation(
    my_did: &str,
    conversation_id: &str,
    up_to: u64,
) -> Option<ReadMark> {
    let conversation = if let Some(peer) = conversation_id
        .strip_prefix("dm-")
        .and_then(|rest| rest.strip_prefix(my_did))
        .and_then(|rest| rest.strip_prefix('-'))
    {
        if peer.is_empty() {
            return None;
        }
        read_mark::Conversation::PeerDid(peer.to_string())
    } else if let Some(gid) = conversation_id.strip_prefix("group-") {
        read_mark::Conversation::GroupId(groups::b64d(gid).ok()?)
    } else {
        return None;
    };
    Some(ReadMark {
        conversation: Some(conversation),
        up_to_timestamp: up_to,
    })
}

/// Build one `SyncRead` covering every pending mark. Unmappable ids are
/// dropped (logged) rather than retried forever.
pub(crate) fn build_sync_read(my_did: &str, pending: &HashMap<String, u64>) -> SyncRead {
    let marks = pending
        .iter()
        .filter_map(|(conv_id, &up_to)| {
            let mark = read_mark_for_conversation(my_did, conv_id, up_to);
            if mark.is_none() {
                tracing::warn!("[sync] no read mark for conversation {conv_id}; dropping");
            }
            mark
        })
        .collect();
    SyncRead { marks }
}

/// Send `pending` to my other devices as one `SyncRead`. Holds `inner` across
/// the send — the crypto-send carve-out in `core/CLAUDE.md`; this runs on the
/// background task, never on a UI path.
async fn send_read_marks(core: &AppCore, pending: &HashMap<String, u64>) -> Result<(), AppError> {
    let sync = build_sync_read(&core.did, pending);
    if sync.marks.is_empty() {
        return Ok(());
    }
    let env = ContentMessage {
        body: Some(Body::SyncRead(sync)),
        timestamp_ms: Timestamp::now().as_millis() as u64,
        profile_key: Vec::new(),
        expire_timer_secs: 0,
    };
    let ws = core.ws.lock().expect("ws mutex poisoned").clone();
    let mut inner = core.inner.lock().await;
    let own = inner.did.clone();
    inner.send_dm(ws.as_ref(), &own, &env.encode_to_vec(), None).await
}

/// Background sender: park until a mark is queued, wait out the debounce,
/// drain everything pending into one `SyncRead`, and send it. On failure the
/// marks are merged back and retried with capped backoff. Self-exits when the
/// last `Arc<AppCore>` drops.
pub(crate) async fn read_sync_loop(weak: Weak<AppCore>) {
    let mut backoff = RETRY_INITIAL;
    loop {
        {
            // Never hold the `Arc` across an idle wait, so AppCore can drop.
            let Some(core) = weak.upgrade() else { return };
            if core.pending_read_marks.lock().unwrap().is_empty() {
                let notify = core.read_sync_notify.clone();
                drop(core);
                let _ = tokio::time::timeout(IDLE_RECHECK, notify.notified()).await;
                continue;
            }
        }
        tokio::time::sleep(DEBOUNCE).await;

        let Some(core) = weak.upgrade() else { return };
        let marks = std::mem::take(&mut *core.pending_read_marks.lock().unwrap());
        if marks.is_empty() {
            continue;
        }
        match send_read_marks(&core, &marks).await {
            Ok(()) => backoff = RETRY_INITIAL,
            Err(e) => {
                tracing::warn!("[sync] SyncRead to own devices failed: {e}");
                {
                    let mut pending = core.pending_read_marks.lock().unwrap();
                    for (conv_id, up_to) in marks {
                        merge_mark(&mut pending, &conv_id, up_to);
                    }
                }
                drop(core);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(RETRY_MAX);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::apply_sync_read;

    const ME: &str = "did:plc:me";

    #[test]
    fn dm_mark_names_the_peer() {
        let mark = read_mark_for_conversation(ME, "dm-did:plc:me-did:plc:bob", 42).unwrap();
        assert_eq!(
            mark.conversation,
            Some(read_mark::Conversation::PeerDid("did:plc:bob".into()))
        );
        assert_eq!(mark.up_to_timestamp, 42);
    }

    #[test]
    fn note_to_self_mark_names_me() {
        let mark = read_mark_for_conversation(ME, "dm-did:plc:me-did:plc:me", 1).unwrap();
        assert_eq!(mark.conversation, Some(read_mark::Conversation::PeerDid(ME.into())));
    }

    #[test]
    fn group_mark_round_trips_group_id_bytes() {
        let gid = vec![7u8; 32];
        let conv = format!("group-{}", groups::b64(&gid));
        let mark = read_mark_for_conversation(ME, &conv, 9).unwrap();
        assert_eq!(mark.conversation, Some(read_mark::Conversation::GroupId(gid)));
    }

    #[test]
    fn unmappable_ids_yield_no_mark() {
        // Another identity's DM, a malformed DM id, junk group b64, unknown kind.
        assert!(read_mark_for_conversation(ME, "dm-did:plc:other-did:plc:bob", 1).is_none());
        assert!(read_mark_for_conversation(ME, "dm-did:plc:me-", 1).is_none());
        assert!(read_mark_for_conversation(ME, "group-!!!", 1).is_none());
        assert!(read_mark_for_conversation(ME, "thread-x", 1).is_none());
    }

    #[test]
    fn merge_keeps_highest_mark_per_conversation() {
        let mut pending = HashMap::new();
        merge_mark(&mut pending, "a", 10);
        merge_mark(&mut pending, "a", 5);
        merge_mark(&mut pending, "b", 3);
        merge_mark(&mut pending, "a", 12);
        assert_eq!(pending.get("a"), Some(&12));
        assert_eq!(pending.get("b"), Some(&3));
    }

    #[test]
    fn build_drops_unmappable_marks() {
        let mut pending = HashMap::new();
        merge_mark(&mut pending, "dm-did:plc:me-did:plc:bob", 1);
        merge_mark(&mut pending, "bogus", 1);
        assert_eq!(build_sync_read(ME, &pending).marks.len(), 1);
    }

    async fn save_unread(store: &store::DeviceStore, id: &str, conv: &str, sender: &str, sent_at: i64) {
        store
            .save_message(&store::messages::HistoryMessage {
                id: id.into(),
                conversation_id: conv.into(),
                sender_did: sender.into(),
                body: "hi".into(),
                sent_at: Timestamp(sent_at),
                edited_at: None,
                read_at: None,
                delivery_status: 2,
                edit_count: 0,
                deleted_at: None,
                kind: 0,
                metadata: None,
                expire_timer_secs: 0,
                expire_at: None,
            })
            .await
            .unwrap();
    }

    /// What one device builds, another device of the same identity applies:
    /// DM and group marks clear exactly the messages at/below the mark, and
    /// re-applying the same mark changes nothing (so it can't echo back).
    #[tokio::test]
    async fn built_sync_read_applies_on_another_device() {
        let other_device = store::DeviceStore::open_in_memory().await.unwrap();
        let bob = "did:plc:bob";
        let dm = format!("dm-{ME}-{bob}");
        let group = format!("group-{}", groups::b64(&[3u8; 32]));
        save_unread(&other_device, "d1", &dm, bob, 100).await;
        save_unread(&other_device, "d2", &dm, bob, 300).await; // after the mark
        save_unread(&other_device, "g1", &group, bob, 100).await;

        let mut pending = HashMap::new();
        merge_mark(&mut pending, &dm, 200);
        merge_mark(&mut pending, &group, 200);
        let sync = build_sync_read(ME, &pending);
        // Survives the wire.
        let sync = SyncRead::decode(sync.encode_to_vec().as_slice()).unwrap();

        let mut changed = apply_sync_read(&other_device, ME, sync.clone()).await;
        changed.sort();
        let mut expected = vec![dm.clone(), group.clone()];
        expected.sort();
        assert_eq!(changed, expected);
        assert_eq!(other_device.unread_count(&dm, ME).await.unwrap(), 1);
        assert_eq!(other_device.unread_count(&group, ME).await.unwrap(), 0);

        assert!(apply_sync_read(&other_device, ME, sync).await.is_empty());
    }
}
