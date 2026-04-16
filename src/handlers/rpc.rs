//! Handler model RPC.
//!
//! Pola reply-to AMQP: publish ke `rpc.requests` dengan correlation_id dan
//! reply_to berupa queue eksklusif, lalu tunggu balasan. Server mengirim
//! timing unmarshal/execute/marshal nyata; latensi jaringan dihitung dari
//! `RTT - server_total`.

use std::time::{Duration, Instant};

use actix_web::{web, HttpResponse};
use futures_util::stream::StreamExt;
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties,
};
use serde::Deserialize;

use crate::broker::dto::{RpcReply, RpcRequest};
use crate::config::{REPLY_TIMEOUT_SECS, RPC_REQUEST_QUEUE};
use crate::models::{InteractionLog, Message};
use crate::state::AppState;
use crate::util::{amqp_error, next_id, now_ts};

#[derive(Deserialize)]
pub struct RpcInput {
    pub caller_id: String,
    pub callee_id: String,
    pub procedure: String,
    pub arguments: Vec<String>,
}

pub async fn simulate_rpc(
    data: web::Data<AppState>,
    input: web::Json<RpcInput>,
) -> HttpResponse {
    let start = Instant::now();
    let mut messages = Vec::new();
    let mut message_order = Vec::new();

    // Reply channel + queue eksklusif untuk panggilan ini.
    let reply_channel = match data.conn.create_channel().await {
        Ok(c) => c,
        Err(e) => return amqp_error("rpc reply channel", e),
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
        Err(e) => return amqp_error("rpc reply queue", e),
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
        Err(e) => return amqp_error("consume rpc reply", e),
    };

    // Langkah 1: Marshalling di client stub (diukur lokal).
    let marshal_started = Instant::now();
    let request_body = RpcRequest {
        procedure: input.procedure.clone(),
        arguments: input.arguments.clone(),
    };
    let request_bytes = match serde_json::to_vec(&request_body) {
        Ok(b) => b,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": format!("marshal: {}", e) }))
        }
    };
    let client_marshal_ms = marshal_started.elapsed().as_secs_f64() * 1000.0;
    messages.push(Message {
        id: next_id(&data.message_counter),
        from: input.caller_id.clone(),
        to: format!("{}_stub", input.caller_id),
        content: format!(
            "MARSHAL: {}({})",
            input.procedure,
            input.arguments.join(", ")
        ),
        msg_type: "marshal".into(),
        timestamp: now_ts(),
        latency_ms: client_marshal_ms,
    });
    message_order.push(format!("{} -> ClientStub [MARSHAL]", input.caller_id));

    // Langkah 2: Network send (bagian dari RTT).
    let correlation_id = uuid::Uuid::new_v4().to_string();
    let rtt_started = Instant::now();
    if let Err(e) = data
        .channel
        .basic_publish(
            "",
            RPC_REQUEST_QUEUE,
            BasicPublishOptions::default(),
            &request_bytes,
            BasicProperties::default()
                .with_reply_to(reply_queue.clone().into())
                .with_correlation_id(correlation_id.clone().into()),
        )
        .await
    {
        return amqp_error("publish rpc", e);
    }

    // Tunggu balasan dengan correlation_id yang cocok.
    let reply_bytes = match tokio::time::timeout(
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
        Ok(Ok(b)) => b,
        Ok(Err(e)) => return amqp_error("receive rpc reply", e),
        Err(_) => {
            return HttpResponse::GatewayTimeout()
                .json(serde_json::json!({ "error": "timeout waiting for rpc server" }))
        }
    };

    let rtt_ms = rtt_started.elapsed().as_secs_f64() * 1000.0;
    let reply: RpcReply = match serde_json::from_slice(&reply_bytes) {
        Ok(r) => r,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": format!("unmarshal reply: {}", e) }))
        }
    };
    let server_total = reply.unmarshal_ms + reply.execute_ms + reply.marshal_ms;
    let net_total = (rtt_ms - server_total).max(0.0);
    let half_net = net_total / 2.0;

    // Langkah 2 message: network send leg.
    messages.push(Message {
        id: next_id(&data.message_counter),
        from: format!("{}_stub", input.caller_id),
        to: format!("{}_stub", input.callee_id),
        content: format!("NETWORK: Serialized call to {}", input.procedure),
        msg_type: "network_send".into(),
        timestamp: now_ts(),
        latency_ms: half_net,
    });
    message_order.push("ClientStub -> Network -> ServerStub [SEND]".into());

    // Langkah 3: Server stub unmarshalling.
    messages.push(Message {
        id: next_id(&data.message_counter),
        from: format!("{}_stub", input.callee_id),
        to: input.callee_id.clone(),
        content: format!(
            "UNMARSHAL: {}({})",
            input.procedure,
            input.arguments.join(", ")
        ),
        msg_type: "unmarshal".into(),
        timestamp: now_ts(),
        latency_ms: reply.unmarshal_ms,
    });
    message_order.push(format!("ServerStub -> {} [UNMARSHAL]", input.callee_id));

    // Langkah 4: Eksekusi prosedur (di task server).
    messages.push(Message {
        id: next_id(&data.message_counter),
        from: input.callee_id.clone(),
        to: input.callee_id.clone(),
        content: format!("EXECUTE: {} -> Result: {}", input.procedure, reply.result),
        msg_type: "execute".into(),
        timestamp: now_ts(),
        latency_ms: reply.execute_ms,
    });
    message_order.push(format!(
        "{} [EXECUTE {}]",
        input.callee_id, input.procedure
    ));

    // Langkah 5: Network return leg.
    messages.push(Message {
        id: next_id(&data.message_counter),
        from: format!("{}_stub", input.callee_id),
        to: format!("{}_stub", input.caller_id),
        content: format!("RETURN: {}", reply.result),
        msg_type: "network_return".into(),
        timestamp: now_ts(),
        latency_ms: half_net,
    });
    message_order.push("ServerStub -> Network -> ClientStub [RETURN]".into());

    // Langkah 6: Client unmarshal.
    let client_unmarshal_started = Instant::now();
    let final_result = reply.result.clone();
    let client_unmarshal_ms = client_unmarshal_started.elapsed().as_secs_f64() * 1000.0;
    messages.push(Message {
        id: next_id(&data.message_counter),
        from: format!("{}_stub", input.caller_id),
        to: input.caller_id.clone(),
        content: format!("RESULT: {}", final_result),
        msg_type: "result".into(),
        timestamp: now_ts(),
        latency_ms: client_unmarshal_ms,
    });
    message_order.push(format!("ClientStub -> {} [RESULT]", input.caller_id));

    let total_latency: f64 = messages.iter().map(|m| m.latency_ms).sum();
    let real_time = start.elapsed().as_secs_f64() * 1000.0;
    let total_time = real_time.max(total_latency);
    let throughput = if total_time > 0.0 {
        (messages.len() as f64) / (total_time / 1000.0)
    } else {
        0.0
    };

    let log = InteractionLog {
        model: "Remote Procedure Call".into(),
        messages,
        total_time_ms: total_time,
        throughput,
        message_order,
    };
    data.interaction_logs.lock().unwrap().push(log.clone());
    HttpResponse::Ok().json(log)
}
