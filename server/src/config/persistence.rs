use std::{
    ffi::OsString,
    fmt, fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use cyder_tools::log::warn;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use super::{SecretEncryptionConfig, paths::ConfigPaths, programmatic_default_config_for_paths};

pub const DEFAULT_CONFIG_SNAPSHOT_VERSION: u32 = 2;

pub const CYDER_DATA_DIR_ENV: &str = "CYDER_DATA_DIR";
pub const CYDER_CONFIG_PATH_ENV: &str = "CYDER_CONFIG_PATH";

const DEFAULT_RELEASE_DATA_DIR: &str = "/data/cyder";
const CONTAINER_TMP_DIR: &str = "/tmp/cyder-api";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildProfile {
    Debug,
    Release,
}

impl BuildProfile {
    pub fn current() -> Self {
        if cfg!(debug_assertions) {
            Self::Debug
        } else {
            Self::Release
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistencePaths {
    pub data_dir: Option<PathBuf>,
    pub config_dir: PathBuf,
    pub db_dir: PathBuf,
    pub sqlite_db_path: PathBuf,
    pub tmp_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPathSet {
    pub default_config_path: PathBuf,
    pub user_config_path: PathBuf,
    pub user_config_path_required: bool,
    pub persistence: PersistencePaths,
    pub ignored_empty_environment_variables: Vec<String>,
}

#[derive(Debug)]
pub enum ConfigBootstrapError {
    SerializeDefault(serde_yaml::Error),
    InvalidDefaultSnapshot(String),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for ConfigBootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SerializeDefault(source) => {
                write!(f, "failed to serialize default configuration: {source}")
            }
            Self::InvalidDefaultSnapshot(source) => {
                write!(
                    f,
                    "invalid generated default configuration snapshot: {source}"
                )
            }
            Self::Io {
                operation,
                path,
                source,
            } => write!(f, "failed to {operation} '{}': {source}", path.display()),
        }
    }
}

impl std::error::Error for ConfigBootstrapError {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersistenceEnvironment {
    data_dir: Option<OsString>,
    config_path: Option<OsString>,
}

impl PersistenceEnvironment {
    pub fn current() -> Self {
        Self {
            data_dir: std::env::var_os(CYDER_DATA_DIR_ENV),
            config_path: std::env::var_os(CYDER_CONFIG_PATH_ENV),
        }
    }

    #[cfg(test)]
    pub fn from_values(data_dir: Option<&str>, config_path: Option<&str>) -> Self {
        Self {
            data_dir: data_dir.map(OsString::from),
            config_path: config_path.map(OsString::from),
        }
    }
}

pub fn resolve_current_path_set() -> ResolvedPathSet {
    resolve_path_set(
        PersistenceEnvironment::current(),
        BuildProfile::current(),
        current_dir(),
        default_debug_data_dir(),
    )
}

pub fn bootstrap_config_paths(paths: &ConfigPaths) -> Result<(), ConfigBootstrapError> {
    if let Some(data_dir) = paths.persistence.data_dir.as_ref() {
        create_dir(data_dir, "create data directory")?;
    }
    create_dir(&paths.persistence.config_dir, "create config directory")?;
    create_dir(&paths.persistence.tmp_dir, "create temporary directory")?;

    warn_about_retired_managed_config_files(&paths.persistence.config_dir);
    create_default_config_if_missing(paths)
}

fn warn_about_retired_managed_config_files(config_dir: &Path) {
    for file_name in ["config.override.yaml", "config.override.history.jsonl"] {
        let path = config_dir.join(file_name);
        if path.exists() {
            warn!(
                "ignoring retired managed configuration file '{}'; move any required settings to config.yaml and restart",
                path.display()
            );
        }
    }
}

fn create_default_config_if_missing(paths: &ConfigPaths) -> Result<(), ConfigBootstrapError> {
    if paths.default_config_path.exists() {
        ensure_existing_file(
            &paths.default_config_path,
            "create default configuration file",
        )?;
        return refresh_default_config_snapshot_if_stale(paths);
    }

    let config = programmatic_default_config_for_paths(paths);
    let yaml = serialize_default_config_snapshot(&config, None)?;
    create_file_if_missing(
        &paths.default_config_path,
        "create default configuration file",
        yaml.as_bytes(),
    )
}

fn refresh_default_config_snapshot_if_stale(
    paths: &ConfigPaths,
) -> Result<(), ConfigBootstrapError> {
    let raw = fs::read_to_string(&paths.default_config_path).map_err(|source| {
        ConfigBootstrapError::Io {
            operation: "read default configuration snapshot",
            path: paths.default_config_path.clone(),
            source,
        }
    })?;
    let value: serde_yaml::Value = serde_yaml::from_str(&raw)
        .map_err(|error| ConfigBootstrapError::InvalidDefaultSnapshot(error.to_string()))?;
    let version = value
        .get("config_snapshot_version")
        .and_then(serde_yaml::Value::as_u64)
        .map(|version| version as u32);
    if version == Some(DEFAULT_CONFIG_SNAPSHOT_VERSION) {
        return Ok(());
    }
    if version.is_some_and(|version| version > DEFAULT_CONFIG_SNAPSHOT_VERSION) {
        return Err(ConfigBootstrapError::InvalidDefaultSnapshot(format!(
            "snapshot version {} is newer than supported version {}",
            version.unwrap_or_default(),
            DEFAULT_CONFIG_SNAPSHOT_VERSION
        )));
    }

    let managed_secret = managed_secret_snapshot(&value)?;
    let config = programmatic_default_config_for_paths(paths);
    let yaml = serialize_default_config_snapshot(&config, managed_secret.as_ref())?;
    replace_file_atomically(
        &paths.default_config_path,
        "replace stale default configuration snapshot",
        yaml.as_bytes(),
    )
}

fn managed_secret_snapshot(
    value: &serde_yaml::Value,
) -> Result<Option<serde_yaml::Mapping>, ConfigBootstrapError> {
    let Some(secret_value) = value.get("secret_encryption") else {
        return Ok(None);
    };
    let _: SecretEncryptionConfig =
        serde_yaml::from_value(secret_value.clone()).map_err(|error| {
            ConfigBootstrapError::InvalidDefaultSnapshot(format!(
                "secret_encryption cannot be safely carried forward: {error}"
            ))
        })?;
    let Some(secret_map) = secret_value.as_mapping() else {
        return Err(ConfigBootstrapError::InvalidDefaultSnapshot(
            "secret_encryption must be a mapping".to_string(),
        ));
    };
    let mut managed = serde_yaml::Mapping::new();
    for field in ["encryption_key", "previous_encryption_key"] {
        let key = serde_yaml::Value::String(field.to_string());
        if let Some(raw_value) = secret_map.get(&key) {
            if !raw_value.is_null() && raw_value.as_str().is_none() {
                return Err(ConfigBootstrapError::InvalidDefaultSnapshot(format!(
                    "secret_encryption.{field} must be a string or null"
                )));
            }
            managed.insert(key, raw_value.clone());
        }
    }
    Ok(Some(managed))
}

fn serialize_default_config_snapshot(
    config: &super::FinalConfig,
    managed_secret: Option<&serde_yaml::Mapping>,
) -> Result<String, ConfigBootstrapError> {
    let mut value = serde_yaml::to_value(config).map_err(ConfigBootstrapError::SerializeDefault)?;
    let Some(root) = value.as_mapping_mut() else {
        return Err(ConfigBootstrapError::InvalidDefaultSnapshot(
            "program defaults did not serialize to a mapping".to_string(),
        ));
    };
    root.insert(
        serde_yaml::Value::String("config_snapshot_version".to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(DEFAULT_CONFIG_SNAPSHOT_VERSION)),
    );
    if let Some(managed_secret) = managed_secret {
        root.insert(
            serde_yaml::Value::String("secret_encryption".to_string()),
            serde_yaml::Value::Mapping(managed_secret.clone()),
        );
    }
    serde_yaml::to_string(&value).map_err(ConfigBootstrapError::SerializeDefault)
}

fn replace_file_atomically(
    path: &Path,
    operation: &'static str,
    bytes: &[u8],
) -> Result<(), ConfigBootstrapError> {
    if let Some(parent) = path.parent() {
        create_dir(parent, "create parent directory")?;
    }
    let temp_path = unique_temp_path_for(path);
    if let Err(error) = write_new_file(&temp_path, operation, bytes) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    match fs::rename(&temp_path, path) {
        Ok(()) => {
            if let Some(parent) = path.parent() {
                sync_parent_dir(parent);
            }
            Ok(())
        }
        Err(source) => {
            let _ = fs::remove_file(&temp_path);
            Err(ConfigBootstrapError::Io {
                operation,
                path: path.to_path_buf(),
                source,
            })
        }
    }
}

fn create_dir(path: &Path, operation: &'static str) -> Result<(), ConfigBootstrapError> {
    if path.as_os_str().is_empty() {
        return Ok(());
    }

    fs::create_dir_all(path).map_err(|source| ConfigBootstrapError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    })
}

fn create_file_if_missing(
    path: &Path,
    operation: &'static str,
    bytes: &[u8],
) -> Result<(), ConfigBootstrapError> {
    create_file_if_missing_inner(path, operation, bytes, false)
}

fn create_file_if_missing_inner(
    path: &Path,
    operation: &'static str,
    bytes: &[u8],
    fail_before_install: bool,
) -> Result<(), ConfigBootstrapError> {
    if path.exists() {
        return ensure_existing_file(path, operation);
    }

    if let Some(parent) = path.parent() {
        create_dir(parent, "create parent directory")?;
    }

    let temp_path = unique_temp_path_for(path);
    let write_result = write_new_file(&temp_path, operation, bytes);
    if let Err(err) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    if fail_before_install {
        let _ = fs::remove_file(&temp_path);
        return Err(ConfigBootstrapError::Io {
            operation: "install bootstrapped configuration file",
            path: path.to_path_buf(),
            source: io::Error::other("simulated bootstrap file install failure"),
        });
    }

    if path.exists() {
        let _ = fs::remove_file(&temp_path);
        return ensure_existing_file(path, operation);
    }

    match fs::rename(&temp_path, path) {
        Ok(()) => {
            if let Some(parent) = path.parent() {
                sync_parent_dir(parent);
            }
            Ok(())
        }
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temp_path);
            Ok(())
        }
        Err(source) => {
            let _ = fs::remove_file(&temp_path);
            Err(ConfigBootstrapError::Io {
                operation,
                path: path.to_path_buf(),
                source,
            })
        }
    }
}

fn ensure_existing_file(path: &Path, operation: &'static str) -> Result<(), ConfigBootstrapError> {
    if path.is_file() {
        return Ok(());
    }
    Err(ConfigBootstrapError::Io {
        operation,
        path: path.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "path exists but is not a file",
        ),
    })
}

