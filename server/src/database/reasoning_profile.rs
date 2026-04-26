use std::collections::HashMap;
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
    AnthropicThinkingBudget,
    Gemini25ThinkingBudget,
    Gemini3ThinkingLevel,
}

impl ReasoningPatchFamily {
    pub const ALL: [ReasoningPatchFamily; 5] = [
        ReasoningPatchFamily::OpenAiChatReasoningEffort,
        ReasoningPatchFamily::OpenAiResponsesReasoning,
        ReasoningPatchFamily::AnthropicThinkingBudget,
        ReasoningPatchFamily::Gemini25ThinkingBudget,
        ReasoningPatchFamily::Gemini3ThinkingLevel,
    ];

    pub fn as_key(self) -> &'static str {
        match self {
            ReasoningPatchFamily::OpenAiChatReasoningEffort => "openai_chat_reasoning_effort",
            ReasoningPatchFamily::OpenAiResponsesReasoning => "openai_responses_reasoning",
            ReasoningPatchFamily::AnthropicThinkingBudget => "anthropic_thinking_budget",
            ReasoningPatchFamily::Gemini25ThinkingBudget => "gemini25_thinking_budget",
            ReasoningPatchFamily::Gemini3ThinkingLevel => "gemini3_thinking_level",
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

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, serde::Serialize)]
    #[diesel(table_name = reasoning_profile)]
    pub struct ReasoningProfile {
        pub id: i64,
        pub profile_key: String,
        pub name: String,
        pub description: Option<String>,
        pub family_key: String,
        pub is_enabled: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Deserialize, Debug)]
    #[diesel(table_name = reasoning_profile)]
    pub struct NewReasoningProfile {
        pub id: i64,
        pub profile_key: String,
        pub name: String,
        pub description: Option<String>,
        pub family_key: String,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset, Deserialize, Debug, Clone, Default)]
    #[diesel(table_name = reasoning_profile)]
    pub struct UpdateReasoningProfileData {
        pub profile_key: Option<String>,
        pub name: Option<String>,
        pub description: Option<Option<String>>,
        pub family_key: Option<String>,
        pub is_enabled: Option<bool>,
    }

    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, serde::Serialize)]
    #[diesel(table_name = reasoning_profile_preset)]
    pub struct ReasoningProfilePreset {
        pub id: i64,
        pub profile_id: i64,
        pub preset_key: String,
        pub expose_in_models: bool,
        pub is_enabled: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Deserialize, Debug)]
    #[diesel(table_name = reasoning_profile_preset)]
    pub struct NewReasoningProfilePreset {
        pub id: i64,
        pub profile_id: i64,
        pub preset_key: String,
        pub expose_in_models: bool,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset, Deserialize, Debug, Clone, Default)]
    #[diesel(table_name = reasoning_profile_preset)]
    pub struct UpdateReasoningProfilePresetData {
        pub preset_key: Option<String>,
        pub expose_in_models: Option<bool>,
        pub is_enabled: Option<bool>,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningProfilePresetView {
    pub preset: ReasoningProfilePreset,
    pub preset_key: ReasoningPreset,
    pub suffix: String,
    pub requires_reasoning: bool,
    pub allowed_operation_kinds: Vec<String>,
}

impl ReasoningProfilePresetView {
    fn from_row(row: ReasoningProfilePreset) -> DbResult<Self> {
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
pub struct ReasoningProfileWithPresets {
    pub profile: ReasoningProfile,
    pub family: ReasoningPatchFamily,
    pub presets: Vec<ReasoningProfilePresetView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningProfileProviderBinding {
    pub provider_id: i64,
    pub provider_key: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningProfileModelBinding {
    pub model_id: i64,
    pub provider_key: String,
    pub model_name: String,
}

fn invalid_param(message: impl Into<String>) -> BaseError {
    BaseError::ParamInvalid(Some(message.into()))
}

fn database_config_error(message: impl Into<String>) -> BaseError {
    BaseError::DatabaseFatal(Some(message.into()))
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

fn validate_non_empty(field: &str, value: &str) -> DbResult<()> {
    if value.trim().is_empty() {
        Err(invalid_param(format!("{field} must not be empty")))
    } else {
        Ok(())
    }
}

pub fn validate_active_reasoning_profile_id(field: &str, profile_id: Option<i64>) -> DbResult<()> {
    let Some(profile_id) = profile_id else {
        return Ok(());
    };

    let conn = &mut get_connection()?;
    db_execute!(conn, {
        let active_id = reasoning_profile::table
            .filter(
                reasoning_profile::dsl::id
                    .eq(profile_id)
                    .and(reasoning_profile::dsl::deleted_at.is_null())
                    .and(reasoning_profile::dsl::is_enabled.eq(true)),
            )
            .select(reasoning_profile::dsl::id)
            .first::<i64>(conn)
            .optional()
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "failed to validate reasoning profile {profile_id} for {field}: {err}"
                )))
            })?;

        if active_id.is_some() {
            Ok(())
        } else {
            Err(BaseError::ParamInvalid(Some(format!(
                "{field} {profile_id} must reference an active reasoning profile"
            ))))
        }
    })
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

impl ReasoningProfile {
    pub fn create(
        profile_key_val: &str,
        name_val: &str,
        description_val: Option<&str>,
        family_key_val: &str,
        is_enabled_val: bool,
    ) -> DbResult<Self> {
        validate_non_empty("profile_key", profile_key_val)?;
        validate_non_empty("name", name_val)?;
        let family = validate_family_key(family_key_val)?;
        let now = Utc::now().timestamp_millis();
        let new_profile = NewReasoningProfile {
            id: ID_GENERATOR.generate_id(),
            profile_key: profile_key_val.trim().to_string(),
            name: name_val.trim().to_string(),
            description: description_val.map(ToString::to_string),
            family_key: family.as_key().to_string(),
            is_enabled: is_enabled_val,
            created_at: now,
            updated_at: now,
        };

        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = diesel::insert_into(reasoning_profile::table)
                .values(NewReasoningProfileDb::to_db(&new_profile))
                .returning(ReasoningProfileDb::as_returning())
                .get_result::<ReasoningProfileDb>(conn)
                .map_err(|err| map_write_error("failed to create reasoning profile", err))?;
            Ok(row.from_db())
        })
    }

    pub fn update(id_value: i64, data: &UpdateReasoningProfileData) -> DbResult<Self> {
        if let Some(profile_key) = data.profile_key.as_deref() {
            validate_non_empty("profile_key", profile_key)?;
        }
        if let Some(name) = data.name.as_deref() {
            validate_non_empty("name", name)?;
        }

        let mut data = data.clone();
        if let Some(family_key) = data.family_key.as_deref() {
            data.family_key = Some(validate_family_key(family_key)?.as_key().to_string());
        }

        let now = Utc::now().timestamp_millis();
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = diesel::update(
                reasoning_profile::table.filter(
                    reasoning_profile::dsl::id
                        .eq(id_value)
                        .and(reasoning_profile::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                UpdateReasoningProfileDataDb::to_db(&data),
                reasoning_profile::dsl::updated_at.eq(now),
            ))
            .returning(ReasoningProfileDb::as_returning())
            .get_result::<ReasoningProfileDb>(conn)
            .map_err(|err| match err {
                diesel::result::Error::NotFound => {
                    BaseError::NotFound(Some(format!("reasoning profile {id_value} not found")))
                }
                other => map_write_error("failed to update reasoning profile", other),
            })?;
            Ok(row.from_db())
        })
    }

    pub fn delete(id_value: i64) -> DbResult<usize> {
        let now = Utc::now().timestamp_millis();
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let affected = diesel::update(
                reasoning_profile::table.filter(
                    reasoning_profile::dsl::id
                        .eq(id_value)
                        .and(reasoning_profile::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                reasoning_profile::dsl::deleted_at.eq(Some(now)),
                reasoning_profile::dsl::is_enabled.eq(false),
                reasoning_profile::dsl::updated_at.eq(now),
            ))
            .execute(conn)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "failed to delete reasoning profile {id_value}: {err}"
                )))
            })?;
            Ok(affected)
        })
    }

    pub fn list_active_with_presets() -> DbResult<Vec<ReasoningProfileWithPresets>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let profile_rows = reasoning_profile::table
                .filter(
                    reasoning_profile::dsl::deleted_at
                        .is_null()
                        .and(reasoning_profile::dsl::is_enabled.eq(true)),
                )
                .order(reasoning_profile::dsl::profile_key.asc())
                .select(ReasoningProfileDb::as_select())
                .load::<ReasoningProfileDb>(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to list active reasoning profiles: {err}"
                    )))
                })?;

            let profiles: Vec<ReasoningProfile> = profile_rows
                .into_iter()
                .map(ReasoningProfileDb::from_db)
                .collect();
            if profiles.is_empty() {
                return Ok(Vec::new());
            }

            let profile_ids: Vec<i64> = profiles.iter().map(|profile| profile.id).collect();
            let preset_rows = reasoning_profile_preset::table
                .filter(
                    reasoning_profile_preset::dsl::profile_id
                        .eq_any(&profile_ids)
                        .and(reasoning_profile_preset::dsl::deleted_at.is_null())
                        .and(reasoning_profile_preset::dsl::is_enabled.eq(true)),
                )
                .order((
                    reasoning_profile_preset::dsl::profile_id.asc(),
                    reasoning_profile_preset::dsl::preset_key.asc(),
                ))
                .select(ReasoningProfilePresetDb::as_select())
                .load::<ReasoningProfilePresetDb>(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to list active reasoning profile presets: {err}"
                    )))
                })?;

            let mut presets_by_profile: HashMap<i64, Vec<ReasoningProfilePresetView>> =
                HashMap::new();
            for row in preset_rows {
                let view = ReasoningProfilePresetView::from_row(row.from_db())?;
                presets_by_profile
                    .entry(view.preset.profile_id)
                    .or_default()
                    .push(view);
            }

            profiles
                .into_iter()
                .map(|profile| {
                    let family = parse_family_key(&profile.family_key)?;
                    Ok(ReasoningProfileWithPresets {
                        presets: presets_by_profile.remove(&profile.id).unwrap_or_default(),
                        profile,
                        family,
                    })
                })
                .collect()
        })
    }

    pub fn list_with_presets() -> DbResult<Vec<ReasoningProfileWithPresets>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let profile_rows = reasoning_profile::table
                .filter(reasoning_profile::dsl::deleted_at.is_null())
                .order(reasoning_profile::dsl::profile_key.asc())
                .select(ReasoningProfileDb::as_select())
                .load::<ReasoningProfileDb>(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to list reasoning profiles: {err}"
                    )))
                })?;

            let profiles: Vec<ReasoningProfile> = profile_rows
                .into_iter()
                .map(ReasoningProfileDb::from_db)
                .collect();
            if profiles.is_empty() {
                return Ok(Vec::new());
            }

            let profile_ids: Vec<i64> = profiles.iter().map(|profile| profile.id).collect();
            let preset_rows = reasoning_profile_preset::table
                .filter(
                    reasoning_profile_preset::dsl::profile_id
                        .eq_any(&profile_ids)
                        .and(reasoning_profile_preset::dsl::deleted_at.is_null()),
                )
                .order((
                    reasoning_profile_preset::dsl::profile_id.asc(),
                    reasoning_profile_preset::dsl::preset_key.asc(),
                ))
                .select(ReasoningProfilePresetDb::as_select())
                .load::<ReasoningProfilePresetDb>(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to list reasoning profile presets: {err}"
                    )))
                })?;

            let mut presets_by_profile: HashMap<i64, Vec<ReasoningProfilePresetView>> =
                HashMap::new();
            for row in preset_rows {
                let view = ReasoningProfilePresetView::from_row(row.from_db())?;
                presets_by_profile
                    .entry(view.preset.profile_id)
                    .or_default()
                    .push(view);
            }

            profiles
                .into_iter()
                .map(|profile| {
                    let family = parse_family_key(&profile.family_key)?;
                    Ok(ReasoningProfileWithPresets {
                        presets: presets_by_profile.remove(&profile.id).unwrap_or_default(),
                        profile,
                        family,
                    })
                })
                .collect()
        })
    }

    pub fn get_active_with_presets_by_id(
        id_value: i64,
    ) -> DbResult<Option<ReasoningProfileWithPresets>> {
        Ok(Self::list_active_with_presets()?
            .into_iter()
            .find(|profile| profile.profile.id == id_value))
    }

    pub fn get_with_presets_by_id(id_value: i64) -> DbResult<Option<ReasoningProfileWithPresets>> {
        Ok(Self::list_with_presets()?
            .into_iter()
            .find(|profile| profile.profile.id == id_value))
    }

    pub fn list_provider_bindings(
        profile_id_value: i64,
    ) -> DbResult<Vec<ReasoningProfileProviderBinding>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = provider::table
                .filter(
                    provider::dsl::default_reasoning_profile_id
                        .eq(profile_id_value)
                        .and(provider::dsl::deleted_at.is_null()),
                )
                .order(provider::dsl::provider_key.asc())
                .select((provider::dsl::id, provider::dsl::provider_key))
                .load::<(i64, String)>(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to list providers using reasoning profile {profile_id_value}: {err}"
                    )))
                })?;

            Ok(rows
                .into_iter()
                .map(
                    |(provider_id, provider_key)| ReasoningProfileProviderBinding {
                        provider_id,
                        provider_key,
                    },
                )
                .collect())
        })
    }

    pub fn list_model_bindings(
        profile_id_value: i64,
    ) -> DbResult<Vec<ReasoningProfileModelBinding>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = model::table
                .inner_join(provider::table.on(provider::dsl::id.eq(model::dsl::provider_id)))
                .filter(
                    model::dsl::reasoning_profile_override_id
                        .eq(profile_id_value)
                        .and(model::dsl::deleted_at.is_null())
                        .and(provider::dsl::deleted_at.is_null()),
                )
                .order((
                    provider::dsl::provider_key.asc(),
                    model::dsl::model_name.asc(),
                ))
                .select((
                    model::dsl::id,
                    provider::dsl::provider_key,
                    model::dsl::model_name,
                ))
                .load::<(i64, String, String)>(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to list models using reasoning profile {profile_id_value}: {err}"
                    )))
                })?;

            Ok(rows
                .into_iter()
                .map(
                    |(model_id, provider_key, model_name)| ReasoningProfileModelBinding {
                        model_id,
                        provider_key,
                        model_name,
                    },
                )
                .collect())
        })
    }
}

