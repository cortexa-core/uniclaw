# Robot Brain Foundation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the robot brain foundation — robot description, world state, hardware bridge (mock + serial), action system, safety monitor, perception (camera + cloud VLM), and voice pipeline (STT + TTS). A desktop companion robot as the first demo.

**Architecture:** Robot runtime as Tokio tasks alongside existing agent worker. HardwareBridge trait abstracts mock/serial/ROS2. World state via `watch` channel (one writer, many readers). Actions as LLM tools auto-registered from robot.toml.

**Tech Stack:** Rust, Tokio, serde, tokio-serial, cpal (audio), ort (ONNX for Piper TTS)

---

## Phase 3a: Robot Description + Runtime + Mock Bridge

### Task 1: HardwareBridge trait and MockBridge

**Files:**
- Create: `src/robot/bridge/mod.rs`
- Create: `src/robot/bridge/mock.rs`
- Create: `src/robot/mod.rs`

- [ ] **Step 1: Create the HardwareBridge trait**

Create `src/robot/bridge/mod.rs`:

```rust
pub mod mock;

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

/// A command sent to the hardware (actuators, LEDs, etc.)
#[derive(Debug, Clone)]
pub enum HardwareCommand {
    ServoSet { name: String, angle: f32, speed_deg_s: Option<f32> },
    MotorSet { name: String, speed: f32, duration_ms: Option<u64> },
    LedSet { name: String, r: u8, g: u8, b: u8 },
    LedPattern { name: String, pattern: String },
    Ping,
    EmergencyStop,
}

/// A sensor reading from the hardware
#[derive(Debug, Clone)]
pub enum SensorValue {
    Distance(f32),
    Temperature(f32),
    Orientation { roll: f32, pitch: f32, yaw: f32 },
    Boolean(bool),
    Raw(i32),
}

/// Abstraction over hardware communication.
/// Implementations: MockBridge, SerialBridge, RosBridge.
#[async_trait]
pub trait HardwareBridge: Send + Sync {
    /// Send a command to an actuator
    async fn send_command(&self, cmd: HardwareCommand) -> Result<()>;

    /// Read a specific sensor
    async fn read_sensor(&self, sensor_id: &str) -> Result<SensorValue>;

    /// Read all sensors at once
    async fn read_all_sensors(&self) -> Result<HashMap<String, SensorValue>>;

    /// Send heartbeat (keep MCU watchdog alive)
    async fn heartbeat(&self) -> Result<()>;

    /// Emergency stop all actuators
    async fn emergency_stop(&self) -> Result<()>;

    /// Bridge name for logging
    fn name(&self) -> &str;
}
```

- [ ] **Step 2: Create MockBridge**

Create `src/robot/bridge/mock.rs`:

```rust
use super::{HardwareBridge, HardwareCommand, SensorValue};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

/// Mock bridge for development. Logs commands, returns synthetic sensor data.
pub struct MockBridge {
    command_log: Mutex<Vec<HardwareCommand>>,
    sensors: Mutex<HashMap<String, SensorValue>>,
}

impl MockBridge {
    pub fn new() -> Self {
        let mut sensors = HashMap::new();
        sensors.insert("front_distance".into(), SensorValue::Distance(100.0));
        sensors.insert("battery".into(), SensorValue::Raw(85));
        Self {
            command_log: Mutex::new(Vec::new()),
            sensors: Mutex::new(sensors),
        }
    }

    /// Get logged commands (for testing)
    #[cfg(test)]
    pub fn logged_commands(&self) -> Vec<HardwareCommand> {
        self.command_log.lock().unwrap().clone()
    }
}

#[async_trait]
impl HardwareBridge for MockBridge {
    async fn send_command(&self, cmd: HardwareCommand) -> Result<()> {
        tracing::debug!("[MockBridge] Command: {cmd:?}");
        self.command_log.lock().unwrap().push(cmd);
        Ok(())
    }

    async fn read_sensor(&self, sensor_id: &str) -> Result<SensorValue> {
        self.sensors.lock().unwrap()
            .get(sensor_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Unknown sensor: {sensor_id}"))
    }

    async fn read_all_sensors(&self) -> Result<HashMap<String, SensorValue>> {
        Ok(self.sensors.lock().unwrap().clone())
    }

    async fn heartbeat(&self) -> Result<()> {
        tracing::trace!("[MockBridge] Heartbeat");
        Ok(())
    }

    async fn emergency_stop(&self) -> Result<()> {
        tracing::warn!("[MockBridge] EMERGENCY STOP");
        self.command_log.lock().unwrap().push(HardwareCommand::EmergencyStop);
        Ok(())
    }

    fn name(&self) -> &str {
        "mock"
    }
}
```

