use std::sync::Arc;
use tokio::sync::{mpsc, watch};

use super::bridge::{HardwareBridge, HardwareCommand};
use super::description::RobotDescription;
use super::safety;
use super::world_state::WorldState;

/// The robot runtime: owns the hardware bridge, polls sensors, and executes actions.
///
/// Sensor data is published via a `watch` channel (latest-value semantics).
/// Actions arrive via an `mpsc` channel and are dispatched to the bridge.
pub struct RobotRuntime {
    description: Arc<RobotDescription>,
    bridge: Arc<dyn HardwareBridge>,
    world_tx: watch::Sender<WorldState>,
    world_rx: watch::Receiver<WorldState>,
    action_tx: mpsc::Sender<HardwareCommand>,
    action_rx: Option<mpsc::Receiver<HardwareCommand>>,
}

impl RobotRuntime {
    pub fn new(description: RobotDescription, bridge: Box<dyn HardwareBridge>) -> Self {
        let (world_tx, world_rx) = watch::channel(WorldState::default());
        let (action_tx, action_rx) = mpsc::channel(32);
        Self {
            description: Arc::new(description),
            bridge: Arc::from(bridge),
            world_tx,
            world_rx,
            action_tx,
            action_rx: Some(action_rx),
        }
    }

    /// Get a clone of the world state receiver (latest-value semantics).
    pub fn world_rx(&self) -> watch::Receiver<WorldState> {
        self.world_rx.clone()
    }

    /// Get a clone of the action sender for submitting hardware commands.
    pub fn action_tx(&self) -> mpsc::Sender<HardwareCommand> {
        self.action_tx.clone()
    }

    /// Access the robot description.
    pub fn description(&self) -> &RobotDescription {
        &self.description
    }

    /// Access the hardware bridge.
    #[allow(dead_code)]
    pub fn bridge(&self) -> Arc<dyn HardwareBridge> {
        self.bridge.clone()
    }

    /// Start background tasks: sensor polling and action executor.
    /// Returns join handles for the spawned tasks.
    pub async fn start(&mut self) -> Vec<tokio::task::JoinHandle<()>> {
        let mut tasks = Vec::new();

        // Sensor polling task
        let bridge = self.bridge.clone();
        let world_tx = self.world_tx.clone();
        tasks.push(tokio::spawn(async move {
            let interval = std::time::Duration::from_millis(200);
            loop {
                if let Ok(sensors) = bridge.read_all_sensors().await {
                    world_tx.send_modify(|state| {
                        state.sensors = sensors;
                        state.timestamp = std::time::Instant::now();
                    });
                }
                bridge.heartbeat().await.ok();
                tokio::time::sleep(interval).await;
            }
        }));

        // Action executor task
        if let Some(mut action_rx) = self.action_rx.take() {
            let bridge = self.bridge.clone();
            tasks.push(tokio::spawn(async move {
                while let Some(cmd) = action_rx.recv().await {
                    tracing::debug!("Executing action: {cmd:?}");
                    if let Err(e) = bridge.send_command(cmd).await {
                        tracing::error!("Action execution failed: {e}");
                    }
                }
            }));
        }

        // Safety monitor
        if let Some(ref safety_config) = self.description.safety {
            let safety_rules: Vec<_> = safety_config
                .rules
                .iter()
                .filter_map(|r| match safety::ParsedRule::parse(r) {
                    Ok(parsed) => Some(parsed),
                    Err(e) => {
                        tracing::warn!("Skipping safety rule '{}': {e}", r.name);
                        None
                    }
                })
                .collect();

            if !safety_rules.is_empty() {
                let mut monitor = safety::SafetyMonitor::new(
                    safety_rules,
                    self.world_rx.clone(),
                    self.action_tx.clone(),
                );
                tracing::info!(
                    "Safety monitor active with {} rule(s)",
                    monitor.rules_count()
                );
                tasks.push(tokio::spawn(async move {
                    monitor.run().await;
                }));
            }
        }

        tracing::info!(
            "Robot runtime started: {} sensor(s), {} actuator(s), bridge={}",
            self.description.sensors.len(),
            self.description.actuators.len(),
            self.bridge.name(),
        );
        tasks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::robot::bridge::mock::MockBridge;

    fn test_description() -> RobotDescription {
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

[hardware]
bridge = "mock"
"#;
        toml::from_str(toml_str).unwrap()
    }

    #[tokio::test]
    async fn test_runtime_new() {
        let desc = test_description();
        let bridge = Box::new(MockBridge::new());
        let runtime = RobotRuntime::new(desc, bridge);

        assert_eq!(runtime.description().robot.name, "TestBot");
        assert_eq!(runtime.description().sensors.len(), 1);
        assert_eq!(runtime.description().actuators.len(), 1);
    }

    #[tokio::test]
    async fn test_runtime_start_and_poll() {
        let desc = test_description();
        let bridge = Box::new(MockBridge::new());
        let mut runtime = RobotRuntime::new(desc, bridge);

        let mut world_rx = runtime.world_rx();
        let _tasks = runtime.start().await;

        // Wait for at least one sensor poll
        tokio::time::timeout(std::time::Duration::from_secs(2), world_rx.changed())
            .await
            .expect("timed out waiting for world update")
            .expect("world_rx error");

        let state = world_rx.borrow();
        // MockBridge returns 3 default sensors
        assert!(!state.sensors.is_empty());
    }

    #[tokio::test]
    async fn test_runtime_action_dispatch() {
        let desc = test_description();
        let mock = MockBridge::new();
        let bridge = Box::new(MockBridge::new());
        // We need a shared reference to check the log -- create a new one and use Arc
        drop(mock);

        let shared_bridge = Arc::new(MockBridge::new());
        // Build runtime with a clone as Box<dyn HardwareBridge>
        // Actually, we can't easily do this with Box. Let's just test via the action_tx.
        let desc2 = test_description();
        let mock2 = Arc::new(MockBridge::new());
        let (world_tx, world_rx) = watch::channel(WorldState::default());
        let (action_tx, action_rx) = mpsc::channel(32);

        let mut runtime = RobotRuntime {
            description: Arc::new(desc2),
            bridge: mock2.clone(),
            world_tx,
            world_rx,
            action_tx,
            action_rx: Some(action_rx),
        };

        let _tasks = runtime.start().await;
        let tx = runtime.action_tx();

        tx.send(HardwareCommand::Ping).await.unwrap();
        tx.send(HardwareCommand::ServoSet {
            name: "arm".to_string(),
            angle: 45.0,
            speed_deg_s: None,
        })
        .await
        .unwrap();

        // Give executor a moment to process
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let log = mock2.logged_commands();
        assert!(
            log.len() >= 2,
            "expected at least 2 commands, got {}",
            log.len()
        );
        assert!(matches!(log[0], HardwareCommand::Ping));

        drop(shared_bridge);
    }
}
