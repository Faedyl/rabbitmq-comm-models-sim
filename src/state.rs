//! State bersama yang di-share antar handler actix-web via `web::Data<AppState>`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lapin::{Channel, Connection};

use crate::models::InteractionLog;

pub struct AppState {
    /// Riwayat semua simulasi yang pernah dijalankan (untuk `/api/logs` dan perbandingan).
    pub interaction_logs: Mutex<Vec<InteractionLog>>,

    /// Daftar subscriber per-topic (untuk ditampilkan di UI).
    pub pubsub_topics: Mutex<HashMap<String, Vec<String>>>,

    /// Counter global id pesan.
    pub message_counter: Mutex<usize>,

    /// Handle task consumer latar belakang, satu per (subscriber_id, topic).
    pub subscriber_tasks: Mutex<HashMap<(String, String), tokio::task::JoinHandle<()>>>,

    /// Koneksi AMQP persisten ke RabbitMQ (digunakan untuk membuat channel ad-hoc
    /// seperti reply queue per-request).
    pub conn: Arc<Connection>,

    /// Channel publisher bersama untuk handler HTTP.
    pub channel: Arc<Channel>,
}
