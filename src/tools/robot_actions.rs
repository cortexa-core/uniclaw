use async_trait::async_trait;
use serde_json::json;

use super::registry::{Tool, ToolContext, ToolResult};
use crate::robot::bridge::HardwareCommand;
use crate::robot::description::RobotDescription;

// ---------------------------------------------------------------------------
// SetServoTool
// ---------------------------------------------------------------------------

pub struct SetServoTool;

#[async_trait]
impl Tool for SetServoTool {
    fn name(&self) -> &str {
        "set_servo"
    }

    fn description(&self) -> &str {
        "Set a servo actuator to a specific angle"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["name", "angle"],
            "properties": {
                "name": {"type": "string", "description": "Servo name from robot description"},
                "angle": {"type": "number", "description": "Target angle in degrees"}
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let name = match args["name"].as_str() {
            Some(n) => n.to_string(),
            None => return ToolResult::Error("Missing parameter: name".into()),
        };
        let angle = match args["angle"].as_f64() {
            Some(a) => a as f32,
            None => return ToolResult::Error("Missing parameter: angle".into()),
        };
        let Some(ref tx) = ctx.action_tx else {
            return ToolResult::Error("Not in robot mode".into());
        };
        if tx
            .send(HardwareCommand::ServoSet {
                name: name.clone(),
                angle,
                speed_deg_s: None,
            })
            .await
            .is_err()
        {
            return ToolResult::Error("Action executor unavailable".into());
        }
        ToolResult::Success(format!("Set servo '{name}' to {angle}\u{00b0}"))
    }
}

// ---------------------------------------------------------------------------
// SetLedTool
// ---------------------------------------------------------------------------

pub struct SetLedTool;

#[async_trait]
impl Tool for SetLedTool {
    fn name(&self) -> &str {
        "set_led"
    }

    fn description(&self) -> &str {
        "Set an LED or NeoPixel to a specific RGB color"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["name", "r", "g", "b"],
            "properties": {
                "name": {"type": "string", "description": "LED name from robot description"},
                "r": {"type": "number", "description": "Red (0-255)"},
                "g": {"type": "number", "description": "Green (0-255)"},
                "b": {"type": "number", "description": "Blue (0-255)"}
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let name = match args["name"].as_str() {
            Some(n) => n.to_string(),
            None => return ToolResult::Error("Missing parameter: name".into()),
        };
        let r = match args["r"].as_u64() {
            Some(v) if v <= 255 => v as u8,
            _ => return ToolResult::Error("Missing or invalid parameter: r (0-255)".into()),
        };
        let g = match args["g"].as_u64() {
            Some(v) if v <= 255 => v as u8,
            _ => return ToolResult::Error("Missing or invalid parameter: g (0-255)".into()),
        };
        let b = match args["b"].as_u64() {
            Some(v) if v <= 255 => v as u8,
            _ => return ToolResult::Error("Missing or invalid parameter: b (0-255)".into()),
        };
        let Some(ref tx) = ctx.action_tx else {
            return ToolResult::Error("Not in robot mode".into());
        };
        if tx
            .send(HardwareCommand::LedSet {
                name: name.clone(),
                r,
                g,
                b,
            })
            .await
            .is_err()
        {
            return ToolResult::Error("Action executor unavailable".into());
        }
        ToolResult::Success(format!("Set LED '{name}' to RGB({r}, {g}, {b})"))
    }
}

// ---------------------------------------------------------------------------
// SayTool
// ---------------------------------------------------------------------------

pub struct SayTool;

#[async_trait]
impl Tool for SayTool {
    fn name(&self) -> &str {
        "say"
    }

    fn description(&self) -> &str {
        "Speak text aloud (TTS)"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["text"],
            "properties": {
                "text": {"type": "string", "description": "Text to speak"}
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let text = match args["text"].as_str() {
            Some(t) => t.to_string(),
            None => return ToolResult::Error("Missing parameter: text".into()),
        };
        // TODO: wire to TTS engine when voice pipeline is ready
        tracing::info!("Say: {text}");
        ToolResult::Success(format!("Said: {text}"))
    }
}

// ---------------------------------------------------------------------------
// StopTool
// ---------------------------------------------------------------------------

pub struct StopTool;

#[async_trait]
impl Tool for StopTool {
    fn name(&self) -> &str {
        "stop"
    }

