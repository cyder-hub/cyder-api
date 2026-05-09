use std::collections::HashSet;
use std::str::FromStr;

use diesel::prelude::*;

use crate::{
    controller::BaseError,
    database::{
        DbResult,
        reasoning_config::{
            NewReasoningConfig, NewReasoningConfigPreset, ReasoningConfig, ReasoningConfigMode,
            ReasoningConfigPreset, ReasoningConfigPresetInput, ReasoningConfigPresetView,
            ReasoningConfigScope, ReasoningConfigWithPresets, ReasoningPatchFamily,
            ReasoningPreset,
        },
    },
    utils::ID_GENERATOR,
};

use super::{PortableRepositoryConnection, map_write_error};

#[derive(Debug, Clone)]
pub(crate) struct ReasoningConfigImportInput {
    pub scope: ReasoningConfigScope,
    pub owner_id: i64,
    pub mode: ReasoningConfigMode,
    pub family_key: Option<String>,
    pub presets: Vec<ReasoningConfigPresetInput>,
    pub now: i64,
}

pub(crate) fn get_active_provider_reasoning_config(
    conn: &mut PortableRepositoryConnection<'_>,
    provider_id: i64,
) -> DbResult<Option<ReasoningConfigWithPresets>> {
    get_active_reasoning_config(conn, ReasoningConfigScope::Provider, provider_id)
}

pub(crate) fn get_active_model_reasoning_config(
    conn: &mut PortableRepositoryConnection<'_>,
    model_id: i64,
) -> DbResult<Option<ReasoningConfigWithPresets>> {
    get_active_reasoning_config(conn, ReasoningConfigScope::Model, model_id)
}

