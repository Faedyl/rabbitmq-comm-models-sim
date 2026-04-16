# Tugas 2 — Simulasi Model Komunikasi Terdistribusi (RabbitMQ)

Aplikasi web Rust yang mendemonstrasikan tiga model komunikasi terdistribusi — **Request-Response**, **Publish-Subscribe**, dan **Remote Procedure Call (RPC)** — menggunakan **RabbitMQ** sebagai message broker nyata (bukan simulasi in-memory).

---

## Struktur Proyek

```
Tugas 2/
├── docker-compose.yml            ← RabbitMQ 3 + Management UI
├── main.tex / main.pdf           ← Laporan akademik
├── README.md
└── simulation/
    ├── Cargo.toml
    ├── static/index.html         ← Frontend (Canvas + JS + preset skenario)
    └── src/
        ├── main.rs               ← Bootstrap: connect AMQP, spawn server, bind HTTP
        ├── config.rs             ← Konstanta (nama queue, URL broker, timeout)
        ├── models.rs             ← Domain types (Message, InteractionLog, dst)
        ├── state.rs              ← AppState bersama
        ├── util.rs               ← Helper (now_ts, next_id, amqp_error)
        ├── procedures.rs         ← Logika bisnis prosedur RPC
        ├── broker/               ← Layer infrastruktur AMQP
        │   ├── mod.rs
        │   ├── dto.rs            ← Wire types (RpcRequest, RpcReply, EchoReply)
        │   ├── rpc_server.rs     ← Task konsumer RPC
        │   └── reqresp_server.rs ← Task konsumer echo/request-response
        └── handlers/             ← Layer HTTP (actix-web)
            ├── mod.rs            ← Router (fn configure)
            ├── reqresp.rs        ← POST /api/request-response
            ├── pubsub.rs         ← POST /api/pubsub/{subscribe,publish}
            ├── rpc.rs            ← POST /api/rpc
            └── meta.rs           ← GET /api/comparison, /api/logs, /, POST /api/reset
```

### Pembagian Layer

| Layer | Lokasi | Tanggung jawab |
|---|---|---|
| **Presentation (HTTP)** | `handlers/` | Parsing JSON, validasi input, mapping ke domain, serialisasi respons |
| **Domain / Business** | `procedures.rs`, `models.rs` | Logika prosedur RPC + tipe yang terlihat di frontend |
| **Infrastructure (AMQP)** | `broker/` | Koneksi, channel, server task, wire DTOs |
| **Shared state** | `state.rs` | Struct `AppState` (koneksi, log, subscriber tasks) |
| **Cross-cutting** | `util.rs`, `config.rs` | Helper dan konstanta lintas layer |
| **Composition root** | `main.rs` | Bootstrap dan wiring — 86 baris saja |

---

## Cara Menjalankan

### 1. Jalankan RabbitMQ

```bash
docker compose up -d
```

Management UI: <http://localhost:15672> (user: `guest`, pass: `guest`)

### 2. Jalankan aplikasi

```bash
cd simulation
cargo run
```

Buka <http://127.0.0.1:8080> di browser.

Untuk mengganti URL broker:

```bash
AMQP_URL=amqp://user:pass@host:5672/%2f cargo run
```

---

## Arsitektur AMQP

| Model | Mekanisme RabbitMQ |
|---|---|
| **Request-Response** | Queue `reqresp.requests` + *exclusive reply queue* per request, dikorelasikan via `correlation_id`. |
| **Publish-Subscribe** | *Fanout exchange* `pubsub.{topic}` + queue eksklusif per subscriber. Tiap subscriber punya tokio task konsumer yang berjalan di latar belakang. |
| **RPC** | Queue `rpc.requests` dengan pola `reply_to` + `correlation_id`. Server mengembalikan hasil beserta `unmarshal_ms`, `execute_ms`, dan `marshal_ms` nyata. |

Saat boot, backend membuka koneksi AMQP dan men-spawn dua task server (RPC server & Echo server) yang berjalan selama aplikasi hidup.

---

## Endpoint HTTP

| Method | Path | Deskripsi |
|---|---|---|
| `GET` | `/` | Frontend HTML |
| `POST` | `/api/request-response` | Kirim request-reply via AMQP |
| `POST` | `/api/pubsub/subscribe` | Daftar subscriber + spawn consumer task |
| `POST` | `/api/pubsub/publish` | Publish ke fanout exchange |
| `POST` | `/api/rpc` | Panggil prosedur jarak jauh via AMQP |
| `GET` | `/api/comparison` | Metrik perbandingan ketiga model |
| `GET` | `/api/logs` | Riwayat semua interaksi |
| `POST` | `/api/reset` | Bersihkan state + abort consumer task |

Prosedur RPC yang tersedia:

- **Generik**: `add`, `multiply`, `concat`, `uppercase`, `length`
- **E-commerce**: `validate_card` (Luhn check), `check_inventory`, `calculate_shipping`
- **IoT**: `activate_alarm`, `read_sensor`

---

## Preset Skenario Dunia Nyata

Setiap tab punya tombol preset kuning di atas form untuk mengisi field otomatis:

| Tab | Preset | Isi |
|---|---|---|
| Request-Response | E-commerce: Browse Products | `Browser-App → Catalog-API` query katalog |
| Request-Response | IoT: Dashboard Query | `Dashboard → Gateway-IoT` snapshot sensor |
| Publish-Subscribe | E-commerce: Order Events | `checkout-service` fan-out ke inventory + email + analytics |
| Publish-Subscribe | IoT: Sensor Broadcast | `temp-sensor` fan-out ke dashboard + alarm + db-logger |
| RPC | Validate Card | Luhn check kartu kredit |
| RPC | Check Inventory | Cek stok produk |
| RPC | Calculate Shipping | Hitung ongkir |
| RPC | Activate Alarm | Trigger alarm zona |
| RPC | Read Sensor | Baca nilai sensor |

Tombol **+ Tambahkan Paket Subscriber Skenario** di tab Pub/Sub mendaftarkan ketiga subscriber skenario sekaligus, sehingga efek fan-out broker terlihat instan.

---

## Konsep Utama

| Konsep | Penjelasan |
|---|---|
| **Exchange (fanout)** | Broker menggandakan pesan ke semua queue yang ter-bind — fan-out terjadi di sisi broker, bukan aplikasi. |
| **Reply-to + correlation_id** | Pola AMQP untuk RPC: klien membuat queue balasan sementara, server membalas ke queue tersebut, pesan dipasangkan lewat `correlation_id`. |
| **Exclusive queue** | Queue yang hanya milik satu koneksi dan otomatis terhapus saat koneksi/channel ditutup — cocok untuk reply queue per-request dan queue subscriber. |
| **Latency nyata** | Round-trip time diukur langsung dari `Instant::now()`; latency per-langkah RPC berasal dari pengukuran server yang disisipkan dalam reply payload. |
| **Background consumer task** | Setiap subscriber punya `tokio::spawn` sendiri yang meng-ack pesan dan menulis ke log — publish/subscribe benar-benar asinkron. |

---

## Dependency Utama

- [`actix-web`](https://actix.rs/) — web framework async
- [`lapin`](https://crates.io/crates/lapin) — klien AMQP 0-9-1 untuk Rust
- [`tokio-executor-trait`] / [`tokio-reactor-trait`] — bridge agar lapin jalan di runtime tokio
- [`uuid`] — pembangkit `correlation_id`
