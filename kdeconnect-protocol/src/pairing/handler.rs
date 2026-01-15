//! KDE Connect Device Pairing
//!
//! This module implements TLS-based secure pairing between devices.
//! Devices must be paired before exchanging any functional packets.
//!
//! ## Pairing Protocol
//!
//! 1. **Certificate Generation**: Each device generates a self-signed certificate
//! 2. **Pairing Request**: Device A sends `kdeconnect.pair` with `pair: true`
//! 3. **User Verification**: Users verify SHA256 fingerprints on both devices
//! 4. **Pairing Response**: Device B responds with `pair: true` (accept) or `pair: false` (reject)
//! 5. **Certificate Storage**: Accepted certificates are stored for future connections
//!
//! ## Certificate Requirements
//!
//! - **Algorithm**: RSA 2048-bit
//! - **Organization (O)**: "KDE"
//! - **Organizational Unit (OU)**: "Kde connect"
//! - **Common Name (CN)**: Device UUID
//! - **Validity**: 10 years
//! - **Serial Number**: 10
//!
//! ## Security
//!
//! - Self-signed certificates exchanged on first pairing
//! - SHA256 fingerprint verification prevents MITM attacks
//! - Certificates stored and verified on subsequent connections
//! - Pairing timeout: 30 seconds
//!
//! ## References
//! - [Valent Protocol Reference](https://valent.andyholmes.ca/documentation/protocol.html)
//! - [KDE Connect TLS Implementation](https://invent.kde.org/network/kdeconnect-kde)

use crate::{Packet, ProtocolError, Result};
use openssl::asn1::Asn1Time;
use openssl::bn::{BigNum, MsbOption};
use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::rsa::Rsa;
use openssl::x509::extension::{BasicConstraints, KeyUsage};
use openssl::x509::{X509, X509Name};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, info, warn};

/// Default pairing timeout (30 seconds per protocol specification)
pub const PAIRING_TIMEOUT: Duration = Duration::from_secs(30);

/// Certificate validity period (10 years)
const CERT_VALIDITY_YEARS: i64 = 10;

/// Organization name in certificate
const CERT_ORG: &str = "KDE";

/// Organizational unit in certificate
const CERT_ORG_UNIT: &str = "Kde connect";

/// Pairing status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PairingStatus {
    /// Not paired
    Unpaired,
    /// Pairing request sent, awaiting response
    Requested,
    /// Pairing request received, awaiting user confirmation
    #[serde(rename = "requested_by_peer")]
    RequestedByPeer,
    /// Successfully paired
    Paired,
}

/// Device certificate information
#[derive(Debug, Clone)]
pub struct CertificateInfo {
    /// Device ID (UUID)
    pub device_id: String,

    /// DER-encoded certificate
    pub certificate: Vec<u8>,

    /// DER-encoded private key
    pub private_key: Vec<u8>,

    /// SHA256 fingerprint of certificate (for verification)
    pub fingerprint: String,
}

