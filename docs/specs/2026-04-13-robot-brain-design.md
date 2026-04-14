# UniClaw Robot Brain — End-to-End Architecture Design

**Date**: 2026-04-13
**Version**: v1.0
**Status**: Design review

---

## 1. Strategic Pivot

### Why

The "OpenClaw for cheap hardware" space is saturated — ZeroClaw (29K stars), NullClaw (7K), PicoClaw, IronClaw all compete on the same axis: more providers, more channels, more tools, smaller binary. Diminishing returns.

### New Position

**UniClaw is an open-source AI brain for physical robots.** It gives any robot the ability to perceive, reason, remember, and act — running on a $10-50 SBC with cloud LLM inference.

Nobody does this well. ROS2 is heavy and not LLM-native. NVIDIA Isaac requires expensive hardware. ZeroClaw's GPIO support is bolted on. None of them think about embodiment, perception-action loops, or physical safety as core design principles.

### Target Use Cases

| Use Case | Hardware | Key Capabilities |
|----------|----------|-----------------|
| Desktop companion robot | RPi 4 + Arduino + servos + camera + mic + speaker | See, hear, speak, wave, nod, remember conversations |
| Robot car / rover | RPi + ESP32 + motors + ultrasonic + camera | Navigate, avoid obstacles, explore, report findings |
| Home assistant bot | RPi + wheels + camera + mic + speaker | Patrol, check on things, water plants, security |
| Educational robot | RPi Zero 2W + servos + LEDs + speaker | Teach, respond to touch, play games |
| Pet robot | Any SBC + servos + touch sensor + speaker | React to touch, follow, learn routines |

### First Demo Target

A desktop companion robot that:
- Sees you via camera (cloud vision LLM)
- Hears you via microphone (cloud Whisper STT)
- Speaks via speaker (local Piper TTS)
- Moves head and arms (servo actuators via Arduino)
- Shows emotion via LEDs
- Remembers your conversations and preferences
- Has a personality defined in SOUL.md
- Runs on Raspberry Pi 4 + Arduino Nano

---

## 2. What's Different About Physical AI

A physical agent is NOT "chat agent + GPIO." The fundamental model changes:

| Dimension | Chat Agent | Physical Agent |
|-----------|-----------|----------------|
| **Time model** | Request/response | Continuous — world doesn't wait |
| **Consequences** | Reversible (edit file, redo) | Irreversible (hit wall, broke glass) |
| **Perception** | Text from user | Camera + mic + touch + distance + IMU, all at once, always |
| **Action** | Text output (instant) | Physical movement (takes time, can fail mid-way) |
| **Safety** | "Don't leak keys" | "Don't drive off table" — must work even if LLM is down |
| **Latency** | 2-10s acceptable | 50ms for reflexes, 500ms for reactions, 2s for thinking |
| **Failure** | Error message | Physical damage |
| **Identity** | "I'm an AI assistant" | "I'm a 20cm robot with 2 arms and a camera" |

---

## 3. System Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                    UniClaw Robot Brain                         │
│                    (Single Rust binary on SBC)                │
│                                                                │
│  ┌─────────────────── ROBOT RUNTIME ───────────────────────┐ │
│  │                                                          │ │
│  │  ┌─────────────┐  ┌─────────────┐  ┌────────────────┐  │ │
│  │  │ Perception  │  │   Safety    │  │   Behavior     │  │ │
│  │  │  Pipeline   │  │  Monitor    │  │   Evaluator    │  │ │
│  │  │  (camera,   │  │ (always on, │  │  (idle, greet, │  │ │
│  │  │   mic,      │  │  no LLM)   │  │   follow,etc)  │  │ │
│  │  │   sensors)  │  │             │  │                │  │ │
│  │  └──────┬──────┘  └──────┬──────┘  └───────┬────────┘  │ │
│  │         │                │                  │           │ │
│  │         ▼                ▼                  ▼           │ │
│  │  ┌──────────────────────────────────────────────────┐  │ │
│  │  │               World State (watch channel)         │  │ │
│  │  │  Objects, faces, audio, sensors, body position    │  │ │
│  │  └──────────────────────┬───────────────────────────┘  │ │
│  │                         │                               │ │
│  └─────────────────────────┼───────────────────────────────┘ │
│                            │                                  │
│  ┌─────────────────────────▼───────────────────────────────┐ │
│  │                    AGENT BRAIN                           │ │
│  │                  (existing UniClaw core)                 │ │
│  │                                                          │ │
│  │  Context Builder → Agent Loop (ReAct) → Tool Executor   │ │
│  │       │                  │                    │          │ │
│  │  SOUL.md + robot.toml   LLM (40+ providers)  Actions    │ │
│  │  + world state          + vision support      + tools    │ │
│  │  + memory               + streaming                      │ │
│  └──────────────────────────┬──────────────────────────────┘ │
│                             │                                 │
│  ┌──────────────────────────▼──────────────────────────────┐ │
│  │                   ACTION EXECUTOR                        │ │
│  │  Translates action plans to hardware commands            │ │
│  │  Monitors execution, reports completion/failure          │ │
│  └──────────────────────────┬──────────────────────────────┘ │
│                             │                                 │
│  ┌──────────────────────────▼──────────────────────────────┐ │
│  │                   HARDWARE BRIDGE                        │ │
│  │  Serial/I2C protocol to MCU                              │ │
│  │  Watchdog timer, e-stop support                          │ │
│  └──────────────────────────┬──────────────────────────────┘ │
│                             │                                 │
└─────────────────────────────┼─────────────────────────────────┘
                              │ Serial/USB
