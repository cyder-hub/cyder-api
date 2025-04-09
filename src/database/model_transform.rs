use chrono::Utc;
use diesel::prelude::*;

use super::{get_connection, DbResult};
use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

db_object! {
    #[derive(Queryable, Insertable, AsChangeset, Debug)]
    #[diesel(table_name = model_transform)]
    pub struct ModelTransform {
        pub id: i64,
        pub model_name: String,
        pub map_model_name: String,
        pub is_enabled: bool,
        pub is_deleted: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

impl ModelTransform {
    pub fn new(model_name: String, map_model_name: String) -> Self {
        let now = Utc::now().timestamp();
        Self {
            id: ID_GENERATOR.generate_id(),
            model_name,
            map_model_name,
            is_enabled: true,
            is_deleted: false,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn insert_one(model_transform: &ModelTransform) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::insert_into(model_transform::table)
                .values(ModelTransformDb::to_db(model_transform))
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;
            Ok(())
        })
    }

    pub fn delete_one(id: i64) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::update(model_transform::table.filter(model_transform::dsl::id.eq(id)))
                .set(model_transform::dsl::is_deleted.eq(true))
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;
            Ok(())
        })
    }

    pub fn list() -> DbResult<Vec<ModelTransform>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let list = model_transform::table
                .filter(model_transform::dsl::is_deleted.eq(false))
                .load::<ModelTransformDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;
            Ok(list
                .into_iter()
                .map(|db| db.from_db())
                .collect::<Vec<ModelTransform>>())
        })
    }

    pub fn query_one(id: i64) -> DbResult<ModelTransform> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let result = model_transform::table
                .filter(model_transform::dsl::id.eq(id))
                .filter(model_transform::dsl::is_deleted.eq(false))
                .first::<ModelTransformDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

            Ok(result.from_db())
        })
    }

    pub fn update_one(model_transform: &ModelTransform) -> DbResult<ModelTransform> {
        let conn = &mut get_connection();

        db_execute!(conn, {
            diesel::update(model_transform::table)
                .filter(model_transform::dsl::id.eq(&model_transform.id))
                .set(ModelTransformDb::to_db(model_transform))
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))
        })?;

        Self::query_one(model_transform.id)
    }

    pub fn query_one_by_model_name(model_name: String) -> DbResult<Option<ModelTransform>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let result = model_transform::table
                .filter(model_transform::dsl::model_name.eq(model_name))
                .filter(model_transform::dsl::is_deleted.eq(false))
                .first::<ModelTransformDb>(conn)
                .optional()
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

            Ok(result.map(|db| db.from_db()))
        })
    }
}