impl CertificateInfo {
    /// Generate a new self-signed certificate for a device
    ///
    /// # Arguments
    ///
    /// * `device_id` - Unique device identifier (used as Common Name)
    ///
    /// # Examples
    ///
    /// ```
    /// use kdeconnect_protocol::pairing::CertificateInfo;
    ///
    /// let cert_info = CertificateInfo::generate("test_device_id").unwrap();
    /// println!("Fingerprint: {}", cert_info.fingerprint);
    /// ```
    pub fn generate(device_id: impl Into<String>) -> Result<Self> {
        let device_id = device_id.into();

        // Generate RSA 2048-bit key pair
        let rsa = Rsa::generate(2048)?;
        let pkey = PKey::from_rsa(rsa)?;

        // Create X509 certificate builder
        let mut builder = X509::builder()?;

        // Set version to X509v3
        builder.set_version(2)?;

        // Generate random serial number
        let mut serial = BigNum::new()?;
        serial.rand(159, MsbOption::MAYBE_ZERO, false)?;
        let serial = serial.to_asn1_integer()?;
        builder.set_serial_number(&serial)?;

        // Set subject name (DN)
        let mut name = X509Name::builder()?;
        name.append_entry_by_text("O", CERT_ORG)?;
        name.append_entry_by_text("OU", CERT_ORG_UNIT)?;
        name.append_entry_by_text("CN", &device_id)?;
        let name = name.build();
        builder.set_subject_name(&name)?;

        // Set issuer name (same as subject for self-signed)
        builder.set_issuer_name(&name)?;

        // Set validity period (10 years)
        let not_before = Asn1Time::days_from_now(0)?;
        let not_after = Asn1Time::days_from_now(CERT_VALIDITY_YEARS as u32 * 365)?;
        builder.set_not_before(&not_before)?;
        builder.set_not_after(&not_after)?;

        // Set public key
        builder.set_pubkey(&pkey)?;

        // Add X509v3 extensions
        // Note: NOT a CA certificate - this is an end-entity device certificate
        builder.append_extension(BasicConstraints::new().build()?)?;
        builder.append_extension(
            KeyUsage::new()
                .digital_signature()
                .key_encipherment()
                .key_agreement()
                .build()?,
        )?;

        // Sign the certificate with the private key
        builder.sign(&pkey, MessageDigest::sha256())?;

        let cert = builder.build();

        // Get DER-encoded certificate and private key
        let certificate_der = cert.to_der()?;
        let private_key_der = pkey.private_key_to_der()?;

        // Calculate SHA256 fingerprint
        let fingerprint = Self::calculate_fingerprint(&certificate_der);

        info!(
            "Generated certificate for device {} with fingerprint: {}",
            device_id, fingerprint
        );

        Ok(Self {
            device_id,
            certificate: certificate_der,
            private_key: private_key_der,
            fingerprint,
        })
    }

    /// Calculate SHA256 fingerprint of a certificate
    ///
    /// Returns fingerprint in format: XX:XX:XX:...:XX (hex bytes separated by colons)
    pub fn calculate_fingerprint(cert_der: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(cert_der);
        let hash = hasher.finalize();

        // Format as colon-separated hex bytes
        hash.iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(":")
    }

    /// Save certificate and private key to PEM files
    ///
    /// # Arguments
    ///
    /// * `cert_path` - Path to save certificate (.pem)
    /// * `key_path` - Path to save private key (.pem)
    pub fn save_to_files(
        &self,
        cert_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) -> Result<()> {
        let cert_path = cert_path.as_ref();
        let key_path = key_path.as_ref();

        // Create parent directories if they don't exist
        if let Some(parent) = cert_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(parent) = key_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Convert DER to PEM using OpenSSL and save
        let cert = X509::from_der(&self.certificate)?;
        let cert_pem = cert.to_pem()?;
        fs::write(cert_path, cert_pem)?;

        let pkey = PKey::private_key_from_der(&self.private_key)?;
        let key_pem = pkey.private_key_to_pem_pkcs8()?;
        fs::write(key_path, key_pem)?;

        info!(
            "Saved certificate to {:?} and private key to {:?}",
            cert_path, key_path
        );

        Ok(())
    }

    /// Load certificate and private key from PEM files
    pub fn load_from_files(
        cert_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let cert_path = cert_path.as_ref();
        let key_path = key_path.as_ref();

        debug!("Loading certificate from {:?}", cert_path);

        // Read certificate file (PEM format)
        let cert_pem = fs::read(cert_path)?;
        let cert = X509::from_pem(&cert_pem)?;
        let certificate = cert.to_der()?;

        // Read private key file (PEM format)
        let key_pem = fs::read(key_path)?;
        let pkey = PKey::private_key_from_pem(&key_pem)?;
        let private_key = pkey.private_key_to_der()?;

        // Extract device ID from certificate CN
        let device_id = Self::extract_device_id_from_cert(&cert)?;

        // Calculate fingerprint
        let fingerprint = Self::calculate_fingerprint(&certificate);

        info!(
            "Loaded certificate for device {} with fingerprint: {}",
            device_id, fingerprint
        );

        Ok(Self {
            device_id,
            certificate,
            private_key,
            fingerprint,
        })
    }

