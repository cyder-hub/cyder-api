use std::collections::{BTreeSet, HashMap};

use chrono::Utc;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::controller::BaseError;
use crate::database::model::{Model, NewModel};
use crate::database::{DbResult, get_connection};
use crate::{db_execute, db_object};

pub const INHERIT_ALL_MODE: &str = "INHERIT_ALL";
pub const EXPLICIT_MODE: &str = "EXPLICIT";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSourceBindingInput {
    pub source_id: i64,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSourceConfig {
    pub source_selection_mode: String,
    #[serde(default)]
    pub bindings: Vec<ModelSourceBindingInput>,
}

impl ModelSourceConfig {
    pub fn inherit_all() -> Self {
        Self {
            source_selection_mode: INHERIT_ALL_MODE.to_string(),
            bindings: Vec::new(),
        }
    }

    pub fn explicit(bindings: Vec<ModelSourceBindingInput>) -> Self {
        Self {
            source_selection_mode: EXPLICIT_MODE.to_string(),
            bindings,
        }
    }

    fn normalized(&self) -> DbResult<Self> {
        validate_config_shape(self)?;
        let mut normalized = self.clone();
        normalized.bindings.sort_by_key(|binding| binding.source_id);
        Ok(normalized)
    }
}

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, serde::Serialize)]
    #[diesel(table_name = model_source_binding)]
    #[diesel(primary_key(model_id, source_id))]
    pub struct ModelSourceBinding {
        pub model_id: i64,
        pub source_id: i64,
        pub is_default: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Debug, Clone)]
    #[diesel(table_name = model_source_binding)]
    pub struct NewModelSourceBinding {
        pub model_id: i64,
        pub source_id: i64,
        pub is_default: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

pub fn validate_config_shape(config: &ModelSourceConfig) -> DbResult<()> {
    match config.source_selection_mode.as_str() {
        INHERIT_ALL_MODE if !config.bindings.is_empty() => Err(BaseError::ParamInvalid(Some(
            "INHERIT_ALL model source config must not contain bindings".to_string(),
        ))),
        INHERIT_ALL_MODE => Ok(()),
        EXPLICIT_MODE => {
            let mut source_ids = BTreeSet::new();
            let mut default_count = 0;
            for binding in &config.bindings {
                if binding.source_id <= 0 {
                    return Err(BaseError::ParamInvalid(Some(
                        "model source binding source_id must be positive".to_string(),
                    )));
                }
                if !source_ids.insert(binding.source_id) {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "model source {} is bound more than once",
                        binding.source_id
                    ))));
                }
                if binding.is_default {
                    default_count += 1;
                }
            }
            if default_count > 1 {
                return Err(BaseError::ParamInvalid(Some(
                    "a model may have at most one default source".to_string(),
                )));
            }
            Ok(())
        }
        _ => Err(BaseError::ParamInvalid(Some(
            "source_selection_mode must be INHERIT_ALL or EXPLICIT".to_string(),
        ))),
    }
}

fn source_validation_error(message: impl Into<String>) -> BaseError {
    BaseError::ParamInvalid(Some(message.into()))
}

fn validate_provider_and_sources(
    provider_id: i64,
    config: &ModelSourceConfig,
    source_rows: &[(i64, i64, Option<i64>)],
) -> DbResult<()> {
    let expected_ids = config
        .bindings
        .iter()
        .map(|binding| binding.source_id)
        .collect::<BTreeSet<_>>();
    let actual_ids = source_rows
        .iter()
        .map(|(source_id, _, _)| *source_id)
        .collect::<BTreeSet<_>>();

    if expected_ids != actual_ids {
        let missing = expected_ids
            .difference(&actual_ids)
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        return Err(source_validation_error(format!(
            "model source config references missing source(s): {}",
            missing.join(", ")
        )));
    }

    for (source_id, source_provider_id, deleted_at) in source_rows {
        if deleted_at.is_some() {
            return Err(source_validation_error(format!(
                "deleted upstream source {} cannot be bound",
                source_id
            )));
        }
        if *source_provider_id != provider_id {
            return Err(source_validation_error(format!(
                "upstream source {} does not belong to provider {}",
                source_id, provider_id
            )));
        }
    }

    Ok(())
}

