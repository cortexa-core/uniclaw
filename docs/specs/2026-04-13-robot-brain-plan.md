# Robot Brain Foundation — Implementation Plan (v2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the robot brain foundation — robot description, world state, hardware bridge (mock + serial), action system, safety monitor, perception (camera + cloud VLM), and voice pipeline (cloud STT + cloud TTS). Desktop companion robot as first demo.

**Architecture:** Robot runtime as Tokio tasks alongside existing agent worker. HardwareBridge trait abstracts mock/serial/ROS2. World state via `watch` channel (one writer, many readers). Actions as LLM tools auto-registered from robot.toml. Three communication channels only: world_state (watch), action_tx (mpsc), agent_tx (mpsc, existing).

**Tech Stack:** Rust, Tokio, serde, tokio-serial, cpal (audio), tokio-tungstenite (ROS2 bridge)

**Review fixes applied (v2):** Added Task 0 (image support), extended ToolContext with action/world channels, simplified channel architecture (3 channels not 6), added safety rule expression parser detail, switched to cloud TTS for MVP (no ort/Piper dependency), added reference MCU firmware, added audio playback via cpal.

---

## Prerequisites

### Task 0: Add image support to LLM message types and providers

**Why this is first:** The perception pipeline (Task 8) needs to send camera frames to vision LLMs. The current `MessageContent` enum has no image variant. Without this, `take_photo` can't work.

**Files:**
- Modify: `src/llm/types.rs`
- Modify: `src/llm/anthropic.rs`
- Modify: `src/llm/openai.rs`
- Modify: `src/llm/gemini.rs`

- [ ] **Step 1: Add image content variant to MessageContent**

In `src/llm/types.rs`, add a new variant to `MessageContent`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    Text {
        text: String,
    },
    TextWithImage {
        text: String,
        image_base64: String,
        mime_type: String,  // "image/jpeg", "image/png"
    },
    ToolUse {
        text: Option<String>,
        tool_calls: Vec<ToolCall>,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}
