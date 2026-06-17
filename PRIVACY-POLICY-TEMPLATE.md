<!--
=============================================================================
OPERATOR NOTE — READ BEFORE PUBLISHING (delete this whole comment block once done)
=============================================================================

This is a DEFAULT privacy policy for anyone running a homeserver built on the
Avalanche codebase. It is written to be accurate to how the software behaves and
to be strong on privacy by default. You may modify it, but do not weaken a claim
unless your deployment actually behaves differently — false statements about data
handling create legal liability.

1. Fill in every {{PLACEHOLDER}}. At minimum:
   {{OPERATOR_NAME}}, {{OPERATOR_LEGAL_ENTITY}}, {{JURISDICTION}},
   {{CONTACT}} (an email or URL — the GitHub issues page is acceptable),
   {{EFFECTIVE_DATE}}, {{MESSAGE_RETENTION_DAYS}} (software default: 30),
   {{LOG_RETENTION}}, {{BACKUP_POLICY}}, {{LE_PROCESS}}.

2. Verify against YOUR deployment. Several privacy-protective features are
   stage-gated in the codebase. Confirm what your build actually ships before
   relying on the corresponding sentence below, and edit if needed:
     - End-to-end encryption of 1:1 messages — core, on by default.
     - Encrypted profiles (name/avatar/bio) — confirm your build encrypts these.
     - Group sealed-sender / membership opacity — confirm if you run groups.
     - Federation — only relevant once you federate with other servers; the
       "Federation" section can be removed if you run a closed, single server.
     - Projects and bots — remove the "Projects and bots" section if your
       server offers neither. If it does, that section is accurate as written:
       Projects are sandboxed and bots are always-visible participants.
     - Abuse/moderation enforcement ladder — the thresholds quoted are the
       software's defaults; change them to match your actual operator policy,
       or soften to "may" if you have not configured automated enforcement.

3. You are the data controller for your server. Local data-protection law
   (GDPR, CCPA, etc.) applies to you, not to the Avalanche project. Have a
   lawyer review this before publishing if you serve users in a regulated
   jurisdiction.
=============================================================================
-->

# Privacy Policy

**Service:** {{OPERATOR_NAME}} (an Avalanche homeserver)
**Operated by:** {{OPERATOR_LEGAL_ENTITY}}, based in {{JURISDICTION}}
**Effective date:** {{EFFECTIVE_DATE}}
**Contact:** {{CONTACT}}

This server runs the open-source Avalanche messaging software. We, the operator
named above, are responsible for this specific server and the data it holds. We
are not responsible for other servers in the network, which are run by other
operators under their own policies.

This policy explains what we can and cannot see, what we store, and for how long.
Where we say we **cannot** see something, that is a property of the encryption —
not a promise we could choose to break. Where we say we **do** see something, it
is metadata the service needs to deliver your messages.

## The short version

- **We cannot read your messages.** They are end-to-end encrypted on your device
  before they reach us. We only ever hold unreadable ciphertext.
- **We do not collect a phone number, email address, or real name to use the
  service.** Your identity is a cryptographic key (a "DID") that you control.
- **We can see limited delivery metadata** — who is sending to whom in direct
  messages, message timing, and message size — because we have to route and
  deliver messages. We do not sell, rent, or share this for advertising.
- **We keep undelivered messages only briefly** and delete them automatically.
- **We do not run ads or trackers** and we do not profile you for marketing.
- **Optional add-on services ("Projects") and automated participants ("bots")
  can receive information** — but only what you choose to share with them or
  send while they are present. Bots are never hidden (see Section 7).

## 1. Information we store

### Account and identity
When you register, we store:
- Your **decentralized identifier (DID)** and its public key material. This is
  your portable identity; it is not tied to a phone number or email.
- One or more **device records** for your account — a device identifier, a
  registration identifier, and the **public** half of each device's identity key.