- [ ] **Step 3: Create robot module root**

Create `src/robot/mod.rs`:

```rust
pub mod bridge;
```

Add `mod robot;` to `src/main.rs` (or `src/lib.rs`).

- [ ] **Step 4: Add tests**

Add tests in `src/robot/bridge/mock.rs` verifying MockBridge logs commands and returns sensor data.

- [ ] **Step 5: Run tests and commit**

```
git commit -m "Add HardwareBridge trait and MockBridge for development"
```

---

### Task 2: Robot description parser (robot.toml)

**Files:**
- Create: `src/robot/description.rs`
- Modify: `src/robot/mod.rs`

- [ ] **Step 1: Define robot description types**

Create `src/robot/description.rs` with serde structs matching the robot.toml format from the design:

```rust
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct RobotDescription {
    pub robot: RobotInfo,
    #[serde(default)]
    pub body: BodyConfig,
    #[serde(default)]
    pub sensors: Vec<SensorConfig>,
    #[serde(default)]
    pub actuators: Vec<ActuatorConfig>,
    #[serde(default)]
    pub safety: SafetyConfig,
    #[serde(default)]
    pub behaviors: BehaviorConfig,
    #[serde(default)]
    pub perception: PerceptionConfig,
    #[serde(default)]
    pub voice: VoiceConfig,
    #[serde(default)]
    pub hardware: HardwareConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RobotInfo {
    pub name: String,
    #[serde(default = "default_robot_type")]
    pub r#type: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BodyConfig {
    #[serde(default = "default_base")]
    pub base: String,
    #[serde(default)]
    pub weight_kg: f32,
    #[serde(default)]
    pub height_cm: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SensorConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub sensor_type: String,
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub pin: Option<u8>,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub fps: Option<u32>,
    #[serde(default)]
    pub max_range_cm: Option<f32>,
    #[serde(default)]
    pub poll_interval_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActuatorConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub actuator_type: String,
    #[serde(default)]
    pub pin: Option<u8>,
    #[serde(default)]
    pub angle_range: Option<[f32; 2]>,
    #[serde(default)]
    pub default_angle: Option<f32>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub count: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SafetyConfig {
    #[serde(default = "default_watchdog")]
    pub watchdog_timeout_ms: u64,
    #[serde(default)]
    pub emergency_stop_pin: Option<u8>,
    #[serde(default)]
    pub rules: Vec<SafetyRuleConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SafetyRuleConfig {
    pub name: String,
    pub condition: String,
    pub action: String,
    #[serde(default = "default_priority")]
    pub priority: u8,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BehaviorConfig {
    #[serde(default)]
    pub idle: Option<String>,
    #[serde(default)]
    pub on_face_detected: Option<String>,
    #[serde(default)]
    pub on_touch: Option<String>,
    #[serde(default)]
    pub on_name_called: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PerceptionConfig {
    #[serde(default)]
    pub vision_provider: Option<String>,
    #[serde(default)]
    pub vision_model: Option<String>,
    #[serde(default = "default_vision_trigger")]
    pub vision_trigger: String,
    #[serde(default = "default_vision_periodic")]
    pub vision_periodic_secs: u64,
    #[serde(default = "default_true_bool")]
    pub motion_detection: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct VoiceConfig {
    #[serde(default)]
    pub stt_provider: Option<String>,
    #[serde(default)]
    pub stt_model: Option<String>,
    #[serde(default)]
    pub tts_engine: Option<String>,
    #[serde(default)]
    pub tts_model: Option<String>,
    #[serde(default = "default_tts_speed")]
    pub tts_speed: f32,
    #[serde(default)]
    pub vad_enabled: bool,
    #[serde(default)]
    pub wake_word: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HardwareConfig {
    #[serde(default = "default_bridge")]
    pub bridge: String,
    #[serde(default)]
    pub port: Option<String>,
    #[serde(default = "default_baud")]
    pub baud_rate: u32,
    #[serde(default)]
    pub ros2: Option<Ros2Config>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ros2Config {
    pub url: String,
    #[serde(default)]
    pub camera_topic: Option<String>,
    #[serde(default)]
    pub cmd_vel_topic: Option<String>,
    #[serde(default)]
    pub odom_topic: Option<String>,
    #[serde(default)]
    pub scan_topic: Option<String>,
    #[serde(default)]
    pub navigate_action: Option<String>,
}

impl RobotDescription {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {e}", path.display()))?;
        toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse robot.toml: {e}"))
    }

    /// Generate a system prompt section describing the robot's body.
    pub fn to_system_prompt(&self) -> String {
        let mut lines = vec![format!("## Robot Body\n")];
        lines.push(format!("You are {}, a {}.", self.robot.name, self.robot.description));
        lines.push(format!("Base: {} | Weight: {}kg | Height: {}cm\n",
            self.body.base, self.body.weight_kg, self.body.height_cm));

        if !self.sensors.is_empty() {
            lines.push("Sensors:".to_string());
            for s in &self.sensors {
                lines.push(format!("- {} ({})", s.name, s.sensor_type));
            }
        }
        if !self.actuators.is_empty() {
            lines.push("\nActuators:".to_string());
            for a in &self.actuators {
                let range = a.angle_range
                    .map(|r| format!(", range {}°-{}°", r[0], r[1]))
                    .unwrap_or_default();
                lines.push(format!("- {} ({}{})", a.name, a.actuator_type, range));
            }
        }
        lines.join("\n")
    }

    /// List actuator names that exist for tool auto-registration.
    pub fn actuator_names(&self) -> Vec<&str> {
        self.actuators.iter().map(|a| a.name.as_str()).collect()
    }

    /// Check if a specific actuator type exists.
    pub fn has_actuator_type(&self, t: &str) -> bool {
        self.actuators.iter().any(|a| a.actuator_type == t)
    }

    /// Check if a specific sensor type exists.
    pub fn has_sensor_type(&self, t: &str) -> bool {
        self.sensors.iter().any(|a| a.sensor_type == t)
    }
}

// Default functions
fn default_robot_type() -> String { "custom".into() }
fn default_base() -> String { "fixed".into() }
fn default_watchdog() -> u64 { 500 }
fn default_priority() -> u8 { 5 }
fn default_vision_trigger() -> String { "event".into() }
fn default_vision_periodic() -> u64 { 30 }
fn default_true_bool() -> bool { true }
fn default_tts_speed() -> f32 { 1.0 }
fn default_bridge() -> String { "mock".into() }
fn default_baud() -> u32 { 115200 }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_robot_toml() {
        let toml = r#"
[robot]
name = "TestBot"
description = "A test robot"

[hardware]
bridge = "mock"
"#;
        let desc: RobotDescription = toml::from_str(toml).unwrap();
        assert_eq!(desc.robot.name, "TestBot");
        assert_eq!(desc.hardware.bridge, "mock");
        assert!(desc.sensors.is_empty());
        assert!(desc.actuators.is_empty());
    }

    #[test]
    fn test_parse_full_robot_toml() {
        let toml = r#"
[robot]
name = "Buddy"
type = "desktop_companion"
description = "A small desktop robot"

[body]
base = "fixed"
weight_kg = 0.5
height_cm = 20

[[sensors]]
name = "camera"
type = "camera"
device = "/dev/video0"

[[sensors]]
name = "front_distance"
type = "ultrasonic"
pin = 7
max_range_cm = 200

[[actuators]]
name = "left_arm"
type = "servo"
pin = 12
angle_range = [0, 180]
default_angle = 90

[[actuators]]
name = "speaker"
type = "audio_output"

[safety]
watchdog_timeout_ms = 500

[[safety.rules]]
name = "obstacle_stop"
condition = "front_distance < 10"
action = "stop_all_motors"
priority = 10

[perception]
vision_provider = "gemini"
vision_model = "gemini-2.0-flash"

[voice]
stt_provider = "whisper"
tts_engine = "piper"

[hardware]
bridge = "serial"
port = "/dev/ttyUSB0"
"#;
        let desc: RobotDescription = toml::from_str(toml).unwrap();
        assert_eq!(desc.robot.name, "Buddy");
        assert_eq!(desc.sensors.len(), 2);
        assert_eq!(desc.actuators.len(), 2);
        assert_eq!(desc.safety.rules.len(), 1);
        assert!(desc.has_sensor_type("camera"));
        assert!(desc.has_actuator_type("servo"));
        assert!(!desc.has_actuator_type("motor"));
    }

    #[test]
    fn test_system_prompt_generation() {
        let toml = r#"
[robot]
name = "Buddy"
description = "A small desktop robot with arms"

[[actuators]]
name = "left_arm"
type = "servo"
angle_range = [0, 180]

[hardware]
bridge = "mock"
"#;
        let desc: RobotDescription = toml::from_str(toml).unwrap();
        let prompt = desc.to_system_prompt();
        assert!(prompt.contains("Buddy"));
        assert!(prompt.contains("left_arm"));
        assert!(prompt.contains("servo"));
    }

    #[test]
    fn test_ros2_config() {
        let toml = r#"
[robot]
name = "Rover"

[hardware]
bridge = "ros2"

[hardware.ros2]
url = "ws://localhost:9090"
cmd_vel_topic = "/cmd_vel"
odom_topic = "/odom"
"#;
        let desc: RobotDescription = toml::from_str(toml).unwrap();
        assert_eq!(desc.hardware.bridge, "ros2");
        let ros2 = desc.hardware.ros2.unwrap();
        assert_eq!(ros2.url, "ws://localhost:9090");
        assert_eq!(ros2.cmd_vel_topic.unwrap(), "/cmd_vel");
    }
}
```