macro_rules! load_provider_status {
    ($conn:expr, $provider_id:expr) => {{
        let provider_exists = provider::table
            .filter(provider::dsl::id.eq($provider_id))
            .select((provider::dsl::id, provider::dsl::deleted_at))
            .first::<(i64, Option<i64>)>($conn)
            .optional()
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "failed to validate provider {}: {error}",
                    $provider_id
                )))
            })?;

        match provider_exists {
            Some((_, None)) => Ok::<(), BaseError>(()),
            Some((_, Some(_))) => Err(BaseError::NotFound(Some(format!(
                "provider {} is deleted",
                $provider_id
            )))),
            None => Err(BaseError::NotFound(Some(format!(
                "provider {} not found",
                $provider_id
            )))),
        }
    }};
}

macro_rules! load_source_rows {
    ($conn:expr, $source_ids:expr) => {{
        if $source_ids.is_empty() {
            Ok::<Vec<(i64, i64, Option<i64>)>, BaseError>(Vec::new())
        } else {
            upstream_source::table
                .filter(upstream_source::dsl::id.eq_any($source_ids))
                .select((
                    upstream_source::dsl::id,
                    upstream_source::dsl::provider_id,
                    upstream_source::dsl::deleted_at,
                ))
                .load::<(i64, i64, Option<i64>)>($conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to load model source bindings: {error}"
                    )))
                })
        }
    }};
}

macro_rules! insert_model_source_bindings {
    ($conn:expr, $model_id:expr, $config:expr, $now:expr) => {{
        for binding in &$config.bindings {
            diesel::insert_into(model_source_binding::table)
                .values((
                    model_source_binding::dsl::model_id.eq($model_id),
                    model_source_binding::dsl::source_id.eq(binding.source_id),
                    model_source_binding::dsl::is_default.eq(binding.is_default),
                    model_source_binding::dsl::created_at.eq($now),
                    model_source_binding::dsl::updated_at.eq($now),
                ))
                .execute($conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to insert model source binding: {error}"
                    )))
                })?;
        }
        Ok::<(), BaseError>(())
    }};
}

pub fn create_model_with_config(
    new_model: &NewModel,
    config: Option<&ModelSourceConfig>,
) -> DbResult<Model> {
    let effective_config = config
        .cloned()
        .unwrap_or_else(ModelSourceConfig::inherit_all)
        .normalized()?;

    if new_model.source_selection_mode != effective_config.source_selection_mode {
        return Err(source_validation_error(
            "model source mode and source config mode must match",
        ));
    }

    let source_ids = effective_config
        .bindings
        .iter()
        .map(|binding| binding.source_id)
        .collect::<Vec<_>>();
    let conn = &mut get_connection()?;

    db_execute!(conn, {
        conn.transaction::<i64, BaseError, _>(|conn| {
            load_provider_status!(conn, new_model.provider_id)?;
            let source_rows = load_source_rows!(conn, &source_ids)?;
            validate_provider_and_sources(new_model.provider_id, &effective_config, &source_rows)?;

            diesel::insert_into(model::table)
                .values((
                    model::dsl::id.eq(new_model.id),
                    model::dsl::provider_id.eq(new_model.provider_id),
                    model::dsl::model_name.eq(new_model.model_name.clone()),
                    model::dsl::real_model_name.eq(new_model.real_model_name.clone()),
                    model::dsl::source_selection_mode.eq(new_model.source_selection_mode.clone()),
                    model::dsl::is_enabled.eq(new_model.is_enabled),
                    model::dsl::created_at.eq(new_model.created_at),
                    model::dsl::updated_at.eq(new_model.updated_at),
                ))
                .execute(conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to create model with source config: {error}"
                    )))
                })?;

            insert_model_source_bindings!(
                conn,
                new_model.id,
                effective_config,
                new_model.created_at
            )?;
            Ok(new_model.id)
        })
    })
    .and_then(Model::get_by_id)
}

