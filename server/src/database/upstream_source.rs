use diesel::prelude::*;
use std::collections::HashMap;

use crate::controller::BaseError;
use crate::database::{DbResult, get_connection};
use crate::schema::enum_def::UpstreamProfileType;
use crate::{db_execute, db_object};

pub const PRIMARY_SOURCE_KEY: &str = "primary";

db_object! {
    #[derive(Queryable, Selectable, Identifiable, AsChangeset)]
    #[diesel(table_name = upstream_source)]
    pub struct UpstreamSource {
        pub id: i64,
        pub provider_id: i64,
        pub source_key: String,
        pub profile_type: UpstreamProfileType,
        pub endpoint: String,
        pub use_proxy: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable)]
    #[diesel(table_name = upstream_source)]
    pub struct NewUpstreamSource {
        pub id: i64,
        pub provider_id: i64,
        pub source_key: String,
        pub profile_type: UpstreamProfileType,
        pub endpoint: String,
        pub use_proxy: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset)]
    #[diesel(table_name = upstream_source)]
    pub struct UpdateUpstreamSourceData {
        pub profile_type: Option<UpstreamProfileType>,
        pub endpoint: Option<String>,
        pub use_proxy: Option<bool>,
        pub updated_at: i64,
    }
}

impl UpstreamSource {
    pub fn get_unique_active_by_provider_id(provider_id_value: i64) -> DbResult<Self> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = upstream_source::table
                .filter(
                    upstream_source::dsl::provider_id
                        .eq(provider_id_value)
                        .and(upstream_source::dsl::deleted_at.is_null()),
                )
                .order(upstream_source::dsl::id.asc())
                .select(UpstreamSourceDb::as_select())
                .load::<UpstreamSourceDb>(conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to load upstream source for provider {provider_id_value}: {error}"
                    )))
                })?;

            require_exactly_one(
                provider_id_value,
                rows.into_iter().map(UpstreamSourceDb::from_db).collect(),
            )
        })
    }

    pub fn list_unique_active_for_provider_ids(
        provider_ids: &[i64],
    ) -> DbResult<HashMap<i64, Self>> {
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
                let row = row.from_db();
                grouped.entry(row.provider_id).or_default().push(row);
            }

            let mut result = HashMap::with_capacity(provider_ids.len());
            for provider_id in provider_ids {
                let rows = grouped.remove(provider_id).unwrap_or_default();
                result.insert(*provider_id, require_exactly_one(*provider_id, rows)?);
            }
            Ok(result)
        })
    }
}

fn require_exactly_one(provider_id: i64, rows: Vec<UpstreamSource>) -> DbResult<UpstreamSource> {
    let count = rows.len();
    if count != 1 {
        return Err(invalid_source_cardinality(provider_id, count));
    }
    Ok(rows.into_iter().next().expect("cardinality checked"))
}

pub(crate) fn invalid_source_cardinality(provider_id: i64, count: usize) -> BaseError {
    BaseError::DatabaseFatal(Some(format!(
        "provider {provider_id} must have exactly one active primary upstream source; found {count}"
    )))
}
