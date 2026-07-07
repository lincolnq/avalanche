---
title: "Documentation"
description: "How to integrate with Avalanche"
---

## Integration overview

Avalanche is designed so that organizations can build features and automate workflows while still preserving a simple text-message interface for users.

We use the term "projects" to refer to everything you can build on top of Avalanche: automations, bots, websites and so on.

There are a small number of integration points that enable projects to be tightly integrated with your workflows:

* [Bots](/docs/bots): In Avalanche, bots can chat, send images and links, edit their messages, and monitor/administer groups.
* [Authentication](/docs/authentication/): Projects can know who users are, enabling proactive messaging and social features.
* Administration: Projects can be given limited administrative privileges on your server (invite members, listen for new joiners, etc) so your bots can do things like onboard users, add them to groups and so on.
* [Network tab](/docs/network_tab/): Projects can be listed on the Network tab of the app, so that users can see options listed within the app.

Looking ahead, we've also planned a few features which are not available in the app yet:

* Composer buttons (the '+' button in conversations): present options for sending new types of messages -- e.g. GIF search, event invitations.
* Message menu (long press on any message in conversations): present options related to specific messages -- e.g., create a task from a message.
* Participant menu (long press on a participant in a group): present options for a particular group member -- e.g., flag this member.

(If these things are important to your use case, let us know soon so we can prioritize adding them to the app!)

## System design docs

If you want to understand how Avalanche was designed, there's an extensive design document library checked into the docs/ folder in the repo. Here's a link to the overview:

[00 — Design](https://github.com/lincolnq/actnet/blob/main/docs/00-design.md)