pub fn replace_for_model(
    model_id_value: i64,
    config: &ModelSourceConfig,
) -> DbResult<ModelSourceConfig> {
    let normalized = config.normalized()?;
    let source_ids = normalized
        .bindings
        .iter()
        .map(|binding| binding.source_id)
        .collect::<Vec<_>>();
    let now = Utc::now().timestamp_millis();
    let conn = &mut get_connection()?;

    db_execute!(conn, {
        conn.transaction::<ModelSourceConfig, BaseError, _>(|conn| {
            let (provider_id, deleted_at) = model::table
                .filter(model::dsl::id.eq(model_id_value))
                .select((model::dsl::provider_id, model::dsl::deleted_at))
                .first::<(i64, Option<i64>)>(conn)
                .optional()
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to load model {} for source config replacement: {error}",
                        model_id_value
                    )))
                })?
                .ok_or_else(|| {
                    BaseError::NotFound(Some(format!("model {} not found", model_id_value)))
                })?;

            if deleted_at.is_some() {
                return Err(BaseError::NotFound(Some(format!(
                    "model {} is deleted",
                    model_id_value
                ))));
            }

            load_provider_status!(conn, provider_id)?;
            let source_rows = load_source_rows!(conn, &source_ids)?;
            validate_provider_and_sources(provider_id, &normalized, &source_rows)?;

            diesel::delete(
                model_source_binding::table
                    .filter(model_source_binding::dsl::model_id.eq(model_id_value)),
            )
            .execute(conn)
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "failed to clear model {} source bindings: {error}",
                    model_id_value
                )))
            })?;

            diesel::update(model::table.filter(model::dsl::id.eq(model_id_value)))
                .set((
                    model::dsl::source_selection_mode.eq(normalized.source_selection_mode.clone()),
                    model::dsl::updated_at.eq(now),
                ))
                .execute(conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to update model {} source selection mode: {error}",
                        model_id_value
                    )))
                })?;

            insert_model_source_bindings!(conn, model_id_value, normalized, now)?;
            Ok(normalized)
        })
    })
}

pub fn get_config(model_id_value: i64) -> DbResult<ModelSourceConfig> {
    let model = Model::get_by_id(model_id_value)?;
    let bindings = list_visible_by_model_id(model_id_value)?;
    Ok(ModelSourceConfig {
        source_selection_mode: model.source_selection_mode,
        bindings: bindings
            .into_iter()
            .map(|binding| ModelSourceBindingInput {
                source_id: binding.source_id,
                is_default: binding.is_default,
            })
            .collect(),
    })
}

pub fn list_all_by_model_id(model_id_value: i64) -> DbResult<Vec<ModelSourceBinding>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        model_source_binding::table
            .filter(model_source_binding::dsl::model_id.eq(model_id_value))
            .order(model_source_binding::dsl::source_id.asc())
            .select(ModelSourceBindingDb::as_select())
            .load::<ModelSourceBindingDb>(conn)
            .map(|rows| {
                rows.into_iter()
                    .map(ModelSourceBindingDb::from_db)
                    .collect()
            })
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "failed to load all source bindings for model {}: {error}",
                    model_id_value
                )))
            })
    })
}

pub fn list_visible_by_model_id(model_id_value: i64) -> DbResult<Vec<ModelSourceBinding>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        model_source_binding::table
            .inner_join(
                upstream_source::table
                    .on(upstream_source::dsl::id.eq(model_source_binding::dsl::source_id)),
            )
            .filter(
                model_source_binding::dsl::model_id
                    .eq(model_id_value)
                    .and(upstream_source::dsl::deleted_at.is_null()),
            )
            .order(model_source_binding::dsl::source_id.asc())
            .select(ModelSourceBindingDb::as_select())
            .load::<ModelSourceBindingDb>(conn)
            .map(|rows| {
                rows.into_iter()
                    .map(ModelSourceBindingDb::from_db)
                    .collect()
            })
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "failed to load visible source bindings for model {}: {error}",
                    model_id_value
                )))
            })
    })
}

