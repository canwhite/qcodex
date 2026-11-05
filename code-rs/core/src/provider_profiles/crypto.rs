//! Authenticated encryption for the local provider secret file.
//!
//! The secret file is encrypted with ChaCha20-Poly1305 using a key that is
//! held in the platform credential store. The nonce is generated fresh for
//! every write and stored alongside the ciphertext.

use chacha20poly1305::aead::Aead;
use chacha20poly1305::aead::KeyInit;
use chacha20poly1305::ChaCha20Poly1305;
use chacha20poly1305::Nonce;
use rand::RngCore;

/// Length of the symmetric key in bytes.
pub(crate) const ENCRYPTION_KEY_LEN: usize = 32;

/// Length of the nonce in bytes.
const NONCE_LEN: usize = 12;

/// Encrypts `plaintext` with `key`, returning `nonce || ciphertext`.
pub(crate) fn encrypt(key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|err| format!("invalid encryption key: {err}"))?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|err| format!("failed to encrypt provider secret: {err}"))?;

    let mut output = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Decrypts `payload` (as produced by [`encrypt`]) with `key`.
pub(crate) fn decrypt(key: &[u8], payload: &[u8]) -> Result<Vec<u8>, String> {
    if payload.len() < NONCE_LEN {
        return Err("encrypted payload is too short".to_string());
    }
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|err| format!("invalid encryption key: {err}"))?;
    let nonce = Nonce::from_slice(&payload[..NONCE_LEN]);
    cipher
        .decrypt(nonce, &payload[NONCE_LEN..])
        .map_err(|_| "authentication failed; the secret file was tampered with".to_string())
}

/// Returns `count` cryptographically random bytes.
pub(crate) fn random_bytes(count: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; count];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_plaintext() {
        let key = random_bytes(ENCRYPTION_KEY_LEN);
        let plaintext = b"sk-top-secret-value";
        let encrypted = encrypt(&key, plaintext).expect("encrypt");
        assert_ne!(encrypted, plaintext.to_vec());
        assert_eq!(decrypt(&key, &encrypted).expect("decrypt"), plaintext.to_vec());
    }

    #[test]
    fn uses_a_fresh_nonce_each_time() {
        let key = random_bytes(ENCRYPTION_KEY_LEN);
        let first = encrypt(&key, b"same").expect("encrypt");
        let second = encrypt(&key, b"same").expect("encrypt");
        assert_ne!(first, second);
    }

    #[test]
    fn rejects_a_tampered_payload() {
        let key = random_bytes(ENCRYPTION_KEY_LEN);
        let mut encrypted = encrypt(&key, b"payload").expect("encrypt");
        let last = encrypted.len() - 1;
        encrypted[last] ^= 0xff;
        assert!(decrypt(&key, &encrypted).is_err());
    }

    #[test]
    fn rejects_a_wrong_key() {
        let key = random_bytes(ENCRYPTION_KEY_LEN);
        let other = random_bytes(ENCRYPTION_KEY_LEN);
        let encrypted = encrypt(&key, b"payload").expect("encrypt");
        assert!(decrypt(&other, &encrypted).is_err());
    }
}
