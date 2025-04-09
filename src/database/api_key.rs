use chrono::Utc;
use diesel::prelude::*;

use super::{get_connection, DbResult};
use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

db_object! {
    #[derive(Queryable, Insertable, AsChangeset, Debug)]
    #[diesel(table_name = api_keys)]
    pub struct ApiKey {
        pub id: i64,
        pub api_key: String,
        pub name: String,
        pub description: Option<String>,
        pub is_deleted: bool,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

impl ApiKey {
    pub fn new(key: String, name: String, description: Option<String>) -> Self {
        let now = Utc::now().timestamp();
        Self {
            id: ID_GENERATOR.generate_id(),
            api_key: key,
            name,
            description,
            is_deleted: false,
            is_enabled: true,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn insert_one(api_key: &ApiKey) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::insert_into(api_keys::table)
                .values(ApiKeyDb::to_db(api_key))
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;
            Ok(())
        })
    }

    pub fn delete_one(id: i64) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::delete(api_keys::table.filter(api_keys::dsl::id.eq(id)))
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;
            Ok(())
        })
    }

    pub fn list() -> DbResult<Vec<ApiKey>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let list = api_keys::table
                .load::<ApiKeyDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))
                .unwrap();
            Ok(list
                .into_iter()
                .map(|db| db.from_db())
                .collect::<Vec<ApiKey>>())
        })
    }

    pub fn query_by_key(key: &str) -> DbResult<ApiKey> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let result = api_keys::table
                .filter(api_keys::dsl::api_key.eq(key))
                .first::<ApiKeyDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

            Ok(result.from_db())
        })
    }

    pub fn query_one(id: i64) -> DbResult<ApiKey> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let result = api_keys::table
                .filter(api_keys::dsl::id.eq(id))
                .first::<ApiKeyDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

            Ok(result.from_db())
        })
    }

    pub fn update_one(api_key: &ApiKey) -> DbResult<ApiKey> {
        let conn = &mut get_connection();

        db_execute!(conn, {
            diesel::update(api_keys::table)
                .filter(api_keys::dsl::id.eq(&api_key.id))
                .set(ApiKeyDb::to_db(api_key))
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))
        })?;

        Self::query_one(api_key.id)
    }
}
