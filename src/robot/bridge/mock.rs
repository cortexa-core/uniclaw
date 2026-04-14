use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

use super::{HardwareBridge, HardwareCommand, SensorValue};

/// A mock hardware bridge that logs commands and returns configurable sensor data.
/// Useful for testing and development without real hardware.
#[allow(dead_code)]
pub struct MockBridge {
    /// All commands sent, for test inspection.
    pub command_log: Mutex<Vec<HardwareCommand>>,
    /// Pre-configured sensor values to return.
    pub sensors: Mutex<HashMap<String, SensorValue>>,
    /// Whether emergency stop has been triggered.
    pub stopped: Mutex<bool>,
}

impl MockBridge {
    pub fn new() -> Self {
        let mut default_sensors = HashMap::new();
        default_sensors.insert("front_distance".to_string(), SensorValue::Distance(100.0));
        default_sensors.insert("temperature".to_string(), SensorValue::Temperature(22.5));
        default_sensors.insert("motion".to_string(), SensorValue::Boolean(false));

        Self {
            command_log: Mutex::new(Vec::new()),
            sensors: Mutex::new(default_sensors),
            stopped: Mutex::new(false),
        }
    }

    /// Set a sensor value that will be returned by `read_sensor` / `read_all_sensors`.
    #[allow(dead_code)]
    pub fn set_sensor(&self, name: &str, value: SensorValue) {
        self.sensors.lock().unwrap().insert(name.to_string(), value);
    }

    /// Get a copy of all logged commands.
    #[allow(dead_code)]
    pub fn logged_commands(&self) -> Vec<HardwareCommand> {
        self.command_log.lock().unwrap().clone()
    }
}

impl Default for MockBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HardwareBridge for MockBridge {
    async fn send_command(&self, cmd: HardwareCommand) -> Result<()> {
        tracing::debug!("MockBridge: send_command({cmd:?})");
        self.command_log.lock().unwrap().push(cmd);
        Ok(())
    }

    async fn read_sensor(&self, sensor_id: &str) -> Result<SensorValue> {
        self.sensors
            .lock()
            .unwrap()
            .get(sensor_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Unknown sensor: {sensor_id}"))
    }

    async fn read_all_sensors(&self) -> Result<HashMap<String, SensorValue>> {
        Ok(self.sensors.lock().unwrap().clone())
    }

    async fn heartbeat(&self) -> Result<()> {
        // No-op for mock
        Ok(())
    }

    async fn emergency_stop(&self) -> Result<()> {
        tracing::warn!("MockBridge: EMERGENCY STOP");
        *self.stopped.lock().unwrap() = true;
        self.command_log
            .lock()
            .unwrap()
            .push(HardwareCommand::EmergencyStop);
        Ok(())
    }

    fn name(&self) -> &str {
        "mock"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_logs_commands() {
        let bridge = MockBridge::new();

        bridge
            .send_command(HardwareCommand::ServoSet {
                name: "left_arm".to_string(),
                angle: 90.0,
                speed_deg_s: None,
            })
            .await
            .unwrap();

        bridge
            .send_command(HardwareCommand::LedSet {
                name: "led_ring".to_string(),
                r: 255,
                g: 0,
                b: 0,
            })
            .await
            .unwrap();

        let log = bridge.logged_commands();
        assert_eq!(log.len(), 2);
        assert!(matches!(log[0], HardwareCommand::ServoSet { .. }));
        assert!(matches!(log[1], HardwareCommand::LedSet { .. }));
    }

    #[tokio::test]
    async fn test_mock_returns_sensors() {
        let bridge = MockBridge::new();

        // Default sensors
        let dist = bridge.read_sensor("front_distance").await.unwrap();
        assert!(matches!(dist, SensorValue::Distance(d) if (d - 100.0).abs() < f32::EPSILON));

        // Custom sensor
        bridge.set_sensor(
            "gyro",
            SensorValue::Orientation {
                roll: 1.0,
                pitch: 2.0,
                yaw: 3.0,
            },
        );
        let gyro = bridge.read_sensor("gyro").await.unwrap();
        assert!(matches!(gyro, SensorValue::Orientation { .. }));

        // All sensors
        let all = bridge.read_all_sensors().await.unwrap();
        assert!(all.len() >= 4); // 3 defaults + 1 custom

        // Unknown sensor
        assert!(bridge.read_sensor("nonexistent").await.is_err());
    }

    #[tokio::test]
    async fn test_mock_emergency_stop() {
        let bridge = MockBridge::new();

        assert!(!*bridge.stopped.lock().unwrap());
        bridge.emergency_stop().await.unwrap();
        assert!(*bridge.stopped.lock().unwrap());

        // Emergency stop should also be logged
        let log = bridge.logged_commands();
        assert_eq!(log.len(), 1);
        assert!(matches!(log[0], HardwareCommand::EmergencyStop));
    }
}
