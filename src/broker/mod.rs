//! Layer infrastruktur: koneksi AMQP dan task server latar belakang.
//!
//! - `dto` berisi tipe wire yang di-serialize lintas AMQP (request-reply payload).
//! - `rpc_server` adalah consumer RPC yang menjalankan prosedur dan membalas via reply-to.
//! - `reqresp_server` adalah echo server untuk model request-response.

pub mod dto;
mod reqresp_server;
mod rpc_server;

pub use reqresp_server::start_reqresp_server;
pub use rpc_server::start_rpc_server;
