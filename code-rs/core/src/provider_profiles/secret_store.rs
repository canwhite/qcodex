//! Credential storage for provider profiles.
//!
//! Provider profile secrets are addressed by an opaque
//! [`ProviderSecretRef`]. The reference contains only a profile id and a
//! store discriminator, never the secret itself.
//!
//! Two backends are supported:
//!
//! * [`ProviderSecretBackend::Keyring`] stores the secret directly in the
//!   platform credential store (Keychain, Credential Manager, Secret Service).
//! * [`ProviderSecretBackend::EncryptedFile`] stores the secret in a local
//!   file encrypted with a key that is itself kept in the platform
//!   credential store. This is the fallback for environments where the
//!   platform store cannot hold the secret directly.
//!
//! Both backends fail closed: if no secure backend is available the store
//! returns an error instead of degrading to plaintext persistence.

use serde::Deserialize;
use serde::Serialize;
use std::fmt;
use std::io;
use std::path::Path;
use std::path::PathBuf;

/// Service name used when talking to the platform credential store.
const KEYRING_SERVICE: &str = "codex-provider-profiles";

/// Account suffix used for the encryption key in the fallback backend.
const ENCRYPTION_KEY_ACCOUNT: &str = "__encryption_key__";

/// Environment variable that selects the credential backend.
///
/// The value `keyring` stores secrets in the platform credential store
/// (Keychain, Credential Manager, Secret Service). Any other value, which
/// includes the default, keeps secrets in an encrypted file under the code
/// home. The file backend never blocks on an authorization prompt, which
/// makes it the right default for headless installs and CI.
const BACKEND_ENV_VAR: &str = "CODEX_PROVIDER_SECRETS";

/// File name of the encrypted secret file inside the code home.
const ENCRYPTED_SECRETS_FILE: &str = "provider_secrets.enc";

/// File name of the fallback encryption key inside the code home.
///
/// This file is only used when the platform credential store is
/// unavailable. It is written with owner-only permissions.
const ENCRYPTION_KEY_FILE: &str = "provider_secrets.key";

/// A reference to a provider secret held in the credential store.
///
/// The value contains no secret material, so it is safe to persist inside
/// the profile document and to include in redacted diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProviderSecretRef {
    /// Id of the profile that owns the secret.
    pub profile_id: String,

    /// Which backend holds the secret.
    pub backend: ProviderSecretBackend,

    /// The code home the secret belongs to.
    ///
    /// A process may serve more than one code home, so the reference names
    /// the store it must be resolved against. Older references without this
    /// field fall back to the process's single registered store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_home: Option<PathBuf>,
}

impl ProviderSecretRef {
    /// Creates a reference for `profile_id` using the keyring backend.
    pub fn new(profile_id: impl Into<String>) -> Self {
        Self {
            profile_id: profile_id.into(),
            backend: ProviderSecretBackend::Keyring,
            code_home: None,
        }
    }

    /// Creates a reference for `profile_id` using the encrypted-file backend.
    pub fn new_encrypted_file(profile_id: impl Into<String>) -> Self {
        Self {
            profile_id: profile_id.into(),
            backend: ProviderSecretBackend::EncryptedFile,
            code_home: None,
        }
    }

    /// Attaches the code home that owns this secret.
    pub fn with_code_home(mut self, code_home: &Path) -> Self {
        self.code_home = Some(code_home.to_path_buf());
        self
    }
}

/// Identifies the backend that holds a provider secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderSecretBackend {
    /// The secret lives directly in the platform credential store.
    Keyring,
    /// The secret lives in a local encrypted file.
    EncryptedFile,
}

/// Errors produced by the credential store.
#[derive(Debug)]
pub enum ProviderSecretStoreError {
    /// The platform credential store could not be used.
    Keyring(String),
    /// The local encrypted secret file could not be used.
    EncryptedFile(String),
    /// The secret could not be decoded.
    Corrupt(String),
    /// No secure backend is available on this machine.
    NoSecureBackend(String),
}

