use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;

use bincode::{Decode, Encode};
use chrono::Utc;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::{DbResult, get_connection};
use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningPreset {
    Disabled,
    Enabled,
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
    Auto,
}

impl ReasoningPreset {
    pub const ALL: [ReasoningPreset; 7] = [
        ReasoningPreset::Disabled,
        ReasoningPreset::Enabled,
        ReasoningPreset::Low,
        ReasoningPreset::Medium,
        ReasoningPreset::High,
        ReasoningPreset::XHigh,
        ReasoningPreset::Auto,
    ];

    pub fn as_key(self) -> &'static str {
        match self {
            ReasoningPreset::Disabled => "disabled",
            ReasoningPreset::Enabled => "enabled",
            ReasoningPreset::Low => "low",
            ReasoningPreset::Medium => "medium",
            ReasoningPreset::High => "high",
            ReasoningPreset::XHigh => "xhigh",
            ReasoningPreset::Auto => "auto",
        }
    }

    pub fn canonical_suffix(self) -> &'static str {
        match self {
            ReasoningPreset::Disabled => "no-think",
            ReasoningPreset::Enabled => "think",
            ReasoningPreset::Low => "low",
            ReasoningPreset::Medium => "medium",
            ReasoningPreset::High => "high",
            ReasoningPreset::XHigh => "xhigh",
            ReasoningPreset::Auto => "auto",
        }
    }

    pub fn requires_reasoning(self) -> bool {
        !matches!(self, ReasoningPreset::Disabled)
    }

    pub fn allowed_operation_kinds(self) -> Vec<&'static str> {
        vec!["generation"]
    }

    pub fn metadata(self) -> ReasoningPresetMetadata {
        ReasoningPresetMetadata {
            preset_key: self.as_key().to_string(),
            suffix: self.canonical_suffix().to_string(),
            requires_reasoning: self.requires_reasoning(),
            allowed_operation_kinds: self
                .allowed_operation_kinds()
                .into_iter()
                .map(str::to_string)
                .collect(),
        }
    }
}

impl fmt::Display for ReasoningPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_key())
    }
}

impl FromStr for ReasoningPreset {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "disabled" => Ok(ReasoningPreset::Disabled),
            "enabled" => Ok(ReasoningPreset::Enabled),
            "low" => Ok(ReasoningPreset::Low),
            "medium" => Ok(ReasoningPreset::Medium),
            "high" => Ok(ReasoningPreset::High),
            "xhigh" | "x_high" => Ok(ReasoningPreset::XHigh),
            "auto" => Ok(ReasoningPreset::Auto),
            other => Err(format!("unknown reasoning preset key '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningPatchFamily {
    #[serde(rename = "openai_chat_reasoning_effort")]
    OpenAiChatReasoningEffort,
    #[serde(rename = "openai_responses_reasoning")]
    OpenAiResponsesReasoning,
    #[serde(rename = "deepseek_openai_reasoning")]
    DeepSeekOpenAiReasoning,
    AnthropicThinkingBudget,
    Gemini25ThinkingBudget,
    Gemini3ThinkingLevel,
    #[serde(rename = "siliconflow_openai_enable_thinking")]
    SiliconFlowOpenAiEnableThinking,
}

impl ReasoningPatchFamily {
    pub const ALL: [ReasoningPatchFamily; 7] = [
        ReasoningPatchFamily::OpenAiChatReasoningEffort,
        ReasoningPatchFamily::OpenAiResponsesReasoning,
        ReasoningPatchFamily::DeepSeekOpenAiReasoning,
        ReasoningPatchFamily::SiliconFlowOpenAiEnableThinking,
        ReasoningPatchFamily::AnthropicThinkingBudget,
        ReasoningPatchFamily::Gemini25ThinkingBudget,
        ReasoningPatchFamily::Gemini3ThinkingLevel,
    ];

    pub fn as_key(self) -> &'static str {
        match self {
            ReasoningPatchFamily::OpenAiChatReasoningEffort => "openai_chat_reasoning_effort",
            ReasoningPatchFamily::OpenAiResponsesReasoning => "openai_responses_reasoning",
            ReasoningPatchFamily::DeepSeekOpenAiReasoning => "deepseek_openai_reasoning",
            ReasoningPatchFamily::AnthropicThinkingBudget => "anthropic_thinking_budget",
            ReasoningPatchFamily::Gemini25ThinkingBudget => "gemini25_thinking_budget",
            ReasoningPatchFamily::Gemini3ThinkingLevel => "gemini3_thinking_level",
            ReasoningPatchFamily::SiliconFlowOpenAiEnableThinking => {
                "siliconflow_openai_enable_thinking"
            }
        }
    }

    pub fn unsupported_preset_reason(self, preset: ReasoningPreset) -> Option<&'static str> {
        match self {
            ReasoningPatchFamily::OpenAiChatReasoningEffort
            | ReasoningPatchFamily::OpenAiResponsesReasoning => match preset {
                ReasoningPreset::Auto => {
                    Some("OpenAI reasoning effort families do not define provider-managed auto")
                }
                _ => None,
            },
            ReasoningPatchFamily::DeepSeekOpenAiReasoning => match preset {
                ReasoningPreset::Disabled
                | ReasoningPreset::Enabled
                | ReasoningPreset::High
                | ReasoningPreset::XHigh => None,
                ReasoningPreset::Low | ReasoningPreset::Medium => {
                    Some("DeepSeek OpenAI reasoning only exposes enabled/high/xhigh strengths")
                }
                ReasoningPreset::Auto => {
                    Some("DeepSeek OpenAI reasoning does not define provider-managed auto")
                }
            },
            ReasoningPatchFamily::SiliconFlowOpenAiEnableThinking => match preset {
                ReasoningPreset::Disabled | ReasoningPreset::Enabled => None,
                ReasoningPreset::Low
                | ReasoningPreset::Medium
                | ReasoningPreset::High
                | ReasoningPreset::XHigh => {
                    Some("SiliconFlow OpenAI reasoning only supports enable_thinking on/off")
                }
                ReasoningPreset::Auto => {
                    Some("SiliconFlow OpenAI reasoning does not define provider-managed auto")
                }
            },
            ReasoningPatchFamily::AnthropicThinkingBudget => match preset {
                ReasoningPreset::Auto => {
                    Some("Anthropic thinking budget does not define provider-managed auto")
                }
                ReasoningPreset::XHigh => Some("Anthropic thinking budget does not define xhigh"),
                _ => None,
            },
            ReasoningPatchFamily::Gemini25ThinkingBudget => match preset {
                ReasoningPreset::XHigh => Some("Gemini 2.5 thinking budget does not define xhigh"),
                _ => None,
            },
            ReasoningPatchFamily::Gemini3ThinkingLevel => match preset {
                ReasoningPreset::Enabled | ReasoningPreset::Low | ReasoningPreset::High => None,
                ReasoningPreset::Disabled => {
                    Some("Gemini 3 thinking level does not support disabling thinking")
                }
                ReasoningPreset::Medium => Some("Gemini 3 thinking level only exposes low/high"),
                ReasoningPreset::XHigh => Some("Gemini 3 thinking level does not define xhigh"),
                ReasoningPreset::Auto => {
                    Some("Gemini 3 thinking level does not define provider-managed auto")
                }
            },
        }
    }

