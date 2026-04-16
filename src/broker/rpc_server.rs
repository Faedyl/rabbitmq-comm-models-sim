//! Server RPC AMQP: konsumsi dari `rpc.requests`, eksekusi prosedur, balas
//! ke reply queue klien dengan `correlation_id` yang sama.

use std::sync::Arc;
use std::time::Instant;

use futures_util::stream::StreamExt;
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties, Connection,
};

use crate::broker::dto::{RpcReply, RpcRequest};
use crate::config::RPC_REQUEST_QUEUE;
use crate::procedures::execute_procedure;

pub async fn start_rpc_server(conn: Arc<Connection>) -> Result<(), lapin::Error> {
    let channel = conn.create_channel().await?;
    channel
        .queue_declare(
            RPC_REQUEST_QUEUE,
            QueueDeclareOptions {
                durable: false,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;
    let mut consumer = channel
        .basic_consume(
            RPC_REQUEST_QUEUE,
            "rpc_server",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    tokio::spawn(async move {
        // Jaga channel tetap hidup selama task konsumer berjalan.
        let _keep_channel = channel.clone();
        while let Some(delivery) = consumer.next().await {
            let delivery = match delivery {
                Ok(d) => d,
                Err(_) => continue,
            };

            let unmarshal_start = Instant::now();
            let req: RpcRequest = match serde_json::from_slice(&delivery.data) {
                Ok(r) => r,
                Err(_) => {
                    let _ = delivery.ack(BasicAckOptions::default()).await;
                    continue;
                }
            };
            let unmarshal_ms = unmarshal_start.elapsed().as_secs_f64() * 1000.0;

            let execute_start = Instant::now();
            let result = execute_procedure(&req.procedure, &req.arguments);
            let execute_ms = execute_start.elapsed().as_secs_f64() * 1000.0;

            let marshal_start = Instant::now();
            let reply = RpcReply {
                result,
                unmarshal_ms,
                execute_ms,
                marshal_ms: 0.0,
            };
            let mut reply_bytes = match serde_json::to_vec(&reply) {
                Ok(b) => b,
                Err(_) => {
                    let _ = delivery.ack(BasicAckOptions::default()).await;
                    continue;
                }
            };
            // Patch marshal_ms ke body yang sudah ter-serialize (karena kita perlu
            // mengukur waktu setelah serde_json::to_vec selesai).
            let marshal_ms = marshal_start.elapsed().as_secs_f64() * 1000.0;
            if let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(&reply_bytes) {
                v["marshal_ms"] = serde_json::json!(marshal_ms);
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
