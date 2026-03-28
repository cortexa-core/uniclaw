use async_trait::async_trait;
use serde_json::json;

use super::registry::{Tool, ToolContext, ToolResult};

pub struct SystemInfoTool;

#[async_trait]
impl Tool for SystemInfoTool {
    fn name(&self) -> &str {
        "system_info"
    }

    fn description(&self) -> &str {
        "Get device system information including OS, architecture, CPU, memory usage, and uptime."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _args: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let mut info = Vec::new();

        info.push(format!("OS: {} {}", std::env::consts::OS, std::env::consts::ARCH));
        info.push(format!("UniClaw version: {}", env!("CARGO_PKG_VERSION")));

        // Memory info (Linux)
        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
            for line in meminfo.lines().take(3) {
                info.push(format!("Memory: {line}"));
            }
        }

        // Uptime (Linux)
        if let Ok(uptime) = std::fs::read_to_string("/proc/uptime") {
            if let Some(secs_str) = uptime.split_whitespace().next() {
                if let Ok(secs) = secs_str.parse::<f64>() {
                    let hours = (secs / 3600.0) as u64;
                    let mins = ((secs % 3600.0) / 60.0) as u64;
                    info.push(format!("Uptime: {hours}h {mins}m"));
                }
            }
        }

        // CPU temp (Raspberry Pi)
        if let Ok(temp) = std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp") {
            if let Ok(millideg) = temp.trim().parse::<f64>() {
                info.push(format!("CPU temperature: {:.1}°C", millideg / 1000.0));
            }
        }

        // Hostname
        if let Ok(hostname) = std::fs::read_to_string("/etc/hostname") {
            info.push(format!("Hostname: {}", hostname.trim()));
        }

        ToolResult::Success(info.join("\n"))
    }
}