pub(crate) fn upsert_reasoning_config(
    conn: &mut PortableRepositoryConnection<'_>,
    input: &ReasoningConfigImportInput,
) -> DbResult<ReasoningConfigWithPresets> {
    let normalized = normalize_import_input(input)?;
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::{reasoning_config, reasoning_config_preset};
            use crate::database::reasoning_config::_postgres_model::{
                NewReasoningConfigDb, NewReasoningConfigPresetDb, ReasoningConfigDb,
            };

            let existing = find_config_row_postgres(*conn, input.scope, input.owner_id)?;
            let config_id = if let Some(existing) = existing {
                diesel::update(reasoning_config::table.find(existing.id))
                    .set((
                        reasoning_config::dsl::mode.eq(input.mode.as_key()),
                        reasoning_config::dsl::family_key.eq(normalized.family_key.clone()),
                        reasoning_config::dsl::updated_at.eq(input.now),
                    ))
                    .returning(reasoning_config::dsl::id)
                    .get_result::<i64>(*conn)
                    .map_err(|err| map_write_error("Failed to update reasoning config", err))?
            } else {
                let new_config = NewReasoningConfig {
                    id: ID_GENERATOR.generate_id(),
                    scope_kind: input.scope.as_key().to_string(),
                    provider_id: matches!(input.scope, ReasoningConfigScope::Provider)
                        .then_some(input.owner_id),
                    model_id: matches!(input.scope, ReasoningConfigScope::Model)
                        .then_some(input.owner_id),
                    mode: input.mode.as_key().to_string(),
                    family_key: normalized.family_key.clone(),
                    created_at: input.now,
                    updated_at: input.now,
                };
                diesel::insert_into(reasoning_config::table)
                    .values(NewReasoningConfigDb::to_db(&new_config))
                    .returning(reasoning_config::dsl::id)
                    .get_result::<i64>(*conn)
                    .map_err(|err| map_write_error("Failed to create reasoning config", err))?
            };

            diesel::update(
                reasoning_config_preset::table.filter(
                    reasoning_config_preset::dsl::config_id
                        .eq(config_id)
                        .and(reasoning_config_preset::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                reasoning_config_preset::dsl::deleted_at.eq(Some(input.now)),
                reasoning_config_preset::dsl::is_enabled.eq(false),
                reasoning_config_preset::dsl::updated_at.eq(input.now),
            ))
            .execute(*conn)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to replace reasoning config presets: {err}"
                )))
            })?;

            for preset in &normalized.presets {
                let new_preset = NewReasoningConfigPreset {
                    id: ID_GENERATOR.generate_id(),
                    config_id,
                    preset_key: preset.preset_key.clone(),
                    expose_in_models: preset.expose_in_models,
                    is_enabled: preset.is_enabled,
                    created_at: input.now,
                    updated_at: input.now,
                };
                diesel::insert_into(reasoning_config_preset::table)
                    .values(NewReasoningConfigPresetDb::to_db(&new_preset))
                    .execute(*conn)
                    .map_err(|err| {
                        map_write_error("Failed to create reasoning config preset", err)
                    })?;
            }

            let config = reasoning_config::table
                .find(config_id)
                .select(ReasoningConfigDb::as_select())
                .first::<ReasoningConfigDb>(*conn)
                .map(ReasoningConfigDb::from_db)
                .map_err(|err| map_write_error("Failed to reload reasoning config", err))?;
            load_snapshot_postgres(*conn, config)
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::{reasoning_config, reasoning_config_preset};
            use crate::database::reasoning_config::_sqlite_model::{
                NewReasoningConfigDb, NewReasoningConfigPresetDb, ReasoningConfigDb,
            };

            let existing = find_config_row_sqlite(*conn, input.scope, input.owner_id)?;
            let config_id = if let Some(existing) = existing {
                diesel::update(reasoning_config::table.find(existing.id))
                    .set((
                        reasoning_config::dsl::mode.eq(input.mode.as_key()),
                        reasoning_config::dsl::family_key.eq(normalized.family_key.clone()),
                        reasoning_config::dsl::updated_at.eq(input.now),
                    ))
                    .returning(reasoning_config::dsl::id)
                    .get_result::<i64>(*conn)
                    .map_err(|err| map_write_error("Failed to update reasoning config", err))?
            } else {
                let new_config = NewReasoningConfig {
                    id: ID_GENERATOR.generate_id(),
                    scope_kind: input.scope.as_key().to_string(),
                    provider_id: matches!(input.scope, ReasoningConfigScope::Provider)
                        .then_some(input.owner_id),
                    model_id: matches!(input.scope, ReasoningConfigScope::Model)
                        .then_some(input.owner_id),
                    mode: input.mode.as_key().to_string(),
                    family_key: normalized.family_key.clone(),
                    created_at: input.now,
                    updated_at: input.now,
                };
                diesel::insert_into(reasoning_config::table)
                    .values(NewReasoningConfigDb::to_db(&new_config))
                    .returning(reasoning_config::dsl::id)
                    .get_result::<i64>(*conn)
                    .map_err(|err| map_write_error("Failed to create reasoning config", err))?
            };

            diesel::update(
                reasoning_config_preset::table.filter(
                    reasoning_config_preset::dsl::config_id
                        .eq(config_id)
                        .and(reasoning_config_preset::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                reasoning_config_preset::dsl::deleted_at.eq(Some(input.now)),
                reasoning_config_preset::dsl::is_enabled.eq(false),
                reasoning_config_preset::dsl::updated_at.eq(input.now),
            ))
            .execute(*conn)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to replace reasoning config presets: {err}"
                )))
            })?;

            for preset in &normalized.presets {
                let new_preset = NewReasoningConfigPreset {
                    id: ID_GENERATOR.generate_id(),
                    config_id,
                    preset_key: preset.preset_key.clone(),
                    expose_in_models: preset.expose_in_models,
                    is_enabled: preset.is_enabled,
                    created_at: input.now,
                    updated_at: input.now,
                };
                diesel::insert_into(reasoning_config_preset::table)
                    .values(NewReasoningConfigPresetDb::to_db(&new_preset))
                    .execute(*conn)
                    .map_err(|err| {
                        map_write_error("Failed to create reasoning config preset", err)
                    })?;
            }

            let config = reasoning_config::table
                .find(config_id)
                .select(ReasoningConfigDb::as_select())
                .first::<ReasoningConfigDb>(*conn)
                .map(ReasoningConfigDb::from_db)
                .map_err(|err| map_write_error("Failed to reload reasoning config", err))?;
            load_snapshot_sqlite(*conn, config)
        }
    }
}

