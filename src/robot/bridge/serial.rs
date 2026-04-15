use anyhow::{bail, Result};

// ── Wire constants ──────────────────────────────────────────────────────────

const START_BYTE: u8 = 0xAA;

// Command types (SBC → MCU)
const CMD_SERVO_SET: u8 = 0x01;
const CMD_MOTOR_SET: u8 = 0x02;
const CMD_LED_SET: u8 = 0x03;
const CMD_LED_PATTERN: u8 = 0x04;
const CMD_SENSOR_REQUEST: u8 = 0x05;
const CMD_PING: u8 = 0x06;
const CMD_E_STOP: u8 = 0x07;
const CMD_STATUS_REQUEST: u8 = 0x08;
const CMD_HEARTBEAT: u8 = 0x09;

// Response types (MCU → SBC)
const RESP_ACK: u8 = 0x81;
const RESP_SENSOR_DATA: u8 = 0x82;
const RESP_STATUS: u8 = 0x83;
const RESP_ERROR: u8 = 0x84;
const RESP_PONG: u8 = 0x85;

// ── Protocol types (always available, no feature gate) ──────────────────────

/// A binary frame for the serial protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub seq: u8,
    pub cmd_type: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    /// Encode a frame into wire format:
    /// [0xAA] [LEN:u16-LE] [SEQ:u8] [TYPE:u8] [PAYLOAD...] [CRC8]
    ///
    /// LEN covers SEQ + TYPE + PAYLOAD (does not include START, LEN, or CRC).
    pub fn encode(&self) -> Vec<u8> {
        let inner_len = 2 + self.payload.len(); // seq + type + payload
        let len_u16 = inner_len as u16;
        let len_bytes = len_u16.to_le_bytes();

        // CRC is XOR of all bytes from LEN through PAYLOAD
        let mut crc: u8 = 0;
        crc ^= len_bytes[0];
        crc ^= len_bytes[1];
        crc ^= self.seq;
        crc ^= self.cmd_type;
        for &b in &self.payload {
            crc ^= b;
        }

        let mut buf = Vec::with_capacity(1 + 2 + inner_len + 1);
        buf.push(START_BYTE);
        buf.extend_from_slice(&len_bytes);
        buf.push(self.seq);
        buf.push(self.cmd_type);
        buf.extend_from_slice(&self.payload);
        buf.push(crc);
        buf
    }

    /// Decode a frame from a byte buffer. Returns (frame, bytes_consumed) on
    /// success. Returns an error if the buffer is incomplete or the CRC is bad.
    pub fn decode(data: &[u8]) -> Result<(Frame, usize)> {
        // Minimum frame: START(1) + LEN(2) + SEQ(1) + TYPE(1) + CRC(1) = 6
        if data.len() < 6 {
            bail!("incomplete frame: need at least 6 bytes, got {}", data.len());
        }
        if data[0] != START_BYTE {
            bail!("bad start byte: expected 0xAA, got 0x{:02X}", data[0]);
        }

        let len = u16::from_le_bytes([data[1], data[2]]) as usize;
        let total = 1 + 2 + len + 1; // start + len_field + inner + crc
        if data.len() < total {
            bail!(
                "incomplete frame: need {} bytes, got {}",
                total,
                data.len()
            );
        }

        // Verify CRC: XOR of bytes from LEN through PAYLOAD (indices 1..1+2+len)
        let crc_range = &data[1..3 + len];
        let expected_crc = Self::crc8(crc_range);
        let actual_crc = data[total - 1];
        if expected_crc != actual_crc {
            bail!(
                "CRC mismatch: expected 0x{:02X}, got 0x{:02X}",
                expected_crc,
                actual_crc
            );
        }

        let seq = data[3];
        let cmd_type = data[4];
        let payload = data[5..3 + len].to_vec();

        Ok((
            Frame {
                seq,
                cmd_type,
                payload,
            },
            total,
        ))
    }

    /// CRC8: XOR of all provided bytes.
    fn crc8(data: &[u8]) -> u8 {
        data.iter().fold(0u8, |acc, &b| acc ^ b)
    }
}

