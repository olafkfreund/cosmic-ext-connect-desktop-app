//! KDE Connect Transport Layer
//!
//! This module provides network transport for KDE Connect protocol.
//! TLS implementation moved to cosmic-connect-core (rustls-based).

pub mod tcp;

// TLS modules removed - now using cosmic-connect-core
// Old OpenSSL-based TLS moved to cosmic-connect-core with rustls
// pub mod tls;
// pub mod tls_config;

pub use tcp::TcpConnection;

// TLS types now re-exported from cosmic-connect-core in lib.rs
// pub use cosmic_connect_core::crypto::{TlsConnection, TlsServer, TlsConfig};
