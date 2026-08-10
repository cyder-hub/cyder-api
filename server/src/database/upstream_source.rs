use diesel::prelude::*;
use std::collections::HashMap;

use crate::controller::BaseError;
use crate::database::{DbResult, get_connection};
use crate::schema::enum_def::UpstreamProfileType;
use crate::{db_execute, db_object};

db_object! {
    #[derive(Queryable, Selectable, Identifiable, AsChangeset)]
    #[diesel(table_name = upstream_source)]
    pub struct UpstreamSource {
        pub id: i64,
        pub provider_id: i64,
        pub profile_type: UpstreamProfileType,
        pub endpoint: String,
        pub use_proxy: bool,
        pub is_enabled: bool,
        pub is_default: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable)]
    #[diesel(table_name = upstream_source)]
    pub struct NewUpstreamSource {
        pub id: i64,
        pub provider_id: i64,
        pub profile_type: UpstreamProfileType,
        pub endpoint: String,
        pub use_proxy: bool,
        pub is_enabled: bool,
        pub is_default: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset)]
    #[diesel(table_name = upstream_source)]
    pub struct UpdateUpstreamSourceData {
        pub endpoint: Option<String>,
        pub use_proxy: Option<bool>,
        pub is_enabled: Option<bool>,
        pub is_default: Option<bool>,
        pub updated_at: i64,
    }
}

impl UpstreamSource {
    pub fn create(new_source: &NewUpstreamSource) -> DbResult<Self> {
        if new_source.is_default && !new_source.is_enabled {
            return Err(BaseError::ParamInvalid(Some(
                "a default upstream source must be enabled".to_string(),
            )));
        }

        let conn = &mut get_connection()?;
        db_execute!(conn, {
            conn.transaction::<Self, BaseError, _>(|conn| {
                let provider_exists = provider::table
                    .filter(
                        provider::dsl::id
                            .eq(new_source.provider_id)
                            .and(provider::dsl::deleted_at.is_null()),
                    )
                    .select(provider::dsl::id)
                    .first::<i64>(conn)
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

                let now = new_source.updated_at;
                if new_source.is_default {
                    diesel::update(
                        upstream_source::table.filter(
                            upstream_source::dsl::provider_id
                                .eq(new_source.provider_id)
                                .and(upstream_source::dsl::deleted_at.is_null())
                                .and(upstream_source::dsl::is_default.eq(true)),
                        ),
                    )
                    .set((
                        upstream_source::dsl::is_default.eq(false),
                        upstream_source::dsl::updated_at.eq(now),
                    ))
                    .execute(conn)
                    .map_err(|error| {
                        BaseError::DatabaseFatal(Some(format!(
                            "failed to demote existing default source: {error}"
                        )))
                    })?;
                }

                diesel::insert_into(upstream_source::table)
                    .values((
                        upstream_source::dsl::id.eq(new_source.id),
                        upstream_source::dsl::provider_id.eq(new_source.provider_id),
                        upstream_source::dsl::profile_type.eq(new_source.profile_type.clone()),
                        upstream_source::dsl::endpoint.eq(new_source.endpoint.clone()),
                        upstream_source::dsl::use_proxy.eq(new_source.use_proxy),
                        upstream_source::dsl::is_enabled.eq(new_source.is_enabled),
                        upstream_source::dsl::is_default.eq(new_source.is_default),
                        upstream_source::dsl::created_at.eq(new_source.created_at),
                        upstream_source::dsl::updated_at.eq(new_source.updated_at),
                    ))
                    .returning(UpstreamSourceDb::as_returning())
                    .get_result::<UpstreamSourceDb>(conn)
                    .map(UpstreamSourceDb::from_db)
                    .map_err(|error| map_source_write_error("create", error))
            })
        })
    }