    /// Extract device ID from certificate Common Name
    fn extract_device_id_from_cert(cert: &X509) -> Result<String> {
        // Get subject name from certificate
        let subject_name = cert.subject_name();

        // Find CN (Common Name) entry
        for entry in subject_name.entries() {
            if entry.object().nid() == openssl::nid::Nid::COMMONNAME {
                let cn = entry.data().as_utf8()?.to_string();
                return Ok(cn);
            }
        }

        Err(ProtocolError::CertificateValidation(
            "Certificate does not contain Common Name".to_string(),
        ))
    }
}

/// Pairing request/response packet
#[derive(Debug, Clone)]
pub struct PairingPacket {
    /// Whether pairing is requested (true) or rejected/unpaired (false)
    pub pair: bool,
}

impl PairingPacket {
    /// Create a pairing request packet
    pub fn request() -> Packet {
        Packet::new("kdeconnect.pair", json!({ "pair": true }))
    }

    /// Create a pairing accept response packet
    pub fn accept() -> Packet {
        Packet::new("kdeconnect.pair", json!({ "pair": true }))
    }

    /// Create a pairing reject response packet
    pub fn reject() -> Packet {
        Packet::new("kdeconnect.pair", json!({ "pair": false }))
    }

    /// Create an unpair packet
    pub fn unpair() -> Packet {
        Packet::new("kdeconnect.pair", json!({ "pair": false }))
    }

    /// Parse a pairing packet
    pub fn from_packet(packet: &Packet) -> Result<Self> {
        if !packet.is_type("kdeconnect.pair") {
            return Err(ProtocolError::InvalidPacket(
                "Not a pairing packet".to_string(),
            ));
        }

        let pair = packet
            .get_body_field::<bool>("pair")
            .ok_or_else(|| ProtocolError::InvalidPacket("Missing pair field".to_string()))?;

        Ok(Self { pair })
    }
}

/// Pairing handler for managing device pairing
pub struct PairingHandler {
    /// This device's certificate
    certificate: CertificateInfo,

    /// Pairing status
    status: PairingStatus,

    /// Paired device certificates (device_id -> certificate)
    paired_devices: std::collections::HashMap<String, Vec<u8>>,

    /// Certificate storage directory
    cert_dir: PathBuf,
}

impl PairingHandler {
    /// Create a new pairing handler
    ///
    /// # Arguments
    ///
    /// * `device_id` - This device's unique identifier
    /// * `cert_dir` - Directory to store certificates
    pub fn new(device_id: impl Into<String>, cert_dir: impl Into<PathBuf>) -> Result<Self> {
        let device_id = device_id.into();
        let cert_dir = cert_dir.into();

        // Ensure certificate directory exists
        fs::create_dir_all(&cert_dir)?;

        // Load or generate certificate
        let cert_path = cert_dir.join("device_cert.pem");
        let key_path = cert_dir.join("device_key.pem");

        let certificate = if cert_path.exists() && key_path.exists() {
            info!("Loading existing certificate for device {}", device_id);
            CertificateInfo::load_from_files(&cert_path, &key_path)?
        } else {
            info!("Generating new certificate for device {}", device_id);
            let cert = CertificateInfo::generate(&device_id)?;
            cert.save_to_files(&cert_path, &key_path)?;
            cert
        };

        Ok(Self {
            certificate,
            status: PairingStatus::Unpaired,
            paired_devices: std::collections::HashMap::new(),
            cert_dir,
        })
    }

    /// Get this device's certificate fingerprint
    pub fn fingerprint(&self) -> &str {
        &self.certificate.fingerprint
    }

    /// Get this device's certificate
    pub fn certificate(&self) -> &CertificateInfo {
        &self.certificate
    }

    /// Get current pairing status
    pub fn status(&self) -> PairingStatus {
        self.status
    }

    /// Send pairing request
    pub fn request_pairing(&mut self) -> Packet {
        self.status = PairingStatus::Requested;
        info!("Sending pairing request");
        PairingPacket::request()
    }

