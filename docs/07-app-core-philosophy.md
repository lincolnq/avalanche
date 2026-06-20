# 07 — app-core Philosophy

The App Core is the shared Rust core that is behind every Avalanche client. It backs both bots and the mobile apps.

The app core is responsible for:
- connecting to a home server
- owning security: tracking all the crypto state, double ratchets, and so on for all of your conversations. Client code only has to deal with plaintext.
- maintaining a websocket connection to the server and letting you know when incoming messages and other events arrive
- managing the SQL Cipher store that contains all of the crypto, all of the messages, all of your contacts, and basically anything else that an app or a bot would need to store locally and securely. Synchronizing that info with other devices on your account.
- providing an API for bots and apps to do all of the above. In Avalanche, bots are first-class citizens and can do everything users can do.

As of this writing, there is one app core per server connection from a device. If you have multiple accounts, you have multiple app cores. Eventually, perhaps there will be one app core per identity, and each app core will have multiple server connections (if that identity exists on multiple servers).

**Minimal default storage:** Another important point about app-core is that it does not automatically store non-essential information. Specifically, while it does **provide** a message store and a contact store, it **must be told to explicitly store** message content or contacts. (It does automatically store and track cryptographic state and ephemeral state.) You, the user, have to opt in to storing anything non-essential, by e.g. calling `save_message`.

# API sketch

The App Core exposes a Rust and a TypeScript API, both very _async-oriented_: many functions block to do network traffic and some will block indefinitely until an event arrives. 

To give you a sense of how app-core is used, we'll highlight below some of the most important methods exposed on the App Core. Not all methods are listed here; check the api documentation for up-to-date info.

## Account & identity lifecycle

Creating, logging into, and recovering an account on a homeserver.

| Function | Returns |
|---|---|
| `create_account(server_url, db_path, db_key, prf_output: bytes, display_name, invite_token?)` | `AppCore` |
| `login_or_create_bot(server_url, db_path, db_key, display_name, did_suffix?, invite_token?)` | `AppCore` |
| `recover_from_blob(server_url, did, prf_output: bytes, db_path, db_key, display_name)` | `AppCore` |

## Connection & background tasks

Bringing the core online and keeping it there, and observing link health.

| Function | Returns |
|---|---|
| `start_reconnect_task()` | `()` |
| `wait_for_connection_state_change(last: ConnectionState)` | `ConnectionState` |

## Receiving

A single event stream the client drains, surfacing decrypted messages, delivery receipts, edits/reactions, group invites, and sync notifications.

| Function | Returns |
|---|---|
| `next_events()` | `[IncomingEvent]` |

## Sending

The unified message-send entry point plus the per-kind variants and conversation controls.

| Function | Returns |
|---|---|
| `send_message(target: MessageTarget, plaintext: bytes, sent_at_ms: int)` | `()` |
| `send_reaction(target: MessageTarget, target_author, target_sent_at_ms: int, emoji, remove: bool, sent_at_ms: int)` | `()` |
| `send_edit(target: MessageTarget, target_sent_at_ms: int, new_body, sent_at_ms: int)` | `()` |

## Local history

Reading and writing the client's view of conversations against the encrypted store.

| Function | Returns |
|---|---|
| `save_message(msg: StoredMessage)` | `()` |
| `load_conversations()` | `[ConversationSummary]` |
| `load_messages(conversation_id)` | `[StoredMessage]` |
| `mark_messages_read(conversation_id, up_to_sent_at_ms: int)` | `int` |

## Contacts & profiles

Contact curation, the message-request gate, and display-name resolution.

| Function | Returns |
|---|---|
| `list_contacts()` | `[ContactRow]` |
| `touch_contact(did, curated: bool)` | `()` |
| `set_display_name(display_name)` | `()` |
| `contact_display_name(did)` | `String` |
| `get_account_info(did)` | `AccountInfo` |

## Groups

The group lifecycle.

| Function | Returns |
|---|---|
| `create_group(title, description, expiry_seconds: int)` | `CreatedGroup` |
| `invite_member(group_id, recipient_did, role: int)` | `()` |
| `fetch_group_state(group_id)` | `GroupSummary` |

## Push & Projects

Notification registration and Projects integration.

| Function | Returns |
|---|---|
| `register_push_token(device_token, platform, relay_url, environment)` | `()` |
| `fetch_projects()` | `[ProjectInfo]` |
| `request_project_token(project_url)` | `String` |