impl ReasoningProfilePreset {
    pub fn get_by_id(id_value: i64) -> DbResult<Self> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = reasoning_profile_preset::table
                .filter(
                    reasoning_profile_preset::dsl::id
                        .eq(id_value)
                        .and(reasoning_profile_preset::dsl::deleted_at.is_null()),
                )
                .select(ReasoningProfilePresetDb::as_select())
                .first::<ReasoningProfilePresetDb>(conn)
                .map_err(|err| match err {
                    diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                        "reasoning profile preset {id_value} not found"
                    ))),
                    other => BaseError::DatabaseFatal(Some(format!(
                        "failed to fetch reasoning profile preset {id_value}: {other}"
                    ))),
                })?;
            Ok(row.from_db())
        })
    }

    pub fn find_by_profile_and_preset(
        profile_id_value: i64,
        preset_key_value: &str,
    ) -> DbResult<Option<Self>> {
        let preset = validate_preset_key(preset_key_value)?;
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = reasoning_profile_preset::table
                .filter(
                    reasoning_profile_preset::dsl::profile_id
                        .eq(profile_id_value)
                        .and(reasoning_profile_preset::dsl::preset_key.eq(preset.as_key()))
                        .and(reasoning_profile_preset::dsl::deleted_at.is_null()),
                )
                .select(ReasoningProfilePresetDb::as_select())
                .first::<ReasoningProfilePresetDb>(conn)
                .optional()
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to fetch reasoning profile preset '{}' for profile {}: {err}",
                        preset.as_key(),
                        profile_id_value
                    )))
                })?;
            Ok(row.map(ReasoningProfilePresetDb::from_db))
        })
    }

    pub fn create(
        profile_id_val: i64,
        preset_key_val: &str,
        expose_in_models_val: bool,
        is_enabled_val: bool,
    ) -> DbResult<Self> {
        let preset = validate_preset_key(preset_key_val)?;
        let now = Utc::now().timestamp_millis();
        let new_preset = NewReasoningProfilePreset {
            id: ID_GENERATOR.generate_id(),
            profile_id: profile_id_val,
            preset_key: preset.as_key().to_string(),
            expose_in_models: expose_in_models_val,
            is_enabled: is_enabled_val,
            created_at: now,
            updated_at: now,
        };

        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = diesel::insert_into(reasoning_profile_preset::table)
                .values(NewReasoningProfilePresetDb::to_db(&new_preset))
                .returning(ReasoningProfilePresetDb::as_returning())
                .get_result::<ReasoningProfilePresetDb>(conn)
                .map_err(|err| map_write_error("failed to create reasoning profile preset", err))?;
            Ok(row.from_db())
        })
    }

    pub fn update(id_value: i64, data: &UpdateReasoningProfilePresetData) -> DbResult<Self> {
        let mut data = data.clone();
        if let Some(preset_key) = data.preset_key.as_deref() {
            data.preset_key = Some(validate_preset_key(preset_key)?.as_key().to_string());
        }

        let now = Utc::now().timestamp_millis();
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = diesel::update(
                reasoning_profile_preset::table.filter(
                    reasoning_profile_preset::dsl::id
                        .eq(id_value)
                        .and(reasoning_profile_preset::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                UpdateReasoningProfilePresetDataDb::to_db(&data),
                reasoning_profile_preset::dsl::updated_at.eq(now),
            ))
            .returning(ReasoningProfilePresetDb::as_returning())
            .get_result::<ReasoningProfilePresetDb>(conn)
            .map_err(|err| match err {
                diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                    "reasoning profile preset {id_value} not found"
                ))),
                other => map_write_error("failed to update reasoning profile preset", other),
            })?;
            Ok(row.from_db())
        })
    }

    pub fn delete(id_value: i64) -> DbResult<usize> {
        let now = Utc::now().timestamp_millis();
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let affected = diesel::update(
                reasoning_profile_preset::table.filter(
                    reasoning_profile_preset::dsl::id
                        .eq(id_value)
                        .and(reasoning_profile_preset::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                reasoning_profile_preset::dsl::deleted_at.eq(Some(now)),
                reasoning_profile_preset::dsl::is_enabled.eq(false),
                reasoning_profile_preset::dsl::updated_at.eq(now),
            ))
            .execute(conn)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "failed to delete reasoning profile preset {id_value}: {err}"
                )))
            })?;
            Ok(affected)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::TestDbContext;

    #[test]
    fn preset_metadata_is_derived_from_builtin_key() {
        let metadata = ReasoningPreset::High.metadata();
        assert_eq!(metadata.preset_key, "high");
        assert_eq!(metadata.suffix, "high");
        assert!(metadata.requires_reasoning);

        let disabled = ReasoningPreset::Disabled.metadata();
        assert_eq!(disabled.suffix, "no-think");
        assert!(!disabled.requires_reasoning);
    }

    #[test]
    fn repository_rejects_unknown_family_and_preset_keys() {
        let db = TestDbContext::new_sqlite("reasoning-profile-validation.sqlite");
        db.run_sync(|| {
            let profile_err = ReasoningProfile::create("bad", "Bad", None, "not_a_family", true)
                .expect_err("unknown family should be rejected");
            assert!(matches!(profile_err, BaseError::ParamInvalid(_)));

            let profile = ReasoningProfile::create(
                "openai_responses",
                "OpenAI Responses",
                None,
                "openai_responses_reasoning",
                true,
            )
            .expect("valid profile should be created");

            let preset_err = ReasoningProfilePreset::create(profile.id, "budget_1k", true, true)
                .expect_err("unknown preset should be rejected");
            assert!(matches!(preset_err, BaseError::ParamInvalid(_)));
        });
    }

    #[test]
    fn active_snapshot_filters_disabled_and_deleted_profiles_and_presets() {
        let db = TestDbContext::new_sqlite("reasoning-profile-active.sqlite");
        db.run_sync(|| {
            let active = ReasoningProfile::create(
                "openai_chat",
                "OpenAI Chat",
                None,
                "openai_chat_reasoning_effort",
                true,
            )
            .expect("active profile");
            let disabled_profile = ReasoningProfile::create(
                "disabled_profile",
                "Disabled Profile",
                None,
                "openai_responses_reasoning",
                false,
            )
            .expect("disabled profile");
            let deleted_profile = ReasoningProfile::create(
                "deleted_profile",
                "Deleted Profile",
                None,
                "anthropic_thinking_budget",
                true,
            )
            .expect("deleted profile");
            ReasoningProfile::delete(deleted_profile.id).expect("delete profile");

            ReasoningProfilePreset::create(active.id, "high", true, true)
                .expect("active high preset");
            ReasoningProfilePreset::create(active.id, "low", true, false)
                .expect("disabled low preset");
            let deleted_preset = ReasoningProfilePreset::create(active.id, "auto", true, true)
                .expect("deleted auto preset");
            ReasoningProfilePreset::delete(deleted_preset.id).expect("delete preset");
            ReasoningProfilePreset::create(disabled_profile.id, "high", true, true)
                .expect("disabled profile preset");

            let snapshots = ReasoningProfile::list_active_with_presets().expect("active snapshots");
            assert_eq!(snapshots.len(), 1);
            assert_eq!(snapshots[0].profile.id, active.id);
            assert_eq!(
                snapshots[0].family,
                ReasoningPatchFamily::OpenAiChatReasoningEffort
            );
            assert_eq!(snapshots[0].presets.len(), 1);
            assert_eq!(snapshots[0].presets[0].preset_key, ReasoningPreset::High);
            assert_eq!(snapshots[0].presets[0].suffix, "high");
            assert!(snapshots[0].presets[0].requires_reasoning);
        });
    }
}