    pub fn supports_preset(self, preset: ReasoningPreset) -> bool {
        self.unsupported_preset_reason(preset).is_none()
    }

    pub fn supported_presets(self) -> Vec<ReasoningPreset> {
        ReasoningPreset::ALL
            .into_iter()
            .filter(|preset| self.supports_preset(*preset))
            .collect()
    }
}

impl fmt::Display for ReasoningPatchFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_key())
    }
}

impl FromStr for ReasoningPatchFamily {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "openai_chat_reasoning_effort" => Ok(Self::OpenAiChatReasoningEffort),
            "openai_responses_reasoning" => Ok(Self::OpenAiResponsesReasoning),
            "deepseek_openai_reasoning" | "deepseek_openai_thinking" => {
                Ok(Self::DeepSeekOpenAiReasoning)
            }
            "siliconflow_openai_enable_thinking"
            | "siliconflow_openai_reasoning"
            | "siliconflow_openai_thinking"
            | "siliconflow_enable_thinking" => Ok(Self::SiliconFlowOpenAiEnableThinking),
            "anthropic_thinking_budget" => Ok(Self::AnthropicThinkingBudget),
            "gemini25_thinking_budget" | "gemini_25_thinking_budget" => {
                Ok(Self::Gemini25ThinkingBudget)
            }
            "gemini3_thinking_level" | "gemini_3_thinking_level" => Ok(Self::Gemini3ThinkingLevel),
            other => Err(format!("unknown reasoning patch family key '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub struct ReasoningPresetMetadata {
    pub preset_key: String,
    pub suffix: String,
    pub requires_reasoning: bool,
    pub allowed_operation_kinds: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningConfigScope {
    Provider,
    Model,
}

impl ReasoningConfigScope {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::Model => "model",
        }
    }
}

impl fmt::Display for ReasoningConfigScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_key())
    }
}

impl FromStr for ReasoningConfigScope {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "provider" => Ok(Self::Provider),
            "model" => Ok(Self::Model),
            other => Err(format!("unknown reasoning config scope '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningConfigMode {
    Custom,
    Disabled,
}

impl ReasoningConfigMode {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::Custom => "custom",
            Self::Disabled => "disabled",
        }
    }
}

impl fmt::Display for ReasoningConfigMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_key())
    }
}

