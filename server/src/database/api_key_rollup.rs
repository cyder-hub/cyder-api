use diesel::prelude::*;
#[cfg(test)]
use diesel::upsert::excluded;
use serde::{Deserialize, Serialize};

use super::{
    DbResult,
    runtime::{DatabaseRuntime, DatabaseWorkload, db_execute as async_db_execute},
};
use crate::controller::BaseError;
use crate::db_object;

db_object! {
    #[derive(Queryable, Selectable, Debug, Clone, Serialize, Deserialize)]
    #[diesel(table_name = api_key_rollup_daily)]
    pub struct ApiKeyRollupDaily {
        pub api_key_id: i64,
        pub day_bucket: i64,
        pub currency: String,
        pub request_count: i64,
        pub total_input_tokens: i64,
        pub total_output_tokens: i64,
        pub total_reasoning_tokens: i64,
        pub total_tokens: i64,
        pub billed_amount_nanos: i64,
        pub last_request_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Debug, Clone)]
    #[diesel(table_name = api_key_rollup_daily)]
    pub struct NewApiKeyRollupDaily {
        pub api_key_id: i64,
        pub day_bucket: i64,
        pub currency: String,
        pub request_count: i64,
        pub total_input_tokens: i64,
        pub total_output_tokens: i64,
        pub total_reasoning_tokens: i64,
        pub total_tokens: i64,
        pub billed_amount_nanos: i64,
        pub last_request_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Queryable, Selectable, Debug, Clone, Serialize, Deserialize)]
    #[diesel(table_name = api_key_rollup_monthly)]
    pub struct ApiKeyRollupMonthly {
        pub api_key_id: i64,
        pub month_bucket: i64,
        pub currency: String,
        pub request_count: i64,
        pub total_input_tokens: i64,
        pub total_output_tokens: i64,
        pub total_reasoning_tokens: i64,
        pub total_tokens: i64,
        pub billed_amount_nanos: i64,
        pub last_request_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Debug, Clone)]
    #[diesel(table_name = api_key_rollup_monthly)]
    pub struct NewApiKeyRollupMonthly {
        pub api_key_id: i64,
        pub month_bucket: i64,
        pub currency: String,
        pub request_count: i64,
        pub total_input_tokens: i64,
        pub total_output_tokens: i64,
        pub total_reasoning_tokens: i64,
        pub total_tokens: i64,
        pub billed_amount_nanos: i64,
        pub last_request_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

impl ApiKeyRollupDaily {
    pub async fn list_by_bucket(
        database: &DatabaseRuntime,
        api_key_id: i64,
        day_bucket: i64,
    ) -> DbResult<Vec<ApiKeyRollupDaily>> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = api_key_rollup_daily::table
                            .filter(api_key_rollup_daily::dsl::api_key_id.eq(api_key_id))
                            .filter(api_key_rollup_daily::dsl::day_bucket.eq(day_bucket))
                            .select(ApiKeyRollupDailyDb::as_select());
                        let result: Result<Vec<ApiKeyRollupDailyDb>, diesel::result::Error> =
                            diesel_async::RunQueryDsl::load(query, &mut **conn).await;
                        result
                            .map(|rows| {
                                rows.into_iter().map(ApiKeyRollupDailyDb::from_db).collect()
                            })
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to load api key daily rollup for key {} bucket {}: {}",
                                    api_key_id, day_bucket, error
                                )))
                            })
                    })
                })
            })
            .await
    }

    #[cfg(test)]
    pub async fn upsert(
        database: &DatabaseRuntime,
        entry: &NewApiKeyRollupDaily,
    ) -> DbResult<ApiKeyRollupDaily> {
        let entry = entry.clone();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::insert_into(api_key_rollup_daily::table)
                            .values(NewApiKeyRollupDailyDb::to_db(&entry))
                            .on_conflict((
                                api_key_rollup_daily::dsl::api_key_id,
                                api_key_rollup_daily::dsl::day_bucket,
                                api_key_rollup_daily::dsl::currency,
                            ))
                            .do_update()
                            .set((
                                api_key_rollup_daily::dsl::request_count
                                    .eq(excluded(api_key_rollup_daily::dsl::request_count)),
                                api_key_rollup_daily::dsl::total_input_tokens
                                    .eq(excluded(api_key_rollup_daily::dsl::total_input_tokens)),
                                api_key_rollup_daily::dsl::total_output_tokens
                                    .eq(excluded(api_key_rollup_daily::dsl::total_output_tokens)),
                                api_key_rollup_daily::dsl::total_reasoning_tokens.eq(excluded(
                                    api_key_rollup_daily::dsl::total_reasoning_tokens,
                                )),
                                api_key_rollup_daily::dsl::total_tokens
                                    .eq(excluded(api_key_rollup_daily::dsl::total_tokens)),
                                api_key_rollup_daily::dsl::billed_amount_nanos
                                    .eq(excluded(api_key_rollup_daily::dsl::billed_amount_nanos)),
                                api_key_rollup_daily::dsl::last_request_at
                                    .eq(excluded(api_key_rollup_daily::dsl::last_request_at)),
                                api_key_rollup_daily::dsl::updated_at
                                    .eq(excluded(api_key_rollup_daily::dsl::updated_at)),
                            ))
                            .returning(ApiKeyRollupDailyDb::as_returning());
                        diesel_async::RunQueryDsl::get_result::<ApiKeyRollupDailyDb>(
                            query,
                            &mut **conn,
                        )
                        .await
                        .map(ApiKeyRollupDailyDb::from_db)
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to upsert api key daily rollup for key {} bucket {} {}: {}",
                                entry.api_key_id, entry.day_bucket, entry.currency, error
                            )))
                        })
                    })
                })
            })
            .await
    }
}

