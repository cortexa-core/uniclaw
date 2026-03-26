//! MCP transport implementations: stdio (local process) and HTTP (remote)

use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use super::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};

/// Transport-agnostic interface for sending JSON-RPC messages
pub enum Transport {
    Stdio(StdioTransport),
    Http(HttpTransport),
}

impl Transport {
    /// Send a request and wait for the response
    pub async fn request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        match self {
            Transport::Stdio(t) => t.request(method, params).await,
            Transport::Http(t) => t.request(method, params).await,
        }
    }

    /// Send a notification (no response expected)
    pub async fn notify(&self, method: &str) -> Result<()> {
        match self {
            Transport::Stdio(t) => t.notify(method).await,
            Transport::Http(t) => t.notify(method).await,
        }
    }

    /// Shut down the transport
    #[allow(dead_code)]
    pub async fn shutdown(&self) {
        match self {
            Transport::Stdio(t) => t.shutdown().await,
            Transport::Http(_) => {} // nothing to clean up
        }
    }
}

// --- Stdio Transport ---

pub struct StdioTransport {
    stdin: Mutex<tokio::process::ChildStdin>,
    stdout: Mutex<BufReader<tokio::process::ChildStdout>>,
    #[allow(dead_code)] // kept alive for process lifetime, used in shutdown
    child: Mutex<tokio::process::Child>,
}

impl StdioTransport {
    pub async fn spawn(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self> {
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (key, value) in env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn()
            .map_err(|e| anyhow!("Failed to spawn MCP server '{command}': {e}"))?;

        let stdin = child.stdin.take()
            .ok_or_else(|| anyhow!("Failed to capture MCP server stdin"))?;
        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow!("Failed to capture MCP server stdout"))?;

        Ok(Self {
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(BufReader::new(stdout)),
            child: Mutex::new(child),
        })
    }

    async fn request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let req = JsonRpcRequest::new(method, params);
        let req_json = serde_json::to_string(&req)?;

        // Write request
        {
            let mut stdin = self.stdin.lock().await;
            stdin.write_all(req_json.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
        }

        // Read response
        let response = self.read_response().await?;
        response.into_result()
    }

    async fn notify(&self, method: &str) -> Result<()> {
        let notif = JsonRpcNotification::new(method);
        let json = serde_json::to_string(&notif)?;

        let mut stdin = self.stdin.lock().await;
        stdin.write_all(json.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn read_response(&self) -> Result<JsonRpcResponse> {
        let mut stdout = self.stdout.lock().await;
        let mut line = String::new();

        // Read lines until we get a valid JSON-RPC response
        // (skip any non-JSON output like logs)
        loop {
            line.clear();
            let bytes = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                stdout.read_line(&mut line),
            )
            .await
            .map_err(|_| anyhow!("MCP server response timed out (30s)"))??;

            if bytes == 0 {
                return Err(anyhow!("MCP server closed stdout (process may have crashed)"));
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Try to parse as JSON-RPC response
            match serde_json::from_str::<JsonRpcResponse>(trimmed) {
                Ok(resp) => return Ok(resp),
                Err(_) => {
                    // Not a JSON-RPC response — might be server log output, skip
                    tracing::debug!("MCP stdio: skipping non-JSON line: {}", trimmed);
                    continue;
                }
            }
        }
    }

    #[allow(dead_code)]
    async fn shutdown(&self) {
        let mut child = self.child.lock().await;
        child.kill().await.ok();
    }
}

// --- HTTP Transport ---

pub struct HttpTransport {
    url: String,
    client: reqwest::Client,
}

impl HttpTransport {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    async fn request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let req = JsonRpcRequest::new(method, params);

        let response = self.client
            .post(&self.url)
            .header("content-type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| anyhow!("MCP HTTP request failed: {e}"))?;

        if !response.status().is_success() {
            return Err(anyhow!("MCP HTTP error: {}", response.status()));
        }

        let resp: JsonRpcResponse = response.json().await
            .map_err(|e| anyhow!("MCP HTTP response parse error: {e}"))?;

        resp.into_result()
    }

    async fn notify(&self, method: &str) -> Result<()> {
        let notif = JsonRpcNotification::new(method);
        self.client
            .post(&self.url)
            .header("content-type", "application/json")
            .json(&notif)
            .send()
            .await
            .map_err(|e| anyhow!("MCP HTTP notification failed: {e}"))?;
        Ok(())
    }
}