- **Public prekey bundles** (signed prekey, one-time prekeys, post-quantum
  prekey) — public key material only — used so others can start an encrypted
  session with you. One-time prekeys are deleted as they are used.
- An optional **encrypted recovery blob**, which we store as opaque ciphertext we
  cannot read.

We never receive the private halves of your keys; those stay encrypted on your
device.

### Messages
- We store **only the encrypted ciphertext** of messages that are waiting to be
  delivered to a recipient who is currently offline. We cannot decrypt it.
- To route a **direct message**, we necessarily process the sender's and
  recipient's DIDs, the destination device(s), a timestamp, and the size of the
  ciphertext.
- For **group messages**, the sender is concealed from us by design — we see an
  encrypted, opaque member reference rather than the sender's DID.
- We do **not** store message content after delivery, and we delete undelivered
  messages automatically (see *Retention*, below).

### Profile
Your display name, avatar, and bio are **encrypted with a key only you and the
people you share it with hold**. We store the encrypted blob and a version
number. We cannot read your profile, and neither can anyone you have not shared
your profile key with.

### Technical and security data
- We may record limited **registration metadata** (such as the IP address and
  time of signup) to detect fraudulent or automated signups.
- We keep **rate-limiting counters** to prevent abuse; these are content-free and
  cleared automatically.
- Our servers may keep short-lived operational logs (for example, connection
  errors and authentication failures) for security and reliability. We retain
  these for {{LOG_RETENTION}} and do not use them to build profiles of users.

## 2. What we cannot see

Because of the end-to-end encryption built into the software, this server — and
anyone who compromised or seized it — **cannot** obtain:
- the **content** of any message or attachment;
- the **plaintext of your profile** (display name, avatar, bio);
- the **membership lists of groups** (we may see that a group exists and its
  approximate size, but not who is in it);
- your **contact list / social graph**, which is stored only on your own devices
  and is never uploaded to us in readable form.

A seizure of this server would yield encrypted blobs and the list of DIDs
registered here — not your conversations, your contacts, or your real identity.

## 3. How we use information

We use the limited information above only to:
- operate the service: register accounts, deliver messages, distribute public
  keys, and notify your device of new messages;
- keep the service secure and available: prevent spam, abuse, and fraud, and
  diagnose technical problems.

We do **not** use your information for advertising, and we do **not** sell or
rent it to anyone.

## 4. Push notifications and third parties

To wake your app when a message is waiting, we use a push relay together with
Apple Push Notification service (APNs) and/or Google's Firebase Cloud Messaging
(FCM).

- The push relay maps a rotating, **opaque pseudonym** to your device's push
  token. It does **not** receive your identity, your messages, or which server
  you use beyond what is needed to send a wake-up ping.
- The notifications we send are **silent/empty** — they tell your device to
  fetch new data itself. **Apple and Google see only that your app was pinged**,
  not who messaged you or what was said.
- Push pseudonyms rotate periodically and stale entries expire automatically.

Apple and Google process this data under their own privacy policies.

## 5. Abuse, moderation, and enforcement

To keep the service usable, we operate abuse controls. Importantly, **abuse
reports do not contain message content**, and we cannot read messages to
investigate them. Reports identify the reported account, the reporting server,
and a reason category.

Based on content-free signals (such as the number of distinct servers reporting
an account and sending-rate metrics), we may rate-limit, suspend, or ban an
account. {{OPERATOR_NAME}}'s current thresholds and process are: {{ABUSE_POLICY}}.
We do not perform algorithmic scanning of message content — we cannot.

You can block other users; your block list is stored encrypted and synced across
your own devices.

## 6. Federation with other servers

<!-- Remove this section if you operate a closed, single server that does not federate. -->

This server may exchange messages with other servers in the network so that you
can communicate with people who registered elsewhere. When that happens, the
**content stays end-to-end encrypted**, but the other server necessarily learns
routing metadata for those conversations (such as the DIDs involved and timing).
No single server, including ours, holds the full picture of your activity across
the network.

## 7. Projects and bots