```

Update `content_text()` to handle the new variant:

```rust
pub fn content_text(&self) -> &str {
    match &self.content {
        MessageContent::Text { text } => text,
        MessageContent::TextWithImage { text, .. } => text,
        MessageContent::ToolUse { text, .. } => text.as_deref().unwrap_or("[tool call]"),
        MessageContent::ToolResult { content, .. } => content,
    }
}
```

Add a constructor:

```rust
pub fn user_with_image(text: &str, image_base64: String, mime_type: &str) -> Self {
    Self {
        role: Role::User,
        content: MessageContent::TextWithImage {
            text: text.to_string(),
            image_base64,
            mime_type: mime_type.to_string(),
        },
    }
}
```

- [ ] **Step 2: Update Anthropic serializer**

In `src/llm/anthropic.rs`, in `serialize_messages()`, add a case for `TextWithImage`:

```rust
MessageContent::TextWithImage { text, image_base64, mime_type } => {
    result.push(json!({
        "role": msg.role.to_string(),
        "content": [
            {
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": mime_type,
                    "data": image_base64,
                }
            },
            {
                "type": "text",
                "text": text,
            }
        ],
    }));
}
```

- [ ] **Step 3: Update OpenAI serializer**

In `src/llm/openai.rs`, in `serialize_request()`, add a case for `TextWithImage`:

```rust
MessageContent::TextWithImage { text, image_base64, mime_type } => {
    messages.push(json!({
        "role": msg.role.to_string(),
        "content": [
            {
                "type": "image_url",
                "image_url": {
                    "url": format!("data:{mime_type};base64,{image_base64}"),
                }
            },
            {
                "type": "text",
                "text": text,
            }
        ],
    }));
}
```

- [ ] **Step 4: Update Gemini serializer**

In `src/llm/gemini.rs`, in `serialize_messages()`, add a case for `TextWithImage`:

```rust
MessageContent::TextWithImage { text, image_base64, mime_type } => {
    result.push(json!({
        "role": "user",
        "parts": [
            {
                "inlineData": {
                    "mimeType": mime_type,
                    "data": image_base64,
                }
            },
            {"text": text}
        ]
    }));
}
```

- [ ] **Step 5: Update streaming serializers**

The `chat_streaming()` methods in all three providers call `serialize_request()` which already handles the new variant (same serialization code). No additional streaming changes needed.

- [ ] **Step 6: Add tests**

Add tests in each provider verifying image messages serialize correctly. Add a roundtrip test in types.rs for the new variant.

- [ ] **Step 7: Run tests and commit**

```
git commit -m "Add image support to LLM message types for vision-capable providers"
```

---

## Phase 3a: Robot Description + Runtime + Mock Bridge

### Task 1: HardwareBridge trait and MockBridge

**Files:**
- Create: `src/robot/mod.rs`
- Create: `src/robot/bridge/mod.rs`
- Create: `src/robot/bridge/mock.rs`
- Modify: `src/main.rs` (add `mod robot;`)

The HardwareBridge trait and MockBridge implementation as specified in the original plan. No changes from v1.

- [ ] **Step 1: Create HardwareBridge trait** (`src/robot/bridge/mod.rs`)
- [ ] **Step 2: Create MockBridge** (`src/robot/bridge/mock.rs`)
- [ ] **Step 3: Create robot module root** (`src/robot/mod.rs`, add `mod robot;` to main.rs)
- [ ] **Step 4: Add tests for MockBridge**
- [ ] **Step 5: Run tests and commit**

```
git commit -m "Add HardwareBridge trait and MockBridge for development"
```

---

### Task 2: Robot description parser (robot.toml)

**Files:**
- Create: `src/robot/description.rs`
- Create: `data/robot.toml` (example)
- Modify: `src/robot/mod.rs`

The robot description parser as specified in v1. No changes.

- [ ] **Step 1: Create description types and parser** (`src/robot/description.rs`)
- [ ] **Step 2: Register module, create example robot.toml**
- [ ] **Step 3: Add tests** (minimal parse, full parse, system prompt generation, ROS2 config)
- [ ] **Step 4: Run tests and commit**

```
git commit -m "Add robot description parser for robot.toml"
```

---

### Task 3: World state, runtime, CLI, ToolContext extension, context injection

**Files:**
- Create: `src/robot/world_state.rs`
- Create: `src/robot/runtime.rs`
- Modify: `src/robot/mod.rs`
- Modify: `src/main.rs` (add `Robot` subcommand + `run_robot()`)
- Modify: `src/agent/context.rs` (add robot_prompt + world_rx)
- Modify: `src/tools/registry.rs` (extend ToolContext)

This is the largest task — it wires the robot runtime into the existing agent.

- [ ] **Step 1: Create WorldState** (`src/robot/world_state.rs`)

As specified in v1, with `to_context_section()` for LLM prompt injection.

- [ ] **Step 2: Create RobotRuntime** (`src/robot/runtime.rs`)

As specified in v1. Spawns sensor polling task, manages watch channel.

Simplification from v1: Only 3 channels:
- `world_tx/rx` (watch) — sensor data + perception state
- `action_tx/rx` (mpsc) — agent → hardware bridge
- `agent_tx/rx` (mpsc) — existing, voice/text/events → agent

Remove the 3 extra channels from the original design (event_tx, hw_tx, sensor_tx). The runtime writes sensors directly to world_tx. The action executor calls bridge methods directly.

- [ ] **Step 3: Extend ToolContext with robot capabilities**

In `src/tools/registry.rs`, add optional robot fields:

```rust
pub struct ToolContext {
    pub data_dir: PathBuf,
    pub session_id: String,
    pub config: Arc<Config>,
    /// For robot tools: send action commands to the hardware bridge
    pub action_tx: Option<tokio::sync::mpsc::Sender<super::robot::bridge::HardwareCommand>>,
    /// For robot tools: read current world state
    pub world_rx: Option<tokio::sync::watch::Receiver<super::robot::world_state::WorldState>>,
}
```

Update ALL places that construct ToolContext to add `action_tx: None, world_rx: None`. These are:
- `src/agent/loop.rs` (process_inner, where ToolContext is built for tool execution)

In robot mode, these fields are set to `Some(...)`.

- [ ] **Step 4: Inject robot context into ContextBuilder**

In `src/agent/context.rs`, add optional fields:

```rust
pub struct ContextBuilder {
    // ... existing fields ...
    robot_prompt: Option<String>,
    world_rx: Option<tokio::sync::watch::Receiver<crate::robot::world_state::WorldState>>,
}
```

Add setter methods:
```rust
pub fn set_robot_context(
    &mut self,
    robot_prompt: String,
    world_rx: tokio::sync::watch::Receiver<crate::robot::world_state::WorldState>,
) {
    self.robot_prompt = Some(robot_prompt);
    self.world_rx = Some(world_rx);
}
```

In `build_system_prompt()`, after skills section, append:
```rust
if let Some(ref prompt) = self.robot_prompt {
    parts.push(prompt.clone());
}
if let Some(ref rx) = self.world_rx {
    parts.push(rx.borrow().to_context_section());
}
```

Initialize `robot_prompt: None, world_rx: None` in `ContextBuilder::new()`.

- [ ] **Step 5: Add `Robot` CLI subcommand**

In `src/main.rs`, add to `Commands` enum:

```rust
    Robot {
        #[arg(long, default_value = "data/robot.toml")]
        robot_config: PathBuf,
        #[arg(long, short)]
        message: Option<String>,
    },