┌─────────────────────────────▼─────────────────────────────────┐
│              MCU (Arduino/ESP32/STM32)                          │
│  Motors, Servos, LEDs, Distance sensors, IMU, Touch sensors    │
│  Fast loop: 50-100Hz, safety watchdog, PID control             │
└────────────────────────────────────────────────────────────────┘
```

### Two Control Loops

**Fast loop (50-100Hz)** — runs on MCU:
- Read sensors, apply PID, send motor commands
- Hardware watchdog: if no command from SBC for 500ms, stop all motors
- Collision avoidance: if distance < threshold, stop
- This is the "spinal cord" — works even if SBC crashes

**Slow loop (0.5-2Hz)** — runs on SBC via UniClaw:
- Aggregate perception → reason via LLM → generate action plan
- The LLM is NOT in the control loop — it's an advisor
- The robot runtime continuously processes sensor data; the LLM is consulted when the robot needs to think

---

## 4. Process Model

Extends the current UniClaw Tokio task model:

```
main()
  ├── task: robot_runtime          (NEW — continuous sense/decide/act loop)
  │     ├── subtask: perception    (camera capture, VLM calls, sensor polling)
  │     ├── subtask: safety        (monitor world state, enforce limits)
  │     └── subtask: behavior      (evaluate active behavior, trigger actions)
  │
  ├── task: agent_worker           (EXISTING — processes voice/text/behavior inputs)
  ├── task: action_executor        (NEW — translates plans to hardware commands)
  ├── task: hardware_bridge        (NEW — serial comms to MCU)
  ├── task: voice_pipeline         (NEW — STT input + TTS output)
  │
  │  Existing (unchanged):
  ├── task: http_server
  ├── task: mqtt_client
  ├── task: cron_scheduler
  └── task: heartbeat_service
```

### Communication

```rust
// World state: perception writes, everyone reads
let (world_tx, world_rx) = tokio::sync::watch::channel(WorldState::default());

// Actions: agent/behavior → executor
let (action_tx, action_rx) = tokio::sync::mpsc::channel::<ActionCommand>(32);

// Perception events: perception → agent (triggers LLM consultation)
let (event_tx, event_rx) = tokio::sync::mpsc::channel::<PerceptionEvent>(32);

// Hardware commands: executor → bridge
let (hw_tx, hw_rx) = tokio::sync::mpsc::channel::<HardwareCommand>(64);

// Sensor data: bridge → perception
let (sensor_tx, sensor_rx) = tokio::sync::mpsc::channel::<SensorReading>(64);

// Agent input: existing channel (voice/text/events → agent)
let (agent_tx, agent_rx) = tokio::sync::mpsc::channel::<(Input, oneshot::Sender<Output>)>(32);
```

Uses `tokio::sync::watch` for world state — lock-free reads, single writer. No `Arc<Mutex>`.

---

## 5. Robot Description Format

`robot.toml` — describes the physical robot. The agent runtime reads this at startup and injects it into the LLM system prompt.

```toml
[robot]
name = "Buddy"
type = "desktop_companion"  # or "rover", "arm", "pet", "custom"
description = "A small desktop robot with a camera, speaker, and two servo-driven arms"

[body]
base = "fixed"              # "fixed", "differential_drive", "mecanum", "omnidirectional"
weight_kg = 0.5
height_cm = 20

# --- Sensors ---

[[sensors]]
name = "camera"
type = "camera"
device = "/dev/video0"
resolution = "640x480"
fps = 30

[[sensors]]
name = "microphone"
type = "microphone"
device = "default"

[[sensors]]
name = "front_distance"
type = "ultrasonic"
pin = 7                     # MCU pin
max_range_cm = 200
poll_interval_ms = 100

