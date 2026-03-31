# Contributing to UniClaw

Thanks for your interest in contributing!

## Getting Started

```bash
git clone https://github.com/cortexa-core/uniclaw.git
cd uniclaw
cargo build
cargo test
```

## Development Workflow

1. Fork the repo and create a branch from `main`
2. Make your changes
3. Run `cargo build` — zero warnings required
4. Run `cargo test` — all tests must pass
5. Run `cargo fmt` and `cargo clippy`
6. Submit a pull request

## Build with Telegram

```bash
cargo build --features telegram
cargo test --features telegram
```

## Cross-Compile for RPi

```bash
cargo zigbuild --target aarch64-unknown-linux-musl --release
```

Requires `zig` and `cargo-zigbuild`.

## Web UI

The frontend is in `web/` (Svelte 5). To rebuild:

```bash
cd web && npm install && npm run build && cd ..
cargo build  # embeds the built web/dist/ into the binary
```

## Code Style

- Use `anyhow` for error handling in application code
- All async, all Tokio — no std threads
- Feature flags gate optional modules: `#[cfg(feature = "telegram")]`
- API keys from env vars only, never in config files
- File operations restricted to `data/` directory

## Adding a Tool

1. Create `src/tools/your_tool.rs`
2. Implement the `Tool` trait (name, description, parameters_schema, execute)
3. Register in `src/tools/mod.rs`
4. Add tests

## Adding a Channel

1. Create `src/channels/your_channel.rs` behind `#[cfg(feature = "your_channel")]`
2. Implement the `Channel` trait (name, run)
3. Add feature flag to `Cargo.toml`
4. Wire into `spawn_channels()` in `src/channels/mod.rs`

## Adding a Skill

Drop a markdown file in `data/skills/`:

```markdown
---
name: my-skill
description: What it does
requires:
  tools: [shell_exec]
---

Instructions for the agent...
```