impl ApiKeyRollupMonthly {
    pub async fn list_by_bucket(
        database: &DatabaseRuntime,
        api_key_id: i64,
        month_bucket: i64,
    ) -> DbResult<Vec<ApiKeyRollupMonthly>> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = api_key_rollup_monthly::table
                            .filter(api_key_rollup_monthly::dsl::api_key_id.eq(api_key_id))
                            .filter(api_key_rollup_monthly::dsl::month_bucket.eq(month_bucket))
                            .select(ApiKeyRollupMonthlyDb::as_select());
                        let result: Result<Vec<ApiKeyRollupMonthlyDb>, diesel::result::Error> =
                            diesel_async::RunQueryDsl::load(query, &mut **conn).await;
                        result
                            .map(|rows| {
                                rows.into_iter()
                                    .map(ApiKeyRollupMonthlyDb::from_db)
                                    .collect()
                            })
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to load api key monthly rollup for key {} bucket {}: {}",
                                    api_key_id, month_bucket, error
                                )))
                            })
                    })
                })
            })
            .await
    }

    #[cfg(test)]
    pub async fn upsert(
        database: &DatabaseRuntime,
        entry: &NewApiKeyRollupMonthly,
    ) -> DbResult<ApiKeyRollupMonthly> {
        let entry = entry.clone();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::insert_into(api_key_rollup_monthly::table)
                            .values(NewApiKeyRollupMonthlyDb::to_db(&entry))
                            .on_conflict((
                                api_key_rollup_monthly::dsl::api_key_id,
                                api_key_rollup_monthly::dsl::month_bucket,
                                api_key_rollup_monthly::dsl::currency,
                            ))
                            .do_update()
                            .set((
                                api_key_rollup_monthly::dsl::request_count
                                    .eq(excluded(api_key_rollup_monthly::dsl::request_count)),
                                api_key_rollup_monthly::dsl::total_input_tokens
                                    .eq(excluded(api_key_rollup_monthly::dsl::total_input_tokens)),
                                api_key_rollup_monthly::dsl::total_output_tokens
                                    .eq(excluded(api_key_rollup_monthly::dsl::total_output_tokens)),
                                api_key_rollup_monthly::dsl::total_reasoning_tokens.eq(excluded(
                                    api_key_rollup_monthly::dsl::total_reasoning_tokens,
                                )),
                                api_key_rollup_monthly::dsl::total_tokens
                                    .eq(excluded(api_key_rollup_monthly::dsl::total_tokens)),
                                api_key_rollup_monthly::dsl::billed_amount_nanos
                                    .eq(excluded(api_key_rollup_monthly::dsl::billed_amount_nanos)),
                                api_key_rollup_monthly::dsl::last_request_at
                                    .eq(excluded(api_key_rollup_monthly::dsl::last_request_at)),
                                api_key_rollup_monthly::dsl::updated_at
                                    .eq(excluded(api_key_rollup_monthly::dsl::updated_at)),
                            ))
                            .returning(ApiKeyRollupMonthlyDb::as_returning());
                        diesel_async::RunQueryDsl::get_result::<ApiKeyRollupMonthlyDb>(
                            query,
                            &mut **conn,
                        )
                        .await
                        .map(ApiKeyRollupMonthlyDb::from_db)
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to upsert api key monthly rollup for key {} bucket {} {}: {}",
                                entry.api_key_id, entry.month_bucket, entry.currency, error
                            )))
                        })
                    })
                })
            })
            .await
    }
}
