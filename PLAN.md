# MiniClaw — Implementation Plan

## Completed

### Initial Build (9 commits)
- [x] ReAct agent loop with parallel tool execution
- [x] Anthropic + OpenAI-compatible LLM providers with failover
- [x] 13 built-in tools with sandboxing
- [x] Context builder with caching and budget enforcement
- [x] Session persistence (in-memory + JSONL on disk)
- [x] Memory consolidation with MEMORY.md bounds
- [x] CLI: REPL mode + single-shot + init command
- [x] HTTP API (axum): /api/chat, /api/status
- [x] MQTT client for IoT device communication
- [x] Cron scheduler + heartbeat service
- [x] Skill system (markdown files, requirement gating, all-inject)
- [x] Security: path validation, shell injection prevention, credential leak scanning
- [x] Integration tests with MockLlmClient (147 tests, zero warnings)
- [x] Cross-compile for aarch64-linux (RPi)
- [x] Tested with real OpenAI API

### Stats
- 26 source files, ~4,800 lines of Rust
- 3.4MB binary (release, aarch64), 5.5MB RAM idle
- 147 tests passing, zero warnings

---

## Next Steps (Priority Order)

### 1. README + Multi-Target Release
- [x] README.md (features, quick start, architecture, building)
- [ ] build-release.sh for 3 targets (aarch64, armv7, x86_64)
- [ ] Test multi-target build
- [ ] Push to GitHub with README
- [ ] Tag v0.1.0 release

### 2. GitHub Actions CI
- [ ] Workflow: build + test on push
- [ ] Release workflow: build 3 targets on tag push, attach binaries to GitHub Release
- [ ] Badge in README (build status)

### 3. MCP Client
- [ ] MCP client: stdio transport (local subprocess)
- [ ] MCP client: HTTP transport (remote server)
- [ ] Tool discovery via `tools/list`
- [ ] Tool execution via `tools/call`
- [ ] MCP tools registered alongside built-in tools (seamless for LLM)
- [ ] Config: `[[mcp_servers]]` section in TOML
- [ ] Tests with mock MCP server

### 4. Privacy Gate
- [ ] PII regex detector (email, phone, API keys, SSH keys)
- [ ] S1/S2/S3 routing in agent loop (regex-only, no ONNX model yet)
- [ ] PII stripping before cloud LLM call
- [ ] PII rehydration in local memory
- [ ] Dual-track memory (MEMORY.md clean + MEMORY-FULL.md)
- [ ] Config: `[privacy]` section

### 5. Offline Graceful Degradation
- [ ] Detect LLM call failure (network down, API error)
- [ ] Rule-based command handler for basic commands (time, files, system info)
- [ ] "I'm in offline mode" announcement
- [ ] Queue complex requests for when cloud returns

### 6. Context Limit Handling
- [ ] Catch context-limit errors from LLM API
- [ ] Auto-compress: drop oldest messages, preserve tool pairs
- [ ] Retry with compressed context
- [ ] (Learned from PicoClaw's proactive compression)

### 7. Simple Web UI
- [ ] Single HTML page embedded in binary (`include_str!`)
- [ ] Served at `/` by the HTTP server
- [ ] Chat interface with message bubbles
- [ ] No React, no build step — plain HTML + vanilla JS + fetch API

### 8. Voice Pipeline
- [ ] Piper TTS integration (local, ~80MB model, feature-gated)
- [ ] STT via cloud API (Whisper API, Deepgram)
- [ ] WebSocket audio streaming endpoint
- [ ] Config: `[voice]` section

### 9. Hardware I/O
- [ ] GPIO read/write via sysfs
- [ ] I2C bus access (like PicoClaw's i2c.go)
- [ ] System temperature tool improvement
- [ ] Feature-gated `[rpi]`

---

## Release Plan

### v0.1.0 (current)
Everything completed above. CLI + HTTP + MQTT + 13 tools + skills + memory.

### v0.2.0
MCP client + privacy gate + offline mode.

### v0.3.0
Voice pipeline + web UI.

### v0.4.0
Hardware I/O + ONNX privacy classifier.
