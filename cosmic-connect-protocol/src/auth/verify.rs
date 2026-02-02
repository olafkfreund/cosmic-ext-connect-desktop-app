//! Ed25519 signature verification for phone authentication.
//!
//! This module provides the `Verifier` type for verifying Ed25519 signatures
//! from phones during the authentication challenge-response protocol.

use base64::Engine;
use ring::signature::{self, UnparsedPublicKey};

use super::types::{AuthError, Challenge, ChallengeResponse};

/// Ed25519 public key size in bytes.
pub const ED25519_PUBLIC_KEY_SIZE: usize = 32;

/// Ed25519 signature size in bytes.
pub const ED25519_SIGNATURE_SIZE: usize = 64;

/// Verifies Ed25519 signatures from phones.
///
/// The `Verifier` holds a phone's public key and can verify that
/// challenge responses were signed by the corresponding private key.
///
/// # Security Properties
///
/// - Uses `ring` for constant-time signature verification
/// - Validates public key format on construction
/// - Checks signature format before verification
pub struct Verifier {
    /// The raw Ed25519 public key bytes.
    public_key_bytes: Vec<u8>,
}

impl Verifier {
    /// Creates a new `Verifier` from raw public key bytes.
    ///
    /// # Arguments
    ///
    /// * `public_key` - The 32-byte Ed25519 public key.
    ///
    /// # Errors
    ///
    /// Returns `AuthError::InvalidPublicKey` if the key is not exactly 32 bytes.
    pub fn new(public_key: Vec<u8>) -> Result<Self, AuthError> {
        if public_key.len() != ED25519_PUBLIC_KEY_SIZE {
            return Err(AuthError::InvalidPublicKey(format!(
                "Expected {} bytes, got {}",
                ED25519_PUBLIC_KEY_SIZE,
                public_key.len()
            )));
        }

        Ok(Self {
            public_key_bytes: public_key,
        })
    }

    /// Creates a new `Verifier` from a base64-encoded public key.
    ///
    /// # Arguments
    ///
    /// * `public_key_b64` - The base64-encoded Ed25519 public key.
    ///
    /// # Errors
    ///
    /// Returns `AuthError::InvalidPublicKey` if:
    /// - Base64 decoding fails
    /// - The decoded key is not exactly 32 bytes
    pub fn from_base64(public_key_b64: &str) -> Result<Self, AuthError> {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(public_key_b64)
            .map_err(|e| AuthError::InvalidPublicKey(format!("Invalid base64: {e}")))?;

        Self::new(decoded)
    }

    /// Verifies a challenge response signature.
    ///
    /// This method:
    /// 1. Reconstructs the message that should have been signed
    /// 2. Decodes the signature from base64
    /// 3. Verifies the Ed25519 signature using the stored public key
    ///
    /// # Arguments
    ///
    /// * `challenge` - The original challenge that was sent to the phone.
    /// * `response` - The signed response from the phone.
    ///
    /// # Errors
    ///
    /// Returns `AuthError::CryptoError` if signature decoding fails.
    /// Returns `AuthError::InvalidSignature` if the signature is invalid.
    ///
    /// # Security
    ///
    /// The verification is performed in constant time by `ring` to prevent
    /// timing attacks.
    pub fn verify_response(
        &self,
        challenge: &Challenge,
        response: &ChallengeResponse,
    ) -> Result<(), AuthError> {
        // Decode the signature
        let signature_bytes = response.signature_bytes()?;

        // Validate signature length
        if signature_bytes.len() != ED25519_SIGNATURE_SIZE {
            return Err(AuthError::InvalidSignature);
        }

        // Reconstruct the message that was signed
        let message = challenge.signing_message();

        // Create the public key wrapper for verification
        let public_key =
            UnparsedPublicKey::new(&signature::ED25519, self.public_key_bytes.as_slice());

        // Verify the signature (constant-time)
        public_key
            .verify(&message, &signature_bytes)
            .map_err(|_| AuthError::InvalidSignature)
    }

    /// Returns the raw public key bytes.
    #[must_use]
    pub fn public_key_bytes(&self) -> &[u8] {
        &self.public_key_bytes
    }

    /// Returns the public key as a base64-encoded string.
    #[must_use]
    pub fn public_key_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.public_key_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    /// Helper to generate a test keypair and sign a message.
    fn generate_test_keypair() -> (Ed25519KeyPair, Vec<u8>) {
        let rng = SystemRandom::new();
        let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let keypair = Ed25519KeyPair::from_pkcs8(pkcs8_bytes.as_ref()).unwrap();
        let public_key = keypair.public_key().as_ref().to_vec();
        (keypair, public_key)
    }

    fn create_test_challenge() -> Challenge {
        Challenge {
            challenge: base64::engine::general_purpose::STANDARD.encode([42u8; 32]),
            nonce: base64::engine::general_purpose::STANDARD.encode([1u8; 16]),
            timestamp: 1234567890,
            desktop_id: "test-desktop".to_string(),
        }
    }

    #[test]
    fn test_verifier_new_valid() {
        let public_key = vec![0u8; ED25519_PUBLIC_KEY_SIZE];
        let verifier = Verifier::new(public_key.clone());
        assert!(verifier.is_ok());

        let v = verifier.unwrap();
        assert_eq!(v.public_key_bytes(), &public_key);
    }

