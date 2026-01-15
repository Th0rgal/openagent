//! Template encryption utilities for environment variables.
//!
//! Provides AES-256-GCM encryption for sensitive template fields like env vars.
//! Encrypted values are wrapped in `<encrypted v="1">BASE64</encrypted>` format
//! to enable auto-detection and backward compatibility with plaintext values.
//!
//! ## Usage
//!
//! ```ignore
//! use template_crypto::{load_or_create_private_key, encrypt_string, decrypt_string, is_encrypted};
//!
//! // At startup - load or generate key
//! let key = load_or_create_private_key()?;
//!
//! // Encrypt on save
//! let encrypted = encrypt_string(&key, "my-secret-value")?;
//! // Returns: <encrypted v="1">BASE64...</encrypted>
//!
//! // Decrypt on load (handles both encrypted and plaintext)
//! let plaintext = decrypt_string(&key, &encrypted)?;
//! let legacy = decrypt_string(&key, "plain-value")?; // Returns as-is
//! ```

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::RngCore;
use std::path::Path;
use thiserror::Error;

/// Nonce length in bytes (96 bits for AES-GCM)
const NONCE_LENGTH: usize = 12;

/// Key length in bytes (256 bits for AES-256)
const KEY_LENGTH: usize = 32;

/// Prefix for encrypted values
const ENCRYPTED_PREFIX: &str = "<encrypted v=\"1\">";
/// Suffix for encrypted values
const ENCRYPTED_SUFFIX: &str = "</encrypted>";

/// Errors that can occur during template encryption operations.
#[derive(Debug, Error)]
pub enum TemplateCryptoError {
    #[error("Private key not configured. Set PRIVATE_KEY in .env or let the system generate one.")]
    KeyNotConfigured,

    #[error("Invalid private key format: {0}")]
    InvalidKeyFormat(String),

    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    #[error("Failed to write .env file: {0}")]
    EnvWriteFailed(String),

    #[error("Invalid encrypted value format")]
    InvalidFormat,
}

/// Check if a value is in encrypted format.
///
/// Returns true if the value is wrapped in `<encrypted v="1">...</encrypted>`.
pub fn is_encrypted(value: &str) -> bool {
    value.starts_with(ENCRYPTED_PREFIX) && value.ends_with(ENCRYPTED_SUFFIX)
}

/// Extract the base64 payload from an encrypted value.
fn extract_payload(value: &str) -> Option<&str> {
    if !is_encrypted(value) {
        return None;
    }
    let start = ENCRYPTED_PREFIX.len();
    let end = value.len() - ENCRYPTED_SUFFIX.len();
    Some(&value[start..end])
}

/// Encrypt a plaintext string using AES-256-GCM.
///
/// Returns the encrypted value wrapped in `<encrypted v="1">BASE64</encrypted>` format.
/// The BASE64 payload contains: nonce (12 bytes) || ciphertext.
///
/// Returns an error if the value is already encrypted (prevents double-encryption).
pub fn encrypt_string(
    key: &[u8; KEY_LENGTH],
    plaintext: &str,
) -> Result<String, TemplateCryptoError> {
    // Prevent double-encryption
    if is_encrypted(plaintext) {
        return Err(TemplateCryptoError::EncryptionFailed(
            "Value is already encrypted".to_string(),
        ));
    }

    // Generate random nonce
    let mut nonce_bytes = [0u8; NONCE_LENGTH];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    // Create cipher
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| TemplateCryptoError::EncryptionFailed(e.to_string()))?;

    let nonce = Nonce::from_slice(&nonce_bytes);

    // Encrypt
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| TemplateCryptoError::EncryptionFailed(e.to_string()))?;

    // Combine nonce || ciphertext and encode as base64
    let mut payload = Vec::with_capacity(NONCE_LENGTH + ciphertext.len());
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);

    let encoded = BASE64.encode(&payload);

    Ok(format!(
        "{}{}{}",
        ENCRYPTED_PREFIX, encoded, ENCRYPTED_SUFFIX
    ))
}