    fn description(&self) -> &str {
        "Emergency stop — halt all actuators immediately"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let Some(ref tx) = ctx.action_tx else {
            return ToolResult::Error("Not in robot mode".into());
        };
        if tx.send(HardwareCommand::EmergencyStop).await.is_err() {
            return ToolResult::Error("Action executor unavailable".into());
        }
        ToolResult::Success("Emergency stop sent".into())
    }
}

// ---------------------------------------------------------------------------
// GetSensorTool
// ---------------------------------------------------------------------------

pub struct GetSensorTool;

#[async_trait]
impl Tool for GetSensorTool {
    fn name(&self) -> &str {
        "get_sensor"
    }

    fn description(&self) -> &str {
        "Read the current value of a named sensor"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": {"type": "string", "description": "Sensor name from robot description"}
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let name = match args["name"].as_str() {
            Some(n) => n,
            None => return ToolResult::Error("Missing parameter: name".into()),
        };
        let Some(ref rx) = ctx.world_rx else {
            return ToolResult::Error("Not in robot mode".into());
        };
        let state = rx.borrow().clone();
        match state.sensors.get(name) {
            Some(val) => ToolResult::Success(format!("{val:?}")),
            None => ToolResult::Error(format!("Unknown sensor: {name}")),
        }
    }
}

// ---------------------------------------------------------------------------
// GetWorldStateTool
// ---------------------------------------------------------------------------

pub struct GetWorldStateTool;

#[async_trait]
impl Tool for GetWorldStateTool {
    fn name(&self) -> &str {
        "get_world_state"
    }

