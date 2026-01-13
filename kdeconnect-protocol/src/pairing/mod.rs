//! KDE Connect Device Pairing
//!
//! This module implements TLS-based secure pairing between devices.
//!
//! ## Pairing Protocol
//!
//! 1. **Discovery**: Devices discover each other via UDP broadcast
//! 2. **TCP Connection**: Device A connects to Device B's TCP port
//! 3. **Pairing Request**: Device A sends `kdeconnect.pair` with `pair: true`
//! 4. **User Verification**: Users verify SHA256 fingerprints on both devices
//! 5. **Pairing Response**: Device B responds with `pair: true` (accept) or `pair: false` (reject)
//! 6. **Certificate Exchange**: Devices exchange and store certificates
//! 7. **TLS Connection**: Future connections use TLS with stored certificates
//!
//! ## Usage
//!
//! ```no_run
//! use kdeconnect_protocol::pairing::{PairingService, PairingEvent};
//!
//! #[tokio::main]
//! async fn main() {
//!     // Service will be created in daemon
//!     // Events will notify UI of pairing requests
//! }
//! ```

pub mod events;
pub mod handler;
pub mod service;

// Re-export main types
pub use events::PairingEvent;
pub use handler::{CertificateInfo, PairingHandler, PairingPacket, PairingStatus, PAIRING_TIMEOUT};
pub use service::{PairingConfig, PairingService};