    pub fn update(
        source_id_value: i64,
        provider_id_value: i64,
        update: &UpdateUpstreamSourceData,
    ) -> DbResult<Self> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            conn.transaction::<Self, BaseError, _>(|conn| {
                let current = upstream_source::table
                    .filter(
                        upstream_source::dsl::id
                            .eq(source_id_value)
                            .and(upstream_source::dsl::provider_id.eq(provider_id_value))
                            .and(upstream_source::dsl::deleted_at.is_null()),
                    )
                    .select(UpstreamSourceDb::as_select())
                    .first::<UpstreamSourceDb>(conn)
                    .map(UpstreamSourceDb::from_db)
                    .map_err(|error| match error {
                        diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                            "upstream source {source_id_value} not found for provider {provider_id_value}"
                        ))),
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
                    diesel::update(
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
                    ))
                    .execute(conn)
                    .map_err(|error| {
                        BaseError::DatabaseFatal(Some(format!(
                            "failed to demote existing default source: {error}"
                        )))
                    })?;
                }

                diesel::update(upstream_source::table.filter(
                    upstream_source::dsl::id
                        .eq(source_id_value)
                        .and(upstream_source::dsl::provider_id.eq(provider_id_value))
                        .and(upstream_source::dsl::deleted_at.is_null()),
                ))
                .set((
                    upstream_source::dsl::endpoint
                        .eq(update.endpoint.clone().unwrap_or(current.endpoint)),
                    upstream_source::dsl::use_proxy
                        .eq(update.use_proxy.unwrap_or(current.use_proxy)),
                    upstream_source::dsl::is_enabled.eq(is_enabled),
                    upstream_source::dsl::is_default.eq(is_default),
                    upstream_source::dsl::updated_at.eq(update.updated_at),
                ))
                .returning(UpstreamSourceDb::as_returning())
                .get_result::<UpstreamSourceDb>(conn)
                .map(UpstreamSourceDb::from_db)
                .map_err(|error| map_source_write_error("update", error))
            })
        })
    }

    pub fn delete(source_id_value: i64, provider_id_value: i64) -> DbResult<Self> {
        let conn = &mut get_connection()?;
        let now = chrono::Utc::now().timestamp_millis();
        db_execute!(conn, {
            conn.transaction::<Self, BaseError, _>(|conn| {
                let source = diesel::update(
                    upstream_source::table.filter(
                        upstream_source::dsl::id
                            .eq(source_id_value)
                            .and(upstream_source::dsl::provider_id.eq(provider_id_value))
                            .and(upstream_source::dsl::deleted_at.is_null()),
                    ),
                )
                .set((
                    upstream_source::dsl::deleted_at.eq(Some(now)),
                    upstream_source::dsl::is_enabled.eq(false),
                    upstream_source::dsl::is_default.eq(false),
                    upstream_source::dsl::updated_at.eq(now),
                ))
                .returning(UpstreamSourceDb::as_returning())
                .get_result::<UpstreamSourceDb>(conn)
                .map(UpstreamSourceDb::from_db)
                .map_err(|error| match error {
                    diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                        "upstream source {source_id_value} not found for provider {provider_id_value}"
                    ))),
                    other => BaseError::DatabaseFatal(Some(format!(
                        "failed to delete upstream source {source_id_value}: {other}"
                    ))),
                })?;

                let variant_ids = request_patch_variant::table
                    .filter(request_patch_variant::dsl::source_id.eq(source_id_value))
                    .filter(request_patch_variant::dsl::deleted_at.is_null())
                    .select(request_patch_variant::dsl::id)
                    .load::<i64>(conn)
                    .map_err(|error| {
                        BaseError::DatabaseFatal(Some(format!(
                            "failed to load request patch Variants for source {source_id_value}: {error}"
                        )))
                    })?;
                if !variant_ids.is_empty() {
                    diesel::update(
                        request_patch_variant::table
                            .filter(request_patch_variant::dsl::id.eq_any(&variant_ids)),
                    )
                    .set((
                        request_patch_variant::dsl::deleted_at.eq(Some(now)),
                        request_patch_variant::dsl::enabled.eq(false),
                        request_patch_variant::dsl::expose_in_models.eq(false),
                        request_patch_variant::dsl::updated_at.eq(now),
                    ))
                    .execute(conn)
                    .map_err(|error| {
                        BaseError::DatabaseFatal(Some(format!(
                            "failed to delete request patch Variants for source {source_id_value}: {error}"
                        )))
                    })?;
                    diesel::update(
                        request_patch_rule::table
                            .filter(request_patch_rule::dsl::variant_id.eq_any(&variant_ids))
                            .filter(request_patch_rule::dsl::deleted_at.is_null()),
                    )
                    .set(request_patch_rule::dsl::deleted_at.eq(Some(now)))
                    .execute(conn)
                    .map_err(|error| {
                        BaseError::DatabaseFatal(Some(format!(
                            "failed to delete request patch Rules for source {source_id_value}: {error}"
                        )))
                    })?;
                }
                Ok(source)
            })
        })
    }

    pub fn list_active_by_provider_id(provider_id_value: i64) -> DbResult<Vec<Self>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            upstream_source::table
                .filter(
                    upstream_source::dsl::provider_id
                        .eq(provider_id_value)
                        .and(upstream_source::dsl::deleted_at.is_null()),
                )
                .order(upstream_source::dsl::id.asc())
                .select(UpstreamSourceDb::as_select())
                .load::<UpstreamSourceDb>(conn)
                .map(|rows| rows.into_iter().map(UpstreamSourceDb::from_db).collect())
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to load upstream sources for provider {provider_id_value}: {error}"
                    )))
                })
        })
    }

    pub fn list_active_for_provider_ids(provider_ids: &[i64]) -> DbResult<HashMap<i64, Vec<Self>>> {
        if provider_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = upstream_source::table
                .filter(
                    upstream_source::dsl::provider_id
                        .eq_any(provider_ids)
                        .and(upstream_source::dsl::deleted_at.is_null()),
                )
                .order((
                    upstream_source::dsl::provider_id.asc(),
                    upstream_source::dsl::id.asc(),
                ))
                .select(UpstreamSourceDb::as_select())
                .load::<UpstreamSourceDb>(conn)
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

    pub fn get_active_by_id_for_provider(
        source_id_value: i64,
        provider_id_value: i64,
    ) -> DbResult<Self> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            upstream_source::table
                .filter(
                    upstream_source::dsl::id
                        .eq(source_id_value)
                        .and(upstream_source::dsl::provider_id.eq(provider_id_value))
                        .and(upstream_source::dsl::deleted_at.is_null()),
                )
                .select(UpstreamSourceDb::as_select())
                .first::<UpstreamSourceDb>(conn)
                .map(UpstreamSourceDb::from_db)
                .map_err(|error| match error {
                    diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                        "upstream source {source_id_value} not found for provider {provider_id_value}"
                    ))),
                    other => BaseError::DatabaseFatal(Some(format!(
                        "failed to load upstream source {source_id_value}: {other}"
                    ))),
                })
        })
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
mod tests {
    use super::{NewUpstreamSource, UpdateUpstreamSourceData, UpstreamSource};
    use crate::database::TestDbContext;
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
            profile_type,
            endpoint: format!("https://source-{id}.example/v1"),
            use_proxy: false,
            is_enabled,
            is_default,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[tokio::test]
    async fn source_repository_enforces_lifecycle_family_and_ownership_contracts() {
        let database = TestDbContext::new_sqlite("upstream-source-repository-lifecycle.sqlite");
        database
            .run_async(async {
                let empty = Provider::create_optional(&provider(1), None)
                    .expect("zero-source provider should create");
                assert!(empty.upstream_sources.is_empty());
                Provider::create_optional(&provider(2), None)
                    .expect("second provider should create");

                let openai =
                    UpstreamSource::create(&source(11, 1, UpstreamProfileType::Openai, true, true))
                        .expect("default OpenAI source should create");
                let responses = UpstreamSource::create(&source(
                    12,
                    1,
                    UpstreamProfileType::Responses,
                    true,
                    false,
                ))
                .expect("different family source should create");

                let switched = UpstreamSource::update(
                    responses.id,
                    1,
                    &UpdateUpstreamSourceData {
                        endpoint: Some("https://responses.changed.example/v1".to_string()),
                        use_proxy: Some(true),
                        is_enabled: None,
                        is_default: Some(true),
                        updated_at: 2,
                    },
                )
                .expect("default switch should succeed");
                assert_eq!(switched.profile_type, UpstreamProfileType::Responses);
                assert!(switched.is_default);
                assert!(
                    !UpstreamSource::get_active_by_id_for_provider(openai.id, 1)
                        .expect("old source should load")
                        .is_default
                );

                let disabled = UpstreamSource::update(
                    responses.id,
                    1,
                    &UpdateUpstreamSourceData {
                        endpoint: None,
                        use_proxy: None,
                        is_enabled: Some(false),
                        is_default: None,
                        updated_at: 3,
                    },
                )
                .expect("disabling a default should clear default automatically");
                assert!(!disabled.is_enabled);
                assert!(!disabled.is_default);

                let reenabled = UpstreamSource::update(
                    responses.id,
                    1,
                    &UpdateUpstreamSourceData {
                        endpoint: None,
                        use_proxy: None,
                        is_enabled: Some(true),
                        is_default: Some(true),
                        updated_at: 4,
                    },
                )
                .expect("source should re-enable as default");
                assert!(reenabled.is_enabled && reenabled.is_default);

                let invalid = UpstreamSource::update(
                    responses.id,
                    1,
                    &UpdateUpstreamSourceData {
                        endpoint: None,
                        use_proxy: None,
                        is_enabled: Some(false),
                        is_default: Some(true),
                        updated_at: 5,
                    },
                )
                .expect_err("explicit disabled default must fail");
                assert!(matches!(
                    invalid,
                    crate::controller::BaseError::ParamInvalid(_)
                ));
                let unchanged = UpstreamSource::get_active_by_id_for_provider(responses.id, 1)
                    .expect("failed update must preserve source");
                assert!(unchanged.is_enabled && unchanged.is_default);

                let disabled_openai = UpstreamSource::update(
                    openai.id,
                    1,
                    &UpdateUpstreamSourceData {
                        endpoint: None,
                        use_proxy: None,
                        is_enabled: Some(false),
                        is_default: None,
                        updated_at: 5,
                    },
                )
                .expect("non-default source should be disableable");
                assert!(!disabled_openai.is_enabled && !disabled_openai.is_default);

                let source_variant =
                    RequestPatchVariantRepository::create(&RequestPatchVariantInput {
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
                            confirm_dangerous_target: false,
                        }],
                    })
                    .expect("source Variant should be created before source delete");

                let duplicate_family = UpstreamSource::create(&source(
                    13,
                    1,
                    UpstreamProfileType::VertexOpenai,
                    true,
                    true,
                ))
                .expect_err("same wire family must fail even when existing source is disabled");
                assert!(matches!(
                    duplicate_family,
                    crate::controller::BaseError::DatabaseDup(_)
                ));
                assert!(
                    UpstreamSource::get_active_by_id_for_provider(responses.id, 1)
                        .expect("default source should remain after failed create")
                        .is_default
                );

                let wrong_owner = UpstreamSource::update(
                    openai.id,
                    2,
                    &UpdateUpstreamSourceData {
                        endpoint: Some("https://wrong-owner.example/v1".to_string()),
                        use_proxy: None,
                        is_enabled: None,
                        is_default: None,
                        updated_at: 6,
                    },
                )
                .expect_err("cross-provider source mutation must be hidden");
                assert!(matches!(
                    wrong_owner,
                    crate::controller::BaseError::NotFound(_)
                ));

                let deleted = UpstreamSource::delete(openai.id, 1)
                    .expect("source delete should soft-delete the target");
                assert!(deleted.deleted_at.is_some());
                assert!(!deleted.is_enabled && !deleted.is_default);
                assert!(
                    RequestPatchVariantRepository::list_by_source(openai.id)
                        .expect("active source Variants should load")
                        .is_empty()
                );
                let historical =
                    RequestPatchVariantRepository::list_by_source_ids_including_deleted(&[
                        openai.id
                    ])
                    .expect("historical source Variants should load");
                assert_eq!(historical.len(), 1);
                assert_eq!(historical[0].variant.id, source_variant.variant.id);
                assert!(historical[0].variant.deleted_at.is_some());
                assert!(historical[0].rules.is_empty());

                let replacement = UpstreamSource::create(&source(
                    13,
                    1,
                    UpstreamProfileType::VertexOpenai,
                    true,
                    false,
                ))
                .expect("soft deletion should release the wire family");
                assert_eq!(replacement.profile_type, UpstreamProfileType::VertexOpenai);
                assert_eq!(
                    Provider::get_by_id(1)
                        .expect("provider should load")
                        .upstream_sources
                        .len(),
                    2
                );
            })
            .await;
    }
}