```

Add `run_robot()` function:
1. Load config + robot.toml
2. Create bridge (mock/serial based on robot.toml)
3. Create RobotRuntime
4. Create agent with robot context injected
5. Spawn agent worker with action_tx and world_rx in ToolContext
6. Start runtime tasks
7. If `message` provided: single-shot mode. Otherwise: REPL (like chat).
8. Wait for Ctrl+C

- [ ] **Step 6: Add tests and commit**

Test: Load robot.toml → create runtime with MockBridge → verify world state updates → verify context includes robot description.

```
git commit -m "Add world state, runtime, robot CLI, ToolContext extension, and context injection"
```

---

## Phase 3b: Action System + Safety

### Task 4: Action types and executor

**Files:**
- Create: `src/robot/action.rs`
- Modify: `src/robot/mod.rs`
- Modify: `src/robot/runtime.rs` (spawn executor task)

- [ ] **Step 1: Define ActionCommand enum**

```rust
#[derive(Debug, Clone)]
pub enum ActionCommand {
    ServoSet { name: String, angle: f32 },
    MotorSet { name: String, speed: f32, duration_ms: Option<u64> },
    LedSet { name: String, r: u8, g: u8, b: u8 },
    LedPattern { name: String, pattern: String },
    Speak { text: String },
    PlaySound { file: String },
    Stop,
    EmergencyStop,
}
```

- [ ] **Step 2: Create ActionExecutor**

```rust
pub struct ActionExecutor {
    bridge: Arc<dyn HardwareBridge>,
    action_rx: mpsc::Receiver<ActionCommand>,
    world_tx: watch::Sender<WorldState>,
}
```

The executor runs as a Tokio task:
- Receive ActionCommand from channel
- Translate to HardwareCommand and send via bridge
- For `Speak`: call cloud TTS API (or future local TTS), play audio
- Update world state (actuator_positions)
- Log actions for debugging

Note: `Speak` initially just logs the text. Voice output is added in Task 9.

- [ ] **Step 3: Wire executor into RobotRuntime::start()**

Create the action channel in runtime, spawn the executor task, expose `action_tx()` for tools.

- [ ] **Step 4: Add tests and commit**

Test: Send ActionCommand → MockBridge receives corresponding HardwareCommand.

```
git commit -m "Add action types and executor for robot hardware commands"
```

---

### Task 5: Robot action tools

**Files:**
- Create: `src/tools/robot_actions.rs`
- Modify: `src/tools/mod.rs`

- [ ] **Step 1: Create robot action tools**

Tools the LLM can call. Each sends an ActionCommand via the `action_tx` in ToolContext:

| Tool | Parameters | Requires |
|------|-----------|----------|
| `set_servo` | name, angle | Any servo actuator in robot.toml |
| `set_led` | name, r, g, b | Any led/neopixel actuator |
| `set_led_pattern` | name, pattern | Any led/neopixel actuator |
| `say` | text | speaker actuator |
| `stop` | — | Any actuator |
| `get_sensor` | name | Any sensor |
| `get_world_state` | — | Always available in robot mode |

Each tool:
1. Validates parameters against robot.toml (e.g., servo angle within range)
2. Sends ActionCommand to action_tx
3. Returns success/error to LLM

**Auto-registration**: Create a `register_robot_tools(registry, description)` function that only registers tools for capabilities the robot has. If no servos → no `set_servo` tool. If no LEDs → no `set_led` tool.

- [ ] **Step 2: Add `get_world_state` tool**

This tool returns the current world state as text — sensors, perception, body state. Uses `world_rx` from ToolContext.

- [ ] **Step 3: Add tests and commit**

Test with MockBridge: call `set_servo` tool → verify MockBridge received the command.

```
git commit -m "Add robot action tools auto-registered from robot.toml capabilities"
```

---

### Task 6: Safety monitor with expression parser

**Files:**
- Create: `src/robot/safety.rs`
- Modify: `src/robot/mod.rs`
- Modify: `src/robot/runtime.rs` (spawn safety task)

- [ ] **Step 1: Implement safety rule expression parser**

Parse expressions from robot.toml like `"front_distance < 10"` and `"battery < 15"`:

```rust
pub struct ParsedRule {
    pub name: String,
    pub sensor_name: String,
    pub operator: CompareOp,
    pub threshold: f32,
    pub action: SafetyAction,
    pub priority: u8,
}