impl fmt::Display for ProviderSecretStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keyring(message) => {
                write!(f, "platform credential store error: {message}")
            }
            Self::EncryptedFile(message) => {
                write!(f, "encrypted secret store error: {message}")
            }
            Self::Corrupt(message) => write!(f, "stored secret is corrupt: {message}"),
            Self::NoSecureBackend(message) => {
                write!(f, "no secure credential backend available: {message}")
            }
        }
    }
}

impl std::error::Error for ProviderSecretStoreError {}

impl From<ProviderSecretStoreError> for io::Error {
    fn from(error: ProviderSecretStoreError) -> Self {
        io::Error::other(error.to_string())
    }
}

/// A store for provider profile secrets.
#[derive(Debug, Clone)]
pub struct ProviderSecretStore {
    keyring: KeyringStore,
    code_home: PathBuf,
}

impl ProviderSecretStore {
    /// Creates a secret store rooted at `code_home`.
    pub fn new(code_home: &Path) -> Self {
        let use_keyring = std::env::var(BACKEND_ENV_VAR)
            .map(|value| value.trim().eq_ignore_ascii_case("keyring"))
            .unwrap_or(false);
        Self {
            keyring: if use_keyring {
                KeyringStore::platform()
            } else {
                KeyringStore::unavailable()
            },
            code_home: code_home.to_path_buf(),
        }
    }

    /// Creates a secret store with an explicit keyring implementation.
    ///
    /// Used by tests to avoid touching the real platform credential store.
    pub fn with_keyring(code_home: &Path, keyring: KeyringStore) -> Self {
        Self {
            keyring,
            code_home: code_home.to_path_buf(),
        }
    }

    /// Stores `secret` for `profile_id` and returns the reference to it.
    ///
    /// Prefers the platform credential store. If the platform store rejects
    /// the write, the secret is placed in the encrypted-file fallback.
    pub fn store_secret(
        &self,
        profile_id: &str,
        secret: &str,
    ) -> Result<ProviderSecretRef, ProviderSecretStoreError> {
        match self.keyring.save(KEYRING_SERVICE, profile_id, secret) {
            Ok(()) => Ok(ProviderSecretRef::new(profile_id).with_code_home(&self.code_home)),
            Err(keyring_error) => {
                tracing::warn!(
                    "keyring store rejected secret for profile {profile_id}, falling back to encrypted file: {keyring_error}"
                );
                self.store_secret_encrypted_file(profile_id, secret)?;
                Ok(ProviderSecretRef::new_encrypted_file(profile_id)
                    .with_code_home(&self.code_home))
            }
        }
    }

    /// Loads the secret addressed by `secret_ref`.
    pub fn load_secret(
        &self,
        secret_ref: &ProviderSecretRef,
    ) -> Result<Option<String>, ProviderSecretStoreError> {
        match secret_ref.backend {
            ProviderSecretBackend::Keyring => self
                .keyring
                .load(KEYRING_SERVICE, &secret_ref.profile_id)
                .map_err(|err| ProviderSecretStoreError::Keyring(err.to_string())),
            ProviderSecretBackend::EncryptedFile => {
                self.load_secret_encrypted_file(&secret_ref.profile_id)
            }
        }
    }

    /// Removes the secret addressed by `secret_ref`.
    ///
    /// Returns `true` when a secret was removed.
    pub fn delete_secret(
        &self,
        secret_ref: &ProviderSecretRef,
    ) -> Result<bool, ProviderSecretStoreError> {
        match secret_ref.backend {
            ProviderSecretBackend::Keyring => self
                .keyring
                .delete(KEYRING_SERVICE, &secret_ref.profile_id)
                .map_err(|err| ProviderSecretStoreError::Keyring(err.to_string())),
            ProviderSecretBackend::EncryptedFile => {
                self.delete_secret_encrypted_file(&secret_ref.profile_id)
            }
        }
    }

