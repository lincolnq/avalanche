# actnet

The online+offline social network. We help people organize.

Anyone can build a social network these days, but who will use it? Our answer is that we'll build great tools for organizing, and people will install the app because a specific action they're participating in (a rescue, a canvass, etc) requires it. They'll stick around because the network captures and represents the real social connections they formed.

The design centers on Signal-quality encrypted messaging — a unified inbox of all your conversations across all your activism — with a platform for rapidly-built, well-integrated organizing tools: team assignment, action-day maps, Q&A bots, collaborative documents, and more.

## Getting started

```bash
make db-up    # start Postgres
make dev      # run homeserver with debug logging on localhost:3000
```

## Docs

- [00 — Design](docs/00-design.md) — goals, architecture, threat model, and first-party Project designs
- [01 — Technical implementation](docs/01-technical-implementation.md) — tech stack, cryptographic approach, repository structure, and staged build plan
- [10 — Server implementation](docs/10-server-implementation.md) — homeserver PostgreSQL schema and implementation plan
- [11 — Core API sketch](docs/11-core-api-sketch.md) — API design for the `crypto` and `store` crates
- [20 — Project security](docs/20-project-security.md) — security model for Projects (auth, webviews, bots)
- [21 — Chatbot project](docs/21-chatbot-project.md) — design and implementation plan for the first Project
- [30 — Mobile UX](docs/30-mobile-ux.md) — mobile app UX: signup flows, navigation, multi-account