fn get_active_reasoning_config(
    conn: &mut PortableRepositoryConnection<'_>,
    scope: ReasoningConfigScope,
    owner_id: i64,
) -> DbResult<Option<ReasoningConfigWithPresets>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            let Some(config) = find_config_row_postgres(*conn, scope, owner_id)? else {
                return Ok(None);
            };
            load_snapshot_postgres(*conn, config).map(Some)
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            let Some(config) = find_config_row_sqlite(*conn, scope, owner_id)? else {
                return Ok(None);
            };
            load_snapshot_sqlite(*conn, config).map(Some)
        }
    }
}

fn find_config_row_postgres(
    conn: &mut diesel::PgConnection,
    scope: ReasoningConfigScope,
    owner_id: i64,
) -> DbResult<Option<ReasoningConfig>> {
    use crate::database::_postgres_schema::reasoning_config;
    use crate::database::reasoning_config::_postgres_model::ReasoningConfigDb;

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
        ReasoningConfigScope::Model => query.filter(reasoning_config::dsl::model_id.eq(owner_id)),
    };

    query
        .select(ReasoningConfigDb::as_select())
        .first::<ReasoningConfigDb>(conn)
        .optional()
        .map(|row| row.map(ReasoningConfigDb::from_db))
        .map_err(|err| map_write_error("Failed to lookup reasoning config", err))
}

fn find_config_row_sqlite(
    conn: &mut diesel::SqliteConnection,
    scope: ReasoningConfigScope,
    owner_id: i64,
) -> DbResult<Option<ReasoningConfig>> {
    use crate::database::_sqlite_schema::reasoning_config;
    use crate::database::reasoning_config::_sqlite_model::ReasoningConfigDb;

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
        ReasoningConfigScope::Model => query.filter(reasoning_config::dsl::model_id.eq(owner_id)),
    };

    query
        .select(ReasoningConfigDb::as_select())
        .first::<ReasoningConfigDb>(conn)
        .optional()
        .map(|row| row.map(ReasoningConfigDb::from_db))
        .map_err(|err| map_write_error("Failed to lookup reasoning config", err))
}

fn load_snapshot_postgres(
    conn: &mut diesel::PgConnection,
    config: ReasoningConfig,
) -> DbResult<ReasoningConfigWithPresets> {
    use crate::database::_postgres_schema::reasoning_config_preset;
    use crate::database::reasoning_config::_postgres_model::ReasoningConfigPresetDb;

    let presets = reasoning_config_preset::table
        .filter(
            reasoning_config_preset::dsl::config_id
                .eq(config.id)
                .and(reasoning_config_preset::dsl::deleted_at.is_null()),
        )
        .order(reasoning_config_preset::dsl::preset_key.asc())
        .select(ReasoningConfigPresetDb::as_select())
        .load::<ReasoningConfigPresetDb>(conn)
        .map(|rows| {
            rows.into_iter()
                .map(ReasoningConfigPresetDb::from_db)
                .collect()
        })
        .map_err(|err| map_write_error("Failed to load reasoning config presets", err))?;
    build_snapshot(config, presets)
}

fn load_snapshot_sqlite(
    conn: &mut diesel::SqliteConnection,
    config: ReasoningConfig,
) -> DbResult<ReasoningConfigWithPresets> {
    use crate::database::_sqlite_schema::reasoning_config_preset;
    use crate::database::reasoning_config::_sqlite_model::ReasoningConfigPresetDb;

    let presets = reasoning_config_preset::table
        .filter(
            reasoning_config_preset::dsl::config_id
                .eq(config.id)
                .and(reasoning_config_preset::dsl::deleted_at.is_null()),
        )
        .order(reasoning_config_preset::dsl::preset_key.asc())
        .select(ReasoningConfigPresetDb::as_select())
        .load::<ReasoningConfigPresetDb>(conn)
        .map(|rows| {
            rows.into_iter()
                .map(ReasoningConfigPresetDb::from_db)
                .collect()
        })
        .map_err(|err| map_write_error("Failed to load reasoning config presets", err))?;
    build_snapshot(config, presets)
}

