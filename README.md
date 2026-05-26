# actnet

The online+offline social network. We help people organize.

(`actnet` is a temporary name. We're accepting suggestions for a new one!)

Anyone can build a social network these days, but who will use it? Our answer is that we'll build great tools for organizing, and people will install the app because a specific action they're participating in (a rescue, a canvass, etc) requires it. They'll stick around because the network captures and represents the real social connections they formed.

The design centers on Signal-quality encrypted messaging — a unified inbox of all your conversations across all your activism — with a platform for rapidly-built, well-integrated organizing tools: team assignment, action-day maps, Q&A bots, collaborative documents, and more.

<p align="center">
  <img src="docs/screenshot.png" width="200" alt="Chat list">
  <img src="docs/screenshot2.png" width="200" alt="Network tab">
  <img src="docs/screenshot3.png" width="200" alt="Testbot">
</p>

## Getting started

### Prerequisites

We currently do development on MacOS, with Rust, Docker and Xcode:

- [OrbStack](https://orbstack.dev/) (our recommended Docker server, but plain old Docker is ok too)
- [Rust](https://rustup.rs/) (stable)
- [Xcode](https://developer.apple.com/xcode/) 16+ (for the iOS app)
- [XcodeGen](https://github.com/yonaskolb/XcodeGen) — `brew install xcodegen`

### Run the backend

```bash
make dev-all   # starts Postgres, applies migrations, launches homeserver + relay + testbot
```

This runs the homeserver on `localhost:3000` and the [testbot](docs/21-chatbot-project.md) on `localhost:3001`. To make the testbot respond with AI instead of echoing, copy `.env.example` to `.env` and add your [Anthropic API key](https://console.anthropic.com/).

### Run the iOS app on simulator

```bash
make ios       # build Rust → XCFramework, generate Swift bindings, generate Xcode project
```

Then open `mobile/ios/Actnet/Actnet.xcodeproj` in Xcode, select an iPhone simulator, and run.

On first launch, switch to **Dev Server** mode in settings, then create an account pointing at `http://localhost:3000`.

### Run on a real device

You'll need Tailscale on both your phone and laptop, signed in to the same tailnet. Create a free Tailscale account and install the app on both. Your laptop will get a DNS name like `<host>.tail<NNNNN>.ts.net`.

In your `.env` (copy `.env.example` first if you haven't), set:

```
SERVER_URL=http://<host>.tail<NNNNN>.ts.net:3000
```

Then plug your iOS device into your laptop. On the phone, enable **Settings → Privacy & Security → Developer Mode** and trust the laptop when prompted. In Xcode, pick the connected device as the run target and launch. First-time device prep takes a few minutes.

On first launch, switch to **Dev Server** mode in the app's settings — the app currently defaults to mock mode.

To sign up, run `make dev-invite` (install `qrencode` first with `brew install qrencode`) and scan the QR with your phone's camera.

## Docs

- [00 — Design](docs/00-design.md) — goals, architecture, threat model, and first-party Project designs
- [01 — Technical implementation](docs/01-technical-implementation.md) — tech stack, cryptographic approach, repository structure, and staged build plan
- [10 — Server implementation](docs/10-server-implementation.md) — homeserver PostgreSQL schema and implementation plan
- [11 — Core API sketch](docs/11-core-api-sketch.md) — API design for the `crypto` and `store` crates
- [20 — Project security](docs/20-project-security.md) — security model for Projects (auth, webviews, bots)
- [21 — Chatbot project](docs/21-chatbot-project.md) — design and implementation plan for the first Project
- [30 — Mobile UX](docs/30-mobile-ux.md) — mobile app UX: signup flows, navigation, multi-account
