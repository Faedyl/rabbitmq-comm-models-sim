//! Handler model Request-Response.
//!
//! Alur: publish ke queue `reqresp.requests` dengan `reply_to` berupa queue
//! eksklusif baru, lalu tunggu balasan di queue tersebut. Latensi diukur nyata.

use std::time::{Duration, Instant};

use actix_web::{web, HttpResponse};
use futures_util::stream::StreamExt;
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties,
};
use serde::Deserialize;

use crate::broker::dto::EchoReply;
use crate::config::{REPLY_TIMEOUT_SECS, REQRESP_REQUEST_QUEUE};
use crate::models::{InteractionLog, Message};
use crate::state::AppState;
use crate::util::{amqp_error, next_id, now_ts};

#[derive(Deserialize)]
pub struct ReqResInput {
    pub client_id: String,
    pub server_id: String,
    pub request_data: String,
    pub num_requests: Option<usize>,
}

pub async fn simulate_request_response(
    data: web::Data<AppState>,
    input: web::Json<ReqResInput>,
) -> HttpResponse {
    let start = Instant::now();
    let num_requests = input.num_requests.unwrap_or(1);
    let mut messages = Vec::new();
    let mut message_order = Vec::new();

    // Dedicated reply queue untuk run ini. Exclusive + auto-delete agar hilang
    // ketika channel ditutup.
    let reply_channel = match data.conn.create_channel().await {
        Ok(c) => c,
        Err(e) => return amqp_error("create reply channel", e),
    };
    let reply_queue = match reply_channel
        .queue_declare(
            "",
            QueueDeclareOptions {
                exclusive: true,
                auto_delete: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await
    {
        Ok(q) => q.name().to_string(),
        Err(e) => return amqp_error("declare reply queue", e),
    };
    let mut reply_consumer = match reply_channel
        .basic_consume(
            &reply_queue,
            "",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
    {
        Ok(c) => c,
        Err(e) => return amqp_error("consume reply queue", e),
    };

    for i in 0..num_requests {
        let correlation_id = uuid::Uuid::new_v4().to_string();
        let payload = format!("REQUEST #{}: {}", i + 1, input.request_data);

        let req_sent_at = Instant::now();
        if let Err(e) = data
            .channel
            .basic_publish(
                "",
                REQRESP_REQUEST_QUEUE,
                BasicPublishOptions::default(),
                payload.as_bytes(),
                BasicProperties::default()
                    .with_reply_to(reply_queue.clone().into())
                    .with_correlation_id(correlation_id.clone().into()),
            )
            .await
        {
            return amqp_error("publish request", e);
        }

        messages.push(Message {
            id: next_id(&data.message_counter),
            from: input.client_id.clone(),
            to: input.server_id.clone(),
            content: payload,
            msg_type: "request".into(),
            timestamp: now_ts(),
            latency_ms: 0.0, // diisi setelah reply diterima (separuh network RTT)
        });
        let req_idx = messages.len() - 1;
        message_order.push(format!(
            "{} -> {} [REQ #{}]",
            input.client_id,
            input.server_id,
            i + 1
        ));

        // Tunggu balasan dengan correlation_id yang cocok, timeout konfigurasi.
        let reply = match tokio::time::timeout(
            Duration::from_secs(REPLY_TIMEOUT_SECS),
            async {
                while let Some(delivery) = reply_consumer.next().await {
                    match delivery {
                        Ok(d) => {
                            let matches = d
                                .properties
                                .correlation_id()
                                .as_ref()
                                .map(|c| c.as_str() == correlation_id)
                                .unwrap_or(false);
                            let _ = d.ack(BasicAckOptions::default()).await;
                            if matches {
                                return Ok::<_, lapin::Error>(d.data);
                            }
                        }
                        Err(e) => return Err(e),
                    }
                }
                Err(lapin::Error::InvalidChannelState(lapin::ChannelState::Closed))
            },
        )
        .await
        {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(e)) => return amqp_error("receive reply", e),
            Err(_) => {
                return HttpResponse::GatewayTimeout().json(serde_json::json!({
                    "error": "timeout waiting for response from reqresp server"
                }))
            }
        };

        let rtt_ms = req_sent_at.elapsed().as_secs_f64() * 1000.0;
        let reply_body: EchoReply = serde_json::from_slice(&reply).unwrap_or(EchoReply {
            payload: String::from_utf8_lossy(&reply).to_string(),
            server_time_ms: 0.0,
        });
        let net_rtt = (rtt_ms - reply_body.server_time_ms).max(0.0);
        let half_net = net_rtt / 2.0;

        // Isi latensi request leg (client -> server).
        messages[req_idx].latency_ms = half_net;

        // Response: server -> client
        messages.push(Message {
            id: next_id(&data.message_counter),
            from: input.server_id.clone(),
            to: input.client_id.clone(),
            content: format!("RESPONSE #{}: {}", i + 1, reply_body.payload),
            msg_type: "response".into(),
            timestamp: now_ts(),
            latency_ms: half_net + reply_body.server_time_ms,
        });
        message_order.push(format!(
            "{} -> {} [RES #{}]",
            input.server_id,
            input.client_id,
            i + 1
        ));
    }

    let total_latency: f64 = messages.iter().map(|m| m.latency_ms).sum();
    let real_time = start.elapsed().as_secs_f64() * 1000.0;
    let total_time = real_time.max(total_latency);
    let throughput = if total_time > 0.0 {
        (messages.len() as f64) / (total_time / 1000.0)
    } else {
        0.0
    };

    let log = InteractionLog {
        model: "Request-Response".into(),
        messages,
        total_time_ms: total_time,
        throughput,
        message_order,
    };
    data.interaction_logs.lock().unwrap().push(log.clone());
    HttpResponse::Ok().json(log)
}