[[sensors]]
name = "imu"
type = "imu"
bus = "i2c"
address = 0x68

# --- Actuators ---

[[actuators]]
name = "left_arm"
type = "servo"
pin = 12
angle_range = [0, 180]
default_angle = 90

[[actuators]]
name = "right_arm"
type = "servo"
pin = 13
angle_range = [0, 180]
default_angle = 90

[[actuators]]
name = "head_pan"
type = "servo"
pin = 14
angle_range = [-45, 45]
default_angle = 0

[[actuators]]
name = "head_tilt"
type = "servo"
pin = 15
angle_range = [-30, 30]
default_angle = 0

[[actuators]]
name = "speaker"
type = "audio_output"
device = "default"

[[actuators]]
name = "led_ring"
type = "neopixel"
pin = 18
count = 12

# --- Safety ---

[safety]
watchdog_timeout_ms = 500
emergency_stop_pin = 4       # Physical button on MCU

[[safety.rules]]
name = "obstacle_stop"
condition = "front_distance < 10"
action = "stop_all_motors"
priority = 10

[[safety.rules]]
name = "tilt_protection"
condition = "imu.tilt > 30"
action = "stop_all_motors"
priority = 10

[[safety.rules]]
name = "low_battery_warning"
condition = "battery < 15"
action = "speak:Battery is low"
priority = 5

# --- Behaviors ---

[behaviors]
idle = "look_around"
on_face_detected = "greet"
on_touch = "react_to_touch"
on_name_called = "attention"

# --- Perception ---

[perception]
vision_provider = "gemini"
vision_model = "gemini-2.0-flash"
vision_trigger = "event"         # "periodic", "event", "on_demand"
vision_periodic_secs = 30
motion_detection = true

# --- Voice ---

[voice]
stt_provider = "whisper"         # "whisper" (cloud), "whisper_local" (future)
stt_model = "whisper-1"
tts_engine = "piper"             # "piper" (local), "cloud" (future)
tts_model = "en_US-amy-medium"
tts_speed = 1.0
vad_enabled = true               # Voice Activity Detection
wake_word = "hey buddy"          # Optional wake word

# --- Hardware Bridge ---

[hardware]
bridge = "serial"                # "serial", "i2c", "mock"
port = "/dev/ttyUSB0"
baud_rate = 115200
```

This file is:
1. Read at startup to configure the runtime
2. Injected into the LLM system prompt: "You are Buddy, a 20cm desktop robot with a camera, two arms, a speaker, and an LED ring..."
3. Used by the action executor to validate commands (e.g., don't set servo beyond angle_range)
4. Used by the safety monitor to enforce rules

---

## 6. World State

Shared view of the physical world, updated by perception pipeline:

```rust
pub struct WorldState {
    pub timestamp: Instant,

    // Vision (from cloud VLM or local CV)
    pub scene_description: Option<String>,
    pub scene_timestamp: Option<Instant>,
    pub motion_detected: bool,

    // Audio
    pub last_speech: Option<(Instant, String)>,
    pub voice_active: bool,

    // Sensors (from MCU)
    pub sensors: HashMap<String, SensorValue>,

    // Body state
    pub actuator_positions: HashMap<String, f32>,
    pub battery_percent: Option<f32>,
    pub is_moving: bool,

    // Active state
    pub current_behavior: Option<String>,
    pub last_action: Option<(Instant, String)>,
}

pub enum SensorValue {
    Distance(f32),       // cm
    Temperature(f32),    // celsius
    Orientation(f32, f32, f32),  // roll, pitch, yaw
    Boolean(bool),       // touch, button
    Raw(i32),            // generic ADC value
}
```

Published via `tokio::sync::watch` — one writer (perception pipeline), many readers (safety, behavior, agent context builder).

---

## 7. Perception Pipeline

### Phase 1: Cloud VLM Only

```
Camera ──→ Frame buffer (latest JPEG) ──→ On demand/event ──→ Cloud VLM
                                                                  │
Microphone ──→ VAD ──→ STT (Whisper API) ──→ Text ───────────────┤
                                                                  │
MCU Sensors ──→ Serial read ──→ Structured data ─────────────────┤
                                                                  ▼
                                                          World State update
```

**Camera capture**: Use `v4l2` crate (Linux native, pure Rust) or `nokhwa` crate. Capture frames at configured FPS, keep latest frame in buffer. Feature-gated: `#[cfg(feature = "camera")]`.

**Motion detection**: Simple frame differencing — compare current frame to previous, count changed pixels. If change > threshold, set `motion_detected = true`. Pure Rust, ~50 LOC using `image` crate.