- [ ] **Step 2: Register module**

Add `pub mod description;` to `src/robot/mod.rs`.

- [ ] **Step 3: Create example robot.toml**

Create `data/robot.toml` with the desktop companion example from the design spec.

- [ ] **Step 4: Run tests and commit**

```
git commit -m "Add robot description parser for robot.toml"
```

---

### Task 3: World state and robot runtime skeleton

**Files:**
- Create: `src/robot/world_state.rs`
- Create: `src/robot/runtime.rs`
- Modify: `src/robot/mod.rs`
- Modify: `src/main.rs` (add `robot` subcommand)

- [ ] **Step 1: Create WorldState**

Create `src/robot/world_state.rs` with the WorldState struct and a `tokio::sync::watch` channel wrapper.

```rust
use std::collections::HashMap;
use std::time::Instant;

/// Shared view of the physical world.
/// Published via tokio::sync::watch (one writer, many readers, lock-free).
#[derive(Debug, Clone)]
pub struct WorldState {
    pub timestamp: Instant,

    // Vision
    pub scene_description: Option<String>,
    pub scene_timestamp: Option<Instant>,
    pub motion_detected: bool,

    // Audio
    pub last_speech: Option<(Instant, String)>,
    pub voice_active: bool,

    // Sensors (from hardware bridge)
    pub sensors: HashMap<String, super::bridge::SensorValue>,

    // Body state
    pub actuator_positions: HashMap<String, f32>,
    pub battery_percent: Option<f32>,
    pub is_moving: bool,

    // Active state
    pub current_behavior: Option<String>,
    pub last_action: Option<(Instant, String)>,
}

impl Default for WorldState {
    fn default() -> Self {
        Self {
            timestamp: Instant::now(),
            scene_description: None,
            scene_timestamp: None,
            motion_detected: false,
            last_speech: None,
            voice_active: false,
            sensors: HashMap::new(),
            actuator_positions: HashMap::new(),
            battery_percent: None,
            is_moving: false,
            current_behavior: None,
            last_action: None,
        }
    }
}

impl WorldState {
    /// Generate a perception summary for the LLM context.
    pub fn to_context_section(&self) -> String {
        let mut lines = vec!["## Current Perception\n".to_string()];

        if let Some(ref desc) = self.scene_description {
            let age = self.scene_timestamp
                .map(|t| format!("{}s ago", t.elapsed().as_secs()))
                .unwrap_or_else(|| "unknown".into());
            lines.push(format!("Scene ({age}): {desc}"));
        }

        lines.push(format!("Motion detected: {}", self.motion_detected));

        if !self.sensors.is_empty() {
            lines.push("Sensors:".into());
            for (name, value) in &self.sensors {
                lines.push(format!("  {name}: {value:?}"));
            }
        }

        if let Some(pct) = self.battery_percent {
            lines.push(format!("Battery: {pct:.0}%"));
        }

        if let Some(ref behavior) = self.current_behavior {
            lines.push(format!("Current behavior: {behavior}"));
        }

        lines.join("\n")
    }
}
```

