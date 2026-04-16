//! Bootstrap: koneksi AMQP, spawn server task, bind HTTP.
//!
//! Semua logika aplikasi tinggal di modul-modul:
//! - `config`       — konstanta (nama queue, URL broker)
//! - `models`       — tipe domain yang diserialkan ke frontend
//! - `state`        — AppState bersama
//! - `util`         — helper kecil (timestamp, counter, error mapper)
//! - `procedures`   — logika bisnis prosedur RPC
//! - `broker::*`    — koneksi AMQP dan task server latar belakang
//! - `handlers::*`  — actix-web handlers + `configure()` router

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use actix_web::{web, App, HttpServer};
use lapin::{Connection, ConnectionProperties};

mod broker;
mod config;
mod handlers;
mod models;
mod procedures;
mod state;
mod util;

use crate::broker::{start_reqresp_server, start_rpc_server};
use crate::config::{DEFAULT_AMQP_URL, HTTP_BIND_ADDR};
use crate::state::AppState;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let amqp_url = std::env::var("AMQP_URL").unwrap_or_else(|_| DEFAULT_AMQP_URL.to_string());

    let conn_props = ConnectionProperties::default()
        .with_executor(tokio_executor_trait::Tokio::current())
        .with_reactor(tokio_reactor_trait::Tokio);

    let conn = match Connection::connect(&amqp_url, conn_props).await {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("==========================================");
            eprintln!("  ERROR: Tidak dapat terhubung ke RabbitMQ pada {}", amqp_url);
            eprintln!("  Detail: {}", e);
            eprintln!("  Jalankan 'docker compose up -d' dulu dari root project.");
            eprintln!("==========================================");
            return Err(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, e));
        }
    };

    let channel = conn
        .create_channel()
        .await
        .map(Arc::new)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    start_rpc_server(conn.clone())
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    start_reqresp_server(conn.clone())
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let state = web::Data::new(AppState {
        interaction_logs: Mutex::new(Vec::new()),
        pubsub_topics: Mutex::new(HashMap::new()),
        message_counter: Mutex::new(0),
        subscriber_tasks: Mutex::new(HashMap::new()),
        conn,
        channel,
    });

    println!("==========================================");
    println!("  Simulasi Model Komunikasi Terdistribusi");
    println!("  Broker: RabbitMQ @ {}", amqp_url);
    println!("  Buka http://{} di browser", HTTP_BIND_ADDR);
    println!("==========================================");

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .configure(handlers::configure)
    })
    .bind(HTTP_BIND_ADDR)?
    .run()
    .await
}
