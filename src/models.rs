//! Tipe domain yang di-serialize ke frontend.
//!
//! `Message` merepresentasikan satu pesan dalam diagram interaksi.
//! `InteractionLog` merangkum satu kali simulasi (berisi banyak Message).
//! `ComparisonMetrics` dipakai oleh endpoint perbandingan.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: usize,
    pub from: String,
    pub to: String,
    pub content: String,
    pub msg_type: String,
    pub timestamp: String,
    pub latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionLog {
    pub model: String,
    pub messages: Vec<Message>,
    pub total_time_ms: f64,
    pub throughput: f64,
    pub message_order: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonMetrics {
    pub model: String,
    pub avg_latency_ms: f64,
    pub total_messages: usize,
    pub throughput: f64,
    pub ordering_guarantee: String,
    pub coupling: String,
    pub scalability: String,
}
