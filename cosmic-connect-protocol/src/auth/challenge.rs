//! Challenge generation and management for phone authentication.
//!
//! The `ChallengeManager` generates cryptographically secure challenges
//! and tracks them to prevent replay attacks.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use ring::rand::{SecureRandom, SystemRandom};

use super::types::{
    AuthError, Challenge, CHALLENGE_EXPIRY_SECS, CHALLENGE_SIZE, MAX_ACTIVE_CHALLENGES, NONCE_SIZE,
};

/// Manages challenge generation and tracking for phone authentication.
///
/// The `ChallengeManager` is responsible for:
/// - Generating cryptographically secure challenges using `ring::rand::SystemRandom`
/// - Tracking active challenges with their creation timestamps
/// - Preventing nonce reuse (replay attacks)
/// - Enforcing challenge expiry
/// - Limiting active challenges to prevent DoS attacks
pub struct ChallengeManager {
    /// Cryptographically secure random number generator.
    rng: SystemRandom,
    /// Desktop identifier included in challenges.
    desktop_id: String,
    /// Active challenges keyed by nonce (base64 encoded).
    /// Value is (Challenge, creation_timestamp_unix).
    active_challenges: Mutex<HashMap<String, (Challenge, u64)>>,
    /// Previously used nonces to detect replay attacks.
    /// Value is the timestamp when the nonce was used.
    used_nonces: Mutex<HashMap<String, u64>>,
}

impl ChallengeManager {
    /// Creates a new `ChallengeManager` with the given desktop identifier.
    ///
    /// # Arguments
    ///
    /// * `desktop_id` - Unique identifier for this desktop, included in all challenges.
    #[must_use]
    pub fn new(desktop_id: String) -> Self {
        Self {
            rng: SystemRandom::new(),
            desktop_id,
            active_challenges: Mutex::new(HashMap::new()),
            used_nonces: Mutex::new(HashMap::new()),
        }
    }

    /// Generates a new authentication challenge.
    ///
    /// The challenge contains:
    /// - 32 bytes of random data (256-bit security)
    /// - 16-byte nonce for tracking and replay prevention
    /// - Unix timestamp for expiry checking
    /// - Desktop identifier for binding
    ///
    /// # Errors
    ///
    /// Returns `AuthError::CryptoError` if random generation fails.
    /// Returns `AuthError::TooManyChallenges` if the maximum number of active
    /// challenges has been reached (DoS protection).
    pub fn generate_challenge(&self) -> Result<Challenge, AuthError> {
        // Clean up expired challenges first
        self.cleanup_expired_challenges();

        // Check DoS limit
        {
            let challenges = self.active_challenges.lock().map_err(|e| {
                AuthError::CryptoError(format!("Failed to acquire challenges lock: {e}"))
            })?;
            if challenges.len() >= MAX_ACTIVE_CHALLENGES {
                return Err(AuthError::TooManyChallenges);
            }
        }

        // Generate random challenge bytes
        let mut challenge_bytes = [0u8; CHALLENGE_SIZE];
        self.rng.fill(&mut challenge_bytes).map_err(|e| {
            AuthError::CryptoError(format!("Failed to generate random challenge: {e}"))
        })?;

        // Generate random nonce
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        self.rng
            .fill(&mut nonce_bytes)
            .map_err(|e| AuthError::CryptoError(format!("Failed to generate random nonce: {e}")))?;

        let timestamp = current_unix_timestamp();
        let challenge_b64 = base64::engine::general_purpose::STANDARD.encode(challenge_bytes);
        let nonce_b64 = base64::engine::general_purpose::STANDARD.encode(nonce_bytes);

        let challenge = Challenge {
            challenge: challenge_b64,
            nonce: nonce_b64.clone(),
            timestamp,
            desktop_id: self.desktop_id.clone(),
        };

        // Store the challenge
        {
            let mut challenges = self.active_challenges.lock().map_err(|e| {
                AuthError::CryptoError(format!("Failed to acquire challenges lock: {e}"))
            })?;
            challenges.insert(nonce_b64, (challenge.clone(), timestamp));
        }

        Ok(challenge)
    }