- [ ] **Step 2: Create robot runtime skeleton**

Create `src/robot/runtime.rs` — the main orchestrator that spawns perception, safety, and sensor polling tasks:

```rust
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

use super::bridge::HardwareBridge;
use super::description::RobotDescription;
use super::world_state::WorldState;

/// The robot runtime — manages perception, safety, and sensor loops.
pub struct RobotRuntime {
    description: Arc<RobotDescription>,
    bridge: Arc<dyn HardwareBridge>,
    world_tx: watch::Sender<WorldState>,
    world_rx: watch::Receiver<WorldState>,
}

impl RobotRuntime {
    pub fn new(
        description: RobotDescription,
        bridge: Box<dyn HardwareBridge>,
    ) -> Self {
        let (world_tx, world_rx) = watch::channel(WorldState::default());
        Self {
            description: Arc::new(description),
            bridge: Arc::from(bridge),
            world_tx,
            world_rx,
        }
    }

    /// Get a world state receiver (for agent context builder, safety monitor, etc.)
    pub fn world_rx(&self) -> watch::Receiver<WorldState> {
        self.world_rx.clone()
    }

    /// Get the robot description
    pub fn description(&self) -> &RobotDescription {
        &self.description
    }

    /// Start all runtime tasks. Returns join handles.
    pub async fn start(&self) -> Vec<tokio::task::JoinHandle<()>> {
        let mut tasks = Vec::new();

        // Sensor polling task
        let bridge = self.bridge.clone();
        let world_tx = self.world_tx.clone();
        let poll_interval = std::time::Duration::from_millis(200);
        tasks.push(tokio::spawn(async move {
            loop {
                if let Ok(sensors) = bridge.read_all_sensors().await {
                    world_tx.send_modify(|state| {
                        state.sensors = sensors;
                        state.timestamp = std::time::Instant::now();
                    });
                }
                // Send heartbeat to keep watchdog alive
                bridge.heartbeat().await.ok();
                tokio::time::sleep(poll_interval).await;
            }
        }));

        tracing::info!("Robot runtime started with {} sensor(s), {} actuator(s)",
            self.description.sensors.len(),
            self.description.actuators.len(),
        );

        tasks
    }

    /// Get a reference to the hardware bridge (for action executor)
    pub fn bridge(&self) -> Arc<dyn HardwareBridge> {
        self.bridge.clone()
    }
}
```