fn write_new_file(
    path: &Path,
    operation: &'static str,
    bytes: &[u8],
) -> Result<(), ConfigBootstrapError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);

    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => return Ok(()),
        Err(source) => {
            return Err(ConfigBootstrapError::Io {
                operation,
                path: path.to_path_buf(),
                source,
            });
        }
    };

    if let Err(source) = set_owner_read_write_permissions(path) {
        return Err(ConfigBootstrapError::Io {
            operation: "set owner read/write file permissions",
            path: path.to_path_buf(),
            source,
        });
    }

    file.write_all(bytes)
        .map_err(|source| ConfigBootstrapError::Io {
            operation,
            path: path.to_path_buf(),
            source,
        })?;
    file.sync_all().map_err(|source| ConfigBootstrapError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    })
}

fn unique_temp_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("config");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    path.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), nanos))
}

fn sync_parent_dir(parent: &Path) {
    if let Ok(dir) = fs::File::open(parent) {
        let _ = dir.sync_all();
    }
}

#[cfg(unix)]
pub(crate) fn set_owner_read_write_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
pub(crate) fn set_owner_read_write_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

pub fn resolve_path_set(
    environment: PersistenceEnvironment,
    profile: BuildProfile,
    current_dir: PathBuf,
    debug_data_dir: PathBuf,
) -> ResolvedPathSet {
    let mut ignored_empty_environment_variables = Vec::new();
    let data_dir = non_empty_env_path(
        environment.data_dir,
        CYDER_DATA_DIR_ENV,
        &mut ignored_empty_environment_variables,
    )
    .map(|path| absolutize_path(path, &current_dir));
    let user_config_override = non_empty_env_path(
        environment.config_path,
        CYDER_CONFIG_PATH_ENV,
        &mut ignored_empty_environment_variables,
    )
    .map(|path| absolutize_path(path, &current_dir));

    if let Some(data_dir) = data_dir {
        return data_dir_path_set(
            data_dir,
            user_config_override,
            PathBuf::from(CONTAINER_TMP_DIR),
            ignored_empty_environment_variables,
        );
    }

    if profile == BuildProfile::Debug {
        let debug_data_dir = normalize_path(debug_data_dir);
        return data_dir_path_set(
            debug_data_dir.clone(),
            user_config_override,
            debug_data_dir.join("tmp"),
            ignored_empty_environment_variables,
        );
    }

    data_dir_path_set(
        PathBuf::from(DEFAULT_RELEASE_DATA_DIR),
        user_config_override,
        PathBuf::from(CONTAINER_TMP_DIR),
        ignored_empty_environment_variables,
    )
}

