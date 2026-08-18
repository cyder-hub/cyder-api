use diesel::prelude::*;
use std::collections::HashMap;

use crate::controller::BaseError;
use crate::database::{
    DbResult,
    runtime::{
        DatabaseRuntime, DatabaseWorkload, RuntimeConnection, db_execute as async_db_execute,
    },
};
use crate::db_object;
use crate::schema::enum_def::UpstreamProfileType;

db_object! {
    #[derive(Queryable, Selectable, Identifiable, AsChangeset)]
    #[diesel(table_name = upstream_source)]
    pub struct UpstreamSource {
        pub id: i64,
        pub provider_id: i64,
        pub profile_type: UpstreamProfileType,
        pub base_url: String,
        pub use_proxy: bool,
        pub chat_completions_enabled: Option<bool>,
        pub chat_completions_path_override: Option<String>,
        pub embeddings_enabled: Option<bool>,
        pub embeddings_path_override: Option<String>,
        pub rerank_enabled: Option<bool>,
        pub rerank_path_override: Option<String>,
        pub is_enabled: bool,
        pub is_default: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Clone)]
    #[diesel(table_name = upstream_source)]
    pub struct NewUpstreamSource {
        pub id: i64,
        pub provider_id: i64,
        pub profile_type: UpstreamProfileType,
        pub base_url: String,
        pub use_proxy: bool,
        pub chat_completions_enabled: Option<bool>,
        pub chat_completions_path_override: Option<String>,
        pub embeddings_enabled: Option<bool>,
        pub embeddings_path_override: Option<String>,
        pub rerank_enabled: Option<bool>,
        pub rerank_path_override: Option<String>,
        pub is_enabled: bool,
        pub is_default: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset, Clone)]
    #[diesel(table_name = upstream_source)]
    pub struct UpdateUpstreamSourceData {
        pub base_url: Option<String>,
        pub use_proxy: Option<bool>,
        pub chat_completions_enabled: Option<bool>,
        pub chat_completions_path_override: Option<Option<String>>,
        pub embeddings_enabled: Option<bool>,
        pub embeddings_path_override: Option<Option<String>>,
        pub rerank_enabled: Option<bool>,
        pub rerank_path_override: Option<Option<String>>,
        pub is_enabled: Option<bool>,
        pub is_default: Option<bool>,
        pub updated_at: i64,
    }
}