- [ ] **Step 3: Add `robot` CLI subcommand to main.rs**

Add a `Robot` variant to the `Commands` enum:

```rust
    /// Start in robot mode (continuous perception + action loop)
    Robot {
        /// Path to robot description file
        #[arg(long, default_value = "data/robot.toml")]
        robot_config: PathBuf,
    },
```

Add `run_robot()` function that loads robot.toml, creates the bridge, starts the runtime, and spawns the agent worker with robot context.

- [ ] **Step 4: Inject robot description + world state into agent context**

Modify `src/agent/context.rs` to optionally include robot description and world state sections when building the system prompt. Add optional fields:

```rust
pub struct ContextBuilder {
    // ... existing fields ...
    robot_prompt: Option<String>,
    world_rx: Option<watch::Receiver<WorldState>>,
}
```

When building the system prompt, append `robot_prompt` and `world_rx.borrow().to_context_section()` if present.

- [ ] **Step 5: Add tests and commit**

```
git commit -m "Add world state, robot runtime skeleton, and robot CLI command"
```

---

## Phase 3b: Action System + Safety

### Task 4: Action types and executor

**Files:**
- Create: `src/robot/action.rs`
- Modify: `src/robot/mod.rs`

- [ ] **Step 1: Define action types and executor**

Create `src/robot/action.rs` with ActionCommand enum and ActionExecutor that translates actions to hardware bridge commands. The executor runs as a Tokio task, receives actions via mpsc channel.

