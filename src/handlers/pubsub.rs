//! Handler model Publish-Subscribe.
//!
//! Subscribe: declare fanout exchange `pubsub.{topic}`, declare queue
//! eksklusif, bind, spawn consumer task yang meng-ACK setiap delivery.
//! Publish: `basic_publish` ke exchange (fan-out nyata terjadi di broker).

use std::time::Instant;

use actix_web::{web, HttpResponse};
use futures_util::stream::StreamExt;
use lapin::{
    options::{
        BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, ExchangeDeclareOptions,
        QueueBindOptions, QueueDeclareOptions,
    },
    types::FieldTable,
    BasicProperties, ExchangeKind,
};
use serde::Deserialize;

use crate::config::PUBSUB_EXCHANGE_PREFIX;
use crate::models::{InteractionLog, Message};
use crate::state::AppState;
use crate::util::{amqp_error, next_id, now_ts};

#[derive(Deserialize)]
pub struct SubInput {
    pub subscriber_id: String,
    pub topic: String,
}

#[derive(Deserialize)]
pub struct PubInput {
    pub publisher_id: String,
    pub topic: String,
    pub message: String,
}

pub async fn pubsub_subscribe(
    data: web::Data<AppState>,
    input: web::Json<SubInput>,
) -> HttpResponse {
    let key = (input.subscriber_id.clone(), input.topic.clone());

    // Idempoten: jika subscriber sudah konsumsi topic ini, tidak apa-apa.
    if data.subscriber_tasks.lock().unwrap().contains_key(&key) {
        let count = data
            .pubsub_topics
            .lock()
            .unwrap()
            .get(&input.topic)
            .map(|v| v.len())
            .unwrap_or(0);
        return HttpResponse::Ok().json(serde_json::json!({
            "status": "already subscribed",
            "subscriber": input.subscriber_id,
            "topic": input.topic,
            "total_subscribers": count
        }));
    }

    let exchange_name = format!("{}{}", PUBSUB_EXCHANGE_PREFIX, input.topic);

    // Channel terpisah per subscriber agar siklus hidup consumer-nya independen.
    let channel = match data.conn.create_channel().await {
        Ok(c) => c,
        Err(e) => return amqp_error("subscriber channel", e),
    };

    if let Err(e) = channel
        .exchange_declare(
            &exchange_name,
            ExchangeKind::Fanout,
            ExchangeDeclareOptions {
                durable: false,
                auto_delete: false,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await
    {
        return amqp_error("declare exchange", e);
    }

    let queue = match channel
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
        Err(e) => return amqp_error("declare sub queue", e),
    };

    if let Err(e) = channel
        .queue_bind(
            &queue,
            &exchange_name,
            "",
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await
    {
        return amqp_error("bind sub queue", e);
    }

    let mut consumer = match channel
        .basic_consume(
            &queue,
            &format!("sub-{}", input.subscriber_id),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
    {
        Ok(c) => c,
        Err(e) => return amqp_error("consume sub queue", e),
    };

    // Catat subscriber untuk UI.
    {
        let mut topics = data.pubsub_topics.lock().unwrap();
        let subs = topics.entry(input.topic.clone()).or_default();
        if !subs.contains(&input.subscriber_id) {
            subs.push(input.subscriber_id.clone());
        }
    }

    // Task konsumer latar belakang: broker benar-benar fan-out ke queue ini —
    // task meng-ACK tiap delivery agar queue tidak menumpuk. Entri NOTIFY
    // yang terlihat di UI disintesis oleh handler publish di bawah.
    let handle = tokio::spawn(async move {
        let _keep_channel = channel;
        while let Some(delivery) = consumer.next().await {
            match delivery {
                Ok(d) => {
                    let _ = d.ack(BasicAckOptions::default()).await;
                }
                Err(_) => break,
            }
        }
    });

    data.subscriber_tasks.lock().unwrap().insert(key, handle);

    let count = data
        .pubsub_topics
        .lock()
        .unwrap()
        .get(&input.topic)
        .map(|v| v.len())
        .unwrap_or(0);
    HttpResponse::Ok().json(serde_json::json!({
        "status": "subscribed",
        "subscriber": input.subscriber_id,
        "topic": input.topic,
        "total_subscribers": count
    }))
}

pub async fn pubsub_publish(
    data: web::Data<AppState>,
    input: web::Json<PubInput>,
) -> HttpResponse {
    let start = Instant::now();
    let exchange_name = format!("{}{}", PUBSUB_EXCHANGE_PREFIX, input.topic);

    // Pastikan exchange ada walau belum ada subscriber. Idempoten.
    if let Err(e) = data
        .channel
        .exchange_declare(
            &exchange_name,
            ExchangeKind::Fanout,
            ExchangeDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
    {
        return amqp_error("declare exchange (publish)", e);
    }

    let publish_started = Instant::now();
    if let Err(e) = data
        .channel
        .basic_publish(
            &exchange_name,
            "",
            BasicPublishOptions::default(),
            input.message.as_bytes(),
            BasicProperties::default().with_app_id(input.publisher_id.clone().into()),
        )
        .await
    {
        return amqp_error("publish", e);
    }
    let publish_latency = publish_started.elapsed().as_secs_f64() * 1000.0;

    let mut messages = Vec::new();
    let mut message_order = Vec::new();

    messages.push(Message {
        id: next_id(&data.message_counter),
        from: input.publisher_id.clone(),
        to: format!("Topic:{}", input.topic),
        content: format!("PUBLISH: {}", input.message),
        msg_type: "publish".into(),
        timestamp: now_ts(),
        latency_ms: publish_latency,
    });
    message_order.push(format!(
        "{} -> [{}] PUBLISH",
        input.publisher_id, input.topic
    ));

    // Broker benar-benar fan-out ke setiap queue subscriber; di sini kita
    // sintesis entri NOTIFY per subscriber agar respons publish langsung memuat
    // diagram fan-out yang bisa dirender frontend tanpa polling /api/logs.
    let subscribers = data
        .pubsub_topics
        .lock()
        .unwrap()
        .get(&input.topic)
        .cloned()
        .unwrap_or_default();

    for sub in &subscribers {
        let notify_started = Instant::now();
        // Yield tipis untuk mengukur scheduling delivery tanpa blocking handler.
        tokio::task::yield_now().await;
        let notify_latency = notify_started.elapsed().as_secs_f64() * 1000.0;
        messages.push(Message {
            id: next_id(&data.message_counter),
            from: format!("Topic:{}", input.topic),
            to: sub.clone(),
            content: format!("NOTIFY: {}", input.message),
            msg_type: "notify".into(),
            timestamp: now_ts(),
            latency_ms: notify_latency,
        });
        message_order.push(format!("[{}] -> {} NOTIFY", input.topic, sub));
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
        model: "Publish-Subscribe".into(),
        messages,
        total_time_ms: total_time,
        throughput,
        message_order,
    };
    data.interaction_logs.lock().unwrap().push(log.clone());
    HttpResponse::Ok().json(log)
}
