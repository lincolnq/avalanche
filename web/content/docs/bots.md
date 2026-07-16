---
title: "Bots"
date: 2026-07-05T17:20:49-04:00
description: "The most powerful integrations are bots"
---

Your project can sign in to Avalanche and chat with users either via DM or in groups. Bots can do everything that users can do and more. Bots are identified via angular frames around their profile picture, as well as subtly angular message bubbles in conversations. We also recommend that you name bots using a pattern that ends in Bot, e.g., TestBot, AdminBot.

You can code your bots in TypeScript or Rust. We recommend TypeScript (but Rust is usable for low-memory deployments).

*This document is still being written.*

## Basics

Bots are written using `app-core`, an asynchronous library which is responsible for an account on an Avalanche server: connection establishment, authentication, sending and receiving messages, persistent data and more.

In TypeScript, bots are built with our `@theavalanche/app-core` TypeScript library — see the [API reference](/api/).

