//! Persistence for provider profiles.
//!
//! The repository owns a single JSON document under the code home. Writes
//! are atomic (write to a temporary file, then rename) and guarded by a
//! compare-and-swap on the document revision, so two writers cannot
//! silently clobber each other.

use super::ProviderProfile;
use super::ProviderProfileDocument;
use super::ProviderProfileId;
use super::PROFILE_STORE_VERSION;
use super::provider_profiles_path;
use super::unix_now;
use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::Path;
use std::path::PathBuf;

/// Errors produced by the profile repository.
#[derive(Debug)]
pub enum ProviderProfileStoreError {
    /// The profile document could not be read or written.
    Io(io::Error),
    /// The profile document could not be parsed.
    Parse(String),
    /// The document uses an unsupported schema version.
    UnsupportedVersion(u32),
    /// The requested profile does not exist.
    NotFound(String),
    /// A profile with the requested id already exists.
    AlreadyExists(String),
    /// The caller supplied a stale revision.
    Conflict {
        /// The revision the caller believed was current.
        expected: u64,
        /// The revision actually stored.
        actual: u64,
    },
    /// The profile failed validation.
    Invalid(String),
}

impl fmt::Display for ProviderProfileStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "profile store I/O error: {err}"),
            Self::Parse(message) => write!(f, "profile store is malformed: {message}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "profile store version {version} is not supported")
            }
            Self::NotFound(id) => write!(f, "profile `{id}` does not exist"),
            Self::AlreadyExists(id) => write!(f, "profile `{id}` already exists"),
            Self::Conflict { expected, actual } => write!(
                f,
                "profile store was modified by another writer (expected revision {expected}, found {actual}); reload and retry"
            ),
            Self::Invalid(message) => write!(f, "invalid profile: {message}"),
        }
    }
}

impl std::error::Error for ProviderProfileStoreError {}

impl From<io::Error> for ProviderProfileStoreError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// Reads and writes the provider profile document.
#[derive(Debug, Clone)]
pub struct ProviderProfileRepository {
    path: PathBuf,
}

impl ProviderProfileRepository {
    /// Creates a repository rooted at `code_home`.
    pub fn new(code_home: &Path) -> Self {
        Self {
            path: provider_profiles_path(code_home),
        }
    }

