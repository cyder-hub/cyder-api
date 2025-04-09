use chrono::Utc;
use diesel::prelude::Queryable;
use diesel::prelude::*;

use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

use super::get_connection;
use super::provider::ProviderApiKey;
use super::{provider::Provider, DbResult};

db_object! {
    #[derive(Queryable, Insertable, Debug, AsChangeset)]
    #[diesel(table_name = model)]
    pub struct Model {
        pub id: i64,
        pub provider_id: i64,
        pub model_name: String,
        pub real_model_name: Option<String>,
        pub is_deleted: bool,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

impl Model {
    pub fn insert(model: &Self) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::insert_into(model::table)
                .values(&ModelDb::to_db(model))
                .execute(conn)
                .map_err(|_e| BaseError::DatabaseFatal(None))?;
            Ok(())
        })
    }
    pub fn list_by_provider_id(provider_id: i64) -> DbResult<Vec<Model>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let result = model::table
                .filter(model::dsl::provider_id.eq(provider_id))
                .load::<ModelDb>(conn)
                .map_err(|_e| BaseError::DatabaseFatal(None))?;
            Ok(result.into_iter().map(|db| db.from_db()).collect())
        })
    }

    pub fn query_provider_model(
        provider_key: &str,
        model_name: &str,
    ) -> DbResult<(Provider, Vec<ProviderApiKey>, Option<Model>)> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let (provider, provider_api_keys) = Provider::query_key_by_key(provider_key)?;
            let model = model::table
                .filter(
                    model::dsl::model_name
                        .eq(model_name)
                        .and(model::dsl::provider_id.eq(provider.id))
                        .and(model::dsl::is_deleted.eq(false))
                        .and(model::dsl::is_enabled.eq(true)),
                )
                .first::<ModelDb>(conn);
            let model = match model {
                Ok(model) => Some(model.from_db()),
                Err(_) => None,
            };
            Ok((provider, provider_api_keys, model))
        })
    }

    pub fn get(id: i64) -> DbResult<Model> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let model = model::table
                .filter(model::dsl::id.eq(id))
                .first::<ModelDb>(conn)
                .map_err(|_e| BaseError::DatabaseFatal(None))?;
            Ok(model.from_db())
        })
    }

    pub fn update(model: &Model) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::update(model::table.filter(model::dsl::id.eq(model.id)))
                .set(ModelDb::to_db(model))
                .execute(conn)
                .map_err(|_e| BaseError::DatabaseFatal(None))?;
            Ok(())
        })
    }

    pub fn delete(id: i64) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::update(model::table.filter(model::dsl::id.eq(id)))
                .set(model::dsl::is_deleted.eq(true))
                .execute(conn)
                .map_err(|_e| BaseError::DatabaseFatal(None))?;
            Ok(())
        })
    }

    pub fn upsert_by_provider_and_name(
        provider_id: i64,
        model_name: &str,
        real_model_name: Option<&str>,
    ) -> DbResult<Model> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let existing = model::table
                .filter(
                    model::dsl::provider_id
                        .eq(provider_id)
                        .and(model::dsl::model_name.eq(model_name)),
                )
                .first::<ModelDb>(conn);

            let now = Utc::now().timestamp_millis();
            let model = match existing {
                Ok(db) => {
                    let mut model = db.from_db();
                    model.real_model_name = real_model_name.map(|s| s.to_string());
                    model.updated_at = now;
                    model.is_deleted = false;
                    model.is_enabled = true;
                    diesel::update(model::table.filter(model::dsl::id.eq(model.id)))
                        .set(ModelDb::to_db(&model))
                        .execute(conn)
                        .map_err(|_e| BaseError::DatabaseFatal(None))?;
                    model
                }
                Err(_) => {
                    let model = Model {
                        id: ID_GENERATOR.generate_id(),
                        provider_id,
                        model_name: model_name.to_string(),
                        real_model_name: real_model_name.map(|s| s.to_string()),
                        is_deleted: false,
                        is_enabled: true,
                        created_at: now,
                        updated_at: now,
                    };
                    diesel::insert_into(model::table)
                        .values(ModelDb::to_db(&model))
                        .execute(conn)
                        .map_err(|_e| BaseError::DatabaseFatal(None))?;
                    model
                }
            };
            Ok(model)
        })
    }

    pub fn list() -> DbResult<Vec<Model>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let result = model::table
                .load::<ModelDb>(conn)
                .map_err(|_e| BaseError::DatabaseFatal(None))?;
            Ok(result.into_iter().map(|db| db.from_db()).collect())
        })
    }
}