pub enum CompareOp {
    LessThan,
    GreaterThan,
    LessEqual,
    GreaterEqual,
}

pub enum SafetyAction {
    StopAll,
    EmergencyStop,
    Speak(String),
}

impl ParsedRule {
    pub fn parse(config: &SafetyRuleConfig) -> Result<Self> {
        // Parse "sensor_name < value" or "sensor_name > value"
        // Split on whitespace, expect 3 parts
        // ...
    }

    pub fn evaluate(&self, sensors: &HashMap<String, SensorValue>) -> bool {
        if let Some(SensorValue::Distance(d) | SensorValue::Raw(d_raw)) = sensors.get(&self.sensor_name) {
            // Compare against threshold
        }
        false
    }
}
```

- [ ] **Step 2: Implement SafetyMonitor**

```rust
pub struct SafetyMonitor {
    rules: Vec<ParsedRule>,
    world_rx: watch::Receiver<WorldState>,
    action_tx: mpsc::Sender<ActionCommand>,
}
```

Runs as continuous Tokio task:
- Every 200ms (matches sensor poll rate), read world state
- Evaluate all rules in priority order
- If triggered, send action command (stop, e-stop, speak)
- Log triggered rules

- [ ] **Step 3: Wire into RobotRuntime::start()**
- [ ] **Step 4: Add tests**

Test: Set mock sensor to dangerous value → verify safety monitor sends stop command.

```
git commit -m "Add safety monitor with declarative rule evaluation and expression parser"
```

---

## Phase 3c: Serial Bridge

### Task 7: Serial bridge + reference MCU firmware

**Files:**
- Create: `src/robot/bridge/serial.rs`
- Create: `firmware/arduino/uniclaw_mcu/uniclaw_mcu.ino` (reference Arduino sketch)
- Modify: `src/robot/bridge/mod.rs`
- Modify: `Cargo.toml` (add `tokio-serial` dependency, feature-gated)

- [ ] **Step 1: Implement serial protocol**

Binary protocol over UART:
```
Frame: [0xAA] [LEN:u16-LE] [SEQ:u8] [TYPE:u8] [PAYLOAD:...] [CRC8:u8]
```

Command types, response types, CRC calculation as specified in design.

Feature-gated: `#[cfg(feature = "serial")]`