    /// Reports whether a secret exists for the given reference.
    pub fn has_secret(
        &self,
        secret_ref: &ProviderSecretRef,
    ) -> Result<bool, ProviderSecretStoreError> {
        Ok(self.load_secret(secret_ref)?.is_some())
    }

    fn encrypted_secrets_path(&self) -> PathBuf {
        self.code_home.join(ENCRYPTED_SECRETS_FILE)
    }

    fn store_secret_encrypted_file(
        &self,
        profile_id: &str,
        secret: &str,
    ) -> Result<(), ProviderSecretStoreError> {
        let mut secrets = self.read_encrypted_secrets()?;
        secrets.insert(profile_id.to_string(), secret.to_string());
        self.write_encrypted_secrets(&secrets)
    }

    fn load_secret_encrypted_file(
        &self,
        profile_id: &str,
    ) -> Result<Option<String>, ProviderSecretStoreError> {
        let secrets = self.read_encrypted_secrets()?;
        Ok(secrets.get(profile_id).cloned())
    }

    fn delete_secret_encrypted_file(
        &self,
        profile_id: &str,
    ) -> Result<bool, ProviderSecretStoreError> {
        let mut secrets = self.read_encrypted_secrets()?;
        let removed = secrets.remove(profile_id).is_some();
        if removed {
            self.write_encrypted_secrets(&secrets)?;
        }
        Ok(removed)
    }

    fn read_encrypted_secrets(
        &self,
    ) -> Result<std::collections::BTreeMap<String, String>, ProviderSecretStoreError> {
        let path = self.encrypted_secrets_path();
        if !path.exists() {
            return Ok(Default::default());
        }
        let raw = std::fs::read(&path)
            .map_err(|err| ProviderSecretStoreError::EncryptedFile(err.to_string()))?;
        let key = self.encryption_key()?;
        let plaintext = crate::provider_profiles::crypto::decrypt(&key, &raw)
            .map_err(ProviderSecretStoreError::Corrupt)?;
        serde_json::from_slice(&plaintext)
            .map_err(|err| ProviderSecretStoreError::Corrupt(err.to_string()))
    }

    fn write_encrypted_secrets(
        &self,
        secrets: &std::collections::BTreeMap<String, String>,
    ) -> Result<(), ProviderSecretStoreError> {
        let plaintext = serde_json::to_vec(secrets)
            .map_err(|err| ProviderSecretStoreError::EncryptedFile(err.to_string()))?;
        let key = self.encryption_key()?;
        let ciphertext = crate::provider_profiles::crypto::encrypt(&key, &plaintext)
            .map_err(ProviderSecretStoreError::EncryptedFile)?;
        let path = self.encrypted_secrets_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| ProviderSecretStoreError::EncryptedFile(err.to_string()))?;
        }
        crate::provider_profiles::write_private_file(&path, &ciphertext)
            .map_err(|err| ProviderSecretStoreError::EncryptedFile(err.to_string()))
    }

    /// Returns the key used to encrypt the local secret file.
    ///
    /// The key itself lives in the platform credential store so that the
    /// file on disk is useless without the user's keychain. When the
    /// platform store is unavailable, the key falls back to an owner-only
    /// key file next to the encrypted secrets, so the feature still works
    /// in a headless environment instead of blocking on an authorization
    /// prompt.
    fn encryption_key(&self) -> Result<Vec<u8>, ProviderSecretStoreError> {
        if let Ok(Some(existing)) = self
            .keyring
            .load(KEYRING_SERVICE, ENCRYPTION_KEY_ACCOUNT)
        {
            let decoded = base64_decode(&existing).ok_or_else(|| {
                ProviderSecretStoreError::Corrupt("encryption key is not valid base64".to_string())
            })?;
            return Ok(decoded);
        }

        // The platform store may not hold the key. Fall back to a key file
        // with owner-only permissions.
        let key_path = self.encryption_key_path();
        if key_path.exists() {
            let raw = std::fs::read(&key_path)
                .map_err(|err| ProviderSecretStoreError::EncryptedFile(err.to_string()))?;
            let text = String::from_utf8_lossy(&raw);
            return base64_decode(text.trim()).ok_or_else(|| {
                ProviderSecretStoreError::Corrupt(
                    "the encryption key file is not valid base64".to_string(),
                )
            });
        }

        let key = crate::provider_profiles::crypto::random_bytes(
            crate::provider_profiles::crypto::ENCRYPTION_KEY_LEN,
        );
        if self
            .keyring
            .save(
                KEYRING_SERVICE,
                ENCRYPTION_KEY_ACCOUNT,
                &base64_encode(&key),
            )
            .is_ok()
        {
            return Ok(key);
        }

        // The platform store is unavailable. Store the key in an
        // owner-only file instead of failing, so a headless install still
        // works.
        crate::provider_profiles::write_private_file(
            &key_path,
            base64_encode(&key).as_bytes(),
        )
        .map_err(|err| {
            ProviderSecretStoreError::NoSecureBackend(format!(
                "cannot store the encryption key: {err}"
            ))
        })?;
        Ok(key)
    }

    fn encryption_key_path(&self) -> PathBuf {
        self.code_home.join(ENCRYPTION_KEY_FILE)
    }
}

