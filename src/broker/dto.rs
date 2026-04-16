//! Tipe payload yang dipertukarkan antara klien dan server AMQP.
//!
//! Semua di-serialize ke JSON dan dikirim sebagai body pesan AMQP.

use serde::{Deserialize, Serialize};

/// Request RPC: nama prosedur + argumen.
#[derive(Serialize, Deserialize)]
pub struct RpcRequest {
    pub procedure: String,
    pub arguments: Vec<String>,
}

/// Reply RPC: hasil + timing server-side (unmarshal/execute/marshal) dalam ms.
/// Timing server dipakai klien untuk menghitung latensi jaringan nyata.
#[derive(Serialize, Deserialize)]
pub struct RpcReply {
    pub result: String,
    pub unmarshal_ms: f64,
    pub execute_ms: f64,
    pub marshal_ms: f64,
}

/// Reply untuk model request-response (echo server).
#[derive(Serialize, Deserialize)]
pub struct EchoReply {
    pub payload: String,
    pub server_time_ms: f64,
}
