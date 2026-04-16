//! Handler utilitas: perbandingan metrik, dump log, reset state, dan
//! serving frontend HTML.

use std::collections::HashMap;

use actix_web::{web, HttpResponse};

use crate::models::{ComparisonMetrics, InteractionLog};
use crate::state::AppState;

pub async fn get_comparison(data: web::Data<AppState>) -> HttpResponse {
    let logs = data.interaction_logs.lock().unwrap();
    let mut grouped: HashMap<String, Vec<&InteractionLog>> = HashMap::new();
    for log in logs.iter() {
        grouped.entry(log.model.clone()).or_default().push(log);
    }

    let mut comparisons = Vec::new();
    for (model, model_logs) in &grouped {
        let total_msgs: usize = model_logs.iter().map(|l| l.messages.len()).sum();
        let latencies: Vec<f64> = model_logs
            .iter()
            .flat_map(|l| l.messages.iter().map(|m| m.latency_ms))
            .collect();
        let avg_latency = if !latencies.is_empty() {
            latencies.iter().sum::<f64>() / latencies.len() as f64
        } else {
            0.0
        };
        let avg_throughput = if !model_logs.is_empty() {
            model_logs.iter().map(|l| l.throughput).sum::<f64>() / model_logs.len() as f64
        } else {
            0.0
        };

        let (ordering, coupling, scalability) = match model.as_str() {
            "Request-Response" => ("Ketat (sinkron)", "Tight coupling", "Terbatas"),
            "Publish-Subscribe" => (
                "Tidak dijamin (asinkron)",
                "Loose coupling",
                "Sangat baik",
            ),
            "Remote Procedure Call" => (
                "Ketat (sinkron, seperti fungsi lokal)",
                "Tight coupling (perlu interface)",
                "Sedang",
            ),
            _ => ("N/A", "N/A", "N/A"),
        };

        comparisons.push(ComparisonMetrics {
            model: model.clone(),
            avg_latency_ms: (avg_latency * 100.0).round() / 100.0,
            total_messages: total_msgs,
            throughput: (avg_throughput * 100.0).round() / 100.0,
            ordering_guarantee: ordering.into(),
            coupling: coupling.into(),
            scalability: scalability.into(),
        });
    }
    HttpResponse::Ok().json(comparisons)
}

pub async fn get_logs(data: web::Data<AppState>) -> HttpResponse {
    let logs = data.interaction_logs.lock().unwrap();
    HttpResponse::Ok().json(logs.clone())
}

pub async fn reset_state(data: web::Data<AppState>) -> HttpResponse {
    data.interaction_logs.lock().unwrap().clear();
    data.pubsub_topics.lock().unwrap().clear();
    *data.message_counter.lock().unwrap() = 0;
    // Abort semua task subscriber; queue eksklusifnya auto-delete ketika
    // channel ditutup saat task berakhir.
    let mut tasks = data.subscriber_tasks.lock().unwrap();
    for (_, handle) in tasks.drain() {
        handle.abort();
    }
    HttpResponse::Ok().json(serde_json::json!({"status": "reset"}))
}

pub async fn index() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(include_str!("../../static/index.html"))
}