fn build_snapshot(
    config: ReasoningConfig,
    preset_rows: Vec<ReasoningConfigPreset>,
) -> DbResult<ReasoningConfigWithPresets> {
    let scope = ReasoningConfigScope::from_str(&config.scope_kind)
        .map_err(|err| BaseError::DatabaseFatal(Some(err.to_string())))?;
    let mode = ReasoningConfigMode::from_str(&config.mode)
        .map_err(|err| BaseError::DatabaseFatal(Some(err.to_string())))?;
    let family = match config.family_key.as_deref() {
        Some(value) => Some(
            ReasoningPatchFamily::from_str(value)
                .map_err(|err| BaseError::DatabaseFatal(Some(err.to_string())))?,
        ),
        None => None,
    };
    let presets = preset_rows
        .into_iter()
        .map(|preset| {
            let preset_key = ReasoningPreset::from_str(&preset.preset_key)
                .map_err(|err| BaseError::DatabaseFatal(Some(err.to_string())))?;
            let metadata = preset_key.metadata();
            Ok(ReasoningConfigPresetView {
                preset,
                preset_key,
                suffix: metadata.suffix,
                requires_reasoning: metadata.requires_reasoning,
                allowed_operation_kinds: metadata.allowed_operation_kinds,
            })
        })
        .collect::<DbResult<Vec<_>>>()?;

    Ok(ReasoningConfigWithPresets {
        config,
        scope,
        mode,
        family,
        presets,
    })
}

#[derive(Debug, Clone)]
struct NormalizedReasoningImportInput {
    family_key: Option<String>,
    presets: Vec<ReasoningConfigPresetInput>,
}

fn normalize_import_input(
    input: &ReasoningConfigImportInput,
) -> DbResult<NormalizedReasoningImportInput> {
    if matches!(input.scope, ReasoningConfigScope::Provider)
        && matches!(input.mode, ReasoningConfigMode::Disabled)
    {
        return Err(BaseError::ParamInvalid(Some(
            "provider reasoning config does not support disabled mode".to_string(),
        )));
    }

    match input.mode {
        ReasoningConfigMode::Custom => {
            let family_key = input.family_key.as_deref().ok_or_else(|| {
                BaseError::ParamInvalid(Some(
                    "custom reasoning config requires family_key".to_string(),
                ))
            })?;
            let family = ReasoningPatchFamily::from_str(family_key)
                .map_err(|err| BaseError::ParamInvalid(Some(err.to_string())))?;
            let mut seen = HashSet::new();
            let mut presets = Vec::with_capacity(input.presets.len());
            for preset in &input.presets {
                let preset_key = ReasoningPreset::from_str(&preset.preset_key)
                    .map_err(|err| BaseError::ParamInvalid(Some(err.to_string())))?;
                if !seen.insert(preset_key) {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "duplicate reasoning preset '{}'",
                        preset_key.as_key()
                    ))));
                }
                if let Some(reason) = family.unsupported_preset_reason(preset_key) {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "reasoning family '{}' does not support preset '{}': {}",
                        family.as_key(),
                        preset_key.as_key(),
                        reason
                    ))));
                }
                presets.push(ReasoningConfigPresetInput {
                    preset_key: preset_key.as_key().to_string(),
                    expose_in_models: preset.expose_in_models,
                    is_enabled: preset.is_enabled,
                });
            }
            Ok(NormalizedReasoningImportInput {
                family_key: Some(family.as_key().to_string()),
                presets,
            })
        }
        ReasoningConfigMode::Disabled => {
            if input.family_key.is_some() {
                return Err(BaseError::ParamInvalid(Some(
                    "disabled reasoning config must not include family_key".to_string(),
                )));
            }
            if !input.presets.is_empty() {
                return Err(BaseError::ParamInvalid(Some(
                    "disabled reasoning config must not include preset rows".to_string(),
                )));
            }
            Ok(NormalizedReasoningImportInput {
                family_key: None,
                presets: Vec::new(),
            })
        }
    }
}
