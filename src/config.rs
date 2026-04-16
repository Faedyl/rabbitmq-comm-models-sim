//! Konstanta konfigurasi global: nama queue AMQP, prefix exchange, URL default.

/// Queue tetap tempat RPC server mengonsumsi request.
pub const RPC_REQUEST_QUEUE: &str = "rpc.requests";

/// Queue tetap tempat Echo (request-response) server mengonsumsi request.
pub const REQRESP_REQUEST_QUEUE: &str = "reqresp.requests";

/// Prefix nama fanout exchange untuk model publish-subscribe.
/// Exchange per-topic dinamai `pubsub.{topic}`.
pub const PUBSUB_EXCHANGE_PREFIX: &str = "pubsub.";

/// URL broker AMQP default bila env var `AMQP_URL` tidak di-set.
pub const DEFAULT_AMQP_URL: &str = "amqp://guest:guest@localhost:5672/%2f";

/// Alamat bind HTTP server.
pub const HTTP_BIND_ADDR: &str = "127.0.0.1:8080";

/// Timeout menunggu balasan dari server AMQP (detik).
pub const REPLY_TIMEOUT_SECS: u64 = 5;