/// Decrypt an encrypted string or pass through plaintext values.
///
/// If the value is wrapped in `<encrypted v="1">...</encrypted>`, decrypts it.
/// Otherwise, returns the value as-is (backward compatibility with plaintext).
pub fn decrypt_string(key: &[u8; KEY_LENGTH], value: &str) -> Result<String, TemplateCryptoError> {
    // Plaintext passthrough for backward compatibility
    if !is_encrypted(value) {
        return Ok(value.to_string());
    }

    // Extract and decode payload
    let payload_b64 = extract_payload(value).ok_or(TemplateCryptoError::InvalidFormat)?;

    let payload = BASE64
        .decode(payload_b64)
        .map_err(|e| TemplateCryptoError::DecryptionFailed(format!("Invalid base64: {}", e)))?;

    // Payload must contain at least nonce (12 bytes) + some ciphertext
    if payload.len() < NONCE_LENGTH + 1 {
        return Err(TemplateCryptoError::DecryptionFailed(
            "Payload too short".to_string(),
        ));
    }

    let nonce_bytes = &payload[..NONCE_LENGTH];
    let ciphertext = &payload[NONCE_LENGTH..];

    // Create cipher
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| TemplateCryptoError::DecryptionFailed(e.to_string()))?;

    let nonce = Nonce::from_slice(nonce_bytes);

    // Decrypt
    let plaintext = cipher.decrypt(nonce, ciphertext).map_err(|_| {
        TemplateCryptoError::DecryptionFailed("Invalid key or corrupted data".to_string())
    })?;

    String::from_utf8(plaintext)
        .map_err(|e| TemplateCryptoError::DecryptionFailed(format!("Invalid UTF-8: {}", e)))
}

/// Load the private key from the PRIVATE_KEY environment variable.
///
/// Returns None if the variable is not set.
pub fn load_private_key_from_env() -> Result<Option<[u8; KEY_LENGTH]>, TemplateCryptoError> {
    match std::env::var("PRIVATE_KEY") {
        Ok(hex_key) => {
            let hex_key = hex_key.trim();
            if hex_key.is_empty() {
                return Ok(None);
            }
            parse_hex_key(hex_key).map(Some)
        }
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(TemplateCryptoError::InvalidKeyFormat(
            "PRIVATE_KEY contains invalid unicode".to_string(),
        )),
    }
}

/// Parse a hex-encoded key string into a 32-byte array.
fn parse_hex_key(hex_key: &str) -> Result<[u8; KEY_LENGTH], TemplateCryptoError> {
    let bytes = hex::decode(hex_key).map_err(|e| {
        TemplateCryptoError::InvalidKeyFormat(format!("Invalid hex encoding: {}", e))
    })?;

    if bytes.len() != KEY_LENGTH {
        return Err(TemplateCryptoError::InvalidKeyFormat(format!(
            "Key must be {} bytes ({} hex chars), got {} bytes",
            KEY_LENGTH,
            KEY_LENGTH * 2,
            bytes.len()
        )));
    }

    let mut key = [0u8; KEY_LENGTH];
    key.copy_from_slice(&bytes);
    Ok(key)
}

/// Generate a new random 32-byte key.
pub fn generate_private_key() -> [u8; KEY_LENGTH] {
    let mut key = [0u8; KEY_LENGTH];
    rand::thread_rng().fill_bytes(&mut key);
    key
}

/// Load the private key from environment, or generate and persist a new one.
///
/// If PRIVATE_KEY is not set in the environment:
/// 1. Generates a new random key
/// 2. Appends it to the .env file at `env_path`
/// 3. Sets it in the current process environment
///
/// Returns the key (either loaded or newly generated).
pub fn load_or_create_private_key(
    env_path: &Path,
) -> Result<[u8; KEY_LENGTH], TemplateCryptoError> {
    // Try to load from environment first
    if let Some(key) = load_private_key_from_env()? {
        return Ok(key);
    }

    // Generate a new key
    let key = generate_private_key();
    let hex_key = hex::encode(key);

    // Append to .env file
    append_key_to_env_file(env_path, &hex_key)?;

    // Set in current process environment
    std::env::set_var("PRIVATE_KEY", &hex_key);

    tracing::info!(
        "Generated new PRIVATE_KEY and appended to {}",
        env_path.display()
    );

    Ok(key)
}

