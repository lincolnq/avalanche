# 13 — Federation

> **Status:** Proposed — client-side federation (below) is the agreed direction and needs project-owner review before implementation (it changes the wire protocol and adds endpoints). Nothing is built: there is no cross-server messaging today, and the `federation` crate is an empty stub. The earlier server-to-server design is Superseded.
> **Last verified against code:** 2026-10-03

## Summary

Federation means people on different homeservers can find each other, DM, and form
small groups. The proposal is that **servers never talk to each other for
messaging.** A client that wants to reach someone on another server connects to
*that* server directly and delivers the message there, sealed-sender, the same way
it already talks to each of its own servers. Homeservers stay simple single-org
servers with no peering, no server keys and no allowlists, and the sender's own
server learns nothing about cross-server traffic.

## Why federation at all

- No single operator can be coerced into severing the network; a seizure affects one
  community.
- Communities have incompatible operator requirements (jurisdiction, funding,
  trust). Each can self-host and still reach the others.
- Users can leave a bad operator.

## Current design — Built

- **Single-server messaging.** Each `AppCore` account talks to one homeserver. DMs
  and groups work between accounts on the same server; there is no route to a DID
  registered only elsewhere.
- **Multi-account is the cross-server mechanism today.** One identity can hold
  accounts on several servers (`53`); to talk to people on another server you join
  it. Group traffic is always local to the group's hosting server (`03` §3.12).
- **Invite links** use `https://go.theavalanche.net/i/<token>` (`51`).

## Proposed: client-side federation

### Delivery

To message Bob on `b.org`, Alice's client:
1. knows Bob's server from how it learned about Bob: an invite or contact QR, a
   shared contact card, a group's member data, or a move notice (below). No global
   lookup is needed for people you have a path to;
2. fetches Bob's prekey bundle from `b.org` (first contact) or uses the existing
   session;
3. delivers the encrypted message into Bob's queue on `b.org` as a **sealed-sender**
   delivery.

Alice's server `a.org` is not involved. Bob's devices receive it through their normal
connection to `b.org`.

### Who may deliver: delivery keys and first contact

Anonymous delivery needs an abuse gate. Copy Signal's *unidentified access*:

- **Contacts: delivery key.** Bob derives a delivery key from his profile key
  (`HKDF(profile_key, "delivery")`) and registers a hash of it with each of his
  servers. Anyone Bob has shared his profile key with — his accepted contacts — can
  compute it. A sealed delivery (and a prekey fetch) must present it. The server
  learns "someone holding Bob's delivery key", never who.
- **Strangers: identified first contact.** Without the key, the sender makes an
  *identified* delivery: the request is signed with the sender's identity key and
  names the sender. `b.org` verifies the signature, applies per-sender and per-IP
  rate limits, and the message lands as a message request (`12` §1). This is the one
  place a sender is revealed, and it's the case where the recipient sees an unknown
  sender anyway.
- **Consequence for profile keys.** The profile key now gates delivery, so it must
  go only to accepted contacts. Today it is attached even to delivery receipts sent
  to un-accepted requests (`52`, `09`); that has to stop before this ships.
- A server may refuse identified first-contact deliveries entirely (a closed
  community), so "doesn't federate" is just policy.

### Sender certificates

Sealed sender needs a sender certificate the recipient can validate. Each server
already mints certificates for its own users (`03` §3.11). The recipient's client
needs the sender's server trust root: it comes with the contact card or invite, or is
fetched from the sender's server on first contact and pinned. Open question: whether
to keep server-issued certificates or switch to certificates signed by the sender's
identity key (simpler, but loses server-vouched expiry).

### Moving servers

A user who changes servers sends a **move notice** — `{did, new_servers, issued_at}`
signed by their identity key — to every contact over existing sessions. Contacts
verify it against the identity key they already hold and update their route. The old
server can't block it. People without a session need a fresh invite or QR. If the
identity design keeps a public DID document (`50`), it is updated too, but routing
doesn't depend on it.

### What each party learns

| Party | Learns |
|---|---|
| Sender's server | Nothing about cross-server sends (the client goes direct). |
| Recipient's server | A sealed delivery for its user, from an IP. For stranger first contact: the sender's identity. |
| Relay | Unchanged: pseudonym wakeups. |
| Network observer | The sender's client connects to `b.org`. |

### Costs

- **The sender's IP reaches the foreign server**, the same exposure as connecting to
  any server. Tor or a VPN is the opt-out, consistent with the threat model.
- **Routing needs the recipient's current server.** Contacts learn it from move
  notices; strangers need a link.
- **Cross-server delivery policy can't rest on another server's vouching.** "Only
  members of my org can DM me" across servers becomes delivery keys plus message
  requests, enforced per user.
- **Clients connect to foreign servers** for sends and prekey fetches; failures are
  queued and retried like any send (`34`).
- **Abuse-report forwarding** has no server-to-server channel to ride (`12` §3,
  open conflict).

### Contract changes this needs

New server endpoints (unauthenticated sealed delivery with delivery key; identified
first-contact delivery; delivery-key registration; delivery-key-authorized prekey
fetch), a delivery key derived from the profile key, a move-notice message type, and
a stop to profile keys on request receipts. Cross-server casual groups (`03` §6)
become client fan-out over the same delivery path.

## Speculative: Project-to-Project federation

Some Projects want to share data across instances (a shared events calendar, a
movement-wide directory). `00` once committed the substrate to pub/sub and RPC between
Project instances. Under this model that's a Project concern: instances talk plain
HTTPS with their own auth, optionally using "Sign in with Avalanche" (`25`) for user
identity claims. The substrate doesn't provide it.

## Superseded: server-to-server multi-homing

The previous design (2026 drafts) had servers federate:

- Each DID had one **discovery server** (published in PLC) plus member servers.
- A client sent via its discovery server, which delivered locally or resolved the
  recipient's DID and **federated the ciphertext to the recipient's discovery server**.
  Clients learned shorter routes ("for C, route via X") from where messages arrived.
- Servers authenticated each other with Ed25519 keys at `.well-known/actnet-server`,
  with per-origin rate limits and reputation, plus attestation-based trust scoring
  (`12` §5).
- Prekey fetches for non-local recipients were proxied server to server.

**Why superseded:**
- The sender's server saw every cross-server recipient. Seize it and you get the
  user's cross-server social graph.
- It needed a lot of machinery: server keys, origin auth, allowlists, trust scoring,
  learned routes, DID-resolution caching. Every piece was a surface where metadata
  could leak, and it all went against the threat model.
- Server-to-server trust is exactly what activist operators can't evaluate.

**Kept from it:**
- Same-community conversations never leave their server.
- Per-server prekey bundles.
- Migration authority is the user's signed record, never the old server's say-so.
- Join flows show what the new server will see before you join.
- Adding a contact never prompts you to join their server.

## Rationale and rejected alternatives

- **Client-side over server-to-server federation** — above.
- **Full ATProto federation** — public-by-default, no E2E (`00`).
- **Matrix-style room replication across servers** — heavy metadata replication and
  state resolution; we keep group state on one server instead (`03`).
- **Multiple discovery servers per DID** (old design) — moot under the proposal.
