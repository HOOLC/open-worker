# Development

- Preserve existing uncommitted work. Do not reset unrelated changes.
- Use the pnpm version declared in `package.json` (currently 10.33.0) and frozen lockfiles. Use `--locked` for Cargo builds and tests.
- Follow `docs/rust-build-cache.md` when changing shared Rust dependencies or features.
- Rebuild affected binaries before process tests. Read current package scripts and workflows when selecting checks; historical validation reports are not proof for the current working tree.

# Client core and UI

- UI is limited to presentation, animation, input capture and transient interaction state. All client business rules, validation, business state, network requests, persistence, synchronization, retries and operation lifecycles must go through `zork-client-core`.
- UI submits business intents and consumes read-only core snapshots or deltas. It must not construct HTTP methods/paths/bodies, implement business polling, infer delivery/authorization state, or maintain a second mutable business list. Moving a raw request behind a forwarding helper does not satisfy this boundary.
- Platform adapters provide capabilities requested through core-defined interfaces; they do not own business decisions. Core must remain independent of GPUI, Compose and widget lifetimes. Desktop, Android and Web fixtures use the same business contracts.
- Before implementing, refactoring or reviewing client features or core/UI separation, read [zork-client-boundary](.agents/skills/zork-client-boundary/SKILL.md) and follow [the core/UI boundary](docs/client-core-ui-boundary.md). Existing violations listed there are migration work, not exemptions for new code.
- For core-to-UI subscriptions, snapshots/deltas, or high-frequency update delivery, read [zork-client-subscriptions](.agents/skills/zork-client-subscriptions/SKILL.md). Preserve consumer-applied version baselines, bounded recovery and platform scheduling boundaries; track actual migration and performance evidence separately from the design proposal.

# Chat messages

- Before designing, implementing or reviewing Chat/channel behavior, delivered-message caching or interactive message cards, read [zork-chat-messages](.agents/skills/zork-chat-messages/SKILL.md). Keep the authoritative message source append-only while allowing Rust core to update merged client cache records; preserve result-before-request handling and source-based synchronization cursors.

# Local environment instructions

Before source changes, dependency installation, builds, tests, or starting development services, look for a `zork-local-environment` skill under `.agents/skills/` (including grouping subdirectories). If present, read its `SKILL.md` and apply its host and checkout rules before project work. If absent, use the current checkout and the repository's general instructions.

Personal environment skills belong in a locally excluded group, configured through `.git/info/exclude`. Shared skills must not require a contributor's private hosts, usernames, absolute checkout paths, or service endpoints.
