use std::collections::HashMap;
use std::time::Instant;

/// Shared view of the physical world
#[derive(Debug, Clone)]
#[allow(dead_code)] // stub fields used by future robot runtime
pub struct WorldState {
    pub timestamp: Instant,
    pub scene_description: Option<String>,
    pub scene_timestamp: Option<Instant>,
    pub motion_detected: bool,
    pub sensors: HashMap<String, super::bridge::SensorValue>,
    pub actuator_positions: HashMap<String, f32>,
    pub battery_percent: Option<f32>,
    pub is_moving: bool,
    pub current_behavior: Option<String>,
}

impl Default for WorldState {
    fn default() -> Self {
        Self {
            timestamp: Instant::now(),
            scene_description: None,
            scene_timestamp: None,
            motion_detected: false,
            sensors: HashMap::new(),
            actuator_positions: HashMap::new(),
            battery_percent: None,
            is_moving: false,
            current_behavior: None,
        }
    }
}

impl WorldState {
    pub fn to_context_section(&self) -> String {
        let mut lines = vec!["## Current Perception\n".to_string()];
        if let Some(ref desc) = self.scene_description {
            let age = self
                .scene_timestamp
                .map(|t| format!("{}s ago", t.elapsed().as_secs()))
                .unwrap_or_else(|| "unknown".into());
            lines.push(format!("Scene ({age}): {desc}"));
        }
        lines.push(format!(
            "Motion: {}",
            if self.motion_detected {
                "detected"
            } else {
                "none"
            }
        ));
        for (name, value) in &self.sensors {
            lines.push(format!("Sensor {name}: {value:?}"));
        }
        if let Some(pct) = self.battery_percent {
            lines.push(format!("Battery: {pct:.0}%"));
        }
        if let Some(ref behavior) = self.current_behavior {
            lines.push(format!("Behavior: {behavior}"));
        }
        lines.join("\n")
    }
}