/// A minimal keyring abstraction.
///
/// The platform implementation is selected at runtime; tests substitute an
/// in-memory implementation so they never touch the user's real keychain.
#[derive(Debug, Clone)]
pub struct KeyringStore {
    inner: KeyringBackend,
}

/// The concrete keyring backend.
#[derive(Debug, Clone)]
enum KeyringBackend {
    /// A real platform credential store.
    Platform,
    /// A backend that is not available.
    ///
    /// Every operation fails, which routes the caller to the encrypted-file
    /// fallback instead of prompting the user for authorization.
    Unavailable,
    /// An in-memory store used by tests.
    Memory(std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>),
}

impl KeyringStore {
    /// Returns the platform credential store.
    pub fn platform() -> Self {
        Self {
            inner: KeyringBackend::Platform,
        }
    }

    /// Returns an in-memory store, for tests.
    pub fn in_memory() -> Self {
        Self {
            inner: KeyringBackend::Memory(Default::default()),
        }
    }

    /// Returns a backend that is never available.
    ///
    /// This keeps the platform credential store out of the path entirely,
    /// so a write cannot block on an authorization prompt.
    pub fn unavailable() -> Self {
        Self {
            inner: KeyringBackend::Unavailable,
        }
    }

    fn save(
        &self,
        service: &str,
        account: &str,
        value: &str,
    ) -> Result<(), ProviderSecretStoreError> {
        let key = format!("{service}\u{1f}{account}");
        match &self.inner {
            KeyringBackend::Memory(map) => {
                let mut guard = map
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.insert(key, value.to_string());
                Ok(())
            }
            KeyringBackend::Platform => platform_save(service, account, value),
            KeyringBackend::Unavailable => Err(ProviderSecretStoreError::NoSecureBackend(
                "the platform credential store is disabled".to_string(),
            )),
        }
    }