pub fn list_visible_by_model_ids(
    model_ids: &[i64],
) -> DbResult<HashMap<i64, Vec<ModelSourceBinding>>> {
    if model_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let conn = &mut get_connection()?;
    db_execute!(conn, {
        let rows = model_source_binding::table
            .inner_join(
                upstream_source::table
                    .on(upstream_source::dsl::id.eq(model_source_binding::dsl::source_id)),
            )
            .filter(
                model_source_binding::dsl::model_id
                    .eq_any(model_ids)
                    .and(upstream_source::dsl::deleted_at.is_null()),
            )
            .order((
                model_source_binding::dsl::model_id.asc(),
                model_source_binding::dsl::source_id.asc(),
            ))
            .select(ModelSourceBindingDb::as_select())
            .load::<ModelSourceBindingDb>(conn)
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "failed to load visible source bindings for models: {error}"
                )))
            })?;

        let mut grouped = HashMap::new();
        for row in rows {
            let row = row.from_db();
            grouped
                .entry(row.model_id)
                .or_insert_with(Vec::new)
                .push(row);
        }
        Ok(grouped)
    })
}

pub fn list_visible_by_provider_id(
    provider_id_value: i64,
) -> DbResult<HashMap<i64, Vec<ModelSourceBinding>>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        let rows = model_source_binding::table
            .inner_join(model::table.on(model::dsl::id.eq(model_source_binding::dsl::model_id)))
            .inner_join(
                upstream_source::table
                    .on(upstream_source::dsl::id.eq(model_source_binding::dsl::source_id)),
            )
            .filter(
                model::dsl::provider_id
                    .eq(provider_id_value)
                    .and(model::dsl::deleted_at.is_null())
                    .and(upstream_source::dsl::deleted_at.is_null()),
            )
            .order((
                model_source_binding::dsl::model_id.asc(),
                model_source_binding::dsl::source_id.asc(),
            ))
            .select(ModelSourceBindingDb::as_select())
            .load::<ModelSourceBindingDb>(conn)
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "failed to load visible source bindings for provider {}: {error}",
                    provider_id_value
                )))
            })?;

        let mut grouped = HashMap::new();
        for row in rows {
            let row = row.from_db();
            grouped
                .entry(row.model_id)
                .or_insert_with(Vec::new)
                .push(row);
        }
        Ok(grouped)
    })
}

