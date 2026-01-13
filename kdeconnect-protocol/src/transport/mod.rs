//! KDE Connect Transport Layer
//!
//! This module provides network transport for KDE Connect protocol.
//! Currently implements basic TCP for pairing; TLS will be added in Issue #31.

pub mod tcp;

pub use tcp::TcpConnection;