**VLM calls**: Use existing UniClaw vision-capable providers (GPT-4o, Gemini Flash). Encode frame as base64 JPEG, include in message. Triggered by:
- Motion detected (debounced, max 1 call per 5 seconds)
- Agent requests via `look_around` tool
- Periodic timer (configurable, default every 30s when idle)
- User asks "what do you see?"

**Cost control**: VLM calls are the expensive part. Smart triggering keeps costs to ~$1-3/day for an active companion robot.

### Phase 2 (future): Local CV + Cloud VLM

Add ONNX Runtime or OpenCV (feature-gated) for continuous local detection:
- Object detection (YOLO-nano, 5-10fps on RPi 4)
- Face detection + recognition
- Gesture recognition

Local CV handles real-time awareness; cloud VLM handles understanding.

---

## 8. Action System

### Action Types

```rust
pub enum ActionCommand {
    // Actuator control
    SetServo { name: String, angle: f32, speed_deg_s: Option<f32> },
    SetMotor { name: String, speed: f32, duration_ms: Option<u64> },
    SetLed { name: String, color: [u8; 3], brightness: f32 },
    SetLedPattern { name: String, pattern: String },  // "pulse", "rainbow", "breathe"

    // Composite
    Speak { text: String },
    PlaySound { file: String },
    CapturePhoto,
    LookAt { pan: f32, tilt: f32 },

    // Movement (for mobile robots)
    Move { direction_deg: f32, distance_cm: f32, speed: f32 },
    Rotate { degrees: f32, speed: f32 },

    // Meta
    Stop,                          // Stop all actuators
    EmergencyStop,                 // E-stop, highest priority
    StartBehavior { name: String },
    StopBehavior,
    Wait { duration_ms: u64 },
}
```

### LLM-Facing Tools

The LLM doesn't generate `ActionCommand` directly. It calls tools that translate to actions:

```
LLM tool calls         →  Action executor  →  Hardware bridge  →  MCU
─────────────────         ────────────────     ───────────────     ────
move_forward(50cm)    →  Move{0°, 50cm}    →  MOTOR_SET cmd    →  wheels
wave()                →  Sequence[          →  SERVO_SET cmds   →  servo
                          SetServo(arm,180),
                          Wait(500ms),
                          SetServo(arm,90)]
say("hello")          →  Speak{"hello"}    →  Piper TTS        →  speaker
look_at(person)       →  LookAt{pan,tilt}  →  SERVO_SET cmds   →  head servos
set_emotion(happy)    →  SetLedPattern{    →  LED commands      →  LEDs
                          "pulse", green}
take_photo()          →  CapturePhoto      →  v4l2 capture     →  VLM
```

### Robot Action Tools (registered based on robot.toml capabilities)

| Tool | Required Actuator | Description |
|------|------------------|-------------|
| `say` | speaker | Speak text via TTS |
| `move_forward` | motor (differential_drive) | Move forward N cm |
| `move_backward` | motor | Move backward N cm |
| `turn_left` / `turn_right` | motor | Rotate N degrees |
| `stop` | any motor/servo | Stop all movement |
| `look_at` | head_pan + head_tilt servos | Point camera direction |
| `wave` | arm servo | Wave gesture |
| `nod` | head_tilt servo | Nod gesture |
| `set_emotion` | led | Set LED color/pattern for emotion |
| `take_photo` | camera | Capture and describe scene |
| `play_sound` | speaker | Play audio file |
| `set_servo` | any servo | Direct servo control |
| `get_sensor` | any sensor | Read sensor value |
| `remember_location` | — | Save current location to spatial memory |
| `go_to` | motors | Navigate to remembered location |

Tools are **auto-registered** based on what `robot.toml` declares. If the robot has no arms, the `wave` tool doesn't exist. If no wheels, no `move_forward`. The LLM only sees tools that the robot physically has.

---

## 9. Safety Layer

### Principles

1. **Safety is not optional.** It runs even if the LLM is down, network is down, or SBC is overloaded.
2. **Hardware is the last resort.** MCU watchdog stops motors if SBC goes silent.
3. **Rules are declarative.** Defined in `robot.toml`, not hardcoded.
4. **Higher priority wins.** Emergency stop > obstacle avoidance > behavior.

### Implementation

```rust
pub struct SafetyMonitor {
    rules: Vec<SafetyRule>,
    world_rx: watch::Receiver<WorldState>,
    action_tx: mpsc::Sender<ActionCommand>,
}

// Runs as continuous task, checks every 100ms
async fn run(&mut self) {
    loop {
        let state = self.world_rx.borrow().clone();
        for rule in &self.rules {
            if rule.evaluate(&state) {
                self.action_tx.send(rule.action.clone()).await.ok();
                tracing::warn!("Safety rule '{}' triggered", rule.name);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
```

