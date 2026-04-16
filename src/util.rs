//! Helper kecil yang dipakai lintas layer.

use std::sync::Mutex;

use actix_web::HttpResponse;
use chrono::Utc;

/// Timestamp HH:MM:SS.mmm untuk tampilan log.
pub fn now_ts() -> String {
    Utc::now().format("%H:%M:%S%.3f").to_string()
}

/// Ambil id pesan berikutnya (thread-safe, monotonic).
pub fn next_id(counter: &Mutex<usize>) -> usize {
    let mut c = counter.lock().unwrap();
    *c += 1;
    *c
}

/// Konversi `lapin::Error` menjadi HttpResponse 500 dengan konteks.
pub fn amqp_error(ctx: &str, e: lapin::Error) -> HttpResponse {
    HttpResponse::InternalServerError().json(serde_json::json!({
        "error": format!("{}: {}", ctx, e),
    }))
}
