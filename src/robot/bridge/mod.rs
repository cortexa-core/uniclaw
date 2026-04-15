pub mod mock;
#[allow(dead_code)]
pub mod serial;

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

/// A command sent to robot hardware
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum HardwareCommand {
    ServoSet {
        name: String,
        angle: f32,
        speed_deg_s: Option<f32>,
    },
    MotorSet {
        name: String,
        speed: f32,
        duration_ms: Option<u64>,
    },
    LedSet {
        name: String,
        r: u8,
        g: u8,
        b: u8,
    },
    LedPattern {
        name: String,
        pattern: String,
    },
    Ping,
    EmergencyStop,
}

/// A sensor reading
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SensorValue {
    Distance(f32),
    Temperature(f32),
    Orientation { roll: f32, pitch: f32, yaw: f32 },
    Boolean(bool),
    Raw(i32),
}

/// Trait for communicating with robot hardware.
/// Implementations may talk to real GPIO/serial/ROS2, or simulate for testing.
#[async_trait]
#[allow(dead_code)]
pub trait HardwareBridge: Send + Sync {
    /// Send a command to an actuator.
    async fn send_command(&self, cmd: HardwareCommand) -> Result<()>;
    /// Read a single sensor by ID.
    async fn read_sensor(&self, sensor_id: &str) -> Result<SensorValue>;
    /// Read all sensors at once.
    async fn read_all_sensors(&self) -> Result<HashMap<String, SensorValue>>;
    /// Heartbeat / watchdog keep-alive.
    async fn heartbeat(&self) -> Result<()>;
    /// Immediately halt all actuators.
    async fn emergency_stop(&self) -> Result<()>;
    /// Human-readable bridge name (e.g. "mock", "serial", "ros2").
    fn name(&self) -> &str;
}