### Safety Rule Evaluation

Rules in `robot.toml` use simple expressions:

```
"front_distance < 10"    → SensorValue::Distance(d) where d < 10.0
"imu.tilt > 30"          → SensorValue::Orientation where pitch/roll > 30°
"battery < 15"           → battery_percent < 15.0
```

A simple expression parser (~100 LOC) handles `sensor_name operator value` patterns. No need for a full expression language.

---

## 10. Hardware Bridge

### Serial Protocol (SBC ↔ MCU)

Binary protocol over UART (115200 baud default):

```
Frame: [0xAA] [LEN:u16] [SEQ:u8] [TYPE:u8] [PAYLOAD:...] [CRC:u8]

Commands (SBC → MCU):
  0x01 SERVO_SET      { id:u8, angle:u16, speed:u16 }
  0x02 MOTOR_SET      { id:u8, speed:i16, duration_ms:u16 }
  0x03 LED_SET        { id:u8, r:u8, g:u8, b:u8 }
  0x04 LED_PATTERN    { id:u8, pattern:u8, r:u8, g:u8, b:u8 }
  0x05 SENSOR_REQUEST { id:u8 }
  0x06 PING           { }
  0x07 E_STOP         { }
  0x08 STATUS_REQUEST { }
  0x09 HEARTBEAT      { }     // SBC alive signal

Responses (MCU → SBC):
  0x81 ACK            { seq:u8, cmd:u8 }
  0x82 SENSOR_DATA    { id:u8, type:u8, value:i32 }
  0x83 STATUS         { battery:u8, error_flags:u8, sensor_count:u8 }
  0x84 ERROR          { code:u8, seq:u8 }
  0x85 PONG           { }
  0x86 EVENT          { type:u8, data:... }  // MCU-initiated (button press, collision)
```

### Watchdog

MCU expects `HEARTBEAT` command every `watchdog_timeout_ms`. If missed:
1. First miss: MCU reduces all motor speeds to 50%
2. Second miss: MCU stops all motors
3. Third miss: MCU enters safe mode (LEDs flash red)

### Mock Backend

For development without hardware: `bridge = "mock"` in robot.toml. The mock bridge logs all commands and returns synthetic sensor data. Allows full testing of the robot brain without physical hardware.

---

## 11. Voice Pipeline

### STT (Speech-to-Text)

**Phase 1**: Cloud Whisper API
- Microphone → VAD (voice activity detection) → record audio clip → send to Whisper API → text
- VAD: simple energy-threshold detection (when audio level > threshold for > 300ms, start recording; when < threshold for > 500ms, stop)
- Audio format: 16kHz mono PCM → encode as WAV
- Use existing HTTP provider infrastructure

**Future**: Local Whisper.cpp for offline operation

### TTS (Text-to-Speech)

**Phase 1**: Local Piper TTS
- Text → Piper (ONNX-based, ~50MB model) → WAV → speaker
- Runs at 2-5x realtime on RPi 4
- Multiple voices available, configured in robot.toml
- Dependency: `ort` crate (ONNX Runtime) — same dependency needed for future local CV

**Why local TTS**: Latency. Cloud TTS adds 500ms-2s. Local Piper gives <200ms. For a robot that should respond naturally, local TTS is essential.

### Voice Activity Detection (VAD)

Simple energy-based VAD (no ML dependency):
1. Compute RMS energy of each audio frame (20ms)
2. Maintain adaptive noise floor (slow-moving average)
3. Speech detected when energy > noise_floor * threshold for > 300ms
4. Speech ended when energy < noise_floor * threshold for > 500ms
5. Record the speech segment, send to STT

~80 LOC, pure Rust, no dependencies.

---

## 12. Memory Extensions

### Existing Memory (Reusable)

| Type | Storage | Use in Robot |
|------|---------|-------------|
| Episodic (session) | JSONL files | Conversation history per person |
| Semantic (long-term) | MEMORY.md | Learned facts about user, preferences |
| Consolidation | LLM-driven summarization | Keep sessions lean |

### New: Spatial Memory

The robot needs to know where things are. Simple JSON file, updated via tools:

**File**: `data/memory/spatial.json`
```json
{
  "locations": {
    "charger": {
      "description": "In the corner behind the desk",
      "relative": "behind-left",
      "last_visited": "2026-04-13T10:30:00Z"
    }
  },
  "objects": {
    "red_mug": {
      "description": "A red ceramic mug",
      "location": "on the desk, left side",
      "last_seen": "2026-04-13T14:22:00Z"
    }
  }
}
```

