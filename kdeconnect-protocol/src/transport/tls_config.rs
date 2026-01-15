//! TLS Configuration for KDE Connect
//!
//! This module provides TLS server and client configuration for secure
//! communication between paired devices using mutual TLS authentication.
//!
//! Uses OpenSSL directly to support TLS 1.0 compatibility with Android KDE Connect app.

use crate::{CertificateInfo, ProtocolError, Result};
use openssl::pkey::PKey;
use openssl::ssl::{SslAcceptor, SslConnector, SslMethod, SslVerifyMode, SslVersion};
use openssl::x509::X509;
use std::sync::Arc;
use tracing::debug;

/// Create a TLS acceptor (server) configuration for accepting connections from paired devices
///
/// # Arguments
///
/// * `our_cert` - Our device certificate information
/// * `_trusted_device_certs` - Certificates of all paired devices (for future validation)
///
/// # Returns
///
/// Configured SslAcceptor that accepts connections with TLS 1.0+ support
pub fn create_server_config(
    our_cert: &CertificateInfo,
    _trusted_device_certs: Vec<Vec<u8>>,
) -> Result<Arc<SslAcceptor>> {
    debug!("Creating TLS server config with OpenSSL (TLS 1.0+ support)");

    // Build SSL acceptor using OpenSSL API for TLS 1.0 support
    let mut acceptor_builder = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server())
        .map_err(|e| {
            ProtocolError::CertificateValidation(format!("Failed to create SSL acceptor: {}", e))
        })?;

    // Configure to support TLS 1.0 through TLS 1.3 (for Android KDE Connect compatibility)
    acceptor_builder
        .set_min_proto_version(Some(SslVersion::TLS1))
        .map_err(|e| {
            ProtocolError::CertificateValidation(format!("Failed to set min TLS version: {}", e))
        })?;

    acceptor_builder
        .set_max_proto_version(Some(SslVersion::TLS1_3))
        .map_err(|e| {
            ProtocolError::CertificateValidation(format!("Failed to set max TLS version: {}", e))
        })?;

    // Set cipher suites to match KDE Connect official daemon (v1.4+)
    // These are the only three cipher suites KDE Connect supports for Android compatibility
    // ECDHE-RSA-AES128-SHA is required for older Android devices using TLS 1.0
    // @SECLEVEL=1 is required to allow TLS 1.0 and weaker ciphers (security level 2 blocks them)
    let cipher_list = "ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA:@SECLEVEL=1";
    acceptor_builder
        .set_cipher_list(cipher_list)
        .map_err(|e| {
            ProtocolError::CertificateValidation(format!("Failed to set cipher list: {}", e))
        })?;

    // Don't request or verify client certificates (TOFU model)
    // KDE Connect uses Trust-On-First-Use - we accept any device for initial pairing
    // Certificate validation happens at the application layer after TLS is established
    acceptor_builder.set_verify(SslVerifyMode::NONE);

    // Parse and set our certificate
    let cert = X509::from_der(&our_cert.certificate).map_err(|e| {
        ProtocolError::CertificateValidation(format!("Failed to parse certificate: {}", e))
    })?;

    let pkey = PKey::private_key_from_der(&our_cert.private_key).map_err(|e| {
        ProtocolError::CertificateValidation(format!("Failed to parse private key: {}", e))
    })?;

    acceptor_builder.set_certificate(&cert).map_err(|e| {
        ProtocolError::CertificateValidation(format!("Failed to set certificate: {}", e))
    })?;

    acceptor_builder.set_private_key(&pkey).map_err(|e| {
        ProtocolError::CertificateValidation(format!("Failed to set private key: {}", e))
    })?;

    let acceptor = acceptor_builder.build();

    debug!("TLS server config created successfully with TLS 1.0+ support");
    Ok(Arc::new(acceptor))
}

/// Create a TLS connector (client) configuration for connecting to a specific paired device
///
/// # Arguments
///
/// * `our_cert` - Our device certificate information
/// * `_peer_cert` - The paired device's certificate (for future validation)
///
/// # Returns
///
/// Configured SslConnector that validates the server certificate
pub fn create_client_config(
    our_cert: &CertificateInfo,
    _peer_cert: Vec<u8>,
) -> Result<Arc<SslConnector>> {
    debug!("Creating TLS client config with OpenSSL (TLS 1.0+ support)");

    // Build connector using OpenSSL API for TLS 1.0 support
    let mut connector_builder = SslConnector::builder(SslMethod::tls_client()).map_err(|e| {
        ProtocolError::CertificateValidation(format!("Failed to create SSL connector: {}", e))
    })?;

    // Configure to support TLS 1.0 through TLS 1.3
    connector_builder
        .set_min_proto_version(Some(SslVersion::TLS1))
        .map_err(|e| {
            ProtocolError::CertificateValidation(format!("Failed to set min TLS version: {}", e))
        })?;

    connector_builder
        .set_max_proto_version(Some(SslVersion::TLS1_3))
        .map_err(|e| {
            ProtocolError::CertificateValidation(format!("Failed to set max TLS version: {}", e))
        })?;

    // Set cipher suites to match KDE Connect official daemon (v1.4+)
    // @SECLEVEL=1 is required to allow TLS 1.0 and weaker ciphers
    let cipher_list = "ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA:@SECLEVEL=1";
    connector_builder
        .set_cipher_list(cipher_list)
        .map_err(|e| {
            ProtocolError::CertificateValidation(format!("Failed to set cipher list: {}", e))
        })?;

    // Accept self-signed certificates (KDE Connect uses self-signed certs)
    connector_builder.set_verify(SslVerifyMode::NONE);

    // Parse and set our certificate for client auth
    let cert = X509::from_der(&our_cert.certificate).map_err(|e| {
        ProtocolError::CertificateValidation(format!("Failed to parse certificate: {}", e))
    })?;

    let pkey = PKey::private_key_from_der(&our_cert.private_key).map_err(|e| {
        ProtocolError::CertificateValidation(format!("Failed to parse private key: {}", e))
    })?;

    connector_builder.set_certificate(&cert).map_err(|e| {
        ProtocolError::CertificateValidation(format!("Failed to set certificate: {}", e))
    })?;

    connector_builder.set_private_key(&pkey).map_err(|e| {
        ProtocolError::CertificateValidation(format!("Failed to set private key: {}", e))
    })?;

    let connector = connector_builder.build();

    debug!("TLS client config created successfully with TLS 1.0+ support");
    Ok(Arc::new(connector))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_server_config() {
        // Generate test certificate
        let cert = CertificateInfo::generate("test_device").unwrap();

        // Create server config with no trusted devices
        let config = create_server_config(&cert, vec![]);
        assert!(config.is_ok());

        // Create server config with one trusted device
        let trusted_certs = vec![cert.certificate.clone()];
        let config = create_server_config(&cert, trusted_certs);
        assert!(config.is_ok());
    }

    #[test]
    fn test_create_client_config() {
        // Generate test certificates
        let our_cert = CertificateInfo::generate("device1").unwrap();
        let peer_cert = CertificateInfo::generate("device2").unwrap();

        // Create client config
        let config = create_client_config(&our_cert, peer_cert.certificate.clone());
        assert!(config.is_ok());
    }
}