    #[test]
    fn test_verifier_new_invalid_size() {
        let short_key = vec![0u8; 16];
        let result = Verifier::new(short_key);
        assert!(matches!(result, Err(AuthError::InvalidPublicKey(_))));

        let long_key = vec![0u8; 64];
        let result = Verifier::new(long_key);
        assert!(matches!(result, Err(AuthError::InvalidPublicKey(_))));
    }

    #[test]
    fn test_verifier_from_base64_valid() {
        let public_key = vec![0u8; ED25519_PUBLIC_KEY_SIZE];
        let b64 = base64::engine::general_purpose::STANDARD.encode(&public_key);

        let verifier = Verifier::from_base64(&b64);
        assert!(verifier.is_ok());
    }

    #[test]
    fn test_verifier_from_base64_invalid() {
        let result = Verifier::from_base64("not-valid-base64!!!");
        assert!(matches!(result, Err(AuthError::InvalidPublicKey(_))));
    }

    #[test]
    fn test_valid_signature_verification() {
        let (keypair, public_key) = generate_test_keypair();
        let verifier = Verifier::new(public_key).unwrap();

        let challenge = create_test_challenge();
        let message = challenge.signing_message();

        // Sign the message with the private key
        let signature = keypair.sign(&message);
        let signature_b64 = base64::engine::general_purpose::STANDARD.encode(signature.as_ref());

        let response = ChallengeResponse {
            nonce: challenge.nonce.clone(),
            signature: signature_b64,
            phone_id: "test-phone".to_string(),
        };

        // Verification should succeed
        let result = verifier.verify_response(&challenge, &response);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_signature_rejection() {
        let (_, public_key) = generate_test_keypair();
        let verifier = Verifier::new(public_key).unwrap();

        let challenge = create_test_challenge();

        // Create a response with an invalid signature (all zeros)
        let invalid_signature =
            base64::engine::general_purpose::STANDARD.encode([0u8; ED25519_SIGNATURE_SIZE]);

        let response = ChallengeResponse {
            nonce: challenge.nonce.clone(),
            signature: invalid_signature,
            phone_id: "test-phone".to_string(),
        };

        let result = verifier.verify_response(&challenge, &response);
        assert!(matches!(result, Err(AuthError::InvalidSignature)));
    }

    #[test]
    fn test_wrong_key_rejection() {
        let (keypair1, _) = generate_test_keypair();
        let (_, public_key2) = generate_test_keypair();

        // Verifier has keypair2's public key
        let verifier = Verifier::new(public_key2).unwrap();

        let challenge = create_test_challenge();
        let message = challenge.signing_message();

        // Sign with keypair1's private key
        let signature = keypair1.sign(&message);
        let signature_b64 = base64::engine::general_purpose::STANDARD.encode(signature.as_ref());

        let response = ChallengeResponse {
            nonce: challenge.nonce.clone(),
            signature: signature_b64,
            phone_id: "test-phone".to_string(),
        };

        // Verification should fail - wrong key
        let result = verifier.verify_response(&challenge, &response);
        assert!(matches!(result, Err(AuthError::InvalidSignature)));
    }

    #[test]
    fn test_tampered_challenge_rejection() {
        let (keypair, public_key) = generate_test_keypair();
        let verifier = Verifier::new(public_key).unwrap();

        let original_challenge = create_test_challenge();
        let message = original_challenge.signing_message();

        // Sign the original message
        let signature = keypair.sign(&message);
        let signature_b64 = base64::engine::general_purpose::STANDARD.encode(signature.as_ref());

        // Create a tampered challenge with different data
        let tampered_challenge = Challenge {
            challenge: base64::engine::general_purpose::STANDARD.encode([99u8; 32]),
            nonce: original_challenge.nonce.clone(),
            timestamp: original_challenge.timestamp,
            desktop_id: original_challenge.desktop_id.clone(),
        };

        let response = ChallengeResponse {
            nonce: original_challenge.nonce.clone(),
            signature: signature_b64,
            phone_id: "test-phone".to_string(),
        };

        // Verification should fail - message was tampered
        let result = verifier.verify_response(&tampered_challenge, &response);
        assert!(matches!(result, Err(AuthError::InvalidSignature)));
    }

    #[test]
    fn test_wrong_signature_length_rejection() {
        let (_, public_key) = generate_test_keypair();
        let verifier = Verifier::new(public_key).unwrap();

        let challenge = create_test_challenge();

        // Signature with wrong length (32 bytes instead of 64)
        let short_signature = base64::engine::general_purpose::STANDARD.encode([0u8; 32]);

        let response = ChallengeResponse {
            nonce: challenge.nonce.clone(),
            signature: short_signature,
            phone_id: "test-phone".to_string(),
        };

        let result = verifier.verify_response(&challenge, &response);
        assert!(matches!(result, Err(AuthError::InvalidSignature)));
    }

    #[test]
    fn test_public_key_base64() {
        let public_key = vec![1u8; ED25519_PUBLIC_KEY_SIZE];
        let verifier = Verifier::new(public_key.clone()).unwrap();

        let b64 = verifier.public_key_base64();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .unwrap();
        assert_eq!(decoded, public_key);
    }
}