// ── Command frame builders ──────────────────────────────────────────────────

/// SERVO_SET: {id:u8, angle:u16-LE, speed:u16-LE}
pub fn servo_set_frame(seq: u8, id: u8, angle: u16, speed: u16) -> Frame {
    let mut payload = Vec::with_capacity(5);
    payload.push(id);
    payload.extend_from_slice(&angle.to_le_bytes());
    payload.extend_from_slice(&speed.to_le_bytes());
    Frame {
        seq,
        cmd_type: CMD_SERVO_SET,
        payload,
    }
}

/// MOTOR_SET: {id:u8, speed:i16-LE, duration_ms:u16-LE}
pub fn motor_set_frame(seq: u8, id: u8, speed: i16, duration_ms: u16) -> Frame {
    let mut payload = Vec::with_capacity(5);
    payload.push(id);
    payload.extend_from_slice(&speed.to_le_bytes());
    payload.extend_from_slice(&duration_ms.to_le_bytes());
    Frame {
        seq,
        cmd_type: CMD_MOTOR_SET,
        payload,
    }
}

/// LED_SET: {id:u8, r:u8, g:u8, b:u8}
pub fn led_set_frame(seq: u8, id: u8, r: u8, g: u8, b: u8) -> Frame {
    Frame {
        seq,
        cmd_type: CMD_LED_SET,
        payload: vec![id, r, g, b],
    }
}

/// LED_PATTERN: {id:u8, pattern:u8}
pub fn led_pattern_frame(seq: u8, id: u8, pattern: u8) -> Frame {
    Frame {
        seq,
        cmd_type: CMD_LED_PATTERN,
        payload: vec![id, pattern],
    }
}

/// SENSOR_REQUEST: {id:u8}
pub fn sensor_request_frame(seq: u8, id: u8) -> Frame {
    Frame {
        seq,
        cmd_type: CMD_SENSOR_REQUEST,
        payload: vec![id],
    }
}

/// PING: empty payload
pub fn ping_frame(seq: u8) -> Frame {
    Frame {
        seq,
        cmd_type: CMD_PING,
        payload: vec![],
    }
}

/// E_STOP: empty payload
pub fn estop_frame(seq: u8) -> Frame {
    Frame {
        seq,
        cmd_type: CMD_E_STOP,
        payload: vec![],
    }
}

/// STATUS_REQUEST: empty payload
pub fn status_request_frame(seq: u8) -> Frame {
    Frame {
        seq,
        cmd_type: CMD_STATUS_REQUEST,
        payload: vec![],
    }
}

/// HEARTBEAT: empty payload
pub fn heartbeat_frame(seq: u8) -> Frame {
    Frame {
        seq,
        cmd_type: CMD_HEARTBEAT,
        payload: vec![],
    }
}

// ── Response parsing ────────────────────────────────────────────────────────

use super::SensorValue;

/// Parse SENSOR_DATA payload: {id:u8, type:u8, value:i32-LE}
///
/// Sensor types: 0=Distance, 1=Temperature, 2=Boolean, other=Raw.
pub fn parse_sensor_data(payload: &[u8]) -> Result<(u8, SensorValue)> {
    if payload.len() < 6 {
        bail!(
            "SENSOR_DATA payload too short: need 6 bytes, got {}",
            payload.len()
        );
    }
    let id = payload[0];
    let sensor_type = payload[1];
    let raw = i32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]);

    let value = match sensor_type {
        0 => SensorValue::Distance(raw as f32 / 10.0), // mm → cm
        1 => SensorValue::Temperature(raw as f32 / 100.0), // centidegrees → degrees
        2 => SensorValue::Boolean(raw != 0),
        _ => SensorValue::Raw(raw),
    };

    Ok((id, value))
}