    /// Retrieves and consumes a challenge by its nonce.
    ///
    /// This is called when verifying a response. The challenge is removed
    /// from active challenges and the nonce is marked as used.
    ///
    /// # Arguments
    ///
    /// * `nonce` - The base64-encoded nonce from the challenge response.
    ///
    /// # Errors
    ///
    /// Returns `AuthError::ChallengeExpired` if:
    /// - The challenge does not exist
    /// - The challenge has expired (older than 30 seconds)
    ///
    /// Returns `AuthError::NonceReuse` if the nonce was already used.
    pub fn get_and_consume_challenge(&self, nonce: &str) -> Result<Challenge, AuthError> {
        // Check for nonce reuse
        {
            let used = self.used_nonces.lock().map_err(|e| {
                AuthError::CryptoError(format!("Failed to acquire used_nonces lock: {e}"))
            })?;
            if used.contains_key(nonce) {
                return Err(AuthError::NonceReuse);
            }
        }

        // Remove and retrieve the challenge
        let (challenge, created_at) = {
            let mut challenges = self.active_challenges.lock().map_err(|e| {
                AuthError::CryptoError(format!("Failed to acquire challenges lock: {e}"))
            })?;
            challenges
                .remove(nonce)
                .ok_or(AuthError::ChallengeExpired)?
        };

        // Check expiry
        let now = current_unix_timestamp();
        if now.saturating_sub(created_at) > CHALLENGE_EXPIRY_SECS {
            return Err(AuthError::ChallengeExpired);
        }

        // Mark nonce as used
        {
            let mut used = self.used_nonces.lock().map_err(|e| {
                AuthError::CryptoError(format!("Failed to acquire used_nonces lock: {e}"))
            })?;
            used.insert(nonce.to_string(), now);
        }

        Ok(challenge)
    }

    /// Returns the number of currently active challenges.
    ///
    /// This is primarily useful for testing and monitoring.
    pub fn active_challenge_count(&self) -> usize {
        self.active_challenges.lock().map(|c| c.len()).unwrap_or(0)
    }

    /// Cleans up expired challenges and old used nonces.
    ///
    /// This is called automatically before generating new challenges,
    /// but can also be called explicitly for maintenance.
    pub fn cleanup_expired_challenges(&self) {
        let now = current_unix_timestamp();
        let expiry_threshold = now.saturating_sub(CHALLENGE_EXPIRY_SECS);
        // Keep used nonces for 2x the expiry time to catch delayed replays
        let nonce_expiry_threshold = now.saturating_sub(CHALLENGE_EXPIRY_SECS * 2);

        // Clean up expired active challenges
        if let Ok(mut challenges) = self.active_challenges.lock() {
            challenges.retain(|_, (_, created_at)| *created_at > expiry_threshold);
        }

        // Clean up old used nonces
        if let Ok(mut used) = self.used_nonces.lock() {
            used.retain(|_, used_at| *used_at > nonce_expiry_threshold);
        }
    }
}

