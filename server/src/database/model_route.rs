use std::collections::HashSet;

use chrono::Utc;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::{DbResult, api_key::ApiKey, get_connection, model::Model};
use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

// `model` remains the canonical provider-scoped candidate identity.
// Shared entry names and caller-scoped overrides live in this module.

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, serde::Serialize)]
    #[diesel(table_name = model_route)]
    pub struct ModelRoute {
        pub id: i64,
        pub route_name: String,
        pub description: Option<String>,
        pub is_enabled: bool,
        pub expose_in_models: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Debug)]
    #[diesel(table_name = model_route)]
    pub struct NewModelRoute {
        pub id: i64,
        pub route_name: String,
        pub description: Option<String>,
        pub is_enabled: bool,
        pub expose_in_models: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset, Debug, Default)]
    #[diesel(table_name = model_route)]
    pub struct UpdateModelRouteData {
        pub route_name: Option<String>,
        pub description: Option<Option<String>>,
        pub is_enabled: Option<bool>,
        pub expose_in_models: Option<bool>,
    }

    #[derive(Queryable, Selectable, Identifiable, Associations, Debug, Clone, serde::Serialize)]
    #[diesel(belongs_to(ModelRoute, foreign_key = route_id))]
    #[diesel(table_name = model_route_candidate)]
    pub struct ModelRouteCandidate {
        pub id: i64,
        pub route_id: i64,
        pub model_id: i64,
        pub priority: i32,
        pub is_enabled: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Debug)]
    #[diesel(table_name = model_route_candidate)]
    pub struct NewModelRouteCandidate {
        pub id: i64,
        pub route_id: i64,
        pub model_id: i64,
        pub priority: i32,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Queryable, Selectable, Identifiable, Associations, Debug, Clone, serde::Serialize)]
    #[diesel(belongs_to(ApiKey, foreign_key = api_key_id))]
    #[diesel(belongs_to(ModelRoute, foreign_key = target_route_id))]
    #[diesel(table_name = api_key_model_override)]
    pub struct ApiKeyModelOverride {
        pub id: i64,
        pub api_key_id: i64,
        pub source_name: String,
        pub target_route_id: i64,
        pub description: Option<String>,
        pub is_enabled: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Debug)]
    #[diesel(table_name = api_key_model_override)]
    pub struct NewApiKeyModelOverride {
        pub id: i64,
        pub api_key_id: i64,
        pub source_name: String,
        pub target_route_id: i64,
        pub description: Option<String>,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset, Debug, Default)]
    #[diesel(table_name = api_key_model_override)]
    pub struct UpdateApiKeyModelOverrideData {
        pub source_name: Option<String>,
        pub target_route_id: Option<i64>,
        pub description: Option<Option<String>>,
        pub is_enabled: Option<bool>,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRouteCandidateInput {
    pub model_id: i64,
    pub priority: i32,
    pub is_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateModelRoutePayload {
    pub route_name: String,
    pub description: Option<String>,
    pub is_enabled: Option<bool>,
    pub expose_in_models: Option<bool>,
    pub candidates: Vec<ModelRouteCandidateInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateModelRoutePayload {
    pub route_name: Option<String>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub description: Option<Option<String>>,
    pub is_enabled: Option<bool>,
    pub expose_in_models: Option<bool>,
    pub candidates: Option<Vec<ModelRouteCandidateInput>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelRouteCandidateDetail {
    pub candidate: ModelRouteCandidate,
    pub provider_id: i64,
    pub provider_key: String,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub model_is_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelRouteDetail {
    pub route: ModelRoute,
    pub candidates: Vec<ModelRouteCandidateDetail>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelRouteListItem {
    pub route: ModelRoute,
    pub candidate_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyModelOverridePayload {
    pub api_key_id: i64,
    pub source_name: String,
    pub target_route_id: i64,
    pub description: Option<String>,
    pub is_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateApiKeyModelOverridePayload {
    pub source_name: Option<String>,
    pub target_route_id: Option<i64>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub description: Option<Option<String>>,
    pub is_enabled: Option<bool>,
}

fn map_write_error(context: &str, err: diesel::result::Error) -> BaseError {
    match err {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        ) => BaseError::DatabaseDup(Some(context.to_string())),
        other => BaseError::DatabaseFatal(Some(format!("{context}: {other}"))),
    }
}

fn normalize_required_name(field: &str, value: &str) -> DbResult<String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(BaseError::ParamInvalid(Some(format!(
            "{field} must not be empty"
        ))));
    }
    Ok(normalized.to_string())
}

fn validate_candidate_inputs(candidates: &[ModelRouteCandidateInput]) -> DbResult<()> {
    if candidates.is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "model route requires at least one candidate".to_string(),
        )));
    }

    let mut seen_model_ids = HashSet::new();
    for candidate in candidates {
        if !seen_model_ids.insert(candidate.model_id) {
            return Err(BaseError::ParamInvalid(Some(format!(
                "route candidate model_id {} is duplicated",
                candidate.model_id
            ))));
        }
    }

    Ok(())
}

fn validate_candidate_models_exist(candidates: &[ModelRouteCandidateInput]) -> DbResult<()> {
    for candidate in candidates {
        Model::get_by_id(candidate.model_id)?;
    }
    Ok(())
}

fn active_direct_model_names() -> DbResult<Vec<String>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        let rows = provider::table
            .inner_join(model::table.on(model::dsl::provider_id.eq(provider::dsl::id)))
            .filter(
                provider::dsl::deleted_at
                    .is_null()
                    .and(provider::dsl::is_enabled.eq(true))
                    .and(model::dsl::deleted_at.is_null())
                    .and(model::dsl::is_enabled.eq(true)),
            )
            .select((provider::dsl::provider_key, model::dsl::model_name))
            .load::<(String, String)>(conn)
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to load active direct provider/model names: {}",
                    e
                )))
            })?;

        Ok(rows
            .into_iter()
            .map(|(provider_key, model_name)| format!("{provider_key}/{model_name}"))
            .collect())
    })
}