impl UpstreamSource {
    #[cfg(test)]
    pub async fn deleted_at_for_test(
        database: &DatabaseRuntime,
        source_id: i64,
    ) -> DbResult<Option<i64>> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = upstream_source::table
                            .find(source_id)
                            .select(upstream_source::dsl::deleted_at);
                        diesel_async::RunQueryDsl::first::<Option<i64>>(query, &mut **conn)
                            .await
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to load source tombstone {source_id}: {error}"
                                )))
                            })
                    })
                })
            })
            .await
    }

    pub async fn list_active_by_provider_id(
        database: &DatabaseRuntime,
        provider_id_value: i64,
    ) -> DbResult<Vec<Self>> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(Self::list_active_by_provider_id_with_connection(
                    connection,
                    provider_id_value,
                ))
            })
            .await
    }

    pub(crate) async fn list_active_by_provider_id_with_connection(
        connection: &mut RuntimeConnection,
        provider_id_value: i64,
    ) -> DbResult<Vec<Self>> {
        async_db_execute!(connection as conn, {
            let query = upstream_source::table
                .filter(
                    upstream_source::dsl::provider_id
                        .eq(provider_id_value)
                        .and(upstream_source::dsl::deleted_at.is_null()),
                )
                .order(upstream_source::dsl::id.asc())
                .select(UpstreamSourceDb::as_select());
            let result: Result<Vec<UpstreamSourceDb>, diesel::result::Error> =
                diesel_async::RunQueryDsl::load(query, &mut **conn).await;
            result
                .map(|rows| rows.into_iter().map(UpstreamSourceDb::from_db).collect())
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to load upstream sources for provider {provider_id_value}: {error}"
                    )))
                })
        })
    }

    pub async fn list_active_for_provider_ids(
        database: &DatabaseRuntime,
        provider_ids: &[i64],
    ) -> DbResult<HashMap<i64, Vec<Self>>> {
        if provider_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let provider_ids = provider_ids.to_vec();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(Self::list_active_for_provider_ids_with_connection(
                    connection,
                    provider_ids,
                ))
            })
            .await
    }

    pub(crate) async fn list_active_for_provider_ids_with_connection(
        connection: &mut RuntimeConnection,
        provider_ids: Vec<i64>,
    ) -> DbResult<HashMap<i64, Vec<Self>>> {
        if provider_ids.is_empty() {
            return Ok(HashMap::new());
        }
        async_db_execute!(connection as conn, {
            let query = upstream_source::table
                .filter(
                    upstream_source::dsl::provider_id
                        .eq_any(&provider_ids)
                        .and(upstream_source::dsl::deleted_at.is_null()),
                )
                .order((
                    upstream_source::dsl::provider_id.asc(),
                    upstream_source::dsl::id.asc(),
                ))
                .select(UpstreamSourceDb::as_select());
            let rows: Vec<UpstreamSourceDb> = diesel_async::RunQueryDsl::load(query, &mut **conn)
                .await
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to load upstream sources for providers: {error}"
                    )))
                })?;

            let mut grouped: HashMap<i64, Vec<UpstreamSource>> = HashMap::new();
            for row in rows {
                let source = row.from_db();
                grouped.entry(source.provider_id).or_default().push(source);
            }
            Ok(grouped)
        })
    }

    pub async fn create(
        database: &DatabaseRuntime,
        new_source: &NewUpstreamSource,
    ) -> DbResult<Self> {
        if new_source.is_default && !new_source.is_enabled {
            return Err(BaseError::ParamInvalid(Some(
                "a default upstream source must be enabled".to_string(),
            )));
        }
        let new_source = new_source.clone();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        use diesel_async::AsyncConnection;
                        conn.transaction(async move |conn| {
                            let query = provider::table
                                .filter(
                                    provider::dsl::id
                                        .eq(new_source.provider_id)
                                        .and(provider::dsl::deleted_at.is_null()),
                                )
                                .select(provider::dsl::id);
                            let provider_exists = diesel_async::RunQueryDsl::first::<i64>(
                                query, &mut *conn,
                            )
                            .await
                            .optional()
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "failed to validate provider {} for upstream source: {error}",
                                    new_source.provider_id
                                )))
                            })?;
                            if provider_exists.is_none() {
                                return Err(BaseError::NotFound(Some(format!(
                                    "provider {} not found",
                                    new_source.provider_id
                                ))));
                            }
                            if new_source.is_default {
                                let query = diesel::update(
                                    upstream_source::table.filter(
                                        upstream_source::dsl::provider_id
                                            .eq(new_source.provider_id)
                                            .and(upstream_source::dsl::deleted_at.is_null())
                                            .and(upstream_source::dsl::is_default.eq(true)),
                                    ),
                                )
                                .set((
                                    upstream_source::dsl::is_default.eq(false),
                                    upstream_source::dsl::updated_at.eq(new_source.updated_at),
                                ));
                                diesel_async::RunQueryDsl::execute(query, &mut *conn)
                                    .await
                                    .map_err(|error| {
                                        BaseError::DatabaseFatal(Some(format!(
                                            "failed to demote existing default source: {error}"
                                        )))
                                    })?;
                            }
                            let query = diesel::insert_into(upstream_source::table)
                                .values((
                                    upstream_source::dsl::id.eq(new_source.id),
                                    upstream_source::dsl::provider_id.eq(new_source.provider_id),
                                    upstream_source::dsl::profile_type.eq(new_source.profile_type),
                                    upstream_source::dsl::base_url.eq(new_source.base_url),
                                    upstream_source::dsl::use_proxy.eq(new_source.use_proxy),
                                    upstream_source::dsl::chat_completions_enabled
                                        .eq(new_source.chat_completions_enabled),
                                    upstream_source::dsl::chat_completions_path_override
                                        .eq(new_source.chat_completions_path_override),
                                    upstream_source::dsl::embeddings_enabled
                                        .eq(new_source.embeddings_enabled),
                                    upstream_source::dsl::embeddings_path_override
                                        .eq(new_source.embeddings_path_override),
                                    upstream_source::dsl::rerank_enabled
                                        .eq(new_source.rerank_enabled),
                                    upstream_source::dsl::rerank_path_override
                                        .eq(new_source.rerank_path_override),
                                    upstream_source::dsl::is_enabled.eq(new_source.is_enabled),
                                    upstream_source::dsl::is_default.eq(new_source.is_default),
                                    upstream_source::dsl::created_at.eq(new_source.created_at),
                                    upstream_source::dsl::updated_at.eq(new_source.updated_at),
                                ))
                                .returning(UpstreamSourceDb::as_returning());
                            diesel_async::RunQueryDsl::get_result::<UpstreamSourceDb>(
                                query, &mut *conn,
                            )
                            .await
                            .map(UpstreamSourceDb::from_db)
                            .map_err(|error| map_source_write_error("create", error))
                        })
                        .await
                    })
                })
            })
            .await
    }

    pub async fn get_active_by_id_for_provider(
        database: &DatabaseRuntime,
        source_id_value: i64,
        provider_id_value: i64,
    ) -> DbResult<Self> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = upstream_source::table
                            .filter(
                                upstream_source::dsl::id
                                    .eq(source_id_value)
                                    .and(
                                        upstream_source::dsl::provider_id.eq(provider_id_value),
                                    )
                                    .and(upstream_source::dsl::deleted_at.is_null()),
                            )
                            .select(UpstreamSourceDb::as_select());
                        diesel_async::RunQueryDsl::first::<UpstreamSourceDb>(query, &mut **conn)
                            .await
                            .map(UpstreamSourceDb::from_db)
                            .map_err(|error| match error {
                                diesel::result::Error::NotFound => BaseError::NotFound(Some(
                                    format!(
                                        "upstream source {source_id_value} not found for provider {provider_id_value}"
                                    ),
                                )),
                                other => BaseError::DatabaseFatal(Some(format!(
                                    "failed to load upstream source {source_id_value}: {other}"
                                ))),
                            })
                    })
                })
            })
            .await
    }

    pub async fn update(
        database: &DatabaseRuntime,
        source_id_value: i64,
        provider_id_value: i64,
        update: &UpdateUpstreamSourceData,
    ) -> DbResult<Self> {
        let update = update.clone();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        use diesel_async::AsyncConnection;
                        conn.transaction(async move |conn| {
                            let query = upstream_source::table
                                .filter(
                                    upstream_source::dsl::id
                                        .eq(source_id_value)
                                        .and(
                                            upstream_source::dsl::provider_id.eq(provider_id_value),
                                        )
                                        .and(upstream_source::dsl::deleted_at.is_null()),
                                )
                                .select(UpstreamSourceDb::as_select());
                            let current = diesel_async::RunQueryDsl::first::<UpstreamSourceDb>(
                                query,
                                &mut *conn,
                            )
                            .await
                            .map(UpstreamSourceDb::from_db)
                            .map_err(|error| match error {
                                diesel::result::Error::NotFound => BaseError::NotFound(Some(
                                    format!(
                                        "upstream source {source_id_value} not found for provider {provider_id_value}"
                                    ),
                                )),
                                other => BaseError::DatabaseFatal(Some(format!(
                                    "failed to load upstream source {source_id_value}: {other}"
                                ))),
                            })?;
                            let is_enabled = update.is_enabled.unwrap_or(current.is_enabled);
                            if update.is_default == Some(true) && !is_enabled {
                                return Err(BaseError::ParamInvalid(Some(
                                    "a default upstream source must be enabled".to_string(),
                                )));
                            }
                            let is_default = if !is_enabled {
                                false
                            } else {
                                update.is_default.unwrap_or(current.is_default)
                            };
                            if is_default {
                                let query = diesel::update(
                                    upstream_source::table.filter(
                                        upstream_source::dsl::provider_id
                                            .eq(provider_id_value)
                                            .and(upstream_source::dsl::id.ne(source_id_value))
                                            .and(upstream_source::dsl::deleted_at.is_null())
                                            .and(upstream_source::dsl::is_default.eq(true)),
                                    ),
                                )
                                .set((
                                    upstream_source::dsl::is_default.eq(false),
                                    upstream_source::dsl::updated_at.eq(update.updated_at),
                                ));
                                diesel_async::RunQueryDsl::execute(query, &mut *conn)
                                    .await
                                    .map_err(|error| {
                                        BaseError::DatabaseFatal(Some(format!(
                                            "failed to demote existing default source: {error}"
                                        )))
                                    })?;
                            }
                            let query = diesel::update(
                                upstream_source::table.filter(
                                    upstream_source::dsl::id
                                        .eq(source_id_value)
                                        .and(
                                            upstream_source::dsl::provider_id.eq(provider_id_value),
                                        )
                                        .and(upstream_source::dsl::deleted_at.is_null()),
                                ),
                            )
                            .set((
                                upstream_source::dsl::base_url
                                    .eq(update.base_url.unwrap_or(current.base_url)),
                                upstream_source::dsl::use_proxy
                                    .eq(update.use_proxy.unwrap_or(current.use_proxy)),
                                upstream_source::dsl::chat_completions_enabled.eq(
                                    update
                                        .chat_completions_enabled
                                        .or(current.chat_completions_enabled),
                                ),
                                upstream_source::dsl::chat_completions_path_override.eq(
                                    update
                                        .chat_completions_path_override
                                        .unwrap_or(current.chat_completions_path_override),
                                ),
                                upstream_source::dsl::embeddings_enabled.eq(
                                    update.embeddings_enabled.or(current.embeddings_enabled),
                                ),
                                upstream_source::dsl::embeddings_path_override.eq(
                                    update
                                        .embeddings_path_override
                                        .unwrap_or(current.embeddings_path_override),
                                ),
                                upstream_source::dsl::rerank_enabled
                                    .eq(update.rerank_enabled.or(current.rerank_enabled)),
                                upstream_source::dsl::rerank_path_override.eq(
                                    update
                                        .rerank_path_override
                                        .unwrap_or(current.rerank_path_override),
                                ),
                                upstream_source::dsl::is_enabled.eq(is_enabled),
                                upstream_source::dsl::is_default.eq(is_default),
                                upstream_source::dsl::updated_at.eq(update.updated_at),
                            ))
                            .returning(UpstreamSourceDb::as_returning());
                            diesel_async::RunQueryDsl::get_result::<UpstreamSourceDb>(
                                query,
                                &mut *conn,
                            )
                            .await
                            .map(UpstreamSourceDb::from_db)
                            .map_err(|error| map_source_write_error("update", error))
                        })
                        .await
                    })
                })
            })
            .await
    }

    pub async fn delete(
        database: &DatabaseRuntime,
        source_id_value: i64,
        provider_id_value: i64,
    ) -> DbResult<Self> {
        let now = chrono::Utc::now().timestamp_millis();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        use diesel_async::AsyncConnection;
                        conn.transaction(async move |conn| {
                            let query = diesel::update(
                                upstream_source::table.filter(
                                    upstream_source::dsl::id
                                        .eq(source_id_value)
                                        .and(
                                            upstream_source::dsl::provider_id.eq(provider_id_value),
                                        )
                                        .and(upstream_source::dsl::deleted_at.is_null()),
                                ),
                            )
                            .set((
                                upstream_source::dsl::deleted_at.eq(Some(now)),
                                upstream_source::dsl::is_enabled.eq(false),
                                upstream_source::dsl::is_default.eq(false),
                                upstream_source::dsl::updated_at.eq(now),
                            ))
                            .returning(UpstreamSourceDb::as_returning());
                            let source = diesel_async::RunQueryDsl::get_result::<UpstreamSourceDb>(
                                query,
                                &mut *conn,
                            )
                            .await
                            .map(UpstreamSourceDb::from_db)
                            .map_err(|error| match error {
                                diesel::result::Error::NotFound => BaseError::NotFound(Some(
                                    format!(
                                        "upstream source {source_id_value} not found for provider {provider_id_value}"
                                    ),
                                )),
                                other => BaseError::DatabaseFatal(Some(format!(
                                    "failed to delete upstream source {source_id_value}: {other}"
                                ))),
                            })?;
                            let query = request_patch_variant::table
                                .filter(
                                    request_patch_variant::dsl::source_id.eq(source_id_value),
                                )
                                .filter(request_patch_variant::dsl::deleted_at.is_null())
                                .select(request_patch_variant::dsl::id);
                            let variant_ids = diesel_async::RunQueryDsl::load::<i64>(
                                query,
                                &mut *conn,
                            )
                            .await
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "failed to load request patch Variants for source {source_id_value}: {error}"
                                )))
                            })?;
                            if !variant_ids.is_empty() {
                                let query = diesel::update(
                                    request_patch_variant::table.filter(
                                        request_patch_variant::dsl::id.eq_any(&variant_ids),
                                    ),
                                )
                                .set((
                                    request_patch_variant::dsl::deleted_at.eq(Some(now)),
                                    request_patch_variant::dsl::enabled.eq(false),
                                    request_patch_variant::dsl::expose_in_models.eq(false),
                                    request_patch_variant::dsl::updated_at.eq(now),
                                ));
                                diesel_async::RunQueryDsl::execute(query, &mut *conn)
                                    .await
                                    .map_err(|error| {
                                        BaseError::DatabaseFatal(Some(format!(
                                            "failed to delete request patch Variants for source {source_id_value}: {error}"
                                        )))
                                    })?;
                                let query = diesel::update(
                                    request_patch_rule::table
                                        .filter(
                                            request_patch_rule::dsl::variant_id
                                                .eq_any(&variant_ids),
                                        )
                                        .filter(request_patch_rule::dsl::deleted_at.is_null()),
                                )
                                .set(request_patch_rule::dsl::deleted_at.eq(Some(now)));
                                diesel_async::RunQueryDsl::execute(query, &mut *conn)
                                    .await
                                    .map_err(|error| {
                                        BaseError::DatabaseFatal(Some(format!(
                                            "failed to delete request patch Rules for source {source_id_value}: {error}"
                                        )))
                                    })?;
                            }
                            Ok(source)
                        })
                        .await
                    })
                })
            })
            .await
    }
}

