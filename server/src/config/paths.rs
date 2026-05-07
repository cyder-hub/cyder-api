#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use cyder_tools::log::warn;

use super::persistence::{self, PersistencePaths, ResolvedPathSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    pub default_config_path: PathBuf,
    pub user_config_path: PathBuf,
    pub user_config_path_required: bool,
    pub override_config_path: PathBuf,
    pub override_history_path: PathBuf,
    pub persistence: PersistencePaths,
    pub ignored_empty_environment_variables: Vec<String>,
}

impl ConfigPaths {
    pub fn new(
        default_config_path: impl Into<PathBuf>,
        user_config_path: impl Into<PathBuf>,
        override_config_path: impl Into<PathBuf>,
        override_history_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            default_config_path: default_config_path.into(),
            user_config_path: user_config_path.into(),
            user_config_path_required: false,
            override_config_path: override_config_path.into(),
            override_history_path: override_history_path.into(),
            persistence: default_release_persistence_paths(),
            ignored_empty_environment_variables: Vec::new(),
        }
    }

    pub fn for_current_build() -> Self {
        let paths = Self::from_resolved_path_set(persistence::resolve_current_path_set());
        for name in &paths.ignored_empty_environment_variables {
            warn!("ignoring empty environment variable {name}; treating it as unset");
        }
        paths
    }

    fn from_resolved_path_set(resolved: ResolvedPathSet) -> Self {
        Self {
            default_config_path: resolved.default_config_path,
            user_config_path: resolved.user_config_path,
            user_config_path_required: resolved.user_config_path_required,
            override_config_path: resolved.override_config_path,
            override_history_path: resolved.override_history_path,
            persistence: resolved.persistence,
            ignored_empty_environment_variables: resolved.ignored_empty_environment_variables,
        }
    }

    #[cfg(test)]
    pub fn for_test(repo_root: impl AsRef<Path>) -> Self {
        let repo_root = repo_root.as_ref();
        Self::from_resolved_path_set(persistence::resolve_path_set(
            persistence::PersistenceEnvironment::default(),
            persistence::BuildProfile::Debug,
            repo_root.to_path_buf(),
            repo_root.join(".cyder").join("dev"),
        ))
    }
}

fn default_release_persistence_paths() -> PersistencePaths {
    persistence::resolve_path_set(
        persistence::PersistenceEnvironment::default(),
        persistence::BuildProfile::Release,
        PathBuf::from("."),
        PathBuf::from(".cyder").join("dev"),
    )
    .persistence
}
