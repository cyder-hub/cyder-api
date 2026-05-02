use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    pub default_config_path: PathBuf,
    pub user_config_path: PathBuf,
    pub override_config_path: PathBuf,
    pub override_history_path: PathBuf,
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
            override_config_path: override_config_path.into(),
            override_history_path: override_history_path.into(),
        }
    }

    pub fn for_current_build() -> Self {
        if cfg!(debug_assertions) {
            let user_config_path = if Path::new("../config.local.yaml").exists() {
                "../config.local.yaml"
            } else {
                "../config.yaml"
            };

            Self::new(
                "../config.default.yaml",
                user_config_path,
                "../config.override.yaml",
                "../config.override.history.jsonl",
            )
        } else {
            Self::new(
                "config.default.yaml",
                "config.yaml",
                "config.override.yaml",
                "config.override.history.jsonl",
            )
        }
    }

    #[cfg(test)]
    pub fn for_test(base_dir: impl AsRef<Path>) -> Self {
        let base_dir = base_dir.as_ref();
        let local_config_path = base_dir.join("config.local.yaml");
        let user_config_path = if local_config_path.exists() {
            local_config_path
        } else {
            base_dir.join("config.yaml")
        };

        Self::new(
            base_dir.join("config.default.yaml"),
            user_config_path,
            base_dir.join("config.override.yaml"),
            base_dir.join("config.override.history.jsonl"),
        )
    }
}