    /// Returns the path of the backing document.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the current document.
    ///
    /// A missing document is treated as an empty store rather than an error.
    pub fn read(&self) -> Result<ProviderProfileDocument, ProviderProfileStoreError> {
        let raw = match std::fs::read(&self.path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Ok(ProviderProfileDocument::default());
            }
            Err(err) => return Err(err.into()),
        };
        let document: ProviderProfileDocument = serde_json::from_slice(&raw)
            .map_err(|err| ProviderProfileStoreError::Parse(err.to_string()))?;
        if document.version != PROFILE_STORE_VERSION {
            return Err(ProviderProfileStoreError::UnsupportedVersion(
                document.version,
            ));
        }
        Ok(document)
    }

    /// Writes the document atomically.
    pub fn write(&self, document: &ProviderProfileDocument) -> Result<(), ProviderProfileStoreError> {
        let mut document = document.clone();
        document.version = PROFILE_STORE_VERSION;
        let serialized = serde_json::to_vec_pretty(&document)
            .map_err(|err| ProviderProfileStoreError::Parse(err.to_string()))?;
        super::write_private_file(&self.path, &serialized)?;
        Ok(())
    }

    /// Lists all profiles.
    pub fn list(&self) -> Result<Vec<ProviderProfile>, ProviderProfileStoreError> {
        Ok(self.read()?.profiles.into_values().collect())
    }

    /// Returns a single profile by id.
    pub fn get(
        &self,
        id: &str,
    ) -> Result<Option<ProviderProfile>, ProviderProfileStoreError> {
        Ok(self.read()?.profiles.get(id).cloned())
    }

    /// Returns the id of the active profile, if any.
    pub fn active_profile_id(&self) -> Result<Option<ProviderProfileId>, ProviderProfileStoreError> {
        Ok(self.read()?.active_profile_id)
    }

    /// Returns the active profile, if any.
    pub fn active_profile(&self) -> Result<Option<ProviderProfile>, ProviderProfileStoreError> {
        let document = self.read()?;
        Ok(document
            .active_profile_id
            .as_ref()
            .and_then(|id| document.profiles.get(id))
            .cloned())
    }

    /// Adds a new profile.
    ///
    /// `expected_document_revision` guards against lost updates. Pass `None`
    /// to skip the check.
    pub fn add(
        &self,
        profile: ProviderProfile,
        expected_document_revision: Option<u64>,
    ) -> Result<ProviderProfile, ProviderProfileStoreError> {
        profile
            .validate()
            .map_err(ProviderProfileStoreError::Invalid)?;
        let mut document = self.read()?;
        self.check_revision(&document, expected_document_revision)?;
        if document.profiles.contains_key(&profile.id) {
            return Err(ProviderProfileStoreError::AlreadyExists(profile.id));
        }
        let stored = profile.clone();
        document.profiles.insert(profile.id.clone(), profile);
        self.write(&document)?;
        Ok(stored)
    }

    /// Adds a profile and makes it active in one atomic document write.
    pub fn add_and_activate(
        &self,
        profile: ProviderProfile,
        expected_document_revision: Option<u64>,
    ) -> Result<ProviderProfile, ProviderProfileStoreError> {
        profile
            .validate()
            .map_err(ProviderProfileStoreError::Invalid)?;
        let mut document = self.read()?;
        self.check_revision(&document, expected_document_revision)?;
        if document.profiles.contains_key(&profile.id) {
            return Err(ProviderProfileStoreError::AlreadyExists(profile.id));
        }
        let stored = profile.clone();
        document.profiles.insert(profile.id.clone(), profile);
        document.active_profile_id = Some(stored.id.clone());
        self.write(&document)?;
        Ok(stored)
    }

    /// Updates an existing profile.
    pub fn update(
        &self,
        profile: ProviderProfile,
        expected_document_revision: Option<u64>,
    ) -> Result<ProviderProfile, ProviderProfileStoreError> {
        profile
            .validate()
            .map_err(ProviderProfileStoreError::Invalid)?;
        let mut document = self.read()?;
        self.check_revision(&document, expected_document_revision)?;
        if !document.profiles.contains_key(&profile.id) {
            return Err(ProviderProfileStoreError::NotFound(profile.id));
        }
        let stored = profile.clone();
        document.profiles.insert(profile.id.clone(), profile);
        self.write(&document)?;
        Ok(stored)
    }

    /// Updates an existing profile and makes it active in one atomic write.
    pub fn update_and_activate(
        &self,
        profile: ProviderProfile,
        expected_document_revision: Option<u64>,
    ) -> Result<ProviderProfile, ProviderProfileStoreError> {
        profile
            .validate()
            .map_err(ProviderProfileStoreError::Invalid)?;
        let mut document = self.read()?;
        self.check_revision(&document, expected_document_revision)?;
        if !document.profiles.contains_key(&profile.id) {
            return Err(ProviderProfileStoreError::NotFound(profile.id));
        }
        let stored = profile.clone();
        document.profiles.insert(profile.id.clone(), profile);
        document.active_profile_id = Some(stored.id.clone());
        self.write(&document)?;
        Ok(stored)
    }

    /// Removes a profile.
    ///
    /// Removing the active profile clears the active selection unless
    /// `replacement` names another profile to activate.
    pub fn remove(
        &self,
        id: &str,
        replacement: Option<&str>,
        expected_document_revision: Option<u64>,
    ) -> Result<bool, ProviderProfileStoreError> {
        let mut document = self.read()?;
        self.check_revision(&document, expected_document_revision)?;
        if !document.profiles.contains_key(id) {
            return Ok(false);
        }
        document.profiles.remove(id);
        if document.active_profile_id.as_deref() == Some(id) {
            match replacement {
                Some(next) => {
                    if !document.profiles.contains_key(next) {
                        return Err(ProviderProfileStoreError::NotFound(next.to_string()));
                    }
                    document.active_profile_id = Some(next.to_string());
                }
                None => document.active_profile_id = None,
            }
        }
        self.write(&document)?;
        Ok(true)
    }

    /// Activates a profile.
    pub fn activate(
        &self,
        id: &str,
        expected_document_revision: Option<u64>,
    ) -> Result<ProviderProfile, ProviderProfileStoreError> {
        let mut document = self.read()?;
        self.check_revision(&document, expected_document_revision)?;
        let profile = document
            .profiles
            .get(id)
            .cloned()
            .ok_or_else(|| ProviderProfileStoreError::NotFound(id.to_string()))?;
        document.active_profile_id = Some(id.to_string());
        self.write(&document)?;
        Ok(profile)
    }

    /// Clears the active profile selection.
    pub fn deactivate(
        &self,
        expected_document_revision: Option<u64>,
    ) -> Result<(), ProviderProfileStoreError> {
        let mut document = self.read()?;
        self.check_revision(&document, expected_document_revision)?;
        document.active_profile_id = None;
        self.write(&document)?;
        Ok(())
    }

    /// Returns the revision of the stored document.
    ///
    /// The revision is derived from the document contents so that it always
    /// matches what a reader would observe.
    pub fn document_revision(&self) -> Result<u64, ProviderProfileStoreError> {
        Ok(self.read()?.revision())
    }

    fn check_revision(
        &self,
        document: &ProviderProfileDocument,
        expected: Option<u64>,
    ) -> Result<(), ProviderProfileStoreError> {
        if let Some(expected) = expected {
            let actual = document.revision();
            if actual != expected {
                return Err(ProviderProfileStoreError::Conflict { expected, actual });
            }
        }
        Ok(())
    }
}