/// Parse STATUS payload: {battery:u8, error_flags:u8}
pub fn parse_status(payload: &[u8]) -> Result<(u8, u8)> {
    if payload.len() < 2 {
        bail!(
            "STATUS payload too short: need 2 bytes, got {}",
            payload.len()
        );
    }
    Ok((payload[0], payload[1]))
}

// ── Serial bridge (feature-gated) ──────────────────────────────────────────

#[cfg(feature = "serial")]
mod bridge {
    use super::*;
    use crate::robot::bridge::{HardwareBridge, HardwareCommand};
    use anyhow::{bail, Result};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU8, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::Mutex;
    use tokio::time::{timeout, Duration};
    use tokio_serial::SerialPortBuilderExt;

    /// A hardware bridge that communicates with an MCU over UART serial.
    pub struct SerialBridge {
        port: Mutex<tokio_serial::SerialStream>,
        seq: AtomicU8,
        /// Mapping from sensor name → numeric sensor ID on the MCU.
        sensor_map: HashMap<String, u8>,
    }

    impl SerialBridge {
        /// Open a serial port and create a new bridge.
        ///
        /// - `path`: device path, e.g. "/dev/ttyAMA0" or "/dev/ttyUSB0"
        /// - `baud`: baud rate (typically 115200)
        /// - `sensor_map`: maps human-readable sensor names to MCU sensor IDs
        pub fn new(
            path: &str,
            baud: u32,
            sensor_map: HashMap<String, u8>,
        ) -> Result<Self> {
            let port = tokio_serial::new(path, baud).open_native_async()?;
            Ok(Self {
                port: Mutex::new(port),
                seq: AtomicU8::new(0),
                sensor_map,
            })
        }

        fn next_seq(&self) -> u8 {
            self.seq.fetch_add(1, Ordering::Relaxed)
        }

        /// Send a frame and wait for any response frame (up to `timeout_ms`).
        async fn send_and_receive(
            &self,
            frame: Frame,
            timeout_ms: u64,
        ) -> Result<Frame> {
            let wire = frame.encode();
            let mut port = self.port.lock().await;
            port.write_all(&wire).await?;

            let mut buf = vec![0u8; 256];
            let mut collected = Vec::new();

            let result = timeout(Duration::from_millis(timeout_ms), async {
                loop {
                    let n = port.read(&mut buf).await?;
                    collected.extend_from_slice(&buf[..n]);
                    match Frame::decode(&collected) {
                        Ok((resp, _consumed)) => return Ok(resp),
                        Err(_) => continue, // need more bytes
                    }
                }
            })
            .await;

            match result {
                Ok(inner) => inner,
                Err(_) => bail!("serial response timed out after {}ms", timeout_ms),
            }
        }

        /// Send a frame without waiting for a response.
        async fn send_fire_and_forget(&self, frame: Frame) -> Result<()> {
            let wire = frame.encode();
            let mut port = self.port.lock().await;
            port.write_all(&wire).await?;
            Ok(())
        }
    }

    #[async_trait]
    impl HardwareBridge for SerialBridge {
        async fn send_command(&self, cmd: HardwareCommand) -> Result<()> {
            let seq = self.next_seq();
            let frame = match cmd {
                HardwareCommand::ServoSet {
                    name: _,
                    angle,
                    speed_deg_s,
                } => servo_set_frame(seq, 0, angle as u16, speed_deg_s.unwrap_or(0.0) as u16),
                HardwareCommand::MotorSet {
                    name: _,
                    speed,
                    duration_ms,
                } => motor_set_frame(
                    seq,
                    0,
                    speed as i16,
                    duration_ms.unwrap_or(0) as u16,
                ),
                HardwareCommand::LedSet {
                    name: _,
                    r,
                    g,
                    b,
                } => led_set_frame(seq, 0, r, g, b),
                HardwareCommand::LedPattern {
                    name: _,
                    pattern,
                } => {
                    let pat_id: u8 = pattern.parse().unwrap_or(0);
                    led_pattern_frame(seq, 0, pat_id)
                }
                HardwareCommand::Ping => ping_frame(seq),
                HardwareCommand::EmergencyStop => estop_frame(seq),
            };
            // Fire and forget — MCU will ACK but we don't block on it for commands.
            self.send_fire_and_forget(frame).await
        }