<!-- Remove this section if your server offers neither Projects nor bots. -->

The app may offer optional add-on services called **Projects** (for example,
mini-apps that open in a window inside the app) and automated participants called
**bots**. These may be operated by us, by the people who administer this server,
or by third parties. **This policy does not govern what a Project or bot does
with information you give it** — those services have their own data practices,
and we are not responsible for third-party ones.

Two things are true by design, and worth understanding:

- **You decide what you share with a Project.** A Project cannot silently reach
  your messages, contacts, keys, or stored profile. It receives only the
  information you choose to give it — what you type or send into it, and the
  specific details (such as your identifier or profile) that you agree to share
  when you use it. A Project may collect information about your interaction with
  it, so treat anything you put into a Project the way you would treat using any
  other website or app.

- **A bot can read the messages in a conversation it is part of.** A bot is a
  full participant, like any other member of a chat. Messages remain end-to-end
  encrypted in transit, but a bot you are talking to — or one that has been added
  to a group you are in — is a legitimate recipient and can read what is sent in
  that conversation, just as a human participant can. **Bots are never hidden:**
  a bot's presence is always visible to everyone in the conversation, and there
  is no silent-observer mode. If you do not want a bot to receive your messages,
  do not send them in a conversation that includes one.

(This is separate from what *we, the server,* can see: as described in Section 2,
we still cannot read your message content. A bot reads messages because it is an
endpoint in the conversation, not because the server decrypts them.)

## 8. Data retention

- **Undelivered messages:** deleted automatically after {{MESSAGE_RETENTION_DAYS}}
  days (the software default is 30 days), or sooner if a conversation sets a
  shorter disappearing-message timer. We cannot extend a conversation's timer.
- **Delivered messages:** not retained on the server.
- **Account and key material:** retained while your account is active.
- **Rate-limit counters:** cleared on a rolling basis (typically within hours).
- **Logs:** retained for {{LOG_RETENTION}}.
- **Backups:** {{BACKUP_POLICY}}.

## 9. Your rights and choices

Your DID is portable — your identity is not locked to this server.

Depending on where you live, you may have rights to access, correct, export, or
delete your personal data (for example, under the GDPR or CCPA). Because your
messages and profile are encrypted and most of your data lives on your own
device, the personal data we actually hold is limited (see Section 1).

- **Deletion:** You can request deletion of your account on this server. When you
  delete your account, we remove your account records and key material from this
  server. Your DID can be tombstoned in the public directory. Messages already
  delivered to other people's devices, and data held by other servers you have
  communicated with, are outside our control.
- **Access / export:** You may request a copy of the data we hold that is
  associated with your account.

To exercise any of these rights, contact us at {{CONTACT}}.

## 10. Law enforcement and legal requests

We respond to valid legal requests as required by the law of {{JURISDICTION}}.
Our process is: {{LE_PROCESS}}. We can only provide data we actually hold (see
Sections 1 and 2): we **cannot** provide message content, profile contents, or
group membership lists, because we do not have access to them. We have no
backdoor and the software is designed so that we cannot add one without users
being able to detect it.

## 11. Security

We take reasonable technical and organizational measures to protect the data we
hold. No system is perfectly secure. If we become aware of a breach affecting
your personal data, we will notify affected users and relevant authorities as
required by applicable law{{BREACH_TIMELINE}}.

## 12. Children

This service is not directed to children under the age required by the law of
{{JURISDICTION}} (for example, under 13 in the United States, or the applicable
age in your country). We do not knowingly collect personal data from children
below that age.

## 13. Changes to this policy

We may update this policy from time to time. We will post the updated version
with a new effective date and, where required by law, notify users of material
changes.

## 14. Contact

Questions about this policy or your data: {{CONTACT}}.

---

*This server runs the open-source [Avalanche](#) messaging software. The privacy
protections described here are properties of that software's design; this policy
describes how {{OPERATOR_NAME}} operates this particular server.*