fn map_source_write_error(action: &'static str, error: diesel::result::Error) -> BaseError {
    match error {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        ) => BaseError::DatabaseDup(Some(format!(
            "upstream source {action} conflicts with an existing provider Source family or default"
        ))),
        diesel::result::Error::NotFound => {
            BaseError::NotFound(Some("upstream source not found".to_string()))
        }
        other => {
            BaseError::DatabaseFatal(Some(format!("failed to {action} upstream source: {other}")))
        }
    }
}

#[cfg(test)]
impl UpdateUpstreamSourceData {
    pub(crate) fn test_defaults() -> Self {
        Self {
            base_url: None,
            use_proxy: None,
            chat_completions_enabled: None,
            chat_completions_path_override: None,
            embeddings_enabled: None,
            embeddings_path_override: None,
            rerank_enabled: None,
            rerank_path_override: None,
            is_enabled: None,
            is_default: None,
            updated_at: 1,
        }
    }
}

#[cfg(test)]
fn test_operation_defaults(
    profile_type: &UpstreamProfileType,
) -> (Option<bool>, Option<bool>, Option<bool>) {
    match profile_type {
        UpstreamProfileType::Openai | UpstreamProfileType::GeminiOpenai => {
            (Some(true), Some(true), Some(false))
        }
        UpstreamProfileType::OpenaiCompatible => (Some(true), Some(false), Some(false)),
        _ => (None, None, None),
    }
}

