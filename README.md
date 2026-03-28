# UniClaw

A privacy-first AI agent that runs on resource-constrained edge devices — Raspberry Pi, BeagleBone, cheap ARM boards, and beyond.

UniClaw is a lightweight, self-hosted AI agent designed for edge deployment. The agent loop runs locally — the cloud LLM is just an API it calls when it needs to think. Tools, memory, skills, and state all stay on your device.

```
$ ./uniclaw chat --message "What time is it?"
It's 3:42 PM on Tuesday, March 25, 2026.

$ ./uniclaw chat
UniClaw v0.1.0 | gpt-4o-mini | aarch64
Type 'exit' or Ctrl+C to quit.

You> Write a haiku about Raspberry Pi to haiku.txt
UniClaw> [writes file] Done! Here's what I wrote:
  Small board, big ideas
  Kernel hums beneath the case
  Bits bloom in silence

You> What's my system status?
UniClaw> [calls system_info + shell_exec]
  OS: Linux aarch64
  CPU temp: 48.2°C
  Memory: 412MB / 4096MB used
  Disk: 12GB free on /
  Uptime: 3d 7h 22m
```

## Quick Start

**Download a prebuilt binary** from [Releases](https://github.com/cortexa-core/uniclaw/releases), or build from source:

```bash
# Build from source
git clone https://github.com/cortexa-core/uniclaw.git
cd uniclaw
cargo build --release

# Initialize (creates config + default files)
./target/release/uniclaw init

# Set your LLM API key
export OPENAI_API_KEY="your-key"
# or
export ANTHROPIC_API_KEY="your-key"

# Chat
./target/release/uniclaw chat
```

### Configuration

Edit `config/config.toml` to set your LLM provider:

```toml
[llm]
provider = "openai_compatible"    # or "anthropic"
api_key_env = "OPENAI_API_KEY"    # env var name (key never stored in config)
model = "gpt-4o-mini"
base_url = "https://api.openai.com"
```

Supports any OpenAI-compatible API: OpenAI, Ollama (local), Groq, DeepSeek, OpenRouter, vLLM, LMStudio, etc.

## Features

### Agent Loop
- **ReAct pattern**: think → call tools → observe → think again
- **Parallel tool execution**: independent tool calls run concurrently
- **Provider failover**: primary LLM fails → fallback kicks in
- **Max iteration safety**: prevents infinite tool loops

### 13 Built-in Tools

| Tool | What It Does |
|------|-------------|
| `get_time` | Current date/time/timezone |
| `read_file` | Read files (path-sandboxed) |
| `write_file` | Write/create files |
| `edit_file` | Find-and-replace in files |
| `list_dir` | List directory contents |
| `memory_store` | Save facts to long-term memory |
| `memory_read` | Search/read long-term memory |
| `system_info` | CPU, RAM, uptime, temperature |
| `shell_exec` | Sandboxed shell (command whitelist) |
| `http_fetch` | Fetch URLs (with credential leak scanning) |
| `cron_add` | Schedule recurring tasks |
| `cron_list` | List scheduled jobs |
| `cron_remove` | Remove a scheduled job |

### Memory System
- **Sessions**: multi-turn conversations persist across restarts (JSONL)
- **Long-term memory**: agent stores facts in `MEMORY.md` via `memory_store` tool
- **Memory consolidation**: old messages automatically summarized to keep context manageable
- **Daily notes**: date-stamped observations

### Server Mode

```bash
./uniclaw serve
```

Starts HTTP API + cron scheduler + heartbeat service:

```bash
# Chat via HTTP
curl -X POST http://localhost:3001/api/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "What time is it?"}'

# Check status
curl http://localhost:3001/api/status
```

- **Cron**: schedule recurring tasks via `cron_add` tool
- **Heartbeat**: polls `HEARTBEAT.md` every 30 min for pending tasks
- **MQTT**: subscribe/publish for IoT device communication

### Skills

Skills are markdown files that teach the agent domain expertise. Drop a `.md` file in `data/skills/` — no code needed:

```markdown
---
name: garden-monitor
description: Monitor garden sensors
requires:
  tools: [shell_exec]
---

## Garden Monitoring
When asked about garden or plants:
1. Use `shell_exec("mosquitto_sub -t garden/# -C 1")` to read sensors
2. Soil moisture below 30% = needs watering
```

All gated skills are injected into the system prompt. The LLM decides which are relevant. Ships with 3 built-in skills (memory management, file operations, system monitoring).

### Security
- **Path sandboxing**: file tools validate full ancestor chain, can't escape `data/` directory
- **Shell injection prevention**: blocks metacharacters (`;`, `&`, `|`, `` ` ``, `$`)
- **Command whitelist**: only configured commands allowed in `shell_exec`
- **Credential boundary injection**: API keys never enter LLM context, substituted after LLM generates requests, responses scanned for leaked credentials
- **API keys from env vars only**: never stored in config files

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                  UniClaw (single Rust binary)           │
│                                                           │
│  ┌─────────────┐  ┌──────────┐  ┌──────────────────┐   │
│  │ Input        │  │  Agent   │  │  Output           │   │
│  │              │  │  Loop    │  │                    │   │
│  │ • CLI        │  │ (ReAct)  │  │ • CLI              │   │
│  │ • HTTP API   │→ │          │→ │ • HTTP response    │   │
│  │ • MQTT       │  │ Think →  │  │ • MQTT publish     │   │
│  │ • Cron       │  │ Act →    │  │                    │   │
│  │ • Heartbeat  │  │ Observe  │  │                    │   │
│  └─────────────┘  └────┬─────┘  └──────────────────┘   │
│                         │                                 │
│            ┌────────────┼────────────┐                   │
│            ▼            ▼            ▼                   │
│     ┌──────────┐ ┌──────────┐ ┌──────────┐             │
│     │ 13 Tools │ │ Memory   │ │ Skills   │             │
│     │ (local)  │ │ (local)  │ │ (local)  │             │
│     └──────────┘ └──────────┘ └──────────┘             │
│                         │                                 │
│                    Cloud LLM API ◄── only remote call    │
│                    (Anthropic, OpenAI, Ollama, etc.)     │
└─────────────────────────────────────────────────────────┘
```

The agent loop runs **locally**. Only LLM inference is remote. Everything else — tools, memory, skills, sessions, cron — stays on the device.

## Building from Source

### Prerequisites
- Rust 1.75+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- For cross-compilation: `zig` + `cargo-zigbuild`

### Native build

```bash
cargo build --release
# Binary at: target/release/uniclaw
```

### Cross-compile for Raspberry Pi (aarch64)

```bash
brew install zig                    # or: apt install zig
cargo install cargo-zigbuild
rustup target add aarch64-unknown-linux-gnu

cargo zigbuild --target aarch64-unknown-linux-gnu --release
# Binary at: target/aarch64-unknown-linux-gnu/release/uniclaw
```

### Cross-compile for other targets

```bash
# 32-bit ARM (RPi 2, BeagleBone, cheap boards)
rustup target add armv7-unknown-linux-gnueabihf
cargo zigbuild --target armv7-unknown-linux-gnueabihf --release

# x86_64 Linux (mini PCs, VMs, Docker)
rustup target add x86_64-unknown-linux-gnu
cargo zigbuild --target x86_64-unknown-linux-gnu --release
```

### Run tests

```bash
cargo test    # 147 tests, zero warnings
```

## Project Structure

```
src/
  main.rs              CLI, init, chat, serve commands
  config.rs            TOML config loading
  agent/
    loop.rs            ReAct agent loop (~500 lines)
    context.rs         System prompt builder with caching
    memory.rs          Session store + memory consolidation
    skills.rs          Skill loader with requirement gating
  llm/
    types.rs           Canonical types (Message, ToolCall, etc.)
    anthropic.rs       Anthropic Messages API
    openai.rs          OpenAI Chat Completions API
  tools/
    registry.rs        Tool trait + dispatch
    (13 tool files)    Individual tool implementations
  server/
    http.rs            Axum HTTP API
    mqtt.rs            MQTT pub/sub
    cron.rs            Cron scheduler
    heartbeat.rs       Proactive task poller
```

## License

MIT