**Tools**: `remember_location(name, description)`, `remember_object(name, location)`, `where_is(name)`, `forget_location(name)`

The LLM updates spatial memory through tool calls as it perceives the environment. No SLAM needed — just the LLM's spatial reasoning.

### New: Procedural Memory (Behaviors)

Named behavior sequences stored as TOML files in `data/behaviors/`:

**File**: `data/behaviors/greeting.toml`
```toml
[behavior]
name = "greeting"
description = "Wave and say hello when seeing a person"

[[steps]]
action = "look_at"
params = { target = "face" }

[[steps]]
action = "wave"

[[steps]]
action = "say"
params = { text = "Hello! Nice to see you!" }
```

Behaviors are triggered by perception events or requested by the LLM. The behavior evaluator reads these files and executes the step sequences.

---

## 13. Agent Context Changes

The existing context builder assembles: SOUL.md + USER.md + MEMORY.md + daily notes + skills.

For the robot brain, add:

```
## Robot Body
You are Buddy, a 20cm tall desktop companion robot.
Your body: camera (640x480), left arm (servo, 0-180°), right arm (servo, 0-180°),
head pan (servo, -45 to 45°), head tilt (servo, -30 to 30°), speaker, 12 NeoPixel LEDs.
Your base is fixed (you cannot move around the room).

## Current Perception
[Updated every context build]
Time: 2026-04-13 14:30:22 CST
Scene (15s ago): I see Jiekai at their desk, working on a laptop. Coffee mug to the left.
Motion: none
Distance (front): 85cm
Battery: 72%
Current behavior: idle (looking around slowly)
Body: head_pan=0°, head_tilt=0°, left_arm=90°, right_arm=90°, leds=blue-breathe

## Spatial Memory
Charger: behind-left corner
Red mug: on desk, left side (last seen 2 min ago)
```

This gives the LLM full awareness of its body, environment, and situation.

---

## 14. ROS2 Integration (Optional, Future)

For complex robots that need SLAM, motion planning, or inverse kinematics:

```toml
[ros2]
enabled = true
bridge = "topic"   # "topic" (pub/sub) or "service" (request/response)

[[ros2.subscriptions]]
topic = "/scan"
type = "sensor_msgs/LaserScan"
as_sensor = "lidar"

[[ros2.publications]]
topic = "/cmd_vel"
type = "geometry_msgs/Twist"

[[ros2.services]]
name = "/move_base/goal"
type = "move_base_msgs/MoveBaseGoal"
```

Implemented as tools: `ros2_publish(topic, data)`, `ros2_subscribe(topic)`, `ros2_call_service(service, request)`.

The bridge communicates with ROS2 via its DDS protocol or through a simple REST bridge node. This is a Phase 5+ feature.

---

## 15. File System Layout

```
data/
  SOUL.md                    # Agent personality
  USER.md                    # User context
  robot.toml                 # Robot description (NEW)
  memory/
    MEMORY.md                # Semantic memory (existing)
    spatial.json             # Spatial memory (NEW)
    YYYY-MM-DD.md            # Daily notes (existing)
  sessions/
    *.jsonl                  # Conversation sessions (existing)
  skills/
    *.md                     # Agent skills (existing)
  behaviors/                 # (NEW)
    greeting.toml
    idle.toml
    patrol.toml
  sounds/                    # (NEW)
    startup.wav
    notification.wav
  tts_cache/                 # (NEW) Cached TTS audio
    *.wav
  cron.json                  # Scheduled tasks (existing)
  HEARTBEAT.md               # Proactive tasks (existing)
```

---

## 16. Module Structure