#[cfg(test)]
impl NewUpstreamSource {
    pub(crate) fn test_defaults(profile_type: UpstreamProfileType) -> Self {
        let (chat_completions_enabled, embeddings_enabled, rerank_enabled) =
            test_operation_defaults(&profile_type);
        Self {
            id: 0,
            provider_id: 0,
            profile_type: profile_type.clone(),
            base_url: "https://example.test/v1".to_string(),
            use_proxy: false,
            chat_completions_enabled,
            chat_completions_path_override: None,
            embeddings_enabled,
            embeddings_path_override: None,
            rerank_enabled,
            rerank_path_override: None,
            is_enabled: true,
            is_default: false,
            created_at: 1,
            updated_at: 1,
        }
    }
}

#[cfg(test)]
impl UpstreamSource {
    pub(crate) async fn replace_profile_for_test(
        database: &DatabaseRuntime,
        source_id: i64,
        profile_type: UpstreamProfileType,
        base_url: String,
        updated_at: i64,
    ) -> DbResult<()> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::update(upstream_source::table.find(source_id)).set((
                            upstream_source::dsl::profile_type.eq(profile_type),
                            upstream_source::dsl::base_url.eq(base_url),
                            upstream_source::dsl::updated_at.eq(updated_at),
                        ));
                        diesel_async::RunQueryDsl::execute(query, &mut **conn)
                            .await
                            .map(|_| ())
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "failed to replace test source profile {source_id}: {error}"
                                )))
                            })
                    })
                })
            })
            .await
    }

    pub(crate) async fn hard_delete_for_test(
        database: &DatabaseRuntime,
        source_id: i64,
    ) -> DbResult<()> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::delete(upstream_source::table.find(source_id));
                        diesel_async::RunQueryDsl::execute(query, &mut **conn)
                            .await
                            .map(|_| ())
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "failed to hard-delete test source {source_id}: {error}"
                                )))
                            })
                    })
                })
            })
            .await
    }

    pub(crate) fn test_defaults(profile_type: UpstreamProfileType) -> Self {
        let seed = NewUpstreamSource::test_defaults(profile_type);
        Self {
            id: seed.id,
            provider_id: seed.provider_id,
            profile_type: seed.profile_type,
            base_url: seed.base_url,
            use_proxy: seed.use_proxy,
            chat_completions_enabled: seed.chat_completions_enabled,
            chat_completions_path_override: seed.chat_completions_path_override,
            embeddings_enabled: seed.embeddings_enabled,
            embeddings_path_override: seed.embeddings_path_override,
            rerank_enabled: seed.rerank_enabled,
            rerank_path_override: seed.rerank_path_override,
            is_enabled: seed.is_enabled,
            is_default: seed.is_default,
            deleted_at: None,
            created_at: seed.created_at,
            updated_at: seed.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NewUpstreamSource, UpdateUpstreamSourceData, UpstreamSource};
    use crate::database::TestDatabase;
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::request_patch::{
        RequestPatchRuleInput, RequestPatchVariantInput, RequestPatchVariantRepository,
    };
    use crate::schema::enum_def::{
        ProviderApiKeyMode, RequestPatchOperation, RequestPatchPlacement, UpstreamProfileType,
    };
    use serde_json::json;

    fn provider(id: i64) -> NewProvider {
        NewProvider {
            id,
            provider_key: format!("source-repository-{id}"),
            name: format!("Source Repository {id}"),
            is_enabled: true,
            created_at: 1,
            updated_at: 1,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        }
    }

    fn source(
        id: i64,
        provider_id: i64,
        profile_type: UpstreamProfileType,
        is_enabled: bool,
        is_default: bool,
    ) -> NewUpstreamSource {
        NewUpstreamSource {
            id,
            provider_id,
            profile_type: profile_type.clone(),
            base_url: format!("https://source-{id}.example/v1"),
            use_proxy: false,
            is_enabled,
            is_default,
            created_at: 1,
            updated_at: 1,
            ..NewUpstreamSource::test_defaults(profile_type)
        }
    }

    #[tokio::test]
    async fn source_repository_enforces_lifecycle_family_and_ownership_contracts() {
        let database =
            TestDatabase::new_sqlite_default("upstream-source-repository-lifecycle.sqlite").await;
        (async {
            let empty = Provider::create_optional(&database, &provider(1), None)
                .await
                .expect("zero-source provider should create");
            assert!(empty.upstream_sources.is_empty());
            Provider::create_optional(&database, &provider(2), None)
                .await
                .expect("second provider should create");

            let openai = UpstreamSource::create(
                &database,
                &source(11, 1, UpstreamProfileType::Openai, true, true),
            )
            .await
            .expect("default OpenAI source should create");
            let responses = UpstreamSource::create(
                &database,
                &source(12, 1, UpstreamProfileType::Responses, true, false),
            )
            .await
            .expect("different family source should create");

            let switched = UpstreamSource::update(
                &database,
                responses.id,
                1,
                &UpdateUpstreamSourceData {
                    base_url: Some("https://responses.changed.example/v1".to_string()),
                    use_proxy: Some(true),
                    is_enabled: None,
                    is_default: Some(true),
                    updated_at: 2,
                    ..UpdateUpstreamSourceData::test_defaults()
                },
            )
            .await
            .expect("default switch should succeed");
            assert_eq!(switched.profile_type, UpstreamProfileType::Responses);
            assert!(switched.is_default);
            assert!(
                !UpstreamSource::get_active_by_id_for_provider(&database, openai.id, 1)
                    .await
                    .expect("old source should load")
                    .is_default
            );

            let disabled = UpstreamSource::update(
                &database,
                responses.id,
                1,
                &UpdateUpstreamSourceData {
                    base_url: None,
                    use_proxy: None,
                    is_enabled: Some(false),
                    is_default: None,
                    updated_at: 3,
                    ..UpdateUpstreamSourceData::test_defaults()
                },
            )
            .await
            .expect("disabling a default should clear default automatically");
            assert!(!disabled.is_enabled);
            assert!(!disabled.is_default);

            let reenabled = UpstreamSource::update(
                &database,
                responses.id,
                1,
                &UpdateUpstreamSourceData {
                    base_url: None,
                    use_proxy: None,
                    is_enabled: Some(true),
                    is_default: Some(true),
                    updated_at: 4,
                    ..UpdateUpstreamSourceData::test_defaults()
                },
            )
            .await
            .expect("source should re-enable as default");
            assert!(reenabled.is_enabled && reenabled.is_default);

            let invalid = UpstreamSource::update(
                &database,
                responses.id,
                1,
                &UpdateUpstreamSourceData {
                    base_url: None,
                    use_proxy: None,
                    is_enabled: Some(false),
                    is_default: Some(true),
                    updated_at: 5,
                    ..UpdateUpstreamSourceData::test_defaults()
                },
            )
            .await
            .expect_err("explicit disabled default must fail");
            assert!(matches!(
                invalid,
                crate::controller::BaseError::ParamInvalid(_)
            ));
            let unchanged =
                UpstreamSource::get_active_by_id_for_provider(&database, responses.id, 1)
                    .await
                    .expect("failed update must preserve source");
            assert!(unchanged.is_enabled && unchanged.is_default);

            let disabled_openai = UpstreamSource::update(
                &database,
                openai.id,
                1,
                &UpdateUpstreamSourceData {
                    base_url: None,
                    use_proxy: None,
                    is_enabled: Some(false),
                    is_default: None,
                    updated_at: 5,
                    ..UpdateUpstreamSourceData::test_defaults()
                },
            )
            .await
            .expect("non-default source should be disableable");
            assert!(!disabled_openai.is_enabled && !disabled_openai.is_default);

            let source_variant = RequestPatchVariantRepository::create(
                &database,
                &RequestPatchVariantInput {
                    source_id: openai.id,
                    model_id: None,
                    suffix: Some("fast".to_string()),
                    enabled: true,
                    expose_in_models: true,
                    rules: vec![RequestPatchRuleInput {
                        placement: RequestPatchPlacement::Body,
                        target: "/options/temperature".to_string(),
                        operation: RequestPatchOperation::Set,
                        value_json: Some(Some(json!(0.2))),
                        description: None,
                    }],
                },
            )
            .await
            .expect("source Variant should be created before source delete");

            let duplicate_family = UpstreamSource::create(
                &database,
                &source(13, 1, UpstreamProfileType::OpenaiCompatible, true, true),
            )
            .await
            .expect_err("same wire family must fail even when existing source is disabled");
            assert!(matches!(
                duplicate_family,
                crate::controller::BaseError::DatabaseDup(_)
            ));
            assert!(
                UpstreamSource::get_active_by_id_for_provider(&database, responses.id, 1)
                    .await
                    .expect("default source should remain after failed create")
                    .is_default
            );

            let wrong_owner = UpstreamSource::update(
                &database,
                openai.id,
                2,
                &UpdateUpstreamSourceData {
                    base_url: Some("https://wrong-owner.example/v1".to_string()),
                    use_proxy: None,
                    is_enabled: None,
                    is_default: None,
                    updated_at: 6,
                    ..UpdateUpstreamSourceData::test_defaults()
                },
            )
            .await
            .expect_err("cross-provider source mutation must be hidden");
            assert!(matches!(
                wrong_owner,
                crate::controller::BaseError::NotFound(_)
            ));

            let deleted = UpstreamSource::delete(&database, openai.id, 1)
                .await
                .expect("source delete should soft-delete the target");
            assert!(deleted.deleted_at.is_some());
            assert!(!deleted.is_enabled && !deleted.is_default);
            assert!(
                RequestPatchVariantRepository::list_by_source(&database, openai.id)
                    .await
                    .expect("active source Variants should load")
                    .is_empty()
            );
            let historical = RequestPatchVariantRepository::list_by_source_ids_including_deleted(
                &database,
                &[openai.id],
            )
            .await
            .expect("historical source Variants should load");
            assert_eq!(historical.len(), 1);
            assert_eq!(historical[0].variant.id, source_variant.variant.id);
            assert!(historical[0].variant.deleted_at.is_some());
            assert!(historical[0].rules.is_empty());

            let replacement = UpstreamSource::create(
                &database,
                &source(13, 1, UpstreamProfileType::OpenaiCompatible, true, false),
            )
            .await
            .expect("soft deletion should release the wire family");
            assert_eq!(
                replacement.profile_type,
                UpstreamProfileType::OpenaiCompatible
            );
            assert_eq!(
                Provider::get_by_id(&database, 1)
                    .await
                    .expect("provider should load")
                    .upstream_sources
                    .len(),
                2
            );
        })
        .await;
    }
}