/// Returns the current Unix timestamp in seconds.
fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_manager() -> ChallengeManager {
        ChallengeManager::new("test-desktop".to_string())
    }

    #[test]
    fn test_challenge_generation() {
        let manager = create_test_manager();
        let challenge = manager.generate_challenge().unwrap();

        assert!(!challenge.challenge.is_empty());
        assert!(!challenge.nonce.is_empty());
        assert_eq!(challenge.desktop_id, "test-desktop");
        assert!(challenge.timestamp > 0);

        // Verify challenge is 32 bytes when decoded
        let decoded = challenge.challenge_bytes().unwrap();
        assert_eq!(decoded.len(), CHALLENGE_SIZE);

        // Verify nonce is 16 bytes when decoded
        let nonce = challenge.nonce_bytes().unwrap();
        assert_eq!(nonce.len(), NONCE_SIZE);
    }

    #[test]
    fn test_challenge_uniqueness() {
        let manager = create_test_manager();

        let c1 = manager.generate_challenge().unwrap();
        let c2 = manager.generate_challenge().unwrap();
        let c3 = manager.generate_challenge().unwrap();

        // All challenges should be unique
        assert_ne!(c1.challenge, c2.challenge);
        assert_ne!(c2.challenge, c3.challenge);
        assert_ne!(c1.challenge, c3.challenge);

        // All nonces should be unique
        assert_ne!(c1.nonce, c2.nonce);
        assert_ne!(c2.nonce, c3.nonce);
        assert_ne!(c1.nonce, c3.nonce);
    }

    #[test]
    fn test_get_and_consume_challenge() {
        let manager = create_test_manager();

        let challenge = manager.generate_challenge().unwrap();
        let nonce = challenge.nonce.clone();

        // Should successfully retrieve and consume the challenge
        let retrieved = manager.get_and_consume_challenge(&nonce).unwrap();
        assert_eq!(retrieved.challenge, challenge.challenge);
        assert_eq!(retrieved.nonce, challenge.nonce);
    }

    #[test]
    fn test_challenge_not_found() {
        let manager = create_test_manager();

        // Try to get a challenge that doesn't exist
        let result = manager.get_and_consume_challenge("nonexistent-nonce");
        assert!(matches!(result, Err(AuthError::ChallengeExpired)));
    }

    #[test]
    fn test_nonce_reuse_detection() {
        let manager = create_test_manager();

        let challenge = manager.generate_challenge().unwrap();
        let nonce = challenge.nonce.clone();

        // First use should succeed
        let _ = manager.get_and_consume_challenge(&nonce).unwrap();

        // Second use should fail with NonceReuse
        let result = manager.get_and_consume_challenge(&nonce);
        assert!(matches!(result, Err(AuthError::NonceReuse)));
    }

    #[test]
    fn test_active_challenge_count() {
        let manager = create_test_manager();

        assert_eq!(manager.active_challenge_count(), 0);

        let c1 = manager.generate_challenge().unwrap();
        assert_eq!(manager.active_challenge_count(), 1);

        let c2 = manager.generate_challenge().unwrap();
        assert_eq!(manager.active_challenge_count(), 2);

        // Consuming a challenge should reduce the count
        manager.get_and_consume_challenge(&c1.nonce).unwrap();
        assert_eq!(manager.active_challenge_count(), 1);

        manager.get_and_consume_challenge(&c2.nonce).unwrap();
        assert_eq!(manager.active_challenge_count(), 0);
    }

    #[test]
    fn test_dos_protection() {
        let manager = create_test_manager();

        // Generate maximum allowed challenges
        for _ in 0..MAX_ACTIVE_CHALLENGES {
            manager.generate_challenge().unwrap();
        }

        // Next challenge should fail
        let result = manager.generate_challenge();
        assert!(matches!(result, Err(AuthError::TooManyChallenges)));
    }

    #[test]
    fn test_cleanup_expired_challenges() {
        let manager = create_test_manager();

        // Generate a challenge
        let _ = manager.generate_challenge().unwrap();
        assert_eq!(manager.active_challenge_count(), 1);

        // Cleanup should not remove fresh challenges
        manager.cleanup_expired_challenges();
        assert_eq!(manager.active_challenge_count(), 1);
    }

    #[test]
    fn test_consumed_challenge_removed_from_active() {
        let manager = create_test_manager();

        let challenge = manager.generate_challenge().unwrap();
        assert_eq!(manager.active_challenge_count(), 1);

        manager.get_and_consume_challenge(&challenge.nonce).unwrap();
        assert_eq!(manager.active_challenge_count(), 0);
    }
}