fn ensure_not_reserved_direct_model_name(name: &str) -> DbResult<()> {
    if active_direct_model_names()?
        .into_iter()
        .any(|item| item == name)
    {
        return Err(BaseError::ParamInvalid(Some(format!(
            "name '{}' conflicts with an active direct provider/model address",
            name
        ))));
    }
    Ok(())
}

fn make_route_candidate_rows(
    route_id: i64,
    candidates: &[ModelRouteCandidateInput],
    now: i64,
) -> Vec<NewModelRouteCandidate> {
    candidates
        .iter()
        .map(|candidate| NewModelRouteCandidate {
            id: ID_GENERATOR.generate_id(),
            route_id,
            model_id: candidate.model_id,
            priority: candidate.priority,
            is_enabled: candidate.is_enabled.unwrap_or(true),
            created_at: now,
            updated_at: now,
        })
        .collect()
}

impl ModelRoute {
    pub fn get_by_id(id_value: i64) -> DbResult<ModelRoute> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let route = model_route::table
                .filter(
                    model_route::dsl::id
                        .eq(id_value)
                        .and(model_route::dsl::deleted_at.is_null()),
                )
                .select(ModelRouteDb::as_select())
                .first::<ModelRouteDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => {
                        BaseError::NotFound(Some(format!("Model route {} not found", id_value)))
                    }
                    other => BaseError::DatabaseFatal(Some(format!(
                        "Failed to fetch model route {}: {}",
                        id_value, other
                    ))),
                })?;
            Ok(route.from_db())
        })
    }

    pub fn get_active_by_name(name: &str) -> DbResult<Option<ModelRoute>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let route = model_route::table
                .filter(
                    model_route::dsl::route_name
                        .eq(name)
                        .and(model_route::dsl::deleted_at.is_null()),
                )
                .select(ModelRouteDb::as_select())
                .first::<ModelRouteDb>(conn)
                .optional()
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to fetch model route by name '{}': {}",
                        name, e
                    )))
                })?;
            Ok(route.map(ModelRouteDb::from_db))
        })
    }

    pub fn list_summary() -> DbResult<Vec<ModelRouteListItem>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let routes = model_route::table
                .filter(model_route::dsl::deleted_at.is_null())
                .order(model_route::dsl::created_at.desc())
                .select(ModelRouteDb::as_select())
                .load::<ModelRouteDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list model routes: {}", e)))
                })?;

            let mut items = Vec::with_capacity(routes.len());
            for route in routes {
                let route = route.from_db();
                let candidate_count = model_route_candidate::table
                    .inner_join(
                        model::table.on(model_route_candidate::dsl::model_id.eq(model::dsl::id)),
                    )
                    .inner_join(provider::table.on(model::dsl::provider_id.eq(provider::dsl::id)))
                    .filter(
                        model_route_candidate::dsl::route_id
                            .eq(route.id)
                            .and(model_route_candidate::dsl::deleted_at.is_null())
                            .and(model::dsl::deleted_at.is_null())
                            .and(provider::dsl::deleted_at.is_null()),
                    )
                    .count()
                    .get_result::<i64>(conn)
                    .map_err(|e| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to count candidates for route {}: {}",
                            route.id, e
                        )))
                    })?;
                items.push(ModelRouteListItem {
                    route,
                    candidate_count: candidate_count as usize,
                });
            }
            Ok(items)
        })
    }

    pub fn list_by_model_id(model_id_value: i64) -> DbResult<Vec<ModelRoute>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = model_route::table
                .inner_join(
                    model_route_candidate::table
                        .on(model_route::dsl::id.eq(model_route_candidate::dsl::route_id)),
                )
                .filter(
                    model_route::dsl::deleted_at
                        .is_null()
                        .and(model_route_candidate::dsl::deleted_at.is_null())
                        .and(model_route_candidate::dsl::model_id.eq(model_id_value)),
                )
                .select(ModelRouteDb::as_select())
                .distinct()
                .load::<ModelRouteDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list model routes for model {}: {}",
                        model_id_value, e
                    )))
                })?;
            Ok(rows.into_iter().map(ModelRouteDb::from_db).collect())
        })
    }

    pub fn list_by_provider_id(provider_id_value: i64) -> DbResult<Vec<ModelRoute>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = model_route::table
                .inner_join(
                    model_route_candidate::table
                        .on(model_route::dsl::id.eq(model_route_candidate::dsl::route_id)),
                )
                .inner_join(
                    model::table.on(model_route_candidate::dsl::model_id.eq(model::dsl::id)),
                )
                .filter(
                    model_route::dsl::deleted_at
                        .is_null()
                        .and(model_route_candidate::dsl::deleted_at.is_null())
                        .and(model::dsl::deleted_at.is_null())
                        .and(model::dsl::provider_id.eq(provider_id_value)),
                )
                .select(ModelRouteDb::as_select())
                .distinct()
                .load::<ModelRouteDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list model routes for provider {}: {}",
                        provider_id_value, e
                    )))
                })?;
            Ok(rows.into_iter().map(ModelRouteDb::from_db).collect())
        })
    }

    pub fn list_candidate_details(route_id_value: i64) -> DbResult<Vec<ModelRouteCandidateDetail>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = model_route_candidate::table
                .inner_join(
                    model::table.on(model_route_candidate::dsl::model_id.eq(model::dsl::id)),
                )
                .inner_join(provider::table.on(model::dsl::provider_id.eq(provider::dsl::id)))
                .filter(
                    model_route_candidate::dsl::route_id
                        .eq(route_id_value)
                        .and(model_route_candidate::dsl::deleted_at.is_null())
                        .and(model::dsl::deleted_at.is_null())
                        .and(provider::dsl::deleted_at.is_null()),
                )
                .order((
                    model_route_candidate::dsl::priority.asc(),
                    model_route_candidate::dsl::created_at.asc(),
                    model_route_candidate::dsl::id.asc(),
                ))
                .select((
                    ModelRouteCandidateDb::as_select(),
                    model::dsl::provider_id,
                    provider::dsl::provider_key,
                    model::dsl::model_name,
                    model::dsl::real_model_name,
                    model::dsl::is_enabled,
                ))
                .load::<(
                    ModelRouteCandidateDb,
                    i64,
                    String,
                    String,
                    Option<String>,
                    bool,
                )>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list model route candidates for {}: {}",
                        route_id_value, e
                    )))
                })?;

            Ok(rows
                .into_iter()
                .map(
                    |(
                        candidate,
                        provider_id,
                        provider_key,
                        model_name,
                        real_model_name,
                        model_is_enabled,
                    )| {
                        ModelRouteCandidateDetail {
                            candidate: candidate.from_db(),
                            provider_id,
                            provider_key,
                            model_name,
                            real_model_name,
                            model_is_enabled,
                        }
                    },
                )
                .collect())
        })
    }

    pub fn get_detail(id_value: i64) -> DbResult<ModelRouteDetail> {
        let route = Self::get_by_id(id_value)?;
        let candidates = Self::list_candidate_details(id_value)?;
        Ok(ModelRouteDetail { route, candidates })
    }

    pub fn soft_delete_candidates_for_model(model_id_value: i64) -> DbResult<usize> {
        let now = Utc::now().timestamp_millis();
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            diesel::update(
                model_route_candidate::table.filter(
                    model_route_candidate::dsl::model_id
                        .eq(model_id_value)
                        .and(model_route_candidate::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                model_route_candidate::dsl::deleted_at.eq(Some(now)),
                model_route_candidate::dsl::is_enabled.eq(false),
                model_route_candidate::dsl::updated_at.eq(now),
            ))
            .execute(conn)
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to delete model route candidates for model {}: {}",
                    model_id_value, e
                )))
            })
        })
    }

    pub fn soft_delete_candidates_for_provider(provider_id_value: i64) -> DbResult<usize> {
        let now = Utc::now().timestamp_millis();
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let model_ids = model::table
                .filter(model::dsl::provider_id.eq(provider_id_value))
                .select(model::dsl::id)
                .load::<i64>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list models for provider {} while deleting route candidates: {}",
                        provider_id_value, e
                    )))
                })?;

            if model_ids.is_empty() {
                return Ok(0);
            }

            diesel::update(
                model_route_candidate::table.filter(
                    model_route_candidate::dsl::model_id
                        .eq_any(model_ids)
                        .and(model_route_candidate::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                model_route_candidate::dsl::deleted_at.eq(Some(now)),
                model_route_candidate::dsl::is_enabled.eq(false),
                model_route_candidate::dsl::updated_at.eq(now),
            ))
            .execute(conn)
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to delete model route candidates for provider {}: {}",
                    provider_id_value, e
                )))
            })
        })
    }

    pub fn create(payload: &CreateModelRoutePayload) -> DbResult<ModelRouteDetail> {
        let route_name = normalize_required_name("route_name", &payload.route_name)?;
        validate_candidate_inputs(&payload.candidates)?;
        validate_candidate_models_exist(&payload.candidates)?;
        ensure_not_reserved_direct_model_name(&route_name)?;
        if Self::get_active_by_name(&route_name)?.is_some() {
            return Err(BaseError::DatabaseDup(Some(format!(
                "Model route '{}' already exists",
                route_name
            ))));
        }

        let now = Utc::now().timestamp_millis();
        let new_route = NewModelRoute {
            id: ID_GENERATOR.generate_id(),
            route_name,
            description: payload.description.clone(),
            is_enabled: payload.is_enabled.unwrap_or(true),
            expose_in_models: payload.expose_in_models.unwrap_or(true),
            created_at: now,
            updated_at: now,
        };

        let inserted_route = {
            let conn = &mut get_connection()?;
            db_execute!(conn, {
                conn.transaction::<ModelRoute, BaseError, _>(|conn| {
                    let route = diesel::insert_into(model_route::table)
                        .values(NewModelRouteDb::to_db(&new_route))
                        .returning(ModelRouteDb::as_returning())
                        .get_result::<ModelRouteDb>(conn)
                        .map_err(|e| map_write_error("Failed to create model route", e))?
                        .from_db();

                    let candidate_rows =
                        make_route_candidate_rows(route.id, &payload.candidates, now);
                    let candidate_rows_db: Vec<_> = candidate_rows
                        .iter()
                        .map(NewModelRouteCandidateDb::to_db)
                        .collect();
                    diesel::insert_into(model_route_candidate::table)
                        .values(&candidate_rows_db)
                        .execute(conn)
                        .map_err(|e| {
                            map_write_error(
                                &format!(
                                    "Failed to create model route candidates for route {}",
                                    route.id
                                ),
                                e,
                            )
                        })?;

                    Ok(route)
                })
            })?
        };

        Self::get_detail(inserted_route.id)
    }

    pub fn update(id_value: i64, payload: &UpdateModelRoutePayload) -> DbResult<ModelRouteDetail> {
        let existing = Self::get_by_id(id_value)?;
        let route_name = match payload.route_name.as_deref() {
            Some(value) => normalize_required_name("route_name", value)?,
            None => existing.route_name.clone(),
        };
        if let Some(candidates) = payload.candidates.as_ref() {
            validate_candidate_inputs(candidates)?;
            validate_candidate_models_exist(candidates)?;
        }
        ensure_not_reserved_direct_model_name(&route_name)?;
        if let Some(found) = Self::get_active_by_name(&route_name)? {
            if found.id != id_value {
                return Err(BaseError::DatabaseDup(Some(format!(
                    "Model route '{}' already exists",
                    route_name
                ))));
            }
        }

        let now = Utc::now().timestamp_millis();
        let update_data = UpdateModelRouteData {
            route_name: payload.route_name.as_ref().map(|_| route_name.clone()),
            description: payload.description.clone(),
            is_enabled: payload.is_enabled,
            expose_in_models: payload.expose_in_models,
        };

        {
            let conn = &mut get_connection()?;
            db_execute!(conn, {
                conn.transaction::<(), BaseError, _>(|conn| {
                    diesel::update(
                        model_route::table.filter(
                            model_route::dsl::id
                                .eq(id_value)
                                .and(model_route::dsl::deleted_at.is_null()),
                        ),
                    )
                    .set((
                        UpdateModelRouteDataDb::to_db(&update_data),
                        model_route::dsl::updated_at.eq(now),
                    ))
                    .execute(conn)
                    .map_err(|e| {
                        map_write_error(&format!("Failed to update model route {}", id_value), e)
                    })?;

                    if let Some(candidates) = payload.candidates.as_ref() {
                        diesel::update(
                            model_route_candidate::table.filter(
                                model_route_candidate::dsl::route_id
                                    .eq(id_value)
                                    .and(model_route_candidate::dsl::deleted_at.is_null()),
                            ),
                        )
                        .set((
                            model_route_candidate::dsl::deleted_at.eq(Some(now)),
                            model_route_candidate::dsl::updated_at.eq(now),
                        ))
                        .execute(conn)
                        .map_err(|e| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to replace model route candidates for {}: {}",
                                id_value, e
                            )))
                        })?;

                        let candidate_rows = make_route_candidate_rows(id_value, candidates, now);
                        let candidate_rows_db: Vec<_> = candidate_rows
                            .iter()
                            .map(NewModelRouteCandidateDb::to_db)
                            .collect();
                        diesel::insert_into(model_route_candidate::table)
                            .values(&candidate_rows_db)
                            .execute(conn)
                            .map_err(|e| {
                                map_write_error(
                                    &format!(
                                        "Failed to insert replacement model route candidates for {}",
                                        id_value
                                    ),
                                    e,
                                )
                            })?;
                    }

                    Ok(())
                })
            })?;
        }

        Self::get_detail(id_value)
    }

    pub fn delete(id_value: i64) -> DbResult<usize> {
        Self::delete_with_dependents(id_value)
    }

    pub fn delete_with_dependents(id_value: i64) -> DbResult<usize> {
        let now = Utc::now().timestamp_millis();
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            conn.transaction::<usize, BaseError, _>(|conn| {
                let updated = diesel::update(
                    model_route::table.filter(
                        model_route::dsl::id
                            .eq(id_value)
                            .and(model_route::dsl::deleted_at.is_null()),
                    ),
                )
                .set((
                    model_route::dsl::deleted_at.eq(Some(now)),
                    model_route::dsl::is_enabled.eq(false),
                    model_route::dsl::updated_at.eq(now),
                ))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete model route {}: {}",
                        id_value, e
                    )))
                })?;

                if updated == 0 {
                    return Err(BaseError::NotFound(Some(format!(
                        "Model route {} not found",
                        id_value
                    ))));
                }

                diesel::update(
                    model_route_candidate::table.filter(
                        model_route_candidate::dsl::route_id
                            .eq(id_value)
                            .and(model_route_candidate::dsl::deleted_at.is_null()),
                    ),
                )
                .set((
                    model_route_candidate::dsl::deleted_at.eq(Some(now)),
                    model_route_candidate::dsl::is_enabled.eq(false),
                    model_route_candidate::dsl::updated_at.eq(now),
                ))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete model route candidates for {}: {}",
                        id_value, e
                    )))
                })?;

                diesel::update(
                    api_key_model_override::table.filter(
                        api_key_model_override::dsl::target_route_id
                            .eq(id_value)
                            .and(api_key_model_override::dsl::deleted_at.is_null()),
                    ),
                )
                .set((
                    api_key_model_override::dsl::deleted_at.eq(Some(now)),
                    api_key_model_override::dsl::is_enabled.eq(false),
                    api_key_model_override::dsl::updated_at.eq(now),
                ))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete api key model overrides for route {}: {}",
                        id_value, e
                    )))
                })?;

                Ok(updated)
            })
        })
    }
}