/// Append the PRIVATE_KEY to the .env file.
fn append_key_to_env_file(env_path: &Path, hex_key: &str) -> Result<(), TemplateCryptoError> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(env_path)
        .map_err(|e| TemplateCryptoError::EnvWriteFailed(e.to_string()))?;

    // Add a newline before if the file doesn't end with one
    let needs_newline = std::fs::read_to_string(env_path)
        .map(|content| !content.is_empty() && !content.ends_with('\n'))
        .unwrap_or(false);

    let content = format!(
        "{}# Template encryption key (auto-generated). DO NOT COMMIT.\nPRIVATE_KEY={}\n",
        if needs_newline { "\n" } else { "" },
        hex_key
    );

    file.write_all(content.as_bytes())
        .map_err(|e| TemplateCryptoError::EnvWriteFailed(e.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_is_encrypted() {
        assert!(is_encrypted("<encrypted v=\"1\">SGVsbG8=</encrypted>"));
        assert!(!is_encrypted("plaintext"));
        assert!(!is_encrypted("<encrypted>missing version</encrypted>"));
        assert!(!is_encrypted("<encrypted v=\"1\">unclosed"));
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = generate_private_key();
        let plaintext = "my-secret-api-key-12345";

        let encrypted = encrypt_string(&key, plaintext).unwrap();

        // Verify encrypted format
        assert!(is_encrypted(&encrypted));
        assert!(encrypted.starts_with(ENCRYPTED_PREFIX));
        assert!(encrypted.ends_with(ENCRYPTED_SUFFIX));

        // Decrypt and verify
        let decrypted = decrypt_string(&key, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_plaintext_passthrough() {
        let key = generate_private_key();
        let plaintext = "legacy-plaintext-value";

        // Should pass through unchanged
        let result = decrypt_string(&key, plaintext).unwrap();
        assert_eq!(result, plaintext);
    }

    #[test]
    fn test_prevent_double_encryption() {
        let key = generate_private_key();
        let plaintext = "secret";

        let encrypted = encrypt_string(&key, plaintext).unwrap();

        // Trying to encrypt again should fail
        let result = encrypt_string(&key, &encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_key_fails() {
        let key1 = generate_private_key();
        let key2 = generate_private_key();

        let encrypted = encrypt_string(&key1, "secret").unwrap();

        // Decryption with wrong key should fail
        let result = decrypt_string(&key2, &encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_different_encryptions_different_ciphertext() {
        let key = generate_private_key();
        let plaintext = "same-data";

        let encrypted1 = encrypt_string(&key, plaintext).unwrap();
        let encrypted2 = encrypt_string(&key, plaintext).unwrap();

        // Different nonces should produce different ciphertext
        assert_ne!(encrypted1, encrypted2);

        // But both should decrypt to the same value
        assert_eq!(decrypt_string(&key, &encrypted1).unwrap(), plaintext);
        assert_eq!(decrypt_string(&key, &encrypted2).unwrap(), plaintext);
    }

    #[test]
    fn test_parse_hex_key() {
        // Valid 32-byte key (64 hex chars)
        let hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let key = parse_hex_key(hex).unwrap();
        assert_eq!(key.len(), 32);

        // Invalid: wrong length
        let short = "0123456789abcdef";
        assert!(parse_hex_key(short).is_err());

        // Invalid: not hex
        let invalid = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
        assert!(parse_hex_key(invalid).is_err());
    }

    #[test]
    fn test_load_or_create_key_generates_new() {
        // Use temp file as .env
        let temp_env = NamedTempFile::new().unwrap();

        // Clear any existing env var
        std::env::remove_var("PRIVATE_KEY");

        // Should generate and persist
        let key = load_or_create_private_key(temp_env.path()).unwrap();
        assert_eq!(key.len(), KEY_LENGTH);

        // File should contain the key
        let content = std::fs::read_to_string(temp_env.path()).unwrap();
        assert!(content.contains("PRIVATE_KEY="));

        // Clean up env var
        std::env::remove_var("PRIVATE_KEY");
    }

    #[test]
    fn test_empty_string_encryption() {
        let key = generate_private_key();

        // Even empty strings should encrypt/decrypt correctly
        let encrypted = encrypt_string(&key, "").unwrap();
        let decrypted = decrypt_string(&key, &encrypted).unwrap();
        assert_eq!(decrypted, "");
    }

    #[test]
    fn test_unicode_encryption() {
        let key = generate_private_key();
        let plaintext = "émoji: 🔐 and ñ and 中文";

        let encrypted = encrypt_string(&key, plaintext).unwrap();
        let decrypted = decrypt_string(&key, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }
}
