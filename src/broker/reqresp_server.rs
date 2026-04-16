//! Echo server AMQP untuk model request-response: konsumsi dari
//! `reqresp.requests`, balas ke reply queue klien dengan timing server.

use std::sync::Arc;
use std::time::Instant;

use futures_util::stream::StreamExt;
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties, Connection,
};

use crate::broker::dto::EchoReply;
use crate::config::REQRESP_REQUEST_QUEUE;

pub async fn start_reqresp_server(conn: Arc<Connection>) -> Result<(), lapin::Error> {
    let channel = conn.create_channel().await?;
    channel
        .queue_declare(
            REQRESP_REQUEST_QUEUE,
            QueueDeclareOptions {
                durable: false,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;
    let mut consumer = channel
        .basic_consume(
            REQRESP_REQUEST_QUEUE,
            "reqresp_server",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    tokio::spawn(async move {
        let _keep_channel = channel.clone();
        while let Some(delivery) = consumer.next().await {
            let delivery = match delivery {
                Ok(d) => d,
                Err(_) => continue,
            };

            let server_start = Instant::now();
            let payload = String::from_utf8_lossy(&delivery.data).to_string();
            let reply = EchoReply {
                payload: format!("Processed '{}'", payload),
                server_time_ms: 0.0,
            };
            let mut reply_bytes = match serde_json::to_vec(&reply) {
                Ok(b) => b,
                Err(_) => {
                    let _ = delivery.ack(BasicAckOptions::default()).await;
                    continue;
                }
            };
            let server_time_ms = server_start.elapsed().as_secs_f64() * 1000.0;
            if let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(&reply_bytes) {
                v["server_time_ms"] = serde_json::json!(server_time_ms);
                if let Ok(b) = serde_json::to_vec(&v) {
                    reply_bytes = b;
                }
            }

            if let (Some(reply_to), Some(correlation_id)) = (
                delivery.properties.reply_to().as_ref(),
                delivery.properties.correlation_id().as_ref(),
            ) {
                let _ = channel
                    .basic_publish(
                        "",
                        reply_to.as_str(),
                        BasicPublishOptions::default(),
                        &reply_bytes,
                        BasicProperties::default().with_correlation_id(correlation_id.clone()),
                    )
                    .await;
            }
            let _ = delivery.ack(BasicAckOptions::default()).await;
        }
    });
    Ok(())
}