    /// Handle incoming pairing packet
    ///
    /// Returns (should_respond, response_packet)
    pub fn handle_pairing_packet(
        &mut self,
        packet: &Packet,
        device_id: &str,
        device_cert: &[u8],
    ) -> Result<(bool, Option<Packet>)> {
        debug!(
            "Received pairing packet from {} - body: {}",
            device_id, packet.body
        );

        let pairing = PairingPacket::from_packet(packet)?;

        debug!(
            "Processing pairing packet from {} - pair: {}",
            device_id, pairing.pair
        );

        if pairing.pair {
            // Pairing request or accept
            match self.status {
                PairingStatus::Unpaired => {
                    // Received pairing request
                    self.status = PairingStatus::RequestedByPeer;
                    info!("Received pairing request from device {}", device_id);
                    // Don't auto-accept, wait for user confirmation
                    Ok((false, None))
                }
                PairingStatus::Requested => {
                    // Received pairing accept
                    self.store_device_certificate(device_id, device_cert)?;
                    self.status = PairingStatus::Paired;
                    info!("Pairing accepted by device {}", device_id);
                    Ok((false, None))
                }
                PairingStatus::RequestedByPeer => {
                    // Already have a pending request from this device
                    warn!("Received duplicate pairing request from {}", device_id);
                    Ok((false, None))
                }
                PairingStatus::Paired => {
                    // Already paired
                    info!(
                        "Received pairing request from already paired device {}",
                        device_id
                    );
                    Ok((true, Some(PairingPacket::accept())))
                }
            }
        } else {
            // Pairing rejection or unpair
            if self.status == PairingStatus::Paired {
                self.remove_device_certificate(device_id)?;
                info!("Unpaired from device {}", device_id);
            } else {
                info!("Pairing rejected by device {}", device_id);
            }
            self.status = PairingStatus::Unpaired;
            Ok((false, None))
        }
    }

    /// Accept pairing request (user confirmed)
    pub fn accept_pairing(&mut self, device_id: &str, device_cert: &[u8]) -> Result<Packet> {
        if self.status != PairingStatus::RequestedByPeer {
            return Err(ProtocolError::InvalidPacket(
                "No pairing request pending".to_string(),
            ));
        }

        self.store_device_certificate(device_id, device_cert)?;
        self.status = PairingStatus::Paired;
        info!("Accepted pairing with device {}", device_id);

        Ok(PairingPacket::accept())
    }

    /// Reject pairing request (user declined)
    pub fn reject_pairing(&mut self) -> Packet {
        self.status = PairingStatus::Unpaired;
        info!("Rejected pairing request");
        PairingPacket::reject()
    }

    /// Unpair from a device
    pub fn unpair(&mut self, device_id: &str) -> Result<Packet> {
        self.remove_device_certificate(device_id)?;
        self.status = PairingStatus::Unpaired;
        info!("Unpairing from device {}", device_id);
        Ok(PairingPacket::unpair())
    }

    /// Check if a device is paired
    pub fn is_paired(&self, device_id: &str) -> bool {
        self.paired_devices.contains_key(device_id) || self.status == PairingStatus::Paired
    }

    /// Store device certificate
    fn store_device_certificate(&mut self, device_id: &str, cert_der: &[u8]) -> Result<()> {
        let cert_path = self.cert_dir.join(format!("{}.pem", device_id));
        let cert_pem = pem::encode(&pem::Pem::new("CERTIFICATE", cert_der.to_vec()));
        fs::write(&cert_path, cert_pem)?;

        self.paired_devices
            .insert(device_id.to_string(), cert_der.to_vec());
        debug!(
            "Stored certificate for device {} at {:?}",
            device_id, cert_path
        );

        Ok(())
    }

    /// Remove device certificate
    fn remove_device_certificate(&mut self, device_id: &str) -> Result<()> {
        let cert_path = self.cert_dir.join(format!("{}.pem", device_id));
        if cert_path.exists() {
            fs::remove_file(&cert_path)?;
        }

        self.paired_devices.remove(device_id);
        debug!("Removed certificate for device {}", device_id);

        Ok(())
    }