impl FromStr for ReasoningConfigMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "custom" => Ok(Self::Custom),
            "disabled" => Ok(Self::Disabled),
            other => Err(format!("unknown reasoning config mode '{other}'")),
        }
    }
}

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, serde::Serialize)]
    #[diesel(table_name = reasoning_config)]
    pub struct ReasoningConfig {
        pub id: i64,
        pub scope_kind: String,
        pub provider_id: Option<i64>,
        pub model_id: Option<i64>,
        pub mode: String,
        pub family_key: Option<String>,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Deserialize, Debug)]
    #[diesel(table_name = reasoning_config)]
    pub struct NewReasoningConfig {
        pub id: i64,
        pub scope_kind: String,
        pub provider_id: Option<i64>,
        pub model_id: Option<i64>,
        pub mode: String,
        pub family_key: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, serde::Serialize)]
    #[diesel(table_name = reasoning_config_preset)]
    pub struct ReasoningConfigPreset {
        pub id: i64,
        pub config_id: i64,
        pub preset_key: String,
        pub expose_in_models: bool,
        pub is_enabled: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Deserialize, Debug)]
    #[diesel(table_name = reasoning_config_preset)]
    pub struct NewReasoningConfigPreset {
        pub id: i64,
        pub config_id: i64,
        pub preset_key: String,
        pub expose_in_models: bool,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReasoningConfigPresetInput {
    pub preset_key: String,
    pub expose_in_models: bool,
    pub is_enabled: bool,
}

#[derive(Debug, Clone)]
struct NormalizedReasoningConfigPreset {
    preset: ReasoningPreset,
    expose_in_models: bool,
    is_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningConfigPresetView {
    pub preset: ReasoningConfigPreset,
    pub preset_key: ReasoningPreset,
    pub suffix: String,
    pub requires_reasoning: bool,
    pub allowed_operation_kinds: Vec<String>,
}

impl ReasoningConfigPresetView {
    fn from_row(row: ReasoningConfigPreset) -> DbResult<Self> {
        let preset_key = parse_preset_key(&row.preset_key)?;
        Ok(Self {
            preset: row,
            preset_key,
            suffix: preset_key.canonical_suffix().to_string(),
            requires_reasoning: preset_key.requires_reasoning(),
            allowed_operation_kinds: preset_key
                .allowed_operation_kinds()
                .into_iter()
                .map(str::to_string)
                .collect(),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningConfigWithPresets {
    pub config: ReasoningConfig,
    pub scope: ReasoningConfigScope,
    pub mode: ReasoningConfigMode,
    pub family: Option<ReasoningPatchFamily>,
    pub presets: Vec<ReasoningConfigPresetView>,
}

fn invalid_param(message: impl Into<String>) -> BaseError {
    BaseError::ParamInvalid(Some(message.into()))
}

fn database_config_error(message: impl Into<String>) -> BaseError {
    BaseError::DatabaseFatal(Some(message.into()))
}

fn parse_scope_key(value: &str) -> DbResult<ReasoningConfigScope> {
    ReasoningConfigScope::from_str(value).map_err(database_config_error)
}

fn parse_mode_key(value: &str) -> DbResult<ReasoningConfigMode> {
    ReasoningConfigMode::from_str(value).map_err(database_config_error)
}

fn parse_family_key(value: &str) -> DbResult<ReasoningPatchFamily> {
    ReasoningPatchFamily::from_str(value).map_err(database_config_error)
}

fn parse_preset_key(value: &str) -> DbResult<ReasoningPreset> {
    ReasoningPreset::from_str(value).map_err(database_config_error)
}

fn validate_family_key(value: &str) -> DbResult<ReasoningPatchFamily> {
    ReasoningPatchFamily::from_str(value).map_err(invalid_param)
}

fn validate_preset_key(value: &str) -> DbResult<ReasoningPreset> {
    ReasoningPreset::from_str(value).map_err(invalid_param)
}

fn validate_custom_presets(
    family: ReasoningPatchFamily,
    presets: &[ReasoningConfigPresetInput],
) -> DbResult<Vec<NormalizedReasoningConfigPreset>> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(presets.len());

    for input in presets {
        let preset = validate_preset_key(&input.preset_key)?;
        if !seen.insert(preset) {
            return Err(invalid_param(format!(
                "duplicate reasoning preset '{}'",
                preset.as_key()
            )));
        }
        if let Some(reason) = family.unsupported_preset_reason(preset) {
            return Err(invalid_param(format!(
                "reasoning family '{}' does not support preset '{}': {}",
                family.as_key(),
                preset.as_key(),
                reason
            )));
        }
        normalized.push(NormalizedReasoningConfigPreset {
            preset,
            expose_in_models: input.expose_in_models,
            is_enabled: input.is_enabled,
        });
    }

    Ok(normalized)
}

fn normalize_config(
    scope: ReasoningConfigScope,
    mode: ReasoningConfigMode,
    family_key: Option<&str>,
    presets: &[ReasoningConfigPresetInput],
) -> DbResult<(
    Option<ReasoningPatchFamily>,
    Vec<NormalizedReasoningConfigPreset>,
)> {
    if matches!(scope, ReasoningConfigScope::Provider)
        && matches!(mode, ReasoningConfigMode::Disabled)
    {
        return Err(invalid_param(
            "provider reasoning config does not support disabled mode",
        ));
    }

    match mode {
        ReasoningConfigMode::Custom => {
            let family_key = family_key.ok_or_else(|| {
                invalid_param("custom reasoning config requires a family_key".to_string())
            })?;
            let family = validate_family_key(family_key)?;
            let normalized = validate_custom_presets(family, presets)?;
            Ok((Some(family), normalized))
        }
        ReasoningConfigMode::Disabled => {
            if family_key.is_some() {
                return Err(invalid_param(
                    "disabled reasoning config must not include family_key",
                ));
            }
            if !presets.is_empty() {
                return Err(invalid_param(
                    "disabled reasoning config must not include preset rows",
                ));
            }
            Ok((None, Vec::new()))
        }
    }
}

fn map_write_error(action: &str, err: diesel::result::Error) -> BaseError {
    match err {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            info,
        ) => BaseError::DatabaseDup(Some(format!("{action}: {}", info.message()))),
        other => BaseError::DatabaseFatal(Some(format!("{action}: {other}"))),
    }
}

fn build_config_snapshot(
    config: ReasoningConfig,
    preset_rows: Vec<ReasoningConfigPreset>,
) -> DbResult<ReasoningConfigWithPresets> {
    let scope = parse_scope_key(&config.scope_kind)?;
    let mode = parse_mode_key(&config.mode)?;
    let family = match mode {
        ReasoningConfigMode::Custom => Some(parse_family_key(
            config.family_key.as_deref().ok_or_else(|| {
                database_config_error(format!(
                    "custom reasoning config {} is missing family_key",
                    config.id
                ))
            })?,
        )?),
        ReasoningConfigMode::Disabled => {
            if config.family_key.is_some() {
                return Err(database_config_error(format!(
                    "disabled reasoning config {} must not have family_key",
                    config.id
                )));
            }
            None
        }
    };

    let presets = preset_rows
        .into_iter()
        .map(ReasoningConfigPresetView::from_row)
        .collect::<DbResult<Vec<_>>>()?;

    if matches!(mode, ReasoningConfigMode::Disabled) && !presets.is_empty() {
        return Err(database_config_error(format!(
            "disabled reasoning config {} has active preset rows",
            config.id
        )));
    }

    Ok(ReasoningConfigWithPresets {
        config,
        scope,
        mode,
        family,
        presets,
    })
}

fn build_config_snapshots(
    configs: Vec<ReasoningConfig>,
    preset_rows: Vec<ReasoningConfigPreset>,
) -> DbResult<Vec<ReasoningConfigWithPresets>> {
    let mut presets_by_config: HashMap<i64, Vec<ReasoningConfigPreset>> = HashMap::new();
    for row in preset_rows {
        presets_by_config
            .entry(row.config_id)
            .or_default()
            .push(row);
    }

    configs
        .into_iter()
        .map(|config| {
            let rows = presets_by_config.remove(&config.id).unwrap_or_default();
            build_config_snapshot(config, rows)
        })
        .collect()
}

macro_rules! load_reasoning_config_snapshots {
    ($conn:expr, $configs:expr) => {{
        let configs = $configs;
        if configs.is_empty() {
            Ok(Vec::new())
        } else {
            let config_ids: Vec<i64> = configs.iter().map(|config| config.id).collect();
            let preset_rows = reasoning_config_preset::table
                .filter(
                    reasoning_config_preset::dsl::config_id
                        .eq_any(&config_ids)
                        .and(reasoning_config_preset::dsl::deleted_at.is_null()),
                )
                .order((
                    reasoning_config_preset::dsl::config_id.asc(),
                    reasoning_config_preset::dsl::preset_key.asc(),
                ))
                .select(ReasoningConfigPresetDb::as_select())
                .load::<ReasoningConfigPresetDb>($conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to list active reasoning config presets: {err}"
                    )))
                })?;
            let presets = preset_rows
                .into_iter()
                .map(ReasoningConfigPresetDb::from_db)
                .collect();
            build_config_snapshots(configs, presets)
        }
    }};
}

