use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

/// Top-level robot description loaded from robot.toml.
#[derive(Debug, Deserialize)]
pub struct RobotDescription {
    pub robot: RobotInfo,
    #[serde(default)]
    pub body: Option<BodyInfo>,
    #[serde(default)]
    pub sensors: Vec<SensorDef>,
    #[serde(default)]
    pub actuators: Vec<ActuatorDef>,
    #[serde(default)]
    pub safety: Option<SafetyConfig>,
    #[serde(default)]
    pub perception: Option<PerceptionConfig>,
    #[serde(default)]
    pub voice: Option<VoiceConfig>,
    pub hardware: HardwareConfig,
}

#[derive(Debug, Deserialize)]
pub struct RobotInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub robot_type: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct BodyInfo {
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub weight_kg: Option<f32>,
    #[serde(default)]
    pub height_cm: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub struct SensorDef {
    pub name: String,
    #[serde(rename = "type")]
    pub sensor_type: String,
    #[serde(default)]
    pub pin: Option<u32>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub max_range_cm: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub struct ActuatorDef {
    pub name: String,
    #[serde(rename = "type")]
    pub actuator_type: String,
    #[serde(default)]
    pub pin: Option<u32>,
    #[serde(default)]
    pub angle_range: Option<Vec<f32>>,
    #[serde(default)]
    pub default_angle: Option<f32>,
    #[serde(default)]
    pub count: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct SafetyConfig {
    #[serde(default)]
    pub watchdog_timeout_ms: Option<u64>,
    #[serde(default)]
    pub rules: Vec<SafetyRule>,
}

#[derive(Debug, Deserialize)]
pub struct SafetyRule {
    pub name: String,
    pub condition: String,
    pub action: String,
    #[serde(default)]
    pub priority: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct PerceptionConfig {
    #[serde(default)]
    pub vision_provider: Option<String>,
    #[serde(default)]
    pub vision_model: Option<String>,
    #[serde(default)]
    pub motion_detection: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct VoiceConfig {
    #[serde(default)]
    pub stt_provider: Option<String>,
    #[serde(default)]
    pub tts_engine: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HardwareConfig {
    pub bridge: String,
    #[serde(default)]
    pub port: Option<String>,
    #[serde(default)]
    pub baud: Option<u32>,
    #[serde(default)]
    pub ros2: Option<Ros2Config>,
}

#[derive(Debug, Deserialize)]
pub struct Ros2Config {
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub cmd_topic: Option<String>,
    #[serde(default)]
    pub sensor_topic: Option<String>,
}

impl RobotDescription {
    /// Load a robot description from a TOML file.
    pub fn load(path: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let desc: Self =
            toml::from_str(&content).with_context(|| format!("parsing {}", path.display()))?;
        Ok(desc)
    }

    /// Generate a system prompt section describing this robot for LLM context.
    pub fn to_system_prompt(&self) -> String {
        let mut lines = vec![format!(
            "## Robot: {} ({})\n",
            self.robot.name, self.robot.robot_type
        )];
        if !self.robot.description.is_empty() {
            lines.push(self.robot.description.clone());
            lines.push(String::new());
        }

        // Body
        if let Some(ref body) = self.body {
            let mut body_parts = Vec::new();
            if let Some(ref base) = body.base {
                body_parts.push(format!("base={base}"));
            }
            if let Some(w) = body.weight_kg {
                body_parts.push(format!("{w}kg"));
            }
            if let Some(h) = body.height_cm {
                body_parts.push(format!("{h}cm"));
            }
            if !body_parts.is_empty() {
                lines.push(format!("Body: {}", body_parts.join(", ")));
            }
        }

        // Sensors
        if !self.sensors.is_empty() {
            lines.push(String::new());
            lines.push("### Sensors".to_string());
            for s in &self.sensors {
                let mut desc = format!("- **{}** ({})", s.name, s.sensor_type);
                if let Some(pin) = s.pin {
                    desc.push_str(&format!(" pin={pin}"));
                }
                if let Some(ref dev) = s.device {
                    desc.push_str(&format!(" device={dev}"));
                }
                if let Some(range) = s.max_range_cm {
                    desc.push_str(&format!(" max_range={range}cm"));
                }
                lines.push(desc);
            }
        }

        // Actuators
        if !self.actuators.is_empty() {
            lines.push(String::new());
            lines.push("### Actuators".to_string());
            for a in &self.actuators {
                let mut desc = format!("- **{}** ({})", a.name, a.actuator_type);
                if let Some(pin) = a.pin {
                    desc.push_str(&format!(" pin={pin}"));
                }
                if let Some(ref range) = a.angle_range {
                    if range.len() == 2 {
                        desc.push_str(&format!(" range=[{}, {}]", range[0], range[1]));
                    }
                }
                if let Some(count) = a.count {
                    desc.push_str(&format!(" count={count}"));
                }
                lines.push(desc);
            }
        }

        // Safety
        if let Some(ref safety) = self.safety {
            lines.push(String::new());
            lines.push("### Safety".to_string());
            if let Some(wd) = safety.watchdog_timeout_ms {
                lines.push(format!("Watchdog: {wd}ms"));
            }
            for rule in &safety.rules {
                lines.push(format!(
                    "- Rule **{}**: if `{}` then `{}`",
                    rule.name, rule.condition, rule.action
                ));
            }
        }

        lines.join("\n")
    }

    /// Check if the robot has an actuator of the given type.
    #[allow(dead_code)]
    pub fn has_actuator_type(&self, actuator_type: &str) -> bool {
        self.actuators
            .iter()
            .any(|a| a.actuator_type == actuator_type)
    }

    /// Check if the robot has a sensor of the given type.
    #[allow(dead_code)]
    pub fn has_sensor_type(&self, sensor_type: &str) -> bool {
        self.sensors.iter().any(|s| s.sensor_type == sensor_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal() {
        let toml_str = r#"
[robot]
name = "TestBot"
type = "test"

[hardware]
bridge = "mock"
"#;
        let desc: RobotDescription = toml::from_str(toml_str).unwrap();
        assert_eq!(desc.robot.name, "TestBot");
        assert_eq!(desc.robot.robot_type, "test");
        assert_eq!(desc.hardware.bridge, "mock");
        assert!(desc.sensors.is_empty());
        assert!(desc.actuators.is_empty());
        assert!(desc.safety.is_none());
        assert!(desc.perception.is_none());
        assert!(desc.voice.is_none());
        assert!(desc.body.is_none());
    }

    #[test]
    fn test_parse_full() {
        let toml_str = r#"
[robot]
name = "Buddy"
type = "desktop_companion"
description = "A small desktop companion"

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

[perception]
vision_provider = "gemini"
vision_model = "gemini-2.0-flash"
motion_detection = true

[voice]
stt_provider = "whisper"
tts_engine = "cloud"

[hardware]
bridge = "mock"
port = "/dev/ttyUSB0"
baud = 115200
"#;
        let desc: RobotDescription = toml::from_str(toml_str).unwrap();
        assert_eq!(desc.robot.name, "Buddy");
        assert_eq!(desc.robot.robot_type, "desktop_companion");
        assert_eq!(desc.sensors.len(), 2);
        assert_eq!(desc.actuators.len(), 2);
        assert!(desc.has_sensor_type("camera"));
        assert!(desc.has_sensor_type("ultrasonic"));
        assert!(!desc.has_sensor_type("lidar"));
        assert!(desc.has_actuator_type("servo"));
        assert!(desc.has_actuator_type("neopixel"));
        assert!(!desc.has_actuator_type("motor"));

        let safety = desc.safety.as_ref().unwrap();
        assert_eq!(safety.watchdog_timeout_ms, Some(500));
        assert_eq!(safety.rules.len(), 1);
        assert_eq!(safety.rules[0].name, "obstacle_stop");

        let perception = desc.perception.as_ref().unwrap();
        assert_eq!(
            perception.vision_provider.as_deref(),
            Some("gemini")
        );
        assert_eq!(perception.motion_detection, Some(true));

        let voice = desc.voice.as_ref().unwrap();
        assert_eq!(voice.stt_provider.as_deref(), Some("whisper"));

        let body = desc.body.as_ref().unwrap();
        assert_eq!(body.base.as_deref(), Some("fixed"));
        assert_eq!(body.weight_kg, Some(0.5));
    }

    #[test]
    fn test_system_prompt_generation() {
        let toml_str = r#"
[robot]
name = "Buddy"
type = "desktop_companion"
description = "A small desktop companion robot"

[body]
base = "fixed"
weight_kg = 0.5

[[sensors]]
name = "camera"
type = "camera"
device = "/dev/video0"

[[actuators]]
name = "left_arm"
type = "servo"
pin = 12
angle_range = [0, 180]

[safety]
watchdog_timeout_ms = 500

[[safety.rules]]
name = "obstacle_stop"
condition = "front_distance < 10"
action = "stop_all_motors"

[hardware]
bridge = "mock"
"#;
        let desc: RobotDescription = toml::from_str(toml_str).unwrap();
        let prompt = desc.to_system_prompt();

        assert!(prompt.contains("## Robot: Buddy"));
        assert!(prompt.contains("desktop_companion"));
        assert!(prompt.contains("small desktop companion robot"));
        assert!(prompt.contains("### Sensors"));
        assert!(prompt.contains("**camera**"));
        assert!(prompt.contains("### Actuators"));
        assert!(prompt.contains("**left_arm**"));
        assert!(prompt.contains("range=[0, 180]"));
        assert!(prompt.contains("### Safety"));
        assert!(prompt.contains("Watchdog: 500ms"));
        assert!(prompt.contains("obstacle_stop"));
        assert!(prompt.contains("base=fixed"));
        assert!(prompt.contains("0.5kg"));
    }

    #[test]
    fn test_ros2_config() {
        let toml_str = r#"
[robot]
name = "RosBot"
type = "mobile"

[hardware]
bridge = "ros2"

[hardware.ros2]
namespace = "/uniclaw"
cmd_topic = "/cmd_vel"
sensor_topic = "/sensors"
"#;
        let desc: RobotDescription = toml::from_str(toml_str).unwrap();
        assert_eq!(desc.hardware.bridge, "ros2");
        let ros2 = desc.hardware.ros2.as_ref().unwrap();
        assert_eq!(ros2.namespace.as_deref(), Some("/uniclaw"));
        assert_eq!(ros2.cmd_topic.as_deref(), Some("/cmd_vel"));
        assert_eq!(ros2.sensor_topic.as_deref(), Some("/sensors"));
    }
}