    /// Load all paired device certificates
    pub fn load_paired_devices(&mut self) -> Result<()> {
        for entry in fs::read_dir(&self.cert_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("pem") {
                let filename = path.file_stem().and_then(|s| s.to_str());
                if let Some(device_id) = filename {
                    // Skip our own certificate
                    if device_id == "device_cert" || device_id == "device_key" {
                        continue;
                    }

                    // Load certificate (PEM format)
                    let cert_data = fs::read(&path)?;
                    match X509::from_pem(&cert_data) {
                        Ok(cert) => {
                            let cert_der = cert.to_der()?;
                            self.paired_devices
                                .insert(device_id.to_string(), cert_der);
                            debug!("Loaded paired device certificate: {}", device_id);
                        }
                        Err(e) => {
                            warn!("Failed to parse certificate for {}: {}", device_id, e);
                        }
                    }
                }
            }
        }

        info!(
            "Loaded {} paired device certificates",
            self.paired_devices.len()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_certificate_generation() {
        let cert = CertificateInfo::generate("test_device_123").unwrap();

        assert_eq!(cert.device_id, "test_device_123");
        assert!(!cert.certificate.is_empty());
        assert!(!cert.private_key.is_empty());
        assert!(!cert.fingerprint.is_empty());

        // Fingerprint should be in format XX:XX:XX:...
        assert!(cert.fingerprint.contains(':'));
        assert!(cert.fingerprint.len() > 60); // SHA256 is 64 hex chars + 31 colons
    }

    #[test]
    fn test_certificate_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let cert_path = temp_dir.path().join("cert.pem");
        let key_path = temp_dir.path().join("key.pem");

        // Generate and save
        let original = CertificateInfo::generate("test_device").unwrap();
        original.save_to_files(&cert_path, &key_path).unwrap();

        assert!(cert_path.exists());
        assert!(key_path.exists());

        // Load and verify
        let loaded = CertificateInfo::load_from_files(&cert_path, &key_path).unwrap();
        assert_eq!(original.fingerprint, loaded.fingerprint);
    }

    #[test]
    fn test_pairing_packet_creation() {
        let request = PairingPacket::request();
        assert!(request.is_type("kdeconnect.pair"));
        assert_eq!(request.get_body_field::<bool>("pair"), Some(true));

        let accept = PairingPacket::accept();
        assert_eq!(accept.get_body_field::<bool>("pair"), Some(true));

        let reject = PairingPacket::reject();
        assert_eq!(reject.get_body_field::<bool>("pair"), Some(false));
    }

    #[test]
    fn test_pairing_packet_parsing() {
        let packet = PairingPacket::request();
        let parsed = PairingPacket::from_packet(&packet).unwrap();
        assert!(parsed.pair);

        let reject_packet = PairingPacket::reject();
        let parsed_reject = PairingPacket::from_packet(&reject_packet).unwrap();
        assert!(!parsed_reject.pair);
    }

    #[test]
    fn test_pairing_handler_creation() {
        let temp_dir = TempDir::new().unwrap();
        let handler = PairingHandler::new("test_device", temp_dir.path()).unwrap();

        assert_eq!(handler.status(), PairingStatus::Unpaired);
        assert!(!handler.fingerprint().is_empty());
    }

    #[test]
    fn test_pairing_request_flow() {
        let temp_dir = TempDir::new().unwrap();
        let mut handler = PairingHandler::new("test_device", temp_dir.path()).unwrap();

        // Send pairing request
        let request = handler.request_pairing();
        assert_eq!(handler.status(), PairingStatus::Requested);
        assert!(request.is_type("kdeconnect.pair"));
    }

    #[test]
    fn test_certificate_fingerprint() {
        let cert1 = CertificateInfo::generate("device1").unwrap();
        let cert2 = CertificateInfo::generate("device2").unwrap();

        // Different devices should have different fingerprints
        assert_ne!(cert1.fingerprint, cert2.fingerprint);

        // Same certificate should have same fingerprint
        let fp1 = CertificateInfo::calculate_fingerprint(&cert1.certificate);
        let fp2 = CertificateInfo::calculate_fingerprint(&cert1.certificate);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn test_fingerprint_format() {
        let cert = CertificateInfo::generate("test").unwrap();
        let parts: Vec<&str> = cert.fingerprint.split(':').collect();

        // SHA256 produces 32 bytes = 32 parts when split by colons
        assert_eq!(parts.len(), 32);

        // Each part should be 2 hex digits
        for part in parts {
            assert_eq!(part.len(), 2);
            assert!(part.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }
}