pub fn list_visible_by_source_id(source_id_value: i64) -> DbResult<Vec<ModelSourceBinding>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        model_source_binding::table
            .inner_join(model::table.on(model::dsl::id.eq(model_source_binding::dsl::model_id)))
            .inner_join(
                upstream_source::table
                    .on(upstream_source::dsl::id.eq(model_source_binding::dsl::source_id)),
            )
            .filter(
                model_source_binding::dsl::source_id
                    .eq(source_id_value)
                    .and(model::dsl::deleted_at.is_null())
                    .and(upstream_source::dsl::deleted_at.is_null()),
            )
            .order(model_source_binding::dsl::model_id.asc())
            .select(ModelSourceBindingDb::as_select())
            .load::<ModelSourceBindingDb>(conn)
            .map(|rows| {
                rows.into_iter()
                    .map(ModelSourceBindingDb::from_db)
                    .collect()
            })
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "failed to load visible source bindings for source {}: {error}",
                    source_id_value
                )))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::TestDbContext;
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::upstream_source::{NewUpstreamSource, UpstreamSource};
    use crate::schema::enum_def::{ProviderApiKeyMode, UpstreamProfileType};

    fn provider_input(id: i64, key: &str) -> NewProvider {
        NewProvider {
            id,
            provider_key: key.to_string(),
            name: key.to_string(),
            is_enabled: true,
            created_at: 1,
            updated_at: 1,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        }
    }

    fn source_input(id: i64, provider_id: i64, is_default: bool) -> NewUpstreamSource {
        NewUpstreamSource {
            id,
            provider_id,
            profile_type: UpstreamProfileType::Openai,
            endpoint: format!("https://example.com/{id}"),
            use_proxy: false,
            is_enabled: true,
            is_default,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn repository_validates_and_atomically_replaces_model_sources() {
        let db = TestDbContext::new_sqlite("model-source-binding.sqlite");
        db.run_sync(|| {
            Provider::create(
                &provider_input(7001, "provider-a"),
                &source_input(7101, 7001, true),
            )
            .expect("provider should be created");
            let mut second_source = source_input(7102, 7001, false);
            second_source.profile_type = UpstreamProfileType::Anthropic;
            let second =
                UpstreamSource::create(&second_source).expect("second source should be created");
            assert_eq!(second.id, 7102);

            let model = Model::create_with_source_config(
                7001,
                "model-a",
                None,
                true,
                Some(&ModelSourceConfig::explicit(vec![
                    ModelSourceBindingInput {
                        source_id: 7102,
                        is_default: true,
                    },
                ])),
            )
            .expect("model with explicit source config should be created");
            assert_eq!(
                get_config(model.id).expect("config should load"),
                ModelSourceConfig::explicit(vec![ModelSourceBindingInput {
                    source_id: 7102,
                    is_default: true,
                }])
            );

            let invalid = ModelSourceConfig::explicit(vec![ModelSourceBindingInput {
                source_id: 7102,
                is_default: false,
            }]);
            replace_for_model(model.id, &invalid).expect("valid replacement should succeed");
            assert!(
                !list_all_by_model_id(model.id)
                    .expect("all bindings should load")
                    .first()
                    .expect("one binding should remain")
                    .is_default
            );

            let duplicate = ModelSourceConfig::explicit(vec![
                ModelSourceBindingInput {
                    source_id: 7101,
                    is_default: false,
                },
                ModelSourceBindingInput {
                    source_id: 7101,
                    is_default: true,
                },
            ]);
            assert!(matches!(
                replace_for_model(model.id, &duplicate),
                Err(BaseError::ParamInvalid(_))
            ));
            assert_eq!(
                list_all_by_model_id(model.id)
                    .expect("rollback should preserve row")
                    .len(),
                1
            );

            replace_for_model(model.id, &ModelSourceConfig::inherit_all())
                .expect("inherit mode should clear bindings");
            assert!(
                list_all_by_model_id(model.id)
                    .expect("bindings should load")
                    .is_empty()
            );
        });
    }

    #[test]
    fn disabled_sources_are_allowed_but_deleted_sources_are_not() {
        let db = TestDbContext::new_sqlite("model-source-disabled.sqlite");
        db.run_sync(|| {
            Provider::create(
                &provider_input(7201, "provider-b"),
                &source_input(7201, 7201, true),
            )
            .expect("provider should be created");
            UpstreamSource::update(
                7201,
                7201,
                &crate::database::upstream_source::UpdateUpstreamSourceData {
                    endpoint: None,
                    use_proxy: None,
                    is_enabled: Some(false),
                    is_default: Some(false),
                    updated_at: 2,
                },
            )
            .expect("source should be disabled");

            let model = Model::create_with_source_config(
                7201,
                "model-b",
                None,
                true,
                Some(&ModelSourceConfig::explicit(vec![
                    ModelSourceBindingInput {
                        source_id: 7201,
                        is_default: true,
                    },
                ])),
            )
            .expect("disabled source should remain bindable");
            UpstreamSource::delete(7201, 7201).expect("source should be soft deleted");
            assert!(
                get_config(model.id)
                    .expect("visible config should load")
                    .bindings
                    .is_empty()
            );
            assert!(matches!(
                replace_for_model(
                    model.id,
                    &ModelSourceConfig::explicit(vec![ModelSourceBindingInput {
                        source_id: 7201,
                        is_default: true,
                    }])
                ),
                Err(BaseError::ParamInvalid(_))
            ));
            assert_eq!(
                list_all_by_model_id(model.id)
                    .expect("hidden tombstone should remain")
                    .len(),
                1
            );
        });
    }
}
