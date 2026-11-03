use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum GoalStatus {
    Active,
    Paused,
    Complete,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct GoalRecord {
    pub(crate) objective: String,
    pub(crate) status: GoalStatus,
    #[serde(default)]
    pub(crate) token_budget: Option<i64>,
    #[serde(default)]
    pub(crate) blocker: Option<String>,
    #[serde(default)]
    pub(crate) blocker_count: u8,
}

pub(crate) fn load(code_home: &Path, session_id: Uuid) -> io::Result<Option<GoalRecord>> {
    let path = path(code_home, session_id);
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(io::Error::other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn save(code_home: &Path, session_id: Uuid, record: &GoalRecord) -> io::Result<()> {
    let path = path(code_home, session_id);
    let parent = path.parent().expect("goal path has parent");
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec(record).map_err(io::Error::other)?)?;
    fs::rename(temporary, path)
}

pub(crate) fn clear(code_home: &Path, session_id: Uuid) -> io::Result<()> {
    match fs::remove_file(path(code_home, session_id)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn path(code_home: &Path, session_id: Uuid) -> PathBuf {
    code_home.join("goals").join(format!("{session_id}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_goal_round_trips_and_clears() {
        let home = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let record = GoalRecord {
            objective: "ship /goal".to_string(),
            status: GoalStatus::Active,
            token_budget: None,
            blocker: None,
            blocker_count: 0,
        };
        save(home.path(), id, &record).unwrap();
        assert_eq!(load(home.path(), id).unwrap(), Some(record));
        clear(home.path(), id).unwrap();
        assert_eq!(load(home.path(), id).unwrap(), None);
    }

    #[test]
    fn saving_a_new_goal_replaces_prior_lifecycle_state() {
        let home = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let prior = GoalRecord {
            objective: "old objective".to_string(),
            status: GoalStatus::Blocked,
            token_budget: Some(100),
            blocker: Some("waiting on access".to_string()),
            blocker_count: 3,
        };
        let replacement = GoalRecord {
            objective: "new objective".to_string(),
            status: GoalStatus::Active,
            token_budget: None,
            blocker: None,
            blocker_count: 0,
        };

        save(home.path(), id, &prior).unwrap();
        save(home.path(), id, &replacement).unwrap();

        assert_eq!(load(home.path(), id).unwrap(), Some(replacement));
    }
}
