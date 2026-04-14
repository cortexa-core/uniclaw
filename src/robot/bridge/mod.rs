/// A command sent to robot hardware
#[derive(Debug, Clone)]
#[allow(dead_code)] // stub for future robot tools
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
#[allow(dead_code)] // stub for future robot tools
pub enum SensorValue {
    Distance(f32),
    Temperature(f32),
    Orientation {
        roll: f32,
        pitch: f32,
        yaw: f32,
    },
    Boolean(bool),
    Raw(i32),
}