        async fn read_sensor(&self, sensor_id: &str) -> Result<SensorValue> {
            let &hw_id = self
                .sensor_map
                .get(sensor_id)
                .ok_or_else(|| anyhow::anyhow!("Unknown sensor: {sensor_id}"))?;
            let seq = self.next_seq();
            let frame = sensor_request_frame(seq, hw_id);
            let resp = self.send_and_receive(frame, 500).await?;
            if resp.cmd_type != RESP_SENSOR_DATA {
                bail!(
                    "Expected SENSOR_DATA (0x82), got 0x{:02X}",
                    resp.cmd_type
                );
            }
            let (_id, value) = parse_sensor_data(&resp.payload)?;
            Ok(value)
        }

        async fn read_all_sensors(&self) -> Result<HashMap<String, SensorValue>> {
            let mut result = HashMap::new();
            for (name, &hw_id) in &self.sensor_map {
                let seq = self.next_seq();
                let frame = sensor_request_frame(seq, hw_id);
                match self.send_and_receive(frame, 500).await {
                    Ok(resp) if resp.cmd_type == RESP_SENSOR_DATA => {
                        if let Ok((_id, value)) = parse_sensor_data(&resp.payload) {
                            result.insert(name.clone(), value);
                        }
                    }
                    _ => {
                        tracing::warn!("Failed to read sensor {name} (hw_id={hw_id})");
                    }
                }
            }
            Ok(result)
        }

        async fn heartbeat(&self) -> Result<()> {
            let seq = self.next_seq();
            let frame = heartbeat_frame(seq);
            self.send_fire_and_forget(frame).await
        }

        async fn emergency_stop(&self) -> Result<()> {
            let seq = self.next_seq();
            let frame = estop_frame(seq);
            // E-stop: send twice for reliability, don't wait for ACK.
            let wire = frame.encode();
            let mut port = self.port.lock().await;
            port.write_all(&wire).await?;
            port.write_all(&wire).await?;
            Ok(())
        }

        fn name(&self) -> &str {
            "serial"
        }
    }
}

#[cfg(feature = "serial")]
#[allow(unused_imports)]
pub use bridge::SerialBridge;