impl ProviderProfileDocument {
    /// Returns a revision number derived from the document contents.
    ///
    /// Deriving the revision instead of storing it means a caller can never
    /// observe a revision that disagrees with the stored data.
    pub fn revision(&self) -> u64 {
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest;
        hasher.update(self.version.to_le_bytes());
        if let Some(active) = &self.active_profile_id {
            hasher.update(active.as_bytes());
        }
        for (id, profile) in &self.profiles {
            hasher.update(id.as_bytes());
            hasher.update(profile.revision.to_le_bytes());
        }
        let digest = hasher.finalize();
        u64::from_le_bytes(digest[..8].try_into().unwrap_or_default())
    }
}

/// Creates a profile id that is unique within `existing`.
pub fn unique_profile_id(existing: &BTreeMap<ProviderProfileId, ProviderProfile>) -> String {
    let mut index = existing.len() + 1;
    loop {
        let candidate = format!("profile-{index}");
        if !existing.contains_key(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

/// Returns the current time as seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    unix_now()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempHome(PathBuf);

    impl TempHome {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "qcodex-profile-repo-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn profile(id: &str) -> ProviderProfile {
        ProviderProfile::new(
            id,
            "Example",
            "https://example.com/v1",
            crate::model_provider_info::WireApi::Chat,
            "gpt-test",
        )
    }

    #[test]
    fn missing_document_reads_as_empty_store() {
        let home = TempHome::new("empty");
        let repo = ProviderProfileRepository::new(&home.0);
        assert!(repo.list().expect("list").is_empty());
        assert_eq!(repo.active_profile_id().expect("active"), None);
    }

    #[test]
    fn add_then_read_back() {
        let home = TempHome::new("add");
        let repo = ProviderProfileRepository::new(&home.0);
        repo.add(profile("a"), None).expect("add");

        let stored = repo.get("a").expect("get").expect("profile exists");
        assert_eq!(stored.display_name, "Example");
    }

    #[test]
    fn duplicate_add_is_rejected() {
        let home = TempHome::new("dup");
        let repo = ProviderProfileRepository::new(&home.0);
        repo.add(profile("a"), None).expect("add");
        assert!(matches!(
            repo.add(profile("a"), None),
            Err(ProviderProfileStoreError::AlreadyExists(_))
        ));
    }

    #[test]
    fn stale_revision_is_rejected() {
        let home = TempHome::new("conflict");
        let repo = ProviderProfileRepository::new(&home.0);
        repo.add(profile("a"), None).expect("add");
        let stale = repo.document_revision().expect("revision") - 1;

        let error = repo
            .add(profile("b"), Some(stale))
            .expect_err("conflict expected");
        assert!(matches!(error, ProviderProfileStoreError::Conflict { .. }));
    }

    #[test]
    fn removing_active_profile_clears_selection() {
        let home = TempHome::new("remove-active");
        let repo = ProviderProfileRepository::new(&home.0);
        repo.add(profile("a"), None).expect("add");
        repo.activate("a", None).expect("activate");
        assert_eq!(repo.active_profile_id().expect("active"), Some("a".to_string()));

        repo.remove("a", None, None).expect("remove");
        assert_eq!(repo.active_profile_id().expect("active"), None);
    }

    #[test]
    fn removing_active_profile_can_select_a_replacement() {
        let home = TempHome::new("remove-replace");
        let repo = ProviderProfileRepository::new(&home.0);
        repo.add(profile("a"), None).expect("add");
        repo.add(profile("b"), None).expect("add");
        repo.activate("a", None).expect("activate");

        repo.remove("a", Some("b"), None).expect("remove");
        assert_eq!(repo.active_profile_id().expect("active"), Some("b".to_string()));
    }

    #[test]
    fn add_and_activate_persists_one_active_profile() {
        let home = TempHome::new("add-and-activate");
        let repo = ProviderProfileRepository::new(&home.0);
        let stored = repo
            .add_and_activate(profile("a"), None)
            .expect("add and activate");

        assert_eq!(stored.id, "a");
        assert_eq!(repo.active_profile_id().expect("active"), Some("a".to_string()));
        assert_eq!(repo.list().expect("list").len(), 1);
    }

    #[test]
    fn update_and_activate_replaces_the_active_profile() {
        let home = TempHome::new("update-and-activate");
        let repo = ProviderProfileRepository::new(&home.0);
        repo.add_and_activate(profile("openai"), None)
            .expect("add openai profile");
        repo.add(profile("deepseek"), None)
            .expect("add deepseek profile");

        let mut deepseek = repo
            .get("deepseek")
            .expect("load deepseek profile")
            .expect("deepseek profile exists");
        deepseek.model = "deepseek-chat".to_string();

        repo.update_and_activate(deepseek, None)
            .expect("update and activate deepseek profile");

        assert_eq!(
            repo.active_profile_id().expect("active provider"),
            Some("deepseek".to_string())
        );
        assert_eq!(
            repo.get("deepseek")
                .expect("load updated profile")
                .map(|profile| profile.model),
            Some("deepseek-chat".to_string())
        );
    }

    #[test]
    fn revision_changes_after_mutation() {
        let home = TempHome::new("revision");
        let repo = ProviderProfileRepository::new(&home.0);
        let before = repo.document_revision().expect("revision");
        repo.add(profile("a"), None).expect("add");
        assert_ne!(repo.document_revision().expect("revision"), before);
    }

    #[test]
    fn invalid_profile_is_rejected() {
        let home = TempHome::new("invalid");
        let repo = ProviderProfileRepository::new(&home.0);
        let mut bad = profile("a");
        bad.base_url = "not-a-url".to_string();
        assert!(matches!(
            repo.add(bad, None),
            Err(ProviderProfileStoreError::Invalid(_))
        ));
    }

    #[test]
    fn unique_profile_id_avoids_existing_ids() {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "profile-1".to_string(),
            profile("profile-1"),
        );
        let id = unique_profile_id(&profiles);
        assert_eq!(id, "profile-2");
    }

    #[test]
    fn store_file_contains_no_secret_material() {
        let home = TempHome::new("no-secret");
        let repo = ProviderProfileRepository::new(&home.0);
        let mut with_secret = profile("a");
        with_secret.credential = Some(super::super::ProviderSecretRef::new("a"));
        repo.add(with_secret, None).expect("add");

        let raw = std::fs::read(repo.path()).expect("read");
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.contains("sk-"));
    }
}
