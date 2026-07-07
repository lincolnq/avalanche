---
title: "Authentication"
date: 2026-07-05
description: "How to know who your users are"
---

This document describes how Avalanche projects receive information about user identity.

## DIDs

A DID (decentralized identifier) is a string stably identifying a user in Avalanche, potentially across multiple servers. It looks like `did:plc:3fbalqfxvbnmgonoe7hldui6`.

DIDs have different subtypes. For Avalanche, the subtypes we use are `plc` and `local`:

* PLC (Public Ledger of Credentials) DIDs are publicly listed on a server hosted by [Bluesky Social PBC](https://github.com/did-method-plc/did-method-plc). Real-life users should usually (may not *always*) have a PLC DID.
* Local DIDs are reachable only on their homeserver, and should not leave their homeserver. We usually use local DIDs for bots, and sometimes for test users.

Your project should avoid displaying DIDs to end users (except in debug menus, settings pages and such); they are an implementation detail rarely useful to the end user.

## Profiles

A user's display name (and eventually profile picture, bio and anything else that's attached to a user) are stored together in an encrypted *profile bundle*. If you know a user's DID,[^homeserver] you can request that user's encrypted profile bundle.

To decrypt their profile and use their display name/picture/etc, you'll also need the user's *profile key*. The profile key is attached to each message that the user sends in a group or DM.[^profkey]


[^homeserver]: You also need to know which server the user is on (but if you're building projects then you can assume they're on the same homeserver you're developing for).

[^profkey]: It may be necessary for the substrate to add new sharing methods for the profile key, such as when a user authenticates to a project; we haven't specced this out yet but it is a reasonable future extension. Let us know if you need this.


## Authentication via the Network tab

Projects are hosted on a separate URL from the Avalanche server. When your user clicks on your project in the Network tab of the app, the user's client will open your project's URL with a token appended as a query parameter:

```
https://your-project.example.com/?token=iH8L2...tQ
```

Extract the token, then contact the server to validate the token and get the user's identity:

```
GET https://your-homeserver.example.com/v1/project-token/verify?token=iH8L2...tQ
```

```json
{
  "did": "did:plc:3fbalqfxvbnmgonoe7hldui6", 
  /*...other fields...*/
}
```

## Authentication via OAuth

You don't need your user to go through the Network tab. You can also have the user go through a 'Sign in with Avalanche'-type flow. This has two branches:

1) The user is on a device they're logged into Avalanche with. In this case, the Avalanche app will appear with a consent dialog. The user will authorize the app.

2) The user is on a different device. In this case the user will see a QR code and will be prompted to scan the code to authorize on their phone. Similarly, a consent dialog will appear when they scan the code, and then once they approve, the login process will continue.

This is standard OAuth 2.0, so you can use an off-the-shelf OAuth client library. The access token you receive when the flow completes is just a project token, so you resolve the user's DID with the exact same `GET /v1/project-token/verify` call [described above](#authentication-via-the-network-tab).

To use the OAuth flow, your project must be registered as an OAuth client with the homeserver admin at install time.

### Branch detection

You usually can't reliably detect whether the user has the app installed. You don't have to guess perfectly, just pick a sensible default and always offer the other branch as a fallback:

- On mobile web, default to Branch 1. A universal link only opens the app when it's actually installed; if it isn't, the tap just lands on a normal web page, so offer a "sign in from another device" link that switches to Branch 2.
- On desktop, default to Branch 2, showing the QR code. Offer a "I have Avalanche installed on this device" link that switches to Branch 1.

### Branch 1: Same device

Generate a PKCE `code_verifier` and its `code_challenge` plus a random `state`, then send the user to the authorize link. It opens the app:

```
https://go.theavalanche.net/authorize
  ?client_id=YOUR_CLIENT_ID
  &server_url=https://your-homeserver.example.com
  &redirect_uri=https://your-project.example.com/callback
  &code_challenge=BASE64URL_SHA256_OF_VERIFIER
  &code_challenge_method=S256
  &state=RANDOM_STATE
```

After the user consents, the app opens your `redirect_uri` with the authorization code:

```
https://your-project.example.com/callback?code=AUTH_CODE&state=RANDOM_STATE
```

Verify `state` matches, then exchange the code for a token (form-encoded, server-to-server):

```
POST https://your-homeserver.example.com/v1/oauth/token
Content-Type: application/x-www-form-urlencoded

grant_type=authorization_code&code=AUTH_CODE&code_verifier=YOUR_VERIFIER&redirect_uri=https://your-project.example.com/callback&client_id=YOUR_CLIENT_ID
```

```json
{
  "access_token": "iH8L2...tQ",
  "token_type": "Bearer",
  "expires_in": 3600,
  "auth_time": 1751731200
}
```

Then resolve the DID with `/v1/project-token/verify?token=<access_token>` as above.

### Branch 2: Different device

Initiate the flow by submitting a POST request to the Avalanche server:

```
POST https://your-homeserver.example.com/v1/oauth/device_authorization
Content-Type: application/x-www-form-urlencoded

client_id=YOUR_CLIENT_ID
```

It returns the following:

```json
{
  "device_code": "GmR8...9k",
  "user_code": "BCDF-GHJK",
  "verification_uri": "https://go.theavalanche.net/authorize",
  "verification_uri_complete": "https://go.theavalanche.net/authorize?user_code=BCDF-GHJK&server_url=https%3A%2F%2Fyour-homeserver.example.com&client_id=YOUR_CLIENT_ID",
  "expires_in": 120,
  "interval": 5
}
```

Render `verification_uri_complete` as a QR code. The user scans it with their phone, which opens the app to the consent screen. Meanwhile, poll the token endpoint every `interval` seconds with the `device_code`:

```
POST https://your-homeserver.example.com/v1/oauth/token
Content-Type: application/x-www-form-urlencoded

grant_type=urn:ietf:params:oauth:grant-type:device_code&device_code=GmR8...9k&client_id=YOUR_CLIENT_ID
```

Until the user approves, this returns HTTP 400 with an OAuth error body — `{"error":"authorization_pending"}`. Once approved, you get the same token response as branch 1:

```json
{
  "access_token": "iH8L2...tQ",
  "token_type": "Bearer",
  "expires_in": 3600,
  "auth_time": 1751731200
}
```

As before, resolve the DID with `/v1/project-token/verify?token=<access_token>`.
