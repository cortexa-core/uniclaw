use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub agent: AgentConfig,
    pub llm: LlmConfig,
    #[serde(default)]
    pub server: Option<ServerConfig>,
    #[serde(default)]
    pub cron: Option<CronConfig>,
    #[serde(default)]
    pub heartbeat: Option<HeartbeatConfig>,
    #[serde(default)]
    pub mcp_servers: Vec<crate::mcp::client::McpServerConfig>,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    #[allow(dead_code)] // used in future phases for file logging
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_true")]
    pub http_enabled: bool,
    #[serde(default = "default_http_port")]
    pub http_port: u16,
    #[serde(default = "default_http_bind")]
    pub http_bind: String,
    #[serde(default = "default_true")]
    pub mqtt_enabled: bool,
    #[serde(default = "default_mqtt_broker")]
    pub mqtt_broker: String,
    #[serde(default = "default_mqtt_port")]
    pub mqtt_port: u16,
    #[serde(default = "default_device_id")]
    pub mqtt_device_id: String,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct CronConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_cron_interval")]
    pub check_interval_secs: u64,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct HeartbeatConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_heartbeat_interval")]
    pub interval_secs: u64,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct AgentConfig {
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    #[serde(default = "default_max_tool_calls")]
    pub max_tool_calls_per_iteration: usize,
    #[serde(default = "default_consolidation")]
    pub consolidation_threshold: usize,
    #[serde(default = "default_cache_ttl")]
    pub context_cache_ttl_secs: u64,
    #[serde(default = "default_memory_max")]
    pub memory_max_bytes: usize,
    #[serde(default = "default_session_age")]
    pub session_max_age_days: u64,
    #[serde(default = "default_session_count")]
    pub session_max_count: usize,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct LlmConfig {
    pub provider: String,
    #[serde(default)]
    pub api_key_env: String,
    pub model: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    pub fallback: Option<Box<LlmConfig>>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize, Default)]
pub struct ToolsConfig {
    #[serde(default = "default_true")]
    pub shell_enabled: bool,
    #[serde(default)]
    pub shell_allowed_commands: Vec<String>,
    #[serde(default = "default_shell_timeout")]
    pub shell_timeout_secs: u64,
    #[serde(default = "default_true")]
    pub http_fetch_enabled: bool,
    #[serde(default = "default_http_timeout")]
    pub http_fetch_timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    pub file: Option<String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file: None,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Failed to read config at {}: {e}", path.display()))?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse config: {e}"))?;
        Ok(config)
    }
}

impl LlmConfig {
    pub fn api_key(&self) -> Result<String> {
        if self.api_key_env.is_empty() {
            return Ok(String::new());
        }
        std::env::var(&self.api_key_env)
            .map_err(|_| anyhow!("Environment variable {} is not set", self.api_key_env))
    }
}

fn default_max_iterations() -> usize { 10 }
fn default_max_tool_calls() -> usize { 4 }
fn default_consolidation() -> usize { 40 }
fn default_cache_ttl() -> u64 { 60 }
fn default_memory_max() -> usize { 8192 }
fn default_session_age() -> u64 { 30 }
fn default_session_count() -> usize { 100 }
fn default_base_url() -> String { "https://api.anthropic.com".to_string() }
fn default_max_tokens() -> u32 { 1024 }
fn default_temperature() -> f32 { 0.7 }
fn default_timeout() -> u64 { 60 }
fn default_true() -> bool { true }
fn default_shell_timeout() -> u64 { 10 }
fn default_http_timeout() -> u64 { 15 }
fn default_log_level() -> String { "info".to_string() }
fn default_http_port() -> u16 { 3000 }
fn default_http_bind() -> String { "0.0.0.0".to_string() }
fn default_mqtt_broker() -> String { "localhost".to_string() }
fn default_mqtt_port() -> u16 { 1883 }
fn default_device_id() -> String { "uniclaw-01".to_string() }
fn default_cron_interval() -> u64 { 60 }
fn default_heartbeat_interval() -> u64 { 1800 }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config() {
        let toml = r#"
[agent]
max_iterations = 5

[llm]
provider = "anthropic"
api_key_env = "TEST_KEY"
model = "claude-sonnet-4-6"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.agent.max_iterations, 5);
        assert_eq!(config.llm.provider, "anthropic");
        assert_eq!(config.llm.model, "claude-sonnet-4-6");
    }

    #[test]
    fn test_config_defaults() {
        let toml = r#"
[agent]

[llm]
provider = "openai_compatible"
model = "gpt-4o"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.agent.max_iterations, 10);
        assert_eq!(config.agent.consolidation_threshold, 40);
        assert_eq!(config.llm.max_tokens, 1024);
        assert_eq!(config.llm.temperature, 0.7);
    }

    #[test]
    fn test_api_key_from_env() {
        std::env::set_var("UNICLAW_TEST_KEY", "sk-test-123");
        let config = LlmConfig {
            provider: "anthropic".into(),
            api_key_env: "UNICLAW_TEST_KEY".into(),
            model: "test".into(),
            base_url: default_base_url(),
            max_tokens: 1024,
            temperature: 0.7,
            timeout_secs: 60,
            fallback: None,
        };
        assert_eq!(config.api_key().unwrap(), "sk-test-123");
        std::env::remove_var("UNICLAW_TEST_KEY");
    }

    #[test]
    fn test_missing_api_key_error() {
        let config = LlmConfig {
            provider: "anthropic".into(),
            api_key_env: "NONEXISTENT_KEY_12345".into(),
            model: "test".into(),
            base_url: default_base_url(),
            max_tokens: 1024,
            temperature: 0.7,
            timeout_secs: 60,
            fallback: None,
        };
        assert!(config.api_key().is_err());
    }

    #[test]
    fn test_empty_api_key_env_ok() {
        let config = LlmConfig {
            provider: "openai_compatible".into(),
            api_key_env: String::new(),
            model: "test".into(),
            base_url: "http://localhost:11434".into(),
            max_tokens: 1024,
            temperature: 0.7,
            timeout_secs: 60,
            fallback: None,
        };
        assert_eq!(config.api_key().unwrap(), "");
    }
}