impl ReasoningConfig {
    pub fn upsert_provider_config(
        provider_id_value: i64,
        family_key_value: &str,
        presets: &[ReasoningConfigPresetInput],
    ) -> DbResult<ReasoningConfigWithPresets> {
        let (family, presets) = normalize_config(
            ReasoningConfigScope::Provider,
            ReasoningConfigMode::Custom,
            Some(family_key_value),
            presets,
        )?;
        let config_id = Self::upsert_owner_config(
            ReasoningConfigScope::Provider,
            provider_id_value,
            ReasoningConfigMode::Custom,
            family,
            &presets,
        )?;
        Self::get_active_by_id(config_id)?.ok_or_else(|| {
            BaseError::DatabaseFatal(Some(format!(
                "reasoning config {} disappeared after provider upsert",
                config_id
            )))
        })
    }

    pub fn upsert_model_config(
        model_id_value: i64,
        mode: ReasoningConfigMode,
        family_key_value: Option<&str>,
        presets: &[ReasoningConfigPresetInput],
    ) -> DbResult<ReasoningConfigWithPresets> {
        let (family, presets) =
            normalize_config(ReasoningConfigScope::Model, mode, family_key_value, presets)?;
        let config_id = Self::upsert_owner_config(
            ReasoningConfigScope::Model,
            model_id_value,
            mode,
            family,
            &presets,
        )?;
        Self::get_active_by_id(config_id)?.ok_or_else(|| {
            BaseError::DatabaseFatal(Some(format!(
                "reasoning config {} disappeared after model upsert",
                config_id
            )))
        })
    }

    pub fn delete_provider_config(provider_id_value: i64) -> DbResult<usize> {
        Self::delete_owner_config(ReasoningConfigScope::Provider, provider_id_value)
    }

    pub fn delete_model_config(model_id_value: i64) -> DbResult<usize> {
        Self::delete_owner_config(ReasoningConfigScope::Model, model_id_value)
    }

    pub fn get_active_provider_config(
        provider_id_value: i64,
    ) -> DbResult<Option<ReasoningConfigWithPresets>> {
        Self::get_active_by_owner(ReasoningConfigScope::Provider, provider_id_value)
    }

    pub fn get_active_model_config(
        model_id_value: i64,
    ) -> DbResult<Option<ReasoningConfigWithPresets>> {
        Self::get_active_by_owner(ReasoningConfigScope::Model, model_id_value)
    }