- [ ] **Step 2: Add tests and commit**

```
git commit -m "Add action types and executor for robot commands"
```

---

### Task 5: Robot action tools

**Files:**
- Create: `src/tools/robot_actions.rs`
- Modify: `src/tools/mod.rs`

- [ ] **Step 1: Create robot action tools**

Create tools that the LLM can call: `say`, `set_servo`, `set_led`, `stop`, `look_at`, `get_sensor`, `take_photo`. Each tool sends an ActionCommand to the action executor.

Tools are auto-registered based on robot.toml capabilities — if no servos defined, the `set_servo` tool isn't registered.

- [ ] **Step 2: Add tests and commit**

```
git commit -m "Add robot action tools auto-registered from robot.toml capabilities"
```

---

### Task 6: Safety monitor

**Files:**
- Create: `src/robot/safety.rs`
- Modify: `src/robot/mod.rs`

- [ ] **Step 1: Create safety monitor**

Create `src/robot/safety.rs` — runs as a continuous Tokio task, reads world state every 100ms, evaluates safety rules from robot.toml, sends emergency actions if triggered.

Safety rule expression parser handles simple patterns: `sensor_name < value`, `sensor_name > value`.

- [ ] **Step 2: Add tests and commit**

```
git commit -m "Add safety monitor with declarative rule evaluation"
```

---

## Phase 3c: Serial Bridge

### Task 7: Serial bridge implementation

**Files:**
- Create: `src/robot/bridge/serial.rs`
- Modify: `src/robot/bridge/mod.rs`
- Modify: `Cargo.toml` (add `tokio-serial` dependency)

- [ ] **Step 1: Implement serial protocol**

Create `src/robot/bridge/serial.rs` implementing the binary protocol from the design spec (0xAA header, length, type, payload, CRC). Handles command sending and sensor response parsing.

- [ ] **Step 2: Add tests with mock serial port and commit**

```
git commit -m "Add serial bridge for Arduino/ESP32 communication"
```

---

## Phase 3d: Perception + Vision

### Task 8: Camera capture and cloud VLM integration

**Files:**
- Create: `src/robot/perception.rs`
- Create: `src/robot/camera.rs` (feature-gated)
- Create: `src/tools/perception_tools.rs`
- Modify: `Cargo.toml` (add `nokhwa` or `v4l2` dependency, feature-gated)

- [ ] **Step 1: Create perception pipeline**

Create `src/robot/perception.rs` — runs as Tokio task. Captures frames, detects motion (frame differencing), triggers cloud VLM calls on events/periodic timer. Updates world state with scene descriptions.

- [ ] **Step 2: Create camera capture module**

Create `src/robot/camera.rs` behind `#[cfg(feature = "camera")]`. Uses `nokhwa` or `v4l2` crate for frame capture. Encodes to JPEG.

- [ ] **Step 3: Create perception tools**

Create `src/tools/perception_tools.rs` with tools: `take_photo` (capture + VLM describe), `describe_scene` (latest scene description from world state).

- [ ] **Step 4: Add tests and commit**

```
git commit -m "Add perception pipeline with camera capture and cloud VLM"
```

---

## Phase 3e: Voice Pipeline

### Task 9: Voice activity detection + STT

**Files:**
- Create: `src/robot/voice.rs`
- Modify: `Cargo.toml` (add `cpal` for audio capture, feature-gated)