- [ ] **Step 2: Create reference Arduino sketch**

Create `firmware/arduino/uniclaw_mcu/uniclaw_mcu.ino`:
- Parse incoming serial frames
- Respond to PING with PONG
- Respond to SERVO_SET (log to serial monitor)
- Respond to STATUS_REQUEST (return mock battery + sensor data)
- Implement watchdog (stop servos if no heartbeat for 500ms)

This is ~150 LOC of Arduino C. Not Rust, but essential for testing.

- [ ] **Step 3: Add serial protocol unit tests** (encode/decode frames)
- [ ] **Step 4: Commit**

```
git commit -m "Add serial bridge for Arduino/ESP32 and reference MCU firmware"
```

---

## Phase 3d: Perception + Vision

### Task 8: Camera capture and cloud VLM integration

**Files:**
- Create: `src/robot/perception.rs`
- Create: `src/robot/camera.rs` (feature-gated behind `camera`)
- Create: `src/tools/perception_tools.rs`
- Modify: `src/tools/mod.rs`
- Modify: `Cargo.toml` (add `nokhwa` dependency, feature-gated)

**Prerequisite:** Task 0 (image support in LLM types) must be complete.

- [ ] **Step 1: Create camera capture module**

`src/robot/camera.rs` behind `#[cfg(feature = "camera")]`:
- Open camera device via `nokhwa`
- Capture frame → encode as JPEG → return base64 string
- Frame buffer: keep latest frame for on-demand capture

- [ ] **Step 2: Create perception pipeline**

`src/robot/perception.rs`:
- Runs as Tokio task
- Motion detection: compare current frame to previous (pixel difference, pure Rust with `image` crate)
- VLM trigger: on motion detected (debounced) or periodic timer or agent request
- VLM call: create `Message::user_with_image(prompt, base64, "image/jpeg")`, send through a dedicated LLM provider (from perception config)
- Update world state with scene description

- [ ] **Step 3: Create perception tools**

`src/tools/perception_tools.rs`:
- `take_photo`: capture frame → send to VLM → return description (uses image support from Task 0)
- `describe_scene`: return latest scene description from world state (no new VLM call)
- `check_motion`: return whether motion was recently detected

These tools need access to camera + VLM provider. Pass via ToolContext extension or capture in closure during registration.

- [ ] **Step 4: Add tests** (mock camera returns test image, verify VLM message format)
- [ ] **Step 5: Commit**

```
git commit -m "Add perception pipeline with camera capture and cloud VLM vision"
```

---

## Phase 3e: Voice Pipeline

### Task 9: Voice input (STT) + voice output (cloud TTS)

**Files:**
- Create: `src/robot/voice.rs` (feature-gated behind `voice`)
- Modify: `Cargo.toml` (add `cpal` dependency, feature-gated)
- Modify: `src/robot/runtime.rs` (spawn voice task)
- Modify: `src/robot/action.rs` (Speak action uses cloud TTS)

**MVP approach:** Cloud STT (Whisper API) + Cloud TTS (OpenAI TTS or provider TTS). No local Piper for v1 — avoids `ort` dependency entirely. Add local TTS as future optimization.

- [ ] **Step 1: Audio capture + VAD**

`src/robot/voice.rs`:
- Audio capture via `cpal` (16kHz mono)
- Energy-based VAD:
  1. Compute RMS energy per 20ms frame
  2. Adaptive noise floor (exponential moving average)
  3. Speech start: energy > floor * 3.0 for > 300ms
  4. Speech end: energy < floor * 1.5 for > 500ms
  5. Record speech segment as WAV bytes

~80 LOC for VAD, ~50 LOC for cpal setup.

- [ ] **Step 2: Cloud STT (Whisper API)**

Send recorded WAV to Whisper API (OpenAI endpoint):
- POST to `https://api.openai.com/v1/audio/transcriptions`
- Body: multipart form with audio file
- Response: `{"text": "what the user said"}`
- Convert to Input and send to agent_tx