    pub fn list_active_with_presets() -> DbResult<Vec<ReasoningConfigWithPresets>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let config_rows = reasoning_config::table
                .filter(reasoning_config::dsl::deleted_at.is_null())
                .order((
                    reasoning_config::dsl::scope_kind.asc(),
                    reasoning_config::dsl::provider_id.asc(),
                    reasoning_config::dsl::model_id.asc(),
                    reasoning_config::dsl::id.asc(),
                ))
                .select(ReasoningConfigDb::as_select())
                .load::<ReasoningConfigDb>(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to list active reasoning configs: {err}"
                    )))
                })?;
            let configs: Vec<ReasoningConfig> = config_rows
                .into_iter()
                .map(ReasoningConfigDb::from_db)
                .collect();
            load_reasoning_config_snapshots!(conn, configs)
        })
    }

    pub fn list_active_provider_configs(
        provider_ids: &[i64],
    ) -> DbResult<Vec<ReasoningConfigWithPresets>> {
        if provider_ids.is_empty() {
            return Ok(Vec::new());
        }
        Self::list_active_by_owner_ids(ReasoningConfigScope::Provider, provider_ids)
    }

    pub fn list_active_model_configs(
        model_ids: &[i64],
    ) -> DbResult<Vec<ReasoningConfigWithPresets>> {
        if model_ids.is_empty() {
            return Ok(Vec::new());
        }
        Self::list_active_by_owner_ids(ReasoningConfigScope::Model, model_ids)
    }

    fn get_active_by_id(id_value: i64) -> DbResult<Option<ReasoningConfigWithPresets>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let config = reasoning_config::table
                .filter(
                    reasoning_config::dsl::id
                        .eq(id_value)
                        .and(reasoning_config::dsl::deleted_at.is_null()),
                )
                .select(ReasoningConfigDb::as_select())
                .first::<ReasoningConfigDb>(conn)
                .optional()
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to fetch reasoning config {id_value}: {err}"
                    )))
                })?
                .map(ReasoningConfigDb::from_db);

            match config {
                Some(config) => Ok(load_reasoning_config_snapshots!(conn, vec![config])?.pop()),
                None => Ok(None),
            }
        })
    }

    fn get_active_by_owner(
        scope: ReasoningConfigScope,
        owner_id: i64,
    ) -> DbResult<Option<ReasoningConfigWithPresets>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let mut query = reasoning_config::table
                .filter(
                    reasoning_config::dsl::scope_kind
                        .eq(scope.as_key())
                        .and(reasoning_config::dsl::deleted_at.is_null()),
                )
                .into_boxed();
            query = match scope {
                ReasoningConfigScope::Provider => {
                    query.filter(reasoning_config::dsl::provider_id.eq(owner_id))
                }
                ReasoningConfigScope::Model => {
                    query.filter(reasoning_config::dsl::model_id.eq(owner_id))
                }
            };
            let config = query
                .select(ReasoningConfigDb::as_select())
                .first::<ReasoningConfigDb>(conn)
                .optional()
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to fetch active {} reasoning config for owner {}: {err}",
                        scope.as_key(),
                        owner_id
                    )))
                })?
                .map(ReasoningConfigDb::from_db);

            match config {
                Some(config) => Ok(load_reasoning_config_snapshots!(conn, vec![config])?.pop()),
                None => Ok(None),
            }
        })
    }

    fn list_active_by_owner_ids(
        scope: ReasoningConfigScope,
        owner_ids: &[i64],
    ) -> DbResult<Vec<ReasoningConfigWithPresets>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let mut query = reasoning_config::table
                .filter(
                    reasoning_config::dsl::scope_kind
                        .eq(scope.as_key())
                        .and(reasoning_config::dsl::deleted_at.is_null()),
                )
                .into_boxed();
            query = match scope {
                ReasoningConfigScope::Provider => {
                    query.filter(reasoning_config::dsl::provider_id.eq_any(owner_ids))
                }
                ReasoningConfigScope::Model => {
                    query.filter(reasoning_config::dsl::model_id.eq_any(owner_ids))
                }
            };

            let config_rows = query
                .order((
                    reasoning_config::dsl::provider_id.asc(),
                    reasoning_config::dsl::model_id.asc(),
                    reasoning_config::dsl::id.asc(),
                ))
                .select(ReasoningConfigDb::as_select())
                .load::<ReasoningConfigDb>(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to list active {} reasoning configs: {err}",
                        scope.as_key()
                    )))
                })?;
            let configs: Vec<ReasoningConfig> = config_rows
                .into_iter()
                .map(ReasoningConfigDb::from_db)
                .collect();
            load_reasoning_config_snapshots!(conn, configs)
        })
    }

    fn upsert_owner_config(
        scope: ReasoningConfigScope,
        owner_id: i64,
        mode: ReasoningConfigMode,
        family: Option<ReasoningPatchFamily>,
        presets: &[NormalizedReasoningConfigPreset],
    ) -> DbResult<i64> {
        let now = Utc::now().timestamp_millis();
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            conn.transaction::<i64, BaseError, _>(|conn| {
                let mut query = reasoning_config::table
                    .filter(
                        reasoning_config::dsl::scope_kind
                            .eq(scope.as_key())
                            .and(reasoning_config::dsl::deleted_at.is_null()),
                    )
                    .into_boxed();
                query = match scope {
                    ReasoningConfigScope::Provider => {
                        query.filter(reasoning_config::dsl::provider_id.eq(owner_id))
                    }
                    ReasoningConfigScope::Model => {
                        query.filter(reasoning_config::dsl::model_id.eq(owner_id))
                    }
                };
                let existing = query
                    .select(ReasoningConfigDb::as_select())
                    .first::<ReasoningConfigDb>(conn)
                    .optional()
                    .map_err(|err| {
                        BaseError::DatabaseFatal(Some(format!(
                            "failed to fetch existing {} reasoning config for owner {}: {err}",
                            scope.as_key(),
                            owner_id
                        )))
                    })?
                    .map(ReasoningConfigDb::from_db);

                let family_key = family.map(|value| value.as_key().to_string());
                let config_id = if let Some(existing) = existing {
                    diesel::update(reasoning_config::table.find(existing.id))
                        .set((
                            reasoning_config::dsl::mode.eq(mode.as_key()),
                            reasoning_config::dsl::family_key.eq(family_key.clone()),
                            reasoning_config::dsl::updated_at.eq(now),
                        ))
                        .returning(reasoning_config::dsl::id)
                        .get_result::<i64>(conn)
                        .map_err(|err| map_write_error("failed to update reasoning config", err))?
                } else {
                    let new_config = NewReasoningConfig {
                        id: ID_GENERATOR.generate_id(),
                        scope_kind: scope.as_key().to_string(),
                        provider_id: matches!(scope, ReasoningConfigScope::Provider)
                            .then_some(owner_id),
                        model_id: matches!(scope, ReasoningConfigScope::Model).then_some(owner_id),
                        mode: mode.as_key().to_string(),
                        family_key: family_key.clone(),
                        created_at: now,
                        updated_at: now,
                    };
                    diesel::insert_into(reasoning_config::table)
                        .values(NewReasoningConfigDb::to_db(&new_config))
                        .returning(reasoning_config::dsl::id)
                        .get_result::<i64>(conn)
                        .map_err(|err| map_write_error("failed to create reasoning config", err))?
                };

                diesel::update(
                    reasoning_config_preset::table.filter(
                        reasoning_config_preset::dsl::config_id
                            .eq(config_id)
                            .and(reasoning_config_preset::dsl::deleted_at.is_null()),
                    ),
                )
                .set((
                    reasoning_config_preset::dsl::deleted_at.eq(Some(now)),
                    reasoning_config_preset::dsl::is_enabled.eq(false),
                    reasoning_config_preset::dsl::updated_at.eq(now),
                ))
                .execute(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to replace reasoning config presets for config {}: {err}",
                        config_id
                    )))
                })?;

                for preset in presets {
                    let new_preset = NewReasoningConfigPreset {
                        id: ID_GENERATOR.generate_id(),
                        config_id,
                        preset_key: preset.preset.as_key().to_string(),
                        expose_in_models: preset.expose_in_models,
                        is_enabled: preset.is_enabled,
                        created_at: now,
                        updated_at: now,
                    };
                    diesel::insert_into(reasoning_config_preset::table)
                        .values(NewReasoningConfigPresetDb::to_db(&new_preset))
                        .execute(conn)
                        .map_err(|err| {
                            map_write_error("failed to create reasoning config preset", err)
                        })?;
                }

                Ok(config_id)
            })
        })
    }

    fn delete_owner_config(scope: ReasoningConfigScope, owner_id: i64) -> DbResult<usize> {
        let now = Utc::now().timestamp_millis();
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            conn.transaction::<usize, BaseError, _>(|conn| {
                let mut query = reasoning_config::table
                    .filter(
                        reasoning_config::dsl::scope_kind
                            .eq(scope.as_key())
                            .and(reasoning_config::dsl::deleted_at.is_null()),
                    )
                    .into_boxed();
                query = match scope {
                    ReasoningConfigScope::Provider => {
                        query.filter(reasoning_config::dsl::provider_id.eq(owner_id))
                    }
                    ReasoningConfigScope::Model => {
                        query.filter(reasoning_config::dsl::model_id.eq(owner_id))
                    }
                };
                let config_ids = query
                    .select(reasoning_config::dsl::id)
                    .load::<i64>(conn)
                    .map_err(|err| {
                        BaseError::DatabaseFatal(Some(format!(
                            "failed to fetch {} reasoning configs for deletion: {err}",
                            scope.as_key()
                        )))
                    })?;

                if config_ids.is_empty() {
                    return Ok(0);
                }

                let affected = diesel::update(
                    reasoning_config::table.filter(reasoning_config::dsl::id.eq_any(&config_ids)),
                )
                .set((
                    reasoning_config::dsl::deleted_at.eq(Some(now)),
                    reasoning_config::dsl::updated_at.eq(now),
                ))
                .execute(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to delete {} reasoning config for owner {}: {err}",
                        scope.as_key(),
                        owner_id
                    )))
                })?;

                diesel::update(
                    reasoning_config_preset::table.filter(
                        reasoning_config_preset::dsl::config_id
                            .eq_any(&config_ids)
                            .and(reasoning_config_preset::dsl::deleted_at.is_null()),
                    ),
                )
                .set((
                    reasoning_config_preset::dsl::deleted_at.eq(Some(now)),
                    reasoning_config_preset::dsl::is_enabled.eq(false),
                    reasoning_config_preset::dsl::updated_at.eq(now),
                ))
                .execute(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to delete reasoning config presets for owner {}: {err}",
                        owner_id
                    )))
                })?;

                Ok(affected)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::{TestDbContext, open_test_sqlite_connection};
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
    use diesel::connection::SimpleConnection;
    use diesel::{QueryableByName, sql_query};

    fn provider_input(id: i64, key: &str) -> NewProvider {
        let now = Utc::now().timestamp_millis();
        NewProvider {
            id,
            provider_key: key.to_string(),
            name: key.to_string(),
            endpoint: "https://example.com".to_string(),
            use_proxy: false,
            is_enabled: true,
            created_at: now,
            updated_at: now,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        }
    }

    fn create_provider(id: i64, key: &str) -> crate::database::provider::Provider {
        Provider::create(&provider_input(id, key)).expect("provider")
    }

    fn create_model(provider_id: i64, name: &str) -> crate::database::model::Model {
        Model::create(
            provider_id,
            name,
            None,
            true,
            ModelCapabilityFlags::default(),
        )
        .expect("model")
    }

    fn preset(
        preset_key: &str,
        expose_in_models: bool,
        is_enabled: bool,
    ) -> ReasoningConfigPresetInput {
        ReasoningConfigPresetInput {
            preset_key: preset_key.to_string(),
            expose_in_models,
            is_enabled,
        }
    }

    #[test]
    fn provider_config_upsert_replaces_whole_config_and_keeps_no_think_as_custom_preset() {
        let db = TestDbContext::new_sqlite("reasoning-config-provider.sqlite");
        db.run_sync(|| {
            let provider = create_provider(1001, "openai");

            let created = ReasoningConfig::upsert_provider_config(
                provider.id,
                "openai_chat_reasoning_effort",
                &[preset("disabled", true, true), preset("low", false, false)],
            )
            .expect("provider config");

            assert_eq!(created.scope, ReasoningConfigScope::Provider);
            assert_eq!(created.mode, ReasoningConfigMode::Custom);
            assert_eq!(
                created.family,
                Some(ReasoningPatchFamily::OpenAiChatReasoningEffort)
            );
            assert_eq!(created.config.provider_id, Some(provider.id));
            assert_eq!(created.presets.len(), 2);
            let no_think = created
                .presets
                .iter()
                .find(|row| row.preset_key == ReasoningPreset::Disabled)
                .expect("disabled preset row");
            assert_eq!(no_think.suffix, "no-think");
            assert!(no_think.preset.is_enabled);

            let replaced = ReasoningConfig::upsert_provider_config(
                provider.id,
                "openai_responses_reasoning",
                &[preset("high", true, true)],
            )
            .expect("replace provider config");

            assert_eq!(replaced.config.id, created.config.id);
            assert_eq!(
                replaced.family,
                Some(ReasoningPatchFamily::OpenAiResponsesReasoning)
            );
            assert_eq!(replaced.presets.len(), 1);
            assert_eq!(replaced.presets[0].preset_key, ReasoningPreset::High);

            let listed =
                ReasoningConfig::list_active_provider_configs(&[provider.id]).expect("bulk list");
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].config.id, created.config.id);
        });
    }

    #[test]
    fn model_config_supports_inherit_disabled_and_custom_override() {
        let db = TestDbContext::new_sqlite("reasoning-config-model.sqlite");
        db.run_sync(|| {
            let provider = create_provider(2001, "gemini");
            let model = create_model(provider.id, "gemini-2.5-pro");

            assert!(
                ReasoningConfig::get_active_model_config(model.id)
                    .expect("initial model config")
                    .is_none()
            );

            let disabled = ReasoningConfig::upsert_model_config(
                model.id,
                ReasoningConfigMode::Disabled,
                None,
                &[],
            )
            .expect("disabled model config");
            assert_eq!(disabled.scope, ReasoningConfigScope::Model);
            assert_eq!(disabled.mode, ReasoningConfigMode::Disabled);
            assert_eq!(disabled.family, None);
            assert!(disabled.presets.is_empty());

            let deleted = ReasoningConfig::delete_model_config(model.id).expect("inherit delete");
            assert_eq!(deleted, 1);
            assert!(
                ReasoningConfig::get_active_model_config(model.id)
                    .expect("after inherit")
                    .is_none()
            );

            let custom = ReasoningConfig::upsert_model_config(
                model.id,
                ReasoningConfigMode::Custom,
                Some("gemini25_thinking_budget"),
                &[preset("auto", true, true)],
            )
            .expect("custom model config");
            assert_eq!(custom.mode, ReasoningConfigMode::Custom);
            assert_eq!(
                custom.family,
                Some(ReasoningPatchFamily::Gemini25ThinkingBudget)
            );
            assert_eq!(custom.presets.len(), 1);
            assert_eq!(custom.presets[0].preset_key, ReasoningPreset::Auto);

            let listed = ReasoningConfig::list_active_model_configs(&[model.id]).expect("bulk");
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].config.model_id, Some(model.id));
        });
    }

    #[test]
    fn repository_rejects_invalid_family_preset_and_disabled_mode_rows() {
        let db = TestDbContext::new_sqlite("reasoning-config-validation.sqlite");
        db.run_sync(|| {
            let provider = create_provider(3001, "openai-validation");
            let model = create_model(provider.id, "gpt-5");

            let unsupported = ReasoningConfig::upsert_provider_config(
                provider.id,
                "openai_chat_reasoning_effort",
                &[preset("auto", true, true)],
            )
            .expect_err("OpenAI chat family should reject auto");
            assert!(matches!(unsupported, BaseError::ParamInvalid(_)));

            let disabled_with_preset = ReasoningConfig::upsert_model_config(
                model.id,
                ReasoningConfigMode::Disabled,
                None,
                &[preset("disabled", true, true)],
            )
            .expect_err("disabled mode must not retain active preset rows");
            assert!(matches!(disabled_with_preset, BaseError::ParamInvalid(_)));

            let no_think_custom = ReasoningConfig::upsert_model_config(
                model.id,
                ReasoningConfigMode::Custom,
                Some("openai_chat_reasoning_effort"),
                &[preset("disabled", true, true)],
            )
            .expect("no-think is valid as a custom preset");
            assert_eq!(
                no_think_custom.presets[0].preset_key,
                ReasoningPreset::Disabled
            );
            assert_eq!(no_think_custom.presets[0].suffix, "no-think");
        });
    }

    #[test]
    fn repository_accepts_siliconflow_switch_presets_and_rejects_strengths() {
        let db = TestDbContext::new_sqlite("reasoning-config-siliconflow.sqlite");
        db.run_sync(|| {
            let provider = create_provider(3101, "siliconflow-validation");

            let created = ReasoningConfig::upsert_provider_config(
                provider.id,
                "siliconflow_openai_enable_thinking",
                &[
                    preset("disabled", true, true),
                    preset("enabled", true, true),
                ],
            )
            .expect("SiliconFlow switch presets should be accepted");

            assert_eq!(
                created.family,
                Some(ReasoningPatchFamily::SiliconFlowOpenAiEnableThinking)
            );
            assert_eq!(created.presets.len(), 2);
            assert_eq!(created.presets[0].preset_key, ReasoningPreset::Disabled);
            assert_eq!(created.presets[1].preset_key, ReasoningPreset::Enabled);

            let unsupported = ReasoningConfig::upsert_provider_config(
                provider.id,
                "siliconflow_openai_enable_thinking",
                &[preset("high", true, true)],
            )
            .expect_err("SiliconFlow enable_thinking should reject strength presets");
            assert!(matches!(unsupported, BaseError::ParamInvalid(_)));
        });
    }

    #[derive(QueryableByName)]
    struct MigrationConfigRow {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        id: i64,
        #[diesel(sql_type = diesel::sql_types::Text)]
        scope_kind: String,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::BigInt>)]
        provider_id: Option<i64>,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::BigInt>)]
        model_id: Option<i64>,
        #[diesel(sql_type = diesel::sql_types::Text)]
        mode: String,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        family_key: Option<String>,
    }

    #[derive(QueryableByName)]
    struct MigrationPresetRow {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        id: i64,
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        config_id: i64,
        #[diesel(sql_type = diesel::sql_types::Text)]
        preset_key: String,
        #[diesel(sql_type = diesel::sql_types::Bool)]
        is_enabled: bool,
    }

    #[derive(QueryableByName)]
    struct MigrationCountRow {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }

    fn sqlite_named_object_exists(
        conn: &mut diesel::SqliteConnection,
        object_type: &str,
        object_name: &str,
    ) -> bool {
        sql_query("SELECT COUNT(*) AS count FROM sqlite_master WHERE type = ? AND name = ?")
            .bind::<diesel::sql_types::Text, _>(object_type)
            .bind::<diesel::sql_types::Text, _>(object_name)
            .get_result::<MigrationCountRow>(conn)
            .expect("sqlite object lookup")
            .count
            > 0
    }

    fn sqlite_column_exists(
        conn: &mut diesel::SqliteConnection,
        table_name: &str,
        column_name: &str,
    ) -> bool {
        let query = format!(
            "SELECT COUNT(*) AS count FROM pragma_table_info('{table_name}') WHERE name = ?"
        );
        sql_query(query)
            .bind::<diesel::sql_types::Text, _>(column_name)
            .get_result::<MigrationCountRow>(conn)
            .expect("sqlite column lookup")
            .count
            > 0
    }

    #[test]
    fn sqlite_migrations_materialize_legacy_profile_bindings_and_clean_schema() {
        let (_dir, mut conn) =
            open_test_sqlite_connection("reasoning-config-legacy-migration.sqlite");
        conn.batch_execute(
            r#"
            CREATE TABLE cost_catalogs (
                id BIGINT PRIMARY KEY NOT NULL
            );
            CREATE TABLE provider (
                id BIGINT PRIMARY KEY NOT NULL,
                provider_key TEXT NOT NULL,
                name TEXT NOT NULL,
                endpoint TEXT NOT NULL,
                use_proxy BOOLEAN NOT NULL DEFAULT false,
                is_enabled BOOLEAN NOT NULL DEFAULT true,
                deleted_at BIGINT DEFAULT NULL,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL,
                provider_type TEXT NOT NULL DEFAULT 'OPENAI',
                provider_api_key_mode TEXT NOT NULL DEFAULT 'QUEUE',
                default_reasoning_profile_id BIGINT
            );
            CREATE TABLE model (
                id BIGINT PRIMARY KEY NOT NULL,
                provider_id BIGINT NOT NULL,
                cost_catalog_id BIGINT,
                model_name TEXT NOT NULL,
                real_model_name TEXT,
                supports_streaming BOOLEAN NOT NULL DEFAULT true,
                supports_tools BOOLEAN NOT NULL DEFAULT true,
                supports_reasoning BOOLEAN NOT NULL DEFAULT true,
                supports_image_input BOOLEAN NOT NULL DEFAULT true,
                supports_embeddings BOOLEAN NOT NULL DEFAULT true,
                supports_rerank BOOLEAN NOT NULL DEFAULT true,
                is_enabled BOOLEAN NOT NULL DEFAULT true,
                deleted_at BIGINT DEFAULT NULL,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL,
                reasoning_profile_override_id BIGINT
            );
            CREATE TABLE reasoning_profile (
                id BIGINT PRIMARY KEY NOT NULL,
                profile_key TEXT NOT NULL,
                name TEXT NOT NULL,
                description TEXT,
                family_key TEXT NOT NULL,
                is_enabled BOOLEAN NOT NULL,
                deleted_at BIGINT,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL
            );
            CREATE TABLE reasoning_profile_preset (
                id BIGINT PRIMARY KEY NOT NULL,
                profile_id BIGINT NOT NULL,
                preset_key TEXT NOT NULL,
                expose_in_models BOOLEAN NOT NULL,
                is_enabled BOOLEAN NOT NULL,
                deleted_at BIGINT,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL
            );

            INSERT INTO provider (
                id, provider_key, name, endpoint, use_proxy, is_enabled, deleted_at,
                created_at, updated_at, provider_type, provider_api_key_mode,
                default_reasoning_profile_id
            )
                VALUES
                    (10, 'provider-a', 'Provider A', 'https://provider-a.example', false, true, NULL, 100, 100, 'OPENAI', 'QUEUE', 100),
                    (11, 'provider-b', 'Provider B', 'https://provider-b.example', false, true, NULL, 100, 100, 'OPENAI', 'QUEUE', NULL);
            INSERT INTO model (
                id, provider_id, cost_catalog_id, model_name, real_model_name,
                supports_streaming, supports_tools, supports_reasoning,
                supports_image_input, supports_embeddings, supports_rerank,
                is_enabled, deleted_at, created_at, updated_at,
                reasoning_profile_override_id
            )
                VALUES
                    (20, 10, NULL, 'model-a', NULL, true, true, true, true, true, true, true, NULL, 100, 100, 200),
                    (21, 11, NULL, 'model-b', NULL, true, true, true, true, true, true, true, NULL, 100, 100, NULL);
            INSERT INTO reasoning_profile (
                id, profile_key, name, family_key, is_enabled, deleted_at, created_at, updated_at
            )
                VALUES
                    (100, 'provider_profile', 'Provider', 'openai_chat_reasoning_effort', true, NULL, 1000, 1100),
                    (200, 'model_profile', 'Model', 'gemini25_thinking_budget', true, NULL, 2000, 2100),
                    (300, 'unreferenced_profile', 'Unused', 'anthropic_thinking_budget', true, NULL, 3000, 3100);
            INSERT INTO reasoning_profile_preset (
                id, profile_id, preset_key, expose_in_models, is_enabled, deleted_at, created_at, updated_at
            )
                VALUES
                    (1000, 100, 'high', true, true, NULL, 1000, 1100),
                    (1001, 100, 'low', false, false, NULL, 1000, 1100),
                    (2000, 200, 'auto', true, true, NULL, 2000, 2100),
                    (3000, 300, 'high', true, true, NULL, 3000, 3100);
            "#,
        )
        .expect("legacy schema");

        conn.batch_execute(include_str!(
            "../../migrations/sqlite/2026-04-26-120000_reasoning_config_foundation/up.sql"
        ))
        .expect("reasoning config migration");
        conn.batch_execute(include_str!(
            "../../migrations/sqlite/2026-04-27-090000_reasoning_profile_schema_cleanup/up.sql"
        ))
        .expect("reasoning profile cleanup migration");

        let configs = sql_query(
            "SELECT id, scope_kind, provider_id, model_id, mode, family_key
             FROM reasoning_config
             ORDER BY scope_kind, COALESCE(provider_id, model_id)",
        )
        .load::<MigrationConfigRow>(&mut conn)
        .expect("configs");
        assert_eq!(configs.len(), 2);

        let model_config = configs
            .iter()
            .find(|row| row.scope_kind == "model")
            .expect("model config");
        assert_eq!(model_config.id, -2000000000001);
        assert_eq!(model_config.model_id, Some(20));
        assert_eq!(model_config.provider_id, None);
        assert_eq!(model_config.mode, "custom");
        assert_eq!(
            model_config.family_key.as_deref(),
            Some("gemini25_thinking_budget")
        );

        let provider_config = configs
            .iter()
            .find(|row| row.scope_kind == "provider")
            .expect("provider config");
        assert_eq!(provider_config.id, -1000000000001);
        assert_eq!(provider_config.provider_id, Some(10));
        assert_eq!(provider_config.model_id, None);
        assert_eq!(
            provider_config.family_key.as_deref(),
            Some("openai_chat_reasoning_effort")
        );

        let presets = sql_query(
            "SELECT id, config_id, preset_key, is_enabled
             FROM reasoning_config_preset
             ORDER BY config_id, preset_key",
        )
        .load::<MigrationPresetRow>(&mut conn)
        .expect("presets");
        assert_eq!(presets.len(), 3);
        assert!(presets.iter().all(|row| row.id < 0));
        assert!(presets.iter().any(|row| {
            row.config_id == provider_config.id && row.preset_key == "low" && !row.is_enabled
        }));
        assert!(presets.iter().any(|row| {
            row.config_id == provider_config.id && row.preset_key == "high" && row.is_enabled
        }));
        assert!(presets.iter().any(|row| {
            row.config_id == model_config.id && row.preset_key == "auto" && row.is_enabled
        }));

        assert!(!sqlite_named_object_exists(
            &mut conn,
            "table",
            "reasoning_profile"
        ));
        assert!(!sqlite_named_object_exists(
            &mut conn,
            "table",
            "reasoning_profile_preset"
        ));
        assert!(!sqlite_named_object_exists(
            &mut conn,
            "index",
            "idx_provider_default_reasoning_profile_id"
        ));
        assert!(!sqlite_named_object_exists(
            &mut conn,
            "index",
            "idx_model_reasoning_profile_override_id"
        ));
        assert!(!sqlite_column_exists(
            &mut conn,
            "provider",
            "default_reasoning_profile_id"
        ));
        assert!(!sqlite_column_exists(
            &mut conn,
            "model",
            "reasoning_profile_override_id"
        ));

        let fk_violations = sql_query("SELECT COUNT(*) AS count FROM pragma_foreign_key_check")
            .get_result::<MigrationCountRow>(&mut conn)
            .expect("foreign key check");
        assert_eq!(fk_violations.count, 0);
    }
}
