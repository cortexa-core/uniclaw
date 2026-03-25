use anyhow::Result;
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

use crate::agent::{Input, Output};
use crate::config::Config;

#[derive(Serialize)]
struct DeviceStatus {
    status: String,
    version: String,
    model: String,
}

#[derive(Deserialize)]
struct MqttMessage {
    #[serde(default)]
    session_id: String,
    content: String,
}

pub async fn mqtt_task(
    config: &Config,
    inbound_tx: mpsc::Sender<(Input, oneshot::Sender<Output>)>,
) -> Result<()> {
    let device_id = &config.server.as_ref().map(|s| s.mqtt_device_id.as_str()).unwrap_or("miniclaw-01");
    let broker = config.server.as_ref().map(|s| s.mqtt_broker.as_str()).unwrap_or("localhost");
    let port = config.server.as_ref().map(|s| s.mqtt_port).unwrap_or(1883);

    let client_id = format!("miniclaw-{}", &device_id[..device_id.len().min(8)]);
    let mut mqtt_options = MqttOptions::new(&client_id, broker, port);
    mqtt_options.set_keep_alive(Duration::from_secs(30));

    let (client, mut eventloop) = AsyncClient::new(mqtt_options, 10);

    // Subscribe to command topic
    let cmd_topic = format!("miniclaw/{device_id}/command");
    let resp_topic = format!("miniclaw/{device_id}/response");
    let status_topic = format!("miniclaw/{device_id}/status");

    client
        .subscribe(&cmd_topic, QoS::AtLeastOnce)
        .await
        .map_err(|e| anyhow::anyhow!("MQTT subscribe failed: {e}"))?;

    // Publish online status (retained)
    let status = serde_json::to_vec(&DeviceStatus {
        status: "online".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        model: config.llm.model.clone(),
    })?;
    client
        .publish(&status_topic, QoS::AtLeastOnce, true, status)
        .await
        .map_err(|e| anyhow::anyhow!("MQTT status publish failed: {e}"))?;

    tracing::info!("MQTT connected to {broker}:{port}, subscribed to {cmd_topic}");

    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Incoming::Publish(msg))) => {
                let payload = String::from_utf8_lossy(&msg.payload);
                tracing::debug!("MQTT message on {}: {}", msg.topic, payload);

                // Parse message
                let mqtt_msg: MqttMessage = match serde_json::from_str(&payload) {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("Invalid MQTT message: {e}");
                        continue;
                    }
                };

                let session_id = if mqtt_msg.session_id.is_empty() {
                    device_id.to_string()
                } else {
                    mqtt_msg.session_id
                };

                let input = Input {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id,
                    content: mqtt_msg.content,
                };

                let (reply_tx, reply_rx) = oneshot::channel();

                if inbound_tx.send((input, reply_tx)).await.is_err() {
                    tracing::error!("Agent worker channel closed");
                    break;
                }

                // Wait for response and publish
                let client_clone = client.clone();
                let resp_topic_clone = resp_topic.clone();
                tokio::spawn(async move {
                    match tokio::time::timeout(Duration::from_secs(60), reply_rx).await {
                        Ok(Ok(output)) => {
                            let payload = serde_json::json!({
                                "response": output.content,
                            });
                            if let Err(e) = client_clone
                                .publish(
                                    &resp_topic_clone,
                                    QoS::AtLeastOnce,
                                    false,
                                    payload.to_string().as_bytes(),
                                )
                                .await
                            {
                                tracing::error!("MQTT publish failed: {e}");
                            }
                        }
                        Ok(Err(_)) => tracing::error!("Agent worker dropped MQTT request"),
                        Err(_) => tracing::error!("MQTT request timed out"),
                    }
                });
            }
            Ok(_) => {} // other events (connack, suback, etc.)
            Err(e) => {
                tracing::error!("MQTT connection error: {e}");
                // rumqttc auto-reconnects, just log
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    Ok(())
}