impl ApiKeyModelOverride {
    pub fn list_all() -> DbResult<Vec<ApiKeyModelOverride>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = api_key_model_override::table
                .filter(api_key_model_override::dsl::deleted_at.is_null())
                .order(api_key_model_override::dsl::created_at.asc())
                .select(ApiKeyModelOverrideDb::as_select())
                .load::<ApiKeyModelOverrideDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list api key model overrides: {}",
                        e
                    )))
                })?;
            Ok(rows
                .into_iter()
                .map(ApiKeyModelOverrideDb::from_db)
                .collect())
        })
    }

    pub fn get_by_id(id_value: i64) -> DbResult<ApiKeyModelOverride> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = api_key_model_override::table
                .filter(
                    api_key_model_override::dsl::id
                        .eq(id_value)
                        .and(api_key_model_override::dsl::deleted_at.is_null()),
                )
                .select(ApiKeyModelOverrideDb::as_select())
                .first::<ApiKeyModelOverrideDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                        "Api key model override {} not found",
                        id_value
                    ))),
                    other => BaseError::DatabaseFatal(Some(format!(
                        "Failed to fetch api key model override {}: {}",
                        id_value, other
                    ))),
                })?;
            Ok(row.from_db())
        })
    }

    pub fn list_by_api_key_id(api_key_id_value: i64) -> DbResult<Vec<ApiKeyModelOverride>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = api_key_model_override::table
                .filter(
                    api_key_model_override::dsl::api_key_id
                        .eq(api_key_id_value)
                        .and(api_key_model_override::dsl::deleted_at.is_null()),
                )
                .order(api_key_model_override::dsl::created_at.asc())
                .select(ApiKeyModelOverrideDb::as_select())
                .load::<ApiKeyModelOverrideDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list api key model overrides for {}: {}",
                        api_key_id_value, e
                    )))
                })?;
            Ok(rows
                .into_iter()
                .map(ApiKeyModelOverrideDb::from_db)
                .collect())
        })
    }

    pub fn list_by_target_route_id(route_id_value: i64) -> DbResult<Vec<ApiKeyModelOverride>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = api_key_model_override::table
                .filter(
                    api_key_model_override::dsl::target_route_id
                        .eq(route_id_value)
                        .and(api_key_model_override::dsl::deleted_at.is_null()),
                )
                .order(api_key_model_override::dsl::created_at.asc())
                .select(ApiKeyModelOverrideDb::as_select())
                .load::<ApiKeyModelOverrideDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list api key model overrides for route {}: {}",
                        route_id_value, e
                    )))
                })?;
            Ok(rows
                .into_iter()
                .map(ApiKeyModelOverrideDb::from_db)
                .collect())
        })
    }

    pub fn get_active_by_source_name(
        api_key_id_value: i64,
        source_name_value: &str,
    ) -> DbResult<Option<ApiKeyModelOverride>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = api_key_model_override::table
                .filter(
                    api_key_model_override::dsl::api_key_id
                        .eq(api_key_id_value)
                        .and(api_key_model_override::dsl::source_name.eq(source_name_value))
                        .and(api_key_model_override::dsl::deleted_at.is_null()),
                )
                .select(ApiKeyModelOverrideDb::as_select())
                .first::<ApiKeyModelOverrideDb>(conn)
                .optional()
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to fetch api key model override by source name '{}': {}",
                        source_name_value, e
                    )))
                })?;
            Ok(row.map(ApiKeyModelOverrideDb::from_db))
        })
    }

    pub fn create(payload: &CreateApiKeyModelOverridePayload) -> DbResult<ApiKeyModelOverride> {
        let source_name = normalize_required_name("source_name", &payload.source_name)?;
        ApiKey::get_by_id(payload.api_key_id)?;
        ModelRoute::get_by_id(payload.target_route_id)?;
        ensure_not_reserved_direct_model_name(&source_name)?;
        if Self::get_active_by_source_name(payload.api_key_id, &source_name)?.is_some() {
            return Err(BaseError::DatabaseDup(Some(format!(
                "Api key model override '{}' already exists for api key {}",
                source_name, payload.api_key_id
            ))));
        }

        let now = Utc::now().timestamp_millis();
        let new_override = NewApiKeyModelOverride {
            id: ID_GENERATOR.generate_id(),
            api_key_id: payload.api_key_id,
            source_name,
            target_route_id: payload.target_route_id,
            description: payload.description.clone(),
            is_enabled: payload.is_enabled.unwrap_or(true),
            created_at: now,
            updated_at: now,
        };

        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = diesel::insert_into(api_key_model_override::table)
                .values(NewApiKeyModelOverrideDb::to_db(&new_override))
                .returning(ApiKeyModelOverrideDb::as_returning())
                .get_result::<ApiKeyModelOverrideDb>(conn)
                .map_err(|e| map_write_error("Failed to create api key model override", e))?;
            Ok(row.from_db())
        })
    }

    pub fn update(
        id_value: i64,
        payload: &UpdateApiKeyModelOverridePayload,
    ) -> DbResult<ApiKeyModelOverride> {
        let existing = Self::get_by_id(id_value)?;
        let source_name = match payload.source_name.as_deref() {
            Some(value) => normalize_required_name("source_name", value)?,
            None => existing.source_name.clone(),
        };
        let target_route_id = payload.target_route_id.unwrap_or(existing.target_route_id);
        ModelRoute::get_by_id(target_route_id)?;
        ensure_not_reserved_direct_model_name(&source_name)?;
        if let Some(found) = Self::get_active_by_source_name(existing.api_key_id, &source_name)? {
            if found.id != id_value {
                return Err(BaseError::DatabaseDup(Some(format!(
                    "Api key model override '{}' already exists for api key {}",
                    source_name, existing.api_key_id
                ))));
            }
        }

        let now = Utc::now().timestamp_millis();
        let update_data = UpdateApiKeyModelOverrideData {
            source_name: payload.source_name.as_ref().map(|_| source_name),
            target_route_id: payload.target_route_id,
            description: payload.description.clone(),
            is_enabled: payload.is_enabled,
        };

        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = diesel::update(
                api_key_model_override::table.filter(
                    api_key_model_override::dsl::id
                        .eq(id_value)
                        .and(api_key_model_override::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                UpdateApiKeyModelOverrideDataDb::to_db(&update_data),
                api_key_model_override::dsl::updated_at.eq(now),
            ))
            .returning(ApiKeyModelOverrideDb::as_returning())
            .get_result::<ApiKeyModelOverrideDb>(conn)
            .map_err(|e| {
                map_write_error(
                    &format!("Failed to update api key model override {}", id_value),
                    e,
                )
            })?;
            Ok(row.from_db())
        })
    }

    pub fn delete(id_value: i64) -> DbResult<usize> {
        let now = Utc::now().timestamp_millis();
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let updated = diesel::update(
                api_key_model_override::table.filter(
                    api_key_model_override::dsl::id
                        .eq(id_value)
                        .and(api_key_model_override::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                api_key_model_override::dsl::deleted_at.eq(Some(now)),
                api_key_model_override::dsl::is_enabled.eq(false),
                api_key_model_override::dsl::updated_at.eq(now),
            ))
            .execute(conn)
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to delete api key model override {}: {}",
                    id_value, e
                )))
            })?;

            if updated == 0 {
                return Err(BaseError::NotFound(Some(format!(
                    "Api key model override {} not found",
                    id_value
                ))));
            }

            Ok(updated)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ModelRoute, ModelRouteCandidate, ModelRouteCandidateDetail, ModelRouteCandidateInput,
        ModelRouteDetail, ModelRouteListItem, normalize_required_name, validate_candidate_inputs,
    };
    use serde_json::json;

    #[test]
    fn normalize_required_name_trims_and_rejects_empty_values() {
        assert_eq!(
            normalize_required_name("route_name", "  demo-route  ").unwrap(),
            "demo-route"
        );
        assert!(normalize_required_name("route_name", "   ").is_err());
    }

    #[test]
    fn candidate_inputs_require_non_empty_unique_models() {
        assert!(validate_candidate_inputs(&[]).is_err());
        assert!(
            validate_candidate_inputs(&[
                ModelRouteCandidateInput {
                    model_id: 1,
                    priority: 0,
                    is_enabled: Some(true),
                },
                ModelRouteCandidateInput {
                    model_id: 1,
                    priority: 10,
                    is_enabled: Some(true),
                },
            ])
            .is_err()
        );
    }

    #[test]
    fn route_list_item_serializes_with_nested_route() {
        let item = ModelRouteListItem {
            route: ModelRoute {
                id: 1,
                route_name: "manual-smoke-route".to_string(),
                description: Some("demo".to_string()),
                is_enabled: true,
                expose_in_models: true,
                deleted_at: None,
                created_at: 10,
                updated_at: 20,
            },
            candidate_count: 2,
        };

        let value = serde_json::to_value(item).expect("serialize route list item");

        assert_eq!(value.pointer("/route/id"), Some(&json!(1)));
        assert_eq!(
            value.pointer("/route/route_name"),
            Some(&json!("manual-smoke-route"))
        );
        assert_eq!(value.pointer("/candidate_count"), Some(&json!(2)));
    }

    #[test]
    fn route_detail_serializes_with_nested_candidate() {
        let detail = ModelRouteDetail {
            route: ModelRoute {
                id: 1,
                route_name: "manual-smoke-route".to_string(),
                description: None,
                is_enabled: true,
                expose_in_models: true,
                deleted_at: None,
                created_at: 10,
                updated_at: 20,
            },
            candidates: vec![ModelRouteCandidateDetail {
                candidate: ModelRouteCandidate {
                    id: 11,
                    route_id: 1,
                    model_id: 101,
                    priority: 0,
                    is_enabled: true,
                    deleted_at: None,
                    created_at: 10,
                    updated_at: 20,
                },
                provider_id: 7,
                provider_key: "openai".to_string(),
                model_name: "gpt-4.1".to_string(),
                real_model_name: Some("gpt-4.1".to_string()),
                model_is_enabled: true,
            }],
        };

        let value = serde_json::to_value(detail).expect("serialize route detail");

        assert_eq!(
            value.pointer("/candidates/0/candidate/id"),
            Some(&json!(11))
        );
        assert_eq!(
            value.pointer("/candidates/0/candidate/model_id"),
            Some(&json!(101))
        );
        assert_eq!(value.pointer("/candidates/0/provider_id"), Some(&json!(7)));
    }
}