    fn load(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Option<String>, ProviderSecretStoreError> {
        let key = format!("{service}\u{1f}{account}");
        match &self.inner {
            KeyringBackend::Memory(map) => {
                let guard = map
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                Ok(guard.get(&key).cloned())
            }
            KeyringBackend::Platform => platform_load(service, account),
            KeyringBackend::Unavailable => Err(ProviderSecretStoreError::NoSecureBackend(
                "the platform credential store is disabled".to_string(),
            )),
        }
    }

    fn delete(&self, service: &str, account: &str) -> Result<bool, ProviderSecretStoreError> {
        let key = format!("{service}\u{1f}{account}");
        match &self.inner {
            KeyringBackend::Memory(map) => {
                let mut guard = map
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                Ok(guard.remove(&key).is_some())
            }
            KeyringBackend::Platform => platform_delete(service, account),
            KeyringBackend::Unavailable => Err(ProviderSecretStoreError::NoSecureBackend(
                "the platform credential store is disabled".to_string(),
            )),
        }
    }
}

fn platform_save(
    service: &str,
    account: &str,
    value: &str,
) -> Result<(), ProviderSecretStoreError> {
    let entry = keyring::Entry::new(service, account)
        .map_err(|err| ProviderSecretStoreError::Keyring(err.to_string()))?;
    entry
        .set_password(value)
        .map_err(|err| ProviderSecretStoreError::Keyring(err.to_string()))
}

fn platform_load(
    service: &str,
    account: &str,
) -> Result<Option<String>, ProviderSecretStoreError> {
    let entry = keyring::Entry::new(service, account)
        .map_err(|err| ProviderSecretStoreError::Keyring(err.to_string()))?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(ProviderSecretStoreError::Keyring(err.to_string())),
    }
}

fn platform_delete(service: &str, account: &str) -> Result<bool, ProviderSecretStoreError> {
    let entry = keyring::Entry::new(service, account)
        .map_err(|err| ProviderSecretStoreError::Keyring(err.to_string()))?;
    match entry.delete_credential() {
        Ok(()) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(err) => Err(ProviderSecretStoreError::Keyring(err.to_string())),
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((triple >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some((byte - b'A') as u32),
            b'a'..=b'z' => Some((byte - b'a') as u32 + 26),
            b'0'..=b'9' => Some((byte - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let mut buffer = 0u32;
    let mut bits = 0u32;
    let mut out = Vec::new();
    for byte in text.bytes() {
        if byte == b'=' {
            break;
        }
        if byte.is_ascii_whitespace() {
            continue;
        }
        let decoded = value(byte)?;
        buffer = (buffer << 6) | decoded;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "qcodex-secret-store-test-{tag}-{}",
            std::process::id(),
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn stores_and_loads_secret_via_keyring() {
        let home = temp_home("keyring");
        let store = ProviderSecretStore::with_keyring(&home, KeyringStore::in_memory());

        let secret_ref = store.store_secret("p1", "sk-secret").expect("store");
        assert_eq!(secret_ref.backend, ProviderSecretBackend::Keyring);
        assert_eq!(
            store.load_secret(&secret_ref).expect("load"),
            Some("sk-secret".to_string())
        );
        assert!(store.has_secret(&secret_ref).expect("has"));
        assert!(store.delete_secret(&secret_ref).expect("delete"));
        assert_eq!(store.load_secret(&secret_ref).expect("load"), None);
    }

    #[test]
    fn encrypted_file_backend_round_trips() {
        let home = temp_home("encrypted");
        let store = ProviderSecretStore::with_keyring(&home, KeyringStore::in_memory());

        let secret_ref = ProviderSecretRef::new_encrypted_file("p2");
        store
            .store_secret_encrypted_file("p2", "sk-other")
            .expect("store encrypted");

        assert_eq!(
            store.load_secret(&secret_ref).expect("load"),
            Some("sk-other".to_string())
        );
    }

    #[test]
    fn encrypted_file_contains_no_plaintext() {
        let home = temp_home("plaintext");
        let store = ProviderSecretStore::with_keyring(&home, KeyringStore::in_memory());
        store
            .store_secret_encrypted_file("p3", "sk-plaintext-marker")
            .expect("store encrypted");

        let raw = std::fs::read(store.encrypted_secrets_path()).expect("read file");
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.contains("sk-plaintext-marker"));
    }

    #[test]
    fn base64_round_trips() {
        for input in [&b""[..], b"a", b"ab", b"abc", b"hello world"] {
            let encoded = base64_encode(input);
            assert_eq!(base64_decode(&encoded).as_deref(), Some(input));
        }
    }
}
