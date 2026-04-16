//! HTTP layer: actix-web handlers + route registration.
//!
//! `configure(cfg)` didaftarkan di `App::configure(...)` agar main.rs tidak
//! perlu tahu path individual.

use actix_web::web;

mod meta;
mod pubsub;
mod reqresp;
mod rpc;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/", web::get().to(meta::index))
        .route(
            "/api/request-response",
            web::post().to(reqresp::simulate_request_response),
        )
        .route(
            "/api/pubsub/subscribe",
            web::post().to(pubsub::pubsub_subscribe),
        )
        .route(
            "/api/pubsub/publish",
            web::post().to(pubsub::pubsub_publish),
        )
        .route("/api/rpc", web::post().to(rpc::simulate_rpc))
        .route("/api/comparison", web::get().to(meta::get_comparison))
        .route("/api/logs", web::get().to(meta::get_logs))
        .route("/api/reset", web::post().to(meta::reset_state));
}