```
src/
  main.rs                     # CLI + server startup (existing, extend)
  config.rs                   # Configuration (existing, extend)
  lib.rs                      # Library exports

  agent/                      # Agent brain (existing)
    mod.rs
    loop.rs                   # ReAct agent loop
    context.rs                # Context builder (extend with robot/world state)
    memory.rs                 # Session + memory manager
    skills.rs                 # Skill system

  llm/                        # LLM providers (existing, complete)
    mod.rs, types.rs, aliases.rs
    anthropic.rs, openai.rs, gemini.rs
    reliable.rs, router.rs

  tools/                      # Tools (existing + new robot tools)
    mod.rs, registry.rs
    file_ops.rs, shell.rs, edit_file.rs, ...  # existing
    robot_actions.rs          # NEW: move, look, wave, speak, etc.
    spatial_memory.rs         # NEW: remember/recall locations
    perception_tools.rs       # NEW: take_photo, describe_scene

  robot/                      # NEW: Robot runtime
    mod.rs                    # Robot runtime orchestrator
    description.rs            # robot.toml parser
    world_state.rs            # WorldState struct + watch channel
    perception.rs             # Perception pipeline
    camera.rs                 # Camera capture (v4l2, feature-gated)
    action.rs                 # Action types + executor
    safety.rs                 # Safety monitor + rule evaluator
    behavior.rs               # Behavior system
    voice.rs                  # STT + TTS + VAD
    bridge/
      mod.rs                  # HardwareBridge trait
      mock.rs                 # MockBridge (development, logging)
      serial.rs               # SerialBridge (Arduino/ESP32 via UART)
      ros2.rs                 # RosBridge (rosbridge WebSocket → Gazebo/real HW)
      mujoco.rs               # MuJoCoBridge (future)

  server/                     # HTTP + MQTT (existing)
  channels/                   # Telegram etc. (existing)
  mcp/                        # MCP client (existing)
  utils.rs                    # Utilities (existing)
```

### Feature Flags

```toml
[features]
default = []
camera = ["dep:nokhwa"]             # Camera capture
voice = ["dep:ort", "dep:cpal"]     # TTS (Piper via ONNX) + audio capture
opencv = ["dep:opencv"]             # Local CV (future)
onnx = ["dep:ort"]                  # ONNX models (YOLO, face detection)
ros2 = []                           # ROS2 bridge (future)
telegram = ["dep:teloxide"]         # Telegram channel (existing)
```

---

## 17. Build Phases (Revised)

### Phase 1-2 (DONE)
Agent core, 40+ providers, streaming, memory, tools, server, hardening.

### Phase 3: Robot Brain Foundation (4-6 weeks)

**3a: Robot Description + Runtime + Mock Bridge (1 week)**
- `robot.toml` parser and validation
- HardwareBridge trait with MockBridge implementation
- Robot runtime task skeleton (start/stop)
- World state with watch channel
- Inject robot description into agent context
- New CLI command: `uniclaw robot` (starts robot mode)

**3b: Action System + Safety (1 week)**
- Action types (ActionCommand enum) and executor task
- Robot action tools (move, look, wave, say, set_led, etc.)
- Auto-registration based on robot.toml capabilities
- Safety monitor with declarative rule evaluation

**3c: Serial Bridge (1 week)**
- Binary serial protocol implementation
- MCU communication (send commands, receive sensor data)
- Watchdog timer and heartbeat
- Sensor data → world state updates

**3d: Perception + Vision (1 week)**
- Camera capture (v4l2/nokhwa, feature-gated)
- Motion detection (frame differencing)
- Cloud VLM integration (frame → base64 → vision provider)
- Perception event triggers → world state

**3e: Voice Pipeline (1 week)**
- VAD (voice activity detection, energy-based)
- Cloud STT (Whisper API)
- Local TTS (Piper via ONNX)
- Audio capture + playback (cpal crate)

### Phase 4: ROS2 + Simulation (2 weeks)
- RosBridge (rosbridge WebSocket, ~200 LOC)
- ROS2 tools (navigate_to, get_map, ros2_publish, etc.)
- Gazebo integration (via rosbridge — same config for sim and real)
- Web dashboard: 3D robot viewer, sensor panel, action log

### Phase 5: Behaviors + Spatial Memory (2 weeks)
- Behavior file parser (TOML-based behavior sequences)
- Behavior evaluator (event → behavior → action sequence)
- Spatial memory tools (remember_location, where_is)
- Idle behaviors, greeting, attention

### Phase 6: Local CV + Advanced (future)
- ONNX Runtime integration (feature-gated)
- YOLO-nano for object detection
- Face detection + recognition
- MuJoCo WASM bridge (in-browser physics)
- Fleet management (multiple robots via MQTT)
- OTA updates

---

## 18. Simulation & ROS2 Integration

### Design Principle

**UniClaw is the brain, not the body.** Don't build a physics engine. Integrate with existing simulators via the HardwareBridge trait. The same brain code runs against real hardware, Gazebo, MuJoCo, or a mock — config-only switch.

### HardwareBridge Trait (The Abstraction)

```rust
#[async_trait]
pub trait HardwareBridge: Send + Sync {
    async fn send_command(&self, cmd: HardwareCommand) -> Result<()>;
    async fn read_sensor(&self, sensor_id: &str) -> Result<SensorValue>;
    async fn read_all_sensors(&self) -> Result<HashMap<String, SensorValue>>;
    async fn heartbeat(&self) -> Result<()>;
    async fn emergency_stop(&self) -> Result<()>;
    fn name(&self) -> &str;
}
```

Four implementations:

| Bridge | Config | Use Case |
|--------|--------|----------|
| `MockBridge` | `bridge = "mock"` | Development, unit tests, logs everything |
| `SerialBridge` | `bridge = "serial"` | Real hardware (Arduino/ESP32/STM32) |
| `RosBridge` | `bridge = "ros2"` | ROS2 via rosbridge WebSocket → Gazebo, Isaac, real HW |
| `MuJoCoBridge` | `bridge = "mujoco"` | MuJoCo standalone or WASM (future) |

### ROS2 Integration via rosbridge

`rosbridge_suite` is a standard ROS2 package exposing all topics/services via WebSocket + JSON. UniClaw connects as a WebSocket client (~200 LOC). No ROS2 dependency in the UniClaw binary.

Protocol:
```json
{"op": "publish", "topic": "/cmd_vel", "msg": {"linear": {"x": 0.5}}}
{"op": "subscribe", "topic": "/scan", "type": "sensor_msgs/LaserScan"}
{"op": "call_service", "service": "/navigate_to_pose", "args": {...}}
```

**Boundary**: UniClaw decides WHAT to do (intent, reasoning). ROS2 handles HOW (path planning, kinematics, PID control).

| UniClaw (WHAT) | ROS2 (HOW) |
|---|---|
| "Go to the kitchen" | Nav2 path planning + obstacle avoidance |
| "Pick up the red cup" | MoveIt2 motion planning + grasp |
| "What's around me?" | SLAM, sensor fusion |

### Simulation Tiers

**Tier 1: Mock** — Logs commands, returns synthetic sensor data. For brain logic development.

**Tier 2: Gazebo via ROS2** — Full physics for any robot (rover, arm, companion, drone). Standard. Uses rosbridge — same config for simulation and real hardware.

**Tier 3: MuJoCo (WASM or standalone)** — Best physics for manipulation and locomotion. Has browser-native WASM build. Future phase.

**Tier 4: NVIDIA Isaac Sim** — Photorealistic, GPU-required. Connects via ROS2 (same rosbridge). No UniClaw-specific code needed.

### Configuration: Sim vs Real is Config-Only

```toml
# Development (no hardware)
[hardware]
bridge = "mock"

# Simulation (Gazebo)
[hardware]
bridge = "ros2"
[hardware.ros2]
url = "ws://localhost:9090"

# Real hardware (same config as Gazebo!)
[hardware]
bridge = "ros2"
[hardware.ros2]
url = "ws://192.168.1.50:9090"  # Only URL changes

# Direct serial (simple robots, no ROS2)
[hardware]
bridge = "serial"
port = "/dev/ttyUSB0"
baud_rate = 115200
```

### Web Dashboard (Not a Simulator)

The UniClaw web UI provides a visualization dashboard, not a physics engine:
- 3D robot viewer (urdf-loaders + Three.js or MuJoCo WASM renderer)
- Real-time sensor display
- Camera feed (from sim or real camera)
- Action log and manual controls
- Chat/voice interface

This works with ANY backend — real hardware, Gazebo, MuJoCo, or mock.

### ROS2 Tools (Registered When bridge = "ros2")

| Tool | ROS2 Mapping |
|------|-------------|
| `navigate_to(x, y, theta)` | Publish to `/navigate_to_pose` action |
| `get_map()` | Call `/map_server/get_map` service |
| `get_position()` | Subscribe to `/odom` topic |
| `ros2_publish(topic, msg)` | Generic publish |
| `ros2_call_service(service, args)` | Generic service call |

---

## 19. What We Keep vs What Changes

| Component | Status | Changes for Robot Brain |
|-----------|--------|----------------------|
| Agent loop (ReAct) | **Keep** | Add world state to context, add robot tools |
| Memory (sessions, MEMORY.md) | **Keep** | Add spatial.json, behaviors/ |
| Context builder | **Extend** | Add robot description + world state sections |
| 40+ LLM providers | **Keep** | Use vision models for perception |
| Streaming | **Keep** | Use for voice responses |
| Tool system | **Extend** | Add robot action tools, auto-register from robot.toml |
| Config | **Extend** | Add robot.toml parsing |
| HTTP server | **Keep** | Add robot status endpoint |
| MQTT | **Keep** | Robot telemetry + fleet management |
| Cron/heartbeat | **Keep** | Scheduled behaviors (patrol at 2pm) |
| Telegram/channels | **Keep** | Voice message support |
| Safety (path sandbox, rate limit) | **Extend** | Add physical safety rules |

**Estimated new code**: ~2,500-3,500 LOC for Phase 3
**Total after Phase 3**: ~12,000-13,000 LOC