    fn description(&self) -> &str {
        "Get the full current world state including all sensors and status"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let Some(ref rx) = ctx.world_rx else {
            return ToolResult::Error("Not in robot mode".into());
        };
        let state = rx.borrow().clone();
        ToolResult::Success(state.to_context_section())
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register_robot_tools(
    registry: &mut super::registry::ToolRegistry,
    description: &RobotDescription,
) {
    // Always register world state and stop
    registry.register(GetWorldStateTool);
    registry.register(StopTool);

    // Register based on capabilities
    if description.has_actuator_type("servo") {
        registry.register(SetServoTool);
    }
    if description.has_actuator_type("neopixel") || description.has_actuator_type("led") {
        registry.register(SetLedTool);
    }
    if description.has_actuator_type("audio_output") {
        registry.register(SayTool);
    }
    if !description.sensors.is_empty() {
        registry.register(GetSensorTool);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::robot::bridge::SensorValue;
    use crate::robot::world_state::WorldState;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn test_ctx_with_channels() -> (
        ToolContext,
        tokio::sync::mpsc::Receiver<HardwareCommand>,
        tokio::sync::watch::Sender<WorldState>,
    ) {
        let (action_tx, action_rx) = tokio::sync::mpsc::channel(32);
        let (world_tx, world_rx) = tokio::sync::watch::channel(WorldState::default());
        let ctx = ToolContext {
            data_dir: PathBuf::from("/tmp/uniclaw-test"),
            session_id: "test".into(),
            config: Arc::new(
                toml::from_str::<Config>("[agent]\n[llm]\nprovider=\"anthropic\"\nmodel=\"test\"")
                    .unwrap(),
            ),
            action_tx: Some(action_tx),
            world_rx: Some(world_rx),
        };
        (ctx, action_rx, world_tx)
    }

    fn test_ctx_no_robot() -> ToolContext {
        ToolContext {
            data_dir: PathBuf::from("/tmp/uniclaw-test"),
            session_id: "test".into(),
            config: Arc::new(
                toml::from_str::<Config>("[agent]\n[llm]\nprovider=\"anthropic\"\nmodel=\"test\"")
                    .unwrap(),
            ),
            action_tx: None,
            world_rx: None,
        }
    }

    #[tokio::test]
    async fn test_set_servo_sends_command() {
        let (ctx, mut rx, _world_tx) = test_ctx_with_channels();
        let tool = SetServoTool;
        let result = tool
            .execute(json!({"name": "arm", "angle": 45.0}), &ctx)
            .await;
        assert!(!result.is_error());
        assert!(result.content().contains("arm"));
        assert!(result.content().contains("45"));

        let cmd = rx.try_recv().unwrap();
        assert!(matches!(
            cmd,
            HardwareCommand::ServoSet {
                name,
                angle,
                ..
            } if name == "arm" && (angle - 45.0).abs() < f32::EPSILON
        ));
    }

    #[tokio::test]
    async fn test_set_servo_no_robot_mode() {
        let ctx = test_ctx_no_robot();
        let tool = SetServoTool;
        let result = tool
            .execute(json!({"name": "arm", "angle": 45.0}), &ctx)
            .await;
        assert!(result.is_error());
        assert!(result.content().contains("Not in robot mode"));
    }

    #[tokio::test]
    async fn test_set_servo_missing_params() {
        let (ctx, _rx, _world_tx) = test_ctx_with_channels();
        let tool = SetServoTool;
        let result = tool.execute(json!({"name": "arm"}), &ctx).await;
        assert!(result.is_error());
        assert!(result.content().contains("angle"));
    }

    #[tokio::test]
    async fn test_set_led_sends_command() {
        let (ctx, mut rx, _world_tx) = test_ctx_with_channels();
        let tool = SetLedTool;
        let result = tool
            .execute(json!({"name": "ring", "r": 255, "g": 0, "b": 128}), &ctx)
            .await;
        assert!(!result.is_error());
        assert!(result.content().contains("ring"));

        let cmd = rx.try_recv().unwrap();
        assert!(matches!(
            cmd,
            HardwareCommand::LedSet {
                name,
                r: 255,
                g: 0,
                b: 128,
            } if name == "ring"
        ));
    }

    #[tokio::test]
    async fn test_say_tool() {
        let ctx = test_ctx_no_robot();
        let tool = SayTool;
        let result = tool
            .execute(json!({"text": "Hello world"}), &ctx)
            .await;
        assert!(!result.is_error());
        assert_eq!(result.content(), "Said: Hello world");
    }

    #[tokio::test]
    async fn test_stop_sends_emergency_stop() {
        let (ctx, mut rx, _world_tx) = test_ctx_with_channels();
        let tool = StopTool;
        let result = tool.execute(json!({}), &ctx).await;
        assert!(!result.is_error());

        let cmd = rx.try_recv().unwrap();
        assert!(matches!(cmd, HardwareCommand::EmergencyStop));
    }

    #[tokio::test]
    async fn test_get_sensor() {
        let (ctx, _rx, world_tx) = test_ctx_with_channels();
        world_tx.send_modify(|state| {
            state
                .sensors
                .insert("distance".to_string(), SensorValue::Distance(42.5));
        });
        let tool = GetSensorTool;
        let result = tool
            .execute(json!({"name": "distance"}), &ctx)
            .await;
        assert!(!result.is_error());
        assert!(result.content().contains("42.5"));
    }

    #[tokio::test]
    async fn test_get_sensor_unknown() {
        let (ctx, _rx, _world_tx) = test_ctx_with_channels();
        let tool = GetSensorTool;
        let result = tool
            .execute(json!({"name": "nonexistent"}), &ctx)
            .await;
        assert!(result.is_error());
        assert!(result.content().contains("Unknown sensor"));
    }

    #[tokio::test]
    async fn test_get_world_state() {
        let (ctx, _rx, world_tx) = test_ctx_with_channels();
        world_tx.send_modify(|state| {
            state
                .sensors
                .insert("temp".to_string(), SensorValue::Temperature(25.0));
        });
        let tool = GetWorldStateTool;
        let result = tool.execute(json!({}), &ctx).await;
        assert!(!result.is_error());
        assert!(result.content().contains("Perception"));
    }

    #[tokio::test]
    async fn test_register_robot_tools() {
        let toml_str = r#"
[robot]
name = "TestBot"
type = "test"

[[sensors]]
name = "distance"
type = "ultrasonic"

[[actuators]]
name = "arm"
type = "servo"

[[actuators]]
name = "ring"
type = "neopixel"

[hardware]
bridge = "mock"
"#;
        let desc: RobotDescription = toml::from_str(toml_str).unwrap();
        let mut registry = super::super::registry::ToolRegistry::new();
        register_robot_tools(&mut registry, &desc);

        let names = registry.tool_names();
        assert!(names.contains(&"get_world_state"));
        assert!(names.contains(&"stop"));
        assert!(names.contains(&"set_servo"));
        assert!(names.contains(&"set_led"));
        assert!(names.contains(&"get_sensor"));
        // say should NOT be registered (no audio_output actuator)
        assert!(!names.contains(&"say"));
    }
}
