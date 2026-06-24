---
title: "Privacy Policy"
description: "Privacy policy for av.theavalanche.net, a non-production Avalanche demonstration server."
---

> **This is a non-production demonstration server.** av.theavalanche.net exists
> for testing and evaluation of the Avalanche software. Do not use it for
> sensitive communications. Accounts and data may be reset or deleted at any
> time without notice, and the service is provided with no availability or
> retention guarantees.

**Service:** av.theavalanche.net (an Avalanche homeserver)  
**Operated by:** the Avalanche project maintainers, based in the United States  
**Effective date:** June 23, 2026  
**Contact:** [GitHub issues](https://github.com/lincolnq/avalanche/issues)

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
  service.** Your identity is a cryptographic key that you control.
- **We can see limited delivery metadata** — who is sending to whom in direct
  messages, message timing, and message size — because we have to route and
  deliver messages. We do not sell, rent, or share this for advertising.
- **We keep undelivered messages only briefly** and delete them automatically.
- **We do not run ads or trackers** and we do not profile you for marketing.
- **Optional add-on services ("Projects") and automated participants ("bots")
  can receive information** — but only what you choose to share with them or
  send while they are present.

## 1. Information we store

### Account and identity
When you register, we store:
- Your **decentralized identifier (DID)** and its public key material. This is
  your portable identity; it is not tied to a phone number or email.
- One or more **device records** for your account, plus the **public keys and
  authentication tokens** needed so your devices can sign in and so other people
  can start an encrypted conversation with you. We only ever hold *public* key
  material — the private keys that decrypt your messages never leave your device.
- An optional **encrypted recovery blob**, which we store as opaque ciphertext we
  cannot read.

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
- Our servers may keep short-lived operational logs (for example, connection
  errors and authentication failures) for security and reliability. We retain
  these for no more than 30 days and do not use them to build profiles of users.

## 2. What we cannot see

Because of the end-to-end encryption built into the software, this server — and
anyone who compromised or seized it — **cannot** obtain:
- the **content** of any message or attachment;
- the **plaintext of your profile** (display name, avatar, bio);
- the **membership lists of groups** (we may see that a group exists and its
  approximate size, but not who is in it);
- your **contact list**, which is stored only on your own devices.

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

- The push relay maps a rotating, opaque pseudonym to your device. It does not receive your identity, your messages, or other server information beyond what is needed to send a wake-up ping.
- The notifications we send are **empty** — they tell your device to
  fetch new data itself.
- Push pseudonyms rotate periodically and stale entries expire automatically.

Apple and Google process this data under their own privacy policies.

## 5. Abuse, moderation, and enforcement

To keep the service usable, we operate abuse controls. Importantly, **abuse
reports do not contain message content**, and we cannot read messages to
investigate them. Reports identify the reported account, the reporting server,
and a reason category.

Based on content-free signals (such as the number of distinct servers reporting
an account and sending-rate metrics), we may rate-limit, suspend, or ban an
account. As a demonstration server, av.theavalanche.net does not currently run
automated enforcement; we may rate-limit or suspend accounts that abuse the
service at our discretion.

You can block other users; your block list is stored encrypted and synced across
your own devices.

## 6. Projects and bots

The app may offer optional add-on services called **Projects** (that open in a window inside the app) and automated participants called **bots**. These may be operated by us, by the people who administer this server, or by third parties. **This policy does not govern what a Project or bot does with information you give it** — those services have their own data practices, and we are not responsible for third-party ones.

Principles governing Projects and bots:

- **You decide what you share with a Project.** A Project cannot silently reach
  any of your information. It receives only the information you choose to give it — what you type or send into it, and the
  specific details (such as your identifier or profile) that you agree to share
  when you use it. A Project may collect information about your interaction with
  it, so treat anything you put into a Project the way you would treat using any
  other website or app.

- **A bot can read the messages in a conversation it is part of.** A bot is a
  full participant, like any other member of a chat. Messages remain end-to-end
  encrypted in transit, but a bot you are talking to — or one that has been added
  to a group you are in — is a legitimate recipient and can read what is sent in
  that conversation, just as a human participant can. If you do not want a bot to receive your messages, do not send them in a conversation that includes one.

(This is separate from what *we, the server,* can see: as described in Section 2,
we still cannot read your message content. A bot reads messages because it is an
endpoint in the conversation, not because the server decrypts them.)

## 7. Data retention

- **Undelivered messages:** deleted automatically after 30 days (the software
  default), or sooner if a conversation sets a shorter disappearing-message
  timer. We cannot extend a conversation's timer.
- **Delivered messages:** not retained on the server.
- **Account and key material:** retained while your account is active.
- **Rate-limit counters:** cleared on a rolling basis (typically within hours).
- **Logs:** retained for no more than 30 days.
- **Backups:** as a non-production demonstration server, data is not guaranteed
  to be backed up and may be reset or deleted at any time without notice.

## 8. Your rights and choices

Depending on where you live, you may have rights to access, correct, export, or
delete your personal data (for example, under the GDPR or CCPA). Because your
messages and profile are encrypted and most of your data lives on your own
device, the personal data we actually hold is limited (see Section 1).

- **Deletion:** You can request deletion of your account on this server. When you
  delete your account, we remove your account records and key material from this
  server. Messages already delivered to other people's devices, and data held by other servers you have communicated with, are outside our control.
- **Access / export:** You may request a copy of the data we hold that is
  associated with your account.

To exercise any of these rights, contact us via our
[GitHub issues page](https://github.com/lincolnq/avalanche/issues).

## 9. Law enforcement and legal requests

We respond to valid legal requests as required by the law of the United States.
We review each request for validity and scope and respond only as required by
law; because this is a demonstration server holding minimal data, in most cases
we have little or nothing to provide. We can only provide data we actually hold
(see Sections 1 and 2): we **cannot** provide message content, profile contents,
or group membership lists, because we do not have access to them.

## 10. Security

We take reasonable technical and organizational measures to protect the data we
hold. No system is perfectly secure. If we become aware of a breach affecting
your personal data, we will notify affected users and relevant authorities as
required by applicable law.

## 11. Children

This service is not directed to children under the age required by the law of
the United States (under 13). We do not knowingly collect personal data from
children below that age.

## 12. Changes to this policy

We may update this policy from time to time. We will post the updated version
with a new effective date and, where required by law, notify users of material
changes.

## 13. Contact

Questions about this policy or your data: please open an issue on our
[GitHub issues page](https://github.com/lincolnq/avalanche/issues).

---

*This server runs the open-source [Avalanche](https://theavalanche.net/) messaging
software. The privacy protections described here are properties of that software's
design; this policy describes how av.theavalanche.net operates this particular
server.*
</content>
</invoke>