// ── Tests (always compiled, protocol-only) ──────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_encode_decode_roundtrip() {
        let original = Frame {
            seq: 42,
            cmd_type: CMD_PING,
            payload: vec![],
        };
        let wire = original.encode();
        let (decoded, consumed) = Frame::decode(&wire).unwrap();
        assert_eq!(consumed, wire.len());
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_frame_encode_decode_with_payload() {
        let original = servo_set_frame(7, 2, 90, 300);
        let wire = original.encode();
        let (decoded, consumed) = Frame::decode(&wire).unwrap();
        assert_eq!(consumed, wire.len());
        assert_eq!(decoded, original);
        // Verify payload contents
        assert_eq!(decoded.payload[0], 2); // servo id
        assert_eq!(u16::from_le_bytes([decoded.payload[1], decoded.payload[2]]), 90);
        assert_eq!(u16::from_le_bytes([decoded.payload[3], decoded.payload[4]]), 300);
    }

    #[test]
    fn test_crc8_calculation() {
        // CRC is XOR of all bytes from LEN through PAYLOAD.
        // For a PING frame (seq=1, type=0x06, no payload):
        //   LEN = 2 (u16-LE: [0x02, 0x00])
        //   SEQ = 0x01, TYPE = 0x06
        //   CRC = 0x02 ^ 0x00 ^ 0x01 ^ 0x06 = 0x05
        let frame = ping_frame(1);
        let wire = frame.encode();
        // Wire: [0xAA, 0x02, 0x00, 0x01, 0x06, CRC]
        assert_eq!(wire[0], 0xAA);
        assert_eq!(wire[1], 0x02);
        assert_eq!(wire[2], 0x00);
        assert_eq!(wire[3], 0x01);
        assert_eq!(wire[4], 0x06);
        let expected_crc = 0x02 ^ 0x00 ^ 0x01 ^ 0x06;
        assert_eq!(wire[5], expected_crc);
    }

    #[test]
    fn test_servo_set_frame() {
        let frame = servo_set_frame(10, 3, 180, 500);
        assert_eq!(frame.seq, 10);
        assert_eq!(frame.cmd_type, CMD_SERVO_SET);
        assert_eq!(frame.payload.len(), 5); // id + angle(2) + speed(2)
        assert_eq!(frame.payload[0], 3);
        assert_eq!(
            u16::from_le_bytes([frame.payload[1], frame.payload[2]]),
            180
        );
        assert_eq!(
            u16::from_le_bytes([frame.payload[3], frame.payload[4]]),
            500
        );
    }

    #[test]
    fn test_parse_sensor_data() {
        // Build a SENSOR_DATA payload: id=5, type=0 (distance), value=1500 (150.0cm)
        let mut payload = vec![5u8, 0u8];
        payload.extend_from_slice(&1500i32.to_le_bytes());
        let (id, value) = parse_sensor_data(&payload).unwrap();
        assert_eq!(id, 5);
        match value {
            SensorValue::Distance(d) => assert!((d - 150.0).abs() < f32::EPSILON),
            other => panic!("Expected Distance, got {:?}", other),
        }

        // Temperature type
        let mut payload = vec![2u8, 1u8];
        payload.extend_from_slice(&2250i32.to_le_bytes());
        let (id, value) = parse_sensor_data(&payload).unwrap();
        assert_eq!(id, 2);
        match value {
            SensorValue::Temperature(t) => assert!((t - 22.5).abs() < f32::EPSILON),
            other => panic!("Expected Temperature, got {:?}", other),
        }
    }

    #[test]
    fn test_frame_decode_partial() {
        // Only 3 bytes — not enough for any frame
        let data = [0xAA, 0x02, 0x00];
        assert!(Frame::decode(&data).is_err());

        // Full header but missing CRC
        let data = [0xAA, 0x02, 0x00, 0x01, 0x06];
        assert!(Frame::decode(&data).is_err());

        // Wrong start byte
        let data = [0xBB, 0x02, 0x00, 0x01, 0x06, 0x05];
        assert!(Frame::decode(&data).is_err());

        // Bad CRC
        let data = [0xAA, 0x02, 0x00, 0x01, 0x06, 0xFF];
        assert!(Frame::decode(&data).is_err());
    }

    #[test]
    fn test_motor_set_frame() {
        let frame = motor_set_frame(1, 0, -200, 1000);
        assert_eq!(frame.cmd_type, CMD_MOTOR_SET);
        assert_eq!(frame.payload[0], 0);
        assert_eq!(
            i16::from_le_bytes([frame.payload[1], frame.payload[2]]),
            -200
        );
        assert_eq!(
            u16::from_le_bytes([frame.payload[3], frame.payload[4]]),
            1000
        );
    }

    #[test]
    fn test_parse_status() {
        let payload = vec![87, 0x04];
        let (battery, flags) = parse_status(&payload).unwrap();
        assert_eq!(battery, 87);
        assert_eq!(flags, 0x04);

        // Too short
        assert!(parse_status(&[42]).is_err());
    }

    #[test]
    fn test_decode_with_trailing_bytes() {
        let frame = estop_frame(99);
        let mut wire = frame.encode();
        wire.extend_from_slice(&[0xFF, 0xFF, 0xFF]); // trailing junk
        let (decoded, consumed) = Frame::decode(&wire).unwrap();
        assert_eq!(decoded, frame);
        assert_eq!(consumed, wire.len() - 3); // should not consume trailing bytes
    }
}
