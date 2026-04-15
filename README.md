# UniClaw

[![CI](https://github.com/cortexa-core/uniclaw/actions/workflows/ci.yml/badge.svg)](https://github.com/cortexa-core/uniclaw/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/cortexa-core/uniclaw)](https://github.com/cortexa-core/uniclaw/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

An open-source AI brain for physical robots.

UniClaw gives any robot the ability to perceive, reason, remember, and act — running on a $10-50 SBC with cloud LLM inference. Describe your robot in a TOML file, and UniClaw handles the rest: vision via cloud VLMs, voice via STT/TTS, tool-calling for actions, memory across sessions, and safety monitoring.

```
$ uniclaw robot -m "wave at me"
Buddy> [set_servo left_arm to 180°] [wait 500ms] [set_servo left_arm to 90°]
       Hello! *waves* Nice to see you!

$ uniclaw robot
UniClaw Robot v0.1.2 | Buddy
Type 'exit' to quit.

You> What do you see?
Buddy> [take_photo] I see you sitting at your desk with a laptop.
       There's a coffee mug to your left.

You> Remember where my keys are
Buddy> [memory_store] Got it — your keys are on the desk, left side.
```

## What Makes UniClaw Different

This is **not** another chat agent runtime. UniClaw is built for **embodied AI** — robots that exist in the physical world.

| Chat Agent (OpenClaw, etc.) | Physical Agent (UniClaw) |
|-----------------------------|--------------------------|
| Request/response | Continuous — world doesn't wait |
| Reversible (edit a file) | Irreversible (hit a wall) |
| Text in, text out | Camera + mic + sensors in, motors + speech + LEDs out |
| "Don't leak API keys" | "Don't drive off the table" |
| 2-10s latency is fine | 50ms for reflexes, 2s for thinking |

### Two Control Loops

```
FAST (50-100Hz, MCU)          SLOW (0.5-2Hz, SBC + Cloud LLM)
─────────────────────         ────────────────────────────────
Sensors → Safety → Motors     Perception → Reasoning → Action Plan
No LLM. No internet.          Cloud LLM for thinking.
Works if SBC crashes.          Memory, personality, conversation.
The "spinal cord."             The "brain."
```

## Target Use Cases

| Use Case | Hardware | What UniClaw Does |
|----------|----------|-------------------|
| Desktop companion | RPi 4 + Arduino + servos + camera + speaker | See, hear, speak, wave, remember conversations |
| Robot car / rover | RPi + ESP32 + motors + ultrasonic + camera | Navigate, avoid obstacles, explore, report |
| Home assistant bot | RPi + wheels + camera + mic + speaker | Patrol, water plants, security checks |
| Educational robot | RPi Zero 2W + servos + LEDs + speaker | Teach, respond to touch, play games |

## Quick Start

```bash
git clone https://github.com/cortexa-core/uniclaw.git
cd uniclaw
cargo build --release

# Initialize
./target/release/uniclaw init

# Set your LLM API key
export OPENAI_API_KEY="your-key"

# Chat mode (text agent, no hardware)
./target/release/uniclaw chat

# Robot mode (with robot.toml)
./target/release/uniclaw robot

# Server mode (Web UI + HTTP API + MQTT)
./target/release/uniclaw serve
# Open http://localhost:3000
```

## Robot Description (robot.toml)

Describe your robot once. UniClaw auto-registers the right tools, injects the description into the LLM prompt, and enforces safety rules.

```toml
[robot]
name = "Buddy"
type = "desktop_companion"
description = "A small desktop robot with camera, arms, and speaker"

[[sensors]]
name = "camera"
type = "camera"
device = "/dev/video0"

[[sensors]]
name = "front_distance"
type = "ultrasonic"
pin = 7

[[actuators]]
name = "left_arm"
type = "servo"
pin = 12
angle_range = [0, 180]

[[actuators]]
name = "speaker"
type = "audio_output"

[[actuators]]
name = "led_ring"
type = "neopixel"
pin = 18
count = 12

[safety]
watchdog_timeout_ms = 500

[[safety.rules]]
name = "obstacle_stop"
condition = "front_distance < 10"
action = "stop_all_motors"
priority = 10

[hardware]
bridge = "mock"  # or "serial", "ros2"
```

The LLM sees: *"You are Buddy, a small desktop robot with a camera, two servo arms (0-180°), a speaker, and a 12-LED ring."* It only gets tools matching your hardware — no servos, no `set_servo` tool.

## 40+ LLM Providers

Just change `provider = "groq"` in config. Aliases auto-resolve to the correct URL and auth.

```toml
[llm]
provider = "groq"              # or "anthropic", "gemini", "openai", "deepseek", ...
api_key_env = "GROQ_API_KEY"
model = "llama-3.3-70b-versatile"

[llm.fallback]
provider = "ollama"
model = "qwen3:0.6b"
```

**Supported:** OpenAI, Anthropic, Gemini (native), OpenRouter, Groq, DeepSeek, Mistral, xAI/Grok, Together, Fireworks, Cerebras, Perplexity, Cohere, Ollama, LM Studio, vLLM, LiteLLM, llama.cpp, Qwen, GLM/Zhipu, Moonshot/Kimi, MiniMax, Doubao/VolcEngine, Stepfun, Baichuan, Yi, DeepInfra, HuggingFace, Venice, NVIDIA NIM, SambaNova, Hyperbolic, and any OpenAI-compatible endpoint.

**Infrastructure:** Reliable provider with retry + exponential backoff + error classification. Router with hint-based model selection (`hint:fast` → Groq, `hint:reasoning` → Claude). Real SSE streaming from all 3 native backends.

## Hardware Bridges

Same brain, different body. Config-only switch.

```
uniclaw robot (bridge = "mock")    → Development, no hardware needed
uniclaw robot (bridge = "serial")  → Real Arduino/ESP32 via UART
uniclaw robot (bridge = "ros2")    → Gazebo simulation OR real ROS2 robot
```

### Direct Serial (Simple Robots)

Binary protocol over UART to Arduino/ESP32/STM32. Reference firmware included at `firmware/arduino/uniclaw_mcu/`.

```toml
[hardware]
bridge = "serial"
port = "/dev/ttyUSB0"
baud_rate = 115200
```

### ROS2 via rosbridge (Complex Robots)

Connects to ROS2 via standard rosbridge WebSocket. Works with Gazebo, Isaac Sim, or real ROS2 hardware — **same config**.

```toml
[hardware]
bridge = "ros2"

[hardware.ros2]
url = "ws://localhost:9090"
cmd_vel_topic = "/cmd_vel"
odom_topic = "/odom"
navigate_action = "/navigate_to_pose"
```

UniClaw decides **what** to do (intent, reasoning, memory). ROS2 handles **how** (path planning, kinematics, motor control).

## Perception

Cloud vision LLM describes what the camera sees. Smart triggering keeps costs low (~$1-3/day).

```toml
[perception]
vision_provider = "gemini"
vision_model = "gemini-2.0-flash"
vision_trigger = "event"       # or "periodic", "on_demand"
motion_detection = true
```

The LLM context includes real-time perception:
```
## Current Perception
Scene (5s ago): I see Jiekai at their desk, working on a laptop. Coffee mug to the left.
Motion: none
Sensor front_distance: 85cm
Battery: 72%
```

## Voice

Cloud STT (Whisper) + Cloud TTS for voice interaction. Energy-based VAD detects speech without ML dependencies.

```toml
[voice]
stt_provider = "whisper"
tts_engine = "cloud"
vad_enabled = true
```

## Safety

Physical safety is not optional. Safety rules run every 200ms, independently of the LLM.

```toml
[[safety.rules]]
name = "obstacle_stop"
condition = "front_distance < 10"   # cm
action = "stop_all_motors"
priority = 10

[[safety.rules]]
name = "tilt_protection"
condition = "imu_tilt > 30"         # degrees
action = "emergency_stop"
priority = 10
```

**Layers:**
1. MCU hardware watchdog — stops motors if SBC goes silent for 500ms
2. Safety monitor — evaluates rules against sensor data, no LLM involved
3. Agent reasoning — LLM can assess risk for novel situations

## Built-in Tools

### Standard Tools (always available)

| Tool | What |
|------|------|
| `get_time` | Current date/time |
| `read_file` / `write_file` / `edit_file` / `list_dir` | File operations (path-sandboxed) |
| `memory_store` / `memory_read` | Long-term memory |
| `system_info` | CPU, RAM, temp, uptime |
| `shell_exec` | Sandboxed shell (command whitelist) |
| `http_fetch` | Fetch URLs (credential leak scanning) |
| `cron_add` / `cron_list` / `cron_remove` | Schedule tasks |

### Robot Tools (auto-registered from robot.toml)

| Tool | Requires | What |
|------|----------|------|
| `set_servo` | Any servo actuator | Move servo to angle |
| `set_led` | LED/neopixel actuator | Set LED color |
| `say` | Speaker actuator | Speak text via TTS |
| `stop` | Any actuator | Emergency stop all |
| `get_sensor` | Any sensor | Read sensor value |
| `get_world_state` | — | Full perception summary |
| `describe_scene` | Camera | Latest scene description |
| `take_photo` | Camera | Capture + describe |

### ROS2 Tools (when bridge = "ros2")

| Tool | What |
|------|------|
| `navigate_to` | Send navigation goal |
| `ros2_publish` | Publish to any topic |
| `ros2_call_service` | Call any ROS2 service |

## Memory

- **Episodic**: Conversation history per session (JSONL, persists across restarts)
- **Semantic**: Long-term facts in `MEMORY.md` via `memory_store` tool
- **Consolidation**: Old messages auto-summarized by LLM to keep context lean
- **Daily notes**: Date-stamped observations

## Web UI

Built-in dashboard at `http://localhost:3000` (embedded in binary):
- Chat with streaming, markdown, tool call indicators
- Status dashboard with auto-refresh
- Config editor, skills viewer
- Dark/light theme, mobile responsive, PWA installable

## Additional Features

- **MCP support** — Connect any MCP server, tools auto-discovered
- **Skills** — Drop `.md` files in `data/skills/` to teach domain expertise
- **Telegram** — Chat via Telegram bot (`--features telegram`)
- **MQTT** — IoT device communication, robot telemetry
- **Cron + Heartbeat** — Scheduled tasks and proactive task polling
- **SSE Streaming** — Real-time token streaming from all providers

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                    UniClaw (single Rust binary)                │
│                                                                │
│  ┌─────────── ROBOT RUNTIME ──────────────┐                  │
│  │ Perception → World State → Safety       │                  │
│  └──────────────────┬─────────────────────┘                  │
│                     │                                          │
│  ┌──────────────────▼─────────────────────┐                  │
│  │              AGENT BRAIN                │                  │
│  │  Context → ReAct Loop → Tool Executor   │                  │
│  │  SOUL.md + robot.toml + memory + skills │                  │
│  └──────────────────┬─────────────────────┘                  │
│                     │                                          │
│  ┌──────────────────▼─────────────────────┐                  │
│  │           HARDWARE BRIDGE               │                  │
│  │  mock | serial | ros2                   │                  │
│  └──────────────────┬─────────────────────┘                  │
│                     │                                          │
│  Cloud LLM API ◄── only remote call                           │
│  (40+ providers)                                              │
└──────────────────────┬───────────────────────────────────────┘
                       │
            ┌──────────▼──────────┐
            │ MCU / Gazebo / ROS2 │
            │ Motors, servos, LEDs │
            └─────────────────────┘
```

## Building from Source

```bash
# Native build
cargo build --release

# With Telegram
cargo build --release --features telegram

# With serial bridge (for real Arduino/ESP32)
cargo build --release --features serial

# With ROS2 bridge (for Gazebo / ROS2 robots)
cargo build --release --features ros2

# Cross-compile for Raspberry Pi
cargo zigbuild --target aarch64-unknown-linux-gnu --release
```

### Run tests

```bash
cargo test                          # ~220 tests
cargo test --features telegram      # includes Telegram channel tests
cargo clippy -- -D warnings         # lint
```

## Project Structure

```
src/
  main.rs              CLI entry point
  commands/
    chat.rs            Interactive chat mode
    serve.rs           Server mode (HTTP + MQTT + cron)
    robot.rs           Robot mode (continuous perception + action)
  agent/
    loop.rs            ReAct agent loop
    context.rs         System prompt builder (+ robot context injection)
    memory.rs          Session store + memory consolidation
    skills.rs          Skill loader with gating
  llm/
    types.rs           Message types (text, image, tool calls)
    anthropic.rs       Anthropic Messages API + streaming
    openai.rs          OpenAI-compatible API + streaming (40+ providers)
    gemini.rs          Google Gemini API + streaming
    reliable.rs        Retry, backoff, error classification, failover
    router.rs          Hint-based multi-provider routing
    aliases.rs         40+ provider alias resolution
  robot/
    description.rs     robot.toml parser
    runtime.rs         Sensor polling, action executor, safety orchestration
    world_state.rs     Shared perception state (watch channel)
    safety.rs          Declarative safety rules + expression parser
    perception.rs      Camera + motion detection + cloud VLM
    camera.rs          Camera capture abstraction
    voice.rs           VAD + cloud STT + cloud TTS
    bridge/
      mod.rs           HardwareBridge trait
      mock.rs          Mock bridge (development)
      serial.rs        Serial protocol (Arduino/ESP32)
      ros2.rs          rosbridge WebSocket (Gazebo/ROS2)
  tools/
    registry.rs        Tool trait + dispatch
    robot_actions.rs   Robot tools (servo, LED, say, stop, sensor)
    perception_tools.rs  Vision tools (describe_scene, take_photo)
    ros2_tools.rs      ROS2 tools (navigate, publish, service)
    (+ 10 standard tools)
  server/              HTTP API, MQTT, cron, heartbeat, Web UI
  channels/            Telegram (feature-gated)
  mcp/                 MCP client (stdio + HTTP)
firmware/
  arduino/uniclaw_mcu/ Reference Arduino sketch for serial protocol
data/
  robot.toml           Robot description (example: desktop companion)
  SOUL.md              Agent personality
  skills/              Skill markdown files
```

## License

MIT
