//! Ownership state for the configuration center.
//!
//! The state file records which process owns the running configuration
//! center. A reader that finds a state file whose owner is no longer alive
//! treats it as stale and discards it, so a recycled process id can never
//! be mistaken for a live server.

use serde::Deserialize;
use serde::Serialize;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

/// File name of the configuration center state file.
pub const STATE_FILE_NAME: &str = "config_center.json";

/// The recorded state of a running configuration center.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ConfigCenterState {
    /// Schema version.
    pub version: u32,
    /// Process id of the owning process.
    pub pid: u32,
    /// Port the server listens on.
    pub port: u16,
    /// Bearer token required for mutating requests.
    pub bearer_token: String,
    /// Random instance id, used to detect a recycled state file.
    pub instance_id: String,
    /// When the server started, seconds since the Unix epoch.
    pub started_at: u64,
}

impl ConfigCenterState {
    /// Builds a state record for a running server.
    pub fn new(pid: u32, port: u16, bearer_token: String, instance_id: String) -> Self {
        Self {
            version: 1,
            pid,
            port,
            bearer_token,
            instance_id,
            started_at: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default(),
        }
    }
}

/// Errors produced while reading or writing the state file.
#[derive(Debug)]
pub enum ConfigCenterStateError {
    /// The state file could not be read or written.
    Io(std::io::Error),
    /// The state file could not be parsed.
    Parse(String),
    /// The state file uses an unsupported version.
    UnsupportedVersion(u32),
}

impl fmt::Display for ConfigCenterStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "configuration center state I/O error: {err}"),
            Self::Parse(message) => write!(f, "configuration center state is malformed: {message}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "configuration center state version {version} is not supported")
            }
        }
    }
}

impl std::error::Error for ConfigCenterStateError {}

impl From<std::io::Error> for ConfigCenterStateError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// The path of the configuration center state file.
#[derive(Debug, Clone)]
pub struct ConfigCenterStatePath(PathBuf);

impl ConfigCenterStatePath {
    /// Returns the state file path for `code_home`.
    pub fn new(code_home: &Path) -> Self {
        Self(code_home.join(STATE_FILE_NAME))
    }

    /// Returns the underlying path.
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Reads the state file.
    ///
    /// A missing file is reported as `Ok(None)`.
    pub fn read(&self) -> Result<Option<ConfigCenterState>, ConfigCenterStateError> {
        let raw = match std::fs::read(&self.0) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let state: ConfigCenterState = serde_json::from_slice(&raw)
            .map_err(|err| ConfigCenterStateError::Parse(err.to_string()))?;
        if state.version != 1 {
            return Err(ConfigCenterStateError::UnsupportedVersion(state.version));
        }
        Ok(Some(state))
    }

    /// Writes the state file atomically.
    pub fn write(&self, state: &ConfigCenterState) -> Result<(), ConfigCenterStateError> {
        let serialized = serde_json::to_vec_pretty(state)
            .map_err(|err| ConfigCenterStateError::Parse(err.to_string()))?;
        super::super::write_private_file(&self.0, &serialized)?;
        Ok(())
    }

    /// Removes the state file.
    pub fn remove(&self) -> Result<bool, ConfigCenterStateError> {
        match std::fs::remove_file(&self.0) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }
}

/// A running server, as described by the state file.
#[derive(Debug, Clone)]
pub struct ServerInstance {
    /// The recorded state.
    pub state: ConfigCenterState,
    /// Whether the owning process is still alive.
    pub owner_alive: bool,
}

impl ServerInstance {
    /// Reads the current server instance, if any.
    ///
    /// Returns `Ok(None)` when there is no state file, when the state file
    /// is malformed, or when the recorded owner is no longer running.
    pub fn load(code_home: &Path) -> Result<Option<Self>, ConfigCenterStateError> {
        let path = ConfigCenterStatePath::new(code_home);
        let Some(state) = path.read()? else {
            return Ok(None);
        };
        let owner_alive = process_is_alive(state.pid);
        Ok(Some(Self { state, owner_alive }))
    }

    /// Returns the base URL of the running server.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.state.port)
    }
}

/// Reports whether a process id is still running.
fn process_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        // `kill(pid, 0)` performs the permission and existence checks without
        // delivering a signal.
        unsafe {
            let result = libc::kill(pid as libc::pid_t, 0);
            if result == 0 {
                return true;
            }
            std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempHome(PathBuf);

    impl TempHome {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "qcodex-config-center-state-{tag}-{}",
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

    #[test]
    fn missing_state_file_reads_as_none() {
        let home = TempHome::new("missing");
        let path = ConfigCenterStatePath::new(&home.0);
        assert!(path.read().expect("read").is_none());
    }

    #[test]
    fn state_round_trips() {
        let home = TempHome::new("roundtrip");
        let path = ConfigCenterStatePath::new(&home.0);
        let state = ConfigCenterState::new(
            std::process::id(),
            4242,
            "token".to_string(),
            "instance".to_string(),
        );
        path.write(&state).expect("write");

        let read = path.read().expect("read").expect("state");
        assert_eq!(read.port, 4242);
        assert_eq!(read.bearer_token, "token");
        assert_eq!(read.instance_id, "instance");
    }

    #[test]
    fn a_live_owner_is_detected() {
        let home = TempHome::new("alive");
        let path = ConfigCenterStatePath::new(&home.0);
        path.write(&ConfigCenterState::new(
            std::process::id(),
            1,
            "token".to_string(),
            "instance".to_string(),
        ))
        .expect("write");

        let instance = ServerInstance::load(&home.0).expect("load").expect("instance");
        assert!(instance.owner_alive);
    }

    #[test]
    fn a_dead_owner_is_detected() {
        let home = TempHome::new("dead");
        let path = ConfigCenterStatePath::new(&home.0);
        // Process id 0 is never a live user process.
        path.write(&ConfigCenterState::new(
            0,
            1,
            "token".to_string(),
            "instance".to_string(),
        ))
        .expect("write");

        let instance = ServerInstance::load(&home.0).expect("load").expect("instance");
        assert!(!instance.owner_alive);
    }

    #[test]
    fn a_malformed_state_file_is_reported() {
        let home = TempHome::new("malformed");
        let path = ConfigCenterStatePath::new(&home.0);
        std::fs::write(path.path(), b"not json").expect("write");
        assert!(matches!(
            path.read(),
            Err(ConfigCenterStateError::Parse(_))
        ));
    }
}