Use `reqwest` (already a dependency) for the HTTP call.

- [ ] **Step 3: Cloud TTS (for Speak action)**

When ActionExecutor receives `ActionCommand::Speak { text }`:
- POST to TTS API (e.g., OpenAI `https://api.openai.com/v1/audio/speech`)
- Response: audio bytes (mp3/wav)
- Play via `cpal` output stream
- Cache in `data/tts_cache/` (hash of text → audio file)

Audio playback via `cpal`:
- Open default output device
- Write audio samples to output stream
- ~50 LOC for playback

- [ ] **Step 4: Wire into runtime**

Voice task: continuous audio capture → VAD → STT → agent input
Speak action: text → cloud TTS → cpal playback

- [ ] **Step 5: Add tests** (mock audio input, verify STT called, verify TTS cached)
- [ ] **Step 6: Commit**

```
git commit -m "Add voice pipeline with cloud STT (Whisper) and cloud TTS"
```

---

## Phase 4: ROS2 Bridge

### Task 10: ROS2 bridge via rosbridge WebSocket

**Files:**
- Create: `src/robot/bridge/ros2.rs`
- Create: `src/tools/ros2_tools.rs`
- Modify: `src/robot/bridge/mod.rs`
- Modify: `src/tools/mod.rs`
- Modify: `Cargo.toml` (add `tokio-tungstenite` dependency)

- [ ] **Step 1: Implement rosbridge WebSocket client**

`src/robot/bridge/ros2.rs`:
- Connect to rosbridge_server via WebSocket (`tokio-tungstenite`)
- Implement `HardwareBridge` trait:
  - `send_command(ServoSet)` → `{"op": "publish", "topic": "/servo_cmd", "msg": {"name": "...", "angle": ...}}`
  - `send_command(MotorSet)` → `{"op": "publish", "topic": config.cmd_vel_topic, "msg": {"linear": {"x": speed}}}`
  - `read_all_sensors()` → subscribe to sensor topics, maintain latest values
  - `emergency_stop()` → publish to e-stop topic
- Topic names from `robot.toml` `[hardware.ros2]` config
- Automatic reconnection on disconnect

~200 LOC.

- [ ] **Step 2: Create ROS2 tools**

`src/tools/ros2_tools.rs` — auto-registered when `bridge = "ros2"`:

| Tool | What it does |
|------|-------------|
| `navigate_to(x, y, theta)` | Publish to navigate action (from config) |
| `get_position()` | Read latest odom data from world state |
| `get_map()` | Call map_server service |
| `ros2_publish(topic, msg)` | Generic topic publish |
| `ros2_call_service(service, args)` | Generic service call |

- [ ] **Step 3: Add tests** (mock WebSocket server, verify protocol messages)
- [ ] **Step 4: Commit**

```
git commit -m "Add ROS2 bridge via rosbridge WebSocket for Gazebo and real hardware"
```

---

## Verification

### Task 11: Integration tests + final verification

**Files:**
- Create/modify: `tests/robot_test.rs`

- [ ] **Step 1: Robot brain integration test**

Full pipeline with MockBridge:
1. Load robot.toml with servos + sensors
2. Create runtime with MockBridge
3. Start runtime
4. Send text input "wave at me"
5. Agent calls LLM → LLM calls `set_servo` tool → MockBridge logs command
6. Verify MockBridge received `ServoSet` command
7. Verify agent context includes robot description and world state

- [ ] **Step 2: Safety monitor test**

1. Load robot.toml with safety rule "front_distance < 10"
2. Set mock sensor front_distance to 5
3. Verify safety monitor sends Stop action

- [ ] **Step 3: Image message test**

1. Create Message::user_with_image with test base64 data
2. Serialize through each provider (Anthropic, OpenAI, Gemini)
3. Verify correct image encoding in output JSON

- [ ] **Step 4: Run full test suite**

```bash
cargo test
cargo test --features telegram
cargo test --features camera,voice
cargo clippy -- -D warnings
cargo fmt -- --check
```