fn data_dir_path_set(
    data_dir: PathBuf,
    user_config_override: Option<PathBuf>,
    tmp_dir: PathBuf,
    ignored_empty_environment_variables: Vec<String>,
) -> ResolvedPathSet {
    let config_dir = data_dir.join("config");
    let db_dir = data_dir.join("db");
    let user_config_path_required = user_config_override.is_some();

    ResolvedPathSet {
        default_config_path: config_dir.join("config.default.yaml"),
        user_config_path: user_config_override.unwrap_or_else(|| config_dir.join("config.yaml")),
        user_config_path_required,
        persistence: PersistencePaths {
            data_dir: Some(data_dir),
            config_dir,
            db_dir: db_dir.clone(),
            sqlite_db_path: db_dir.join("cyder.sqlite"),
            tmp_dir,
        },
        ignored_empty_environment_variables,
    }
}

fn non_empty_env_path(
    value: Option<OsString>,
    name: &'static str,
    ignored_empty_environment_variables: &mut Vec<String>,
) -> Option<PathBuf> {
    let value = value?;
    if value.as_os_str().is_empty() {
        ignored_empty_environment_variables.push(name.to_string());
        return None;
    }
    Some(PathBuf::from(value))
}

fn absolutize_path(path: PathBuf, current_dir: &Path) -> PathBuf {
    if path.is_absolute() {
        normalize_path(path)
    } else {
        normalize_path(current_dir.join(path))
    }
}

fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn default_debug_data_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = if let Some(parent) = manifest_dir.parent() {
        parent.to_path_buf()
    } else {
        manifest_dir
    };
    repo_root.join(".cyder").join("dev")
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::config::{
        loader::{ConfigLoadOptions, load_effective_config},
        paths::ConfigPaths,
    };

    const TEST_ENCRYPTION_KEY: &str =
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn write_test_config(path: &Path, yaml: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("config parent should be created");
        }
        fs::write(path, yaml).expect("config file should be written");
    }

    #[test]
    fn data_dir_derives_container_paths() {
        let resolved = resolve_path_set(
            PersistenceEnvironment::from_values(Some("/data/cyder"), None),
            BuildProfile::Release,
            PathBuf::from("/work"),
            PathBuf::from("/repo/.cyder/dev"),
        );

        assert_eq!(
            resolved.default_config_path,
            PathBuf::from("/data/cyder/config/config.default.yaml")
        );
        assert_eq!(
            resolved.user_config_path,
            PathBuf::from("/data/cyder/config/config.yaml")
        );
        assert!(!resolved.user_config_path_required);
        assert_eq!(
            resolved.persistence.sqlite_db_path,
            PathBuf::from("/data/cyder/db/cyder.sqlite")
        );
    }

    #[test]
    fn config_path_only_overrides_user_config_path() {
        let resolved = resolve_path_set(
            PersistenceEnvironment::from_values(
                Some("/data/cyder"),
                Some("/etc/cyder/config.yaml"),
            ),
            BuildProfile::Release,
            PathBuf::from("/work"),
            PathBuf::from("/repo/.cyder/dev"),
        );

        assert_eq!(
            resolved.user_config_path,
            PathBuf::from("/etc/cyder/config.yaml")
        );
        assert!(resolved.user_config_path_required);
        assert_eq!(
            resolved.default_config_path,
            PathBuf::from("/data/cyder/config/config.default.yaml")
        );
    }

    #[test]
    fn debug_without_data_dir_uses_dev_data_dir() {
        let resolved = resolve_path_set(
            PersistenceEnvironment::default(),
            BuildProfile::Debug,
            PathBuf::from("/work"),
            PathBuf::from("/repo/.cyder/dev"),
        );

        assert_eq!(
            resolved.user_config_path,
            PathBuf::from("/repo/.cyder/dev/config/config.yaml")
        );
        assert!(!resolved.user_config_path_required);
        assert_eq!(
            resolved.default_config_path,
            PathBuf::from("/repo/.cyder/dev/config/config.default.yaml")
        );
        assert_eq!(
            resolved.persistence.sqlite_db_path,
            PathBuf::from("/repo/.cyder/dev/db/cyder.sqlite")
        );
    }

    #[test]
    fn debug_default_ignores_root_config_files() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        fs::write(temp_dir.path().join("config.local.yaml"), "port: 1111\n")
            .expect("local config should be written");
        fs::write(temp_dir.path().join("config.yaml"), "port: 2222\n")
            .expect("root config should be written");
        let debug_data_dir = temp_dir.path().join(".cyder").join("dev");

        let resolved = resolve_path_set(
            PersistenceEnvironment::default(),
            BuildProfile::Debug,
            temp_dir.path().to_path_buf(),
            debug_data_dir.clone(),
        );

        assert_eq!(
            resolved.user_config_path,
            debug_data_dir.join("config").join("config.yaml")
        );
    }

    #[test]
    fn debug_bootstrap_uses_dev_data_dir_and_leaves_root_configs_unused() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        fs::write(temp_dir.path().join("config.local.yaml"), "port: 1111\n")
            .expect("local config should be written");
        fs::write(temp_dir.path().join("config.yaml"), "port: 2222\n")
            .expect("root config should be written");
        let debug_data_dir = temp_dir.path().join(".cyder").join("dev");
        let resolved = resolve_path_set(
            PersistenceEnvironment::default(),
            BuildProfile::Debug,
            temp_dir.path().to_path_buf(),
            debug_data_dir.clone(),
        );
        let paths = ConfigPaths {
            default_config_path: resolved.default_config_path,
            user_config_path: resolved.user_config_path,
            user_config_path_required: resolved.user_config_path_required,
            persistence: resolved.persistence,
            ignored_empty_environment_variables: resolved.ignored_empty_environment_variables,
        };

        bootstrap_config_paths(&paths).expect("bootstrap should succeed");
        write_test_config(
            &paths.user_config_path,
            &format!("secret_encryption:\n  encryption_key: '{TEST_ENCRYPTION_KEY}'\n"),
        );
        let loaded = load_effective_config(
            &paths,
            ConfigLoadOptions {
                include_environment: false,
            },
        )
        .expect("config should load");

        assert_eq!(loaded.port, 8000);
        assert!(
            debug_data_dir
                .join("config")
                .join("config.default.yaml")
                .is_file()
        );
        assert!(!temp_dir.path().join("config.default.yaml").exists());
    }

    #[test]
    fn relative_env_paths_are_resolved_to_absolute_paths() {
        let resolved = resolve_path_set(
            PersistenceEnvironment::from_values(
                Some("cyder-data/../cyder-data"),
                Some("configs/config.yaml"),
            ),
            BuildProfile::Release,
            PathBuf::from("/work/app"),
            PathBuf::from("/repo/.cyder/dev"),
        );

        assert_eq!(
            resolved.default_config_path,
            PathBuf::from("/work/app/cyder-data/config/config.default.yaml")
        );
        assert_eq!(
            resolved.user_config_path,
            PathBuf::from("/work/app/configs/config.yaml")
        );
        assert!(resolved.user_config_path_required);
    }

    #[test]
    fn empty_env_vars_are_recorded_and_treated_as_unset() {
        let resolved = resolve_path_set(
            PersistenceEnvironment::from_values(Some(""), Some("")),
            BuildProfile::Debug,
            PathBuf::from("/work/app"),
            PathBuf::from("/repo/.cyder/dev"),
        );

        assert_eq!(
            resolved.ignored_empty_environment_variables,
            vec![
                CYDER_DATA_DIR_ENV.to_string(),
                CYDER_CONFIG_PATH_ENV.to_string()
            ]
        );
        assert_eq!(
            resolved.user_config_path,
            PathBuf::from("/repo/.cyder/dev/config/config.yaml")
        );
        assert!(!resolved.user_config_path_required);
    }

    #[test]
    fn release_without_data_dir_uses_default_data_dir() {
        let resolved = resolve_path_set(
            PersistenceEnvironment::default(),
            BuildProfile::Release,
            PathBuf::from("/work/app"),
            PathBuf::from("/repo/.cyder/dev"),
        );

        assert_eq!(
            resolved.default_config_path,
            PathBuf::from("/data/cyder/config/config.default.yaml")
        );
        assert_eq!(
            resolved.user_config_path,
            PathBuf::from("/data/cyder/config/config.yaml")
        );
        assert!(!resolved.user_config_path_required);
        assert_eq!(
            resolved.persistence.data_dir,
            Some(PathBuf::from("/data/cyder"))
        );
        assert_eq!(
            resolved.persistence.sqlite_db_path,
            PathBuf::from("/data/cyder/db/cyder.sqlite")
        );
    }

    #[test]
    fn release_empty_data_dir_env_uses_default_data_dir_without_root_fallback() {
        let resolved = resolve_path_set(
            PersistenceEnvironment::from_values(Some(""), None),
            BuildProfile::Release,
            PathBuf::from("/work/app"),
            PathBuf::from("/repo/.cyder/dev"),
        );

        assert_eq!(
            resolved.ignored_empty_environment_variables,
            vec![CYDER_DATA_DIR_ENV.to_string()]
        );
        assert_eq!(
            resolved.default_config_path,
            PathBuf::from("/data/cyder/config/config.default.yaml")
        );
        assert_eq!(
            resolved.user_config_path,
            PathBuf::from("/data/cyder/config/config.yaml")
        );
        assert!(!resolved.user_config_path_required);
        assert_eq!(
            resolved.persistence.data_dir,
            Some(PathBuf::from("/data/cyder"))
        );
        assert_eq!(
            resolved.persistence.sqlite_db_path,
            PathBuf::from("/data/cyder/db/cyder.sqlite")
        );
    }

    #[test]
    fn bootstrap_creates_persistent_files_and_directories() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());

        bootstrap_config_paths(&paths).expect("bootstrap should succeed");

        assert!(paths.default_config_path.is_file());
        assert!(!paths.persistence.db_dir.exists());
        assert!(paths.persistence.tmp_dir.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn bootstrap_creates_configuration_files_with_owner_read_write_permissions() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());

        bootstrap_config_paths(&paths).expect("bootstrap should succeed");

        for path in [&paths.default_config_path] {
            let mode = fs::metadata(path)
                .unwrap_or_else(|err| panic!("{} metadata should read: {err}", path.display()))
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "{} should be owner read/write", path.display());
        }
    }

    #[test]
    fn bootstrap_does_not_overwrite_existing_default_config() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let existing = "config_snapshot_version: 2\nport: 9123\n";
        write_test_config(&paths.default_config_path, existing);

        bootstrap_config_paths(&paths).expect("bootstrap should succeed");

        assert_eq!(
            fs::read_to_string(&paths.default_config_path).expect("default should read"),
            existing
        );
    }

    #[test]
    fn bootstrap_rebuilds_old_default_snapshot_with_current_defaults_and_preserves_only_managed_secrets()
     {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let key = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        write_test_config(
            &paths.default_config_path,
            &format!(
                "proxy_request:\n  connect_timeout_seconds: 10\n  first_byte_timeout_seconds: 60\n  total_timeout_seconds: null\n  upstream_error_body_limit_bytes: 65536\nsecret_encryption:\n  encryption_key: '{key}'\n"
            ),
        );

        bootstrap_config_paths(&paths).expect("old default snapshot should be rebuilt");
        let rebuilt = fs::read_to_string(&paths.default_config_path)
            .expect("rebuilt default snapshot should read");
        assert!(rebuilt.contains("config_snapshot_version: 2"));
        assert!(rebuilt.contains("request_send_seconds: 7200"));
        assert!(rebuilt.contains("response_idle_seconds: 7200"));
        assert!(rebuilt.contains("total_seconds: 7200"));
        assert!(rebuilt.contains(key));
        assert!(!rebuilt.contains("connect_timeout_seconds: 10\n  first_byte_timeout_seconds"));
        assert!(!rebuilt.contains("first_byte_timeout_seconds"));
        assert!(!rebuilt.contains("\n  total_timeout_seconds:"));
    }

    #[test]
    fn bootstrap_rejects_invalid_old_snapshot_without_overwriting_the_original_file() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        let original = "secret_encryption:\n  encryption_key: 123\n";
        write_test_config(&paths.default_config_path, original);

        let error = bootstrap_config_paths(&paths)
            .expect_err("invalid managed secret should block snapshot rebuild");
        assert!(error.to_string().contains("secret_encryption"));
        assert_eq!(
            fs::read_to_string(&paths.default_config_path).expect("original snapshot should read"),
            original
        );
    }

    #[test]
    fn bootstrap_rejects_existing_configuration_path_that_is_not_a_file() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        fs::create_dir_all(&paths.default_config_path)
            .expect("default config directory should be created");

        let error = bootstrap_config_paths(&paths).expect_err("bootstrap should fail");
        let message = error.to_string();

        assert!(
            message.contains("create default configuration file"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains("path exists but is not a file"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains(&paths.default_config_path.display().to_string()),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn bootstrap_file_install_failure_removes_temporary_file_without_target() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let path = temp_dir.path().join("config.default.yaml");

        let error = create_file_if_missing_inner(
            &path,
            "create default configuration file",
            b"port: 9124\n",
            true,
        )
        .expect_err("simulated install failure should fail");
        let message = error.to_string();

        assert!(
            message.contains("install bootstrapped configuration file"),
            "unexpected error: {message}"
        );
        assert!(!path.exists(), "target file should not be left behind");
        let entries = fs::read_dir(temp_dir.path())
            .expect("temp dir should read")
            .collect::<Result<Vec<_>, _>>()
            .expect("entries should read");
        assert!(entries.is_empty(), "temporary files should be cleaned up");
    }

    #[test]
    fn loader_does_not_rewrite_bootstrapped_default_config() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = ConfigPaths::for_test(temp_dir.path());
        bootstrap_config_paths(&paths).expect("bootstrap should succeed");
        let before = fs::read_to_string(&paths.default_config_path).expect("default should read");
        write_test_config(
            &paths.user_config_path,
            &format!("secret_encryption:\n  encryption_key: '{TEST_ENCRYPTION_KEY}'\n"),
        );

        let first = load_effective_config(
            &paths,
            ConfigLoadOptions {
                include_environment: false,
            },
        )
        .expect("first load should succeed");
        let after_first =
            fs::read_to_string(&paths.default_config_path).expect("default should read");
        let second = load_effective_config(
            &paths,
            ConfigLoadOptions {
                include_environment: false,
            },
        )
        .expect("second load should succeed");
        let after_second =
            fs::read_to_string(&paths.default_config_path).expect("default should read");

        assert_eq!(before, after_first);
        assert_eq!(before, after_second);
        assert_eq!(first.jwt_secret, second.jwt_secret);
        assert_eq!(first.db_url, second.db_url);
    }

    #[test]
    fn bootstrap_error_includes_operation_and_path() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let mut paths = ConfigPaths::for_test(temp_dir.path());
        let blocked = temp_dir.path().join("blocked-tmp-dir");
        fs::write(&blocked, "not a directory").expect("blocking file should be written");
        paths.persistence.tmp_dir = blocked.clone();

        let error = bootstrap_config_paths(&paths).expect_err("bootstrap should fail");
        let message = error.to_string();

        assert!(
            message.contains("create temporary directory"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains(&blocked.display().to_string()),
            "unexpected error: {message}"
        );
    }
}