- [ ] **Step 1: Implement VAD + STT**

Create `src/robot/voice.rs` behind `#[cfg(feature = "voice")]`. Implements:
- Audio capture via `cpal` crate
- Energy-based VAD (voice activity detection)
- Cloud STT via existing HTTP provider (Whisper API)
- Detected speech → Input to agent worker

- [ ] **Step 2: Implement TTS**

Add TTS to voice.rs:
- Text → Piper TTS (ONNX via `ort` crate) → WAV → speaker
- Or fallback: text → cloud TTS API → audio → speaker
- TTS cache (data/tts_cache/) for repeated phrases

- [ ] **Step 3: Add tests and commit**

```
git commit -m "Add voice pipeline with VAD, cloud STT, and local TTS"
```

---

## Phase 4: ROS2 Bridge

### Task 10: ROS2 bridge via rosbridge WebSocket

**Files:**
- Create: `src/robot/bridge/ros2.rs`
- Create: `src/tools/ros2_tools.rs`
- Modify: `src/robot/bridge/mod.rs`
- Modify: `Cargo.toml` (add `tokio-tungstenite` if not present)

- [ ] **Step 1: Implement rosbridge WebSocket client**

Create `src/robot/bridge/ros2.rs` — connects to rosbridge_server via WebSocket. Implements HardwareBridge trait by translating commands to ROS2 topic publishes and sensor reads to topic subscriptions.

The rosbridge JSON protocol:
- Publish: `{"op": "publish", "topic": "/cmd_vel", "msg": {...}}`
- Subscribe: `{"op": "subscribe", "topic": "/scan", "type": "..."}`
- Service: `{"op": "call_service", "service": "/navigate_to_pose", "args": {...}}`

- [ ] **Step 2: Create ROS2 tools**

Create `src/tools/ros2_tools.rs` with tools auto-registered when bridge = "ros2":
- `navigate_to(x, y, theta)` — publish to navigate action
- `get_position()` — read from odom topic
- `ros2_publish(topic, msg)` — generic publish
- `ros2_call_service(service, args)` — generic service call

- [ ] **Step 3: Add tests (mock WebSocket server) and commit**

```
git commit -m "Add ROS2 bridge via rosbridge WebSocket for Gazebo and real hardware"
```

---

## Verification

### Task 11: Integration test + final verification

**Files:**
- Modify: `tests/` (integration tests)

- [ ] **Step 1: Robot brain integration test**

Test the full pipeline with MockBridge: load robot.toml → start runtime → send voice input → agent calls LLM → generates action → MockBridge logs command → verify.

- [ ] **Step 2: Run full test suite**

```bash
cargo test
cargo test --features telegram
cargo test --features camera
cargo test --features voice
cargo clippy -- -D warnings
cargo fmt -- --check
```

- [ ] **Step 3: Commit**

```
git commit -m "Add robot brain integration tests and verify all features"
```

---

## Summary

| Task | Phase | Files | What |
|------|-------|-------|------|
| 1 | 3a | robot/bridge/ | HardwareBridge trait + MockBridge |
| 2 | 3a | robot/description.rs | robot.toml parser |
| 3 | 3a | robot/world_state.rs, runtime.rs, main.rs, context.rs | World state + runtime + CLI + context injection |
| 4 | 3b | robot/action.rs | Action types + executor |
| 5 | 3b | tools/robot_actions.rs | LLM-facing robot tools |
| 6 | 3b | robot/safety.rs | Safety monitor |
| 7 | 3c | robot/bridge/serial.rs | Serial protocol to MCU |
| 8 | 3d | robot/perception.rs, camera.rs, tools/ | Camera + VLM perception |
| 9 | 3e | robot/voice.rs | VAD + STT + TTS |
| 10 | 4 | robot/bridge/ros2.rs, tools/ | ROS2 via rosbridge → Gazebo/real HW |
| 11 | — | tests/ | Integration tests + verification |

**Estimated new code**: ~3,000-4,000 LOC
**New dependencies**: tokio-serial, cpal, ort, nokhwa (all feature-gated)
**Total after**: ~13,000-14,000 LOC