- [ ] **Step 5: Commit**

```
git commit -m "Add robot brain integration tests and verify all features"
```

---

## Summary

| Task | Phase | What | New Deps | Est. LOC |
|------|-------|------|----------|----------|
| **0** | Pre | Image support in LLM types + 3 providers | — | ~150 |
| **1** | 3a | HardwareBridge trait + MockBridge | — | ~150 |
| **2** | 3a | robot.toml parser | — | ~300 |
| **3** | 3a | World state + runtime + CLI + ToolContext + context injection | — | ~400 |
| **4** | 3b | Action types + executor | — | ~200 |
| **5** | 3b | Robot action tools (set_servo, say, get_sensor, etc.) | — | ~250 |
| **6** | 3b | Safety monitor + expression parser | — | ~200 |
| **7** | 3c | Serial bridge + reference Arduino sketch | tokio-serial (feature) | ~300 |
| **8** | 3d | Perception pipeline + camera + VLM | nokhwa (feature) | ~300 |
| **9** | 3e | Voice: cloud STT + cloud TTS + VAD + audio I/O | cpal (feature) | ~350 |
| **10** | 4 | ROS2 bridge + ROS2 tools | tokio-tungstenite | ~300 |
| **11** | — | Integration tests + verification | — | ~200 |
| | | **Total** | | **~3,100** |

### Dependency Summary

| Crate | Feature Gate | Used By | Size Impact |
|-------|-------------|---------|------------|
| `tokio-serial` | `serial` | Serial bridge (Task 7) | ~50KB |
| `nokhwa` | `camera` | Camera capture (Task 8) | ~200KB (depends on v4l2) |
| `cpal` | `voice` | Audio capture + playback (Task 9) | ~100KB (depends on ALSA) |
| `tokio-tungstenite` | `ros2` | ROS2 bridge (Task 10) | ~80KB |
| `image` | — (already optional) | Motion detection (Task 8) | May already be present |

No `ort` (ONNX Runtime) dependency in this phase. Local TTS (Piper) deferred to future phase.

### Task Dependencies

```
Task 0 (image support) ←── Task 8 (perception needs images)

Task 1 (bridge trait) ←── Task 4 (executor uses bridge)
                       ←── Task 7 (serial implements bridge)
                       ←── Task 10 (ros2 implements bridge)

Task 2 (robot.toml) ←── Task 3 (runtime loads description)
                     ←── Task 5 (tools auto-register from description)
                     ←── Task 6 (safety rules from description)

Task 3 (runtime + ToolContext) ←── Task 4 (executor wired into runtime)
                                ←── Task 5 (tools use ToolContext.action_tx)
                                ←── Task 6 (safety wired into runtime)

Task 4 (action executor) ←── Task 5 (tools send ActionCommands)
                          ←── Task 9 (Speak action plays audio)

Tasks 0-6 are the core. Tasks 7-10 are independent extensions. Task 11 is final.
```

### Communication Architecture (Simplified from v1)

```
                     ┌─────────────────────┐
                     │   World State        │
                     │   (watch channel)    │
                     │   1 writer, N readers│
                     └────────┬────────────┘
                              │
        writes ───────────────┤ reads
        │                     │                │
   Sensor Polling       Safety Monitor    Context Builder
   (200ms interval)     (200ms interval)  (per agent turn)

                     ┌─────────────────────┐
                     │   Action Channel     │
                     │   (mpsc)             │
                     └────────┬────────────┘
                              │
   Robot Tools ──sends──→     │ ──receives──→ Action Executor ──→ Bridge
   Safety Monitor ──sends──→  │

                     ┌─────────────────────┐
                     │   Agent Channel      │
                     │   (mpsc, existing)   │
                     └────────┬────────────┘
                              │
   Voice (STT) ──sends──→    │
   HTTP/MQTT ──sends──→      │ ──receives──→ Agent Worker
   Perception events ──sends──→│
```

Three channels. Clean. No over-engineering.
