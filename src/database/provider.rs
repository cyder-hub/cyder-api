use std::collections::HashSet;

use chrono::Utc;
use diesel::prelude::*;

use crate::controller::BaseError;
use crate::database::model::Model;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

use super::{get_connection, DbResult};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct CommitModel {
    pub model_name: String,
    pub real_model_name: Option<String>,
}

#[derive(Deserialize)]
pub struct CommitProviderKey {
    pub api_key: String,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct FullCommitData {
    pub provider_key: String,
    pub name: String,
    pub endpoint: String,
    pub limit_model: bool,
    pub use_proxy: bool,
    pub models: Vec<CommitModel>,
    pub provider_keys: Vec<CommitProviderKey>,
}

db_object! {
    #[derive(Queryable, Insertable, AsChangeset, Debug)]
    #[diesel(table_name = provider)]
    pub struct Provider {
        pub id: i64,
        pub provider_key: String,
        pub name: String,
        pub endpoint: String,
        pub omit_config: Option<String>,
        pub limit_model: bool,
        pub use_proxy: bool,
        pub is_enabled: bool,
        pub is_deleted: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Queryable, Selectable, AsChangeset, Debug)]
    #[diesel(table_name = provider_api_key)]
    pub struct ProviderApiKey {
        pub id: i64,
        pub provider_id: i64,
        pub api_key: String,
        pub description: Option<String>,
        pub is_deleted: bool,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

impl ProviderApiKey {
    fn new(provider_id: i64, api_key: &str, description: Option<&str>) -> Self {
        let now = Utc::now().timestamp_millis();
        Self {
            id: ID_GENERATOR.generate_id(),
            provider_id,
            api_key: api_key.to_string(),
            description: description.map(|s| s.to_string()),
            is_deleted: false,
            is_enabled: true,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn upsert_by_provider_and_key(
        provider_id: i64,
        api_key: &str,
        description: Option<&str>,
    ) -> DbResult<ProviderApiKey> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let existing = provider_api_key::table
                .filter(
                    provider_api_key::dsl::provider_id
                        .eq(provider_id)
                        .and(provider_api_key::dsl::api_key.eq(api_key)),
                )
                .first::<ProviderApiKeyDb>(conn);

            let now = Utc::now().timestamp_millis();
            let key = match existing {
                Ok(db) => {
                    let mut key = db.from_db();
                    key.description = description.map(|s| s.to_string());
                    key.updated_at = now;
                    diesel::update(
                        provider_api_key::table.filter(provider_api_key::dsl::id.eq(key.id)),
                    )
                    .set(ProviderApiKeyDb::to_db(&key))
                    .execute(conn);
                    key
                }
                Err(_) => {
                    let key = ProviderApiKey::new(provider_id, api_key, description);
                    diesel::insert_into(provider_api_key::table)
                        .values(ProviderApiKeyDb::to_db(&key))
                        .execute(conn)
                        .map_err(|_| BaseError::DatabaseFatal(None))?;
                    key
                }
            };
            Ok(key)
        })
    }

    pub fn delete_by_provider_and_key(provider_id: i64, api_key: &str) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::delete(
                provider_api_key::table
                    .filter(provider_api_key::dsl::provider_id.eq(provider_id))
                    .filter(provider_api_key::dsl::api_key.eq(api_key)),
            )
            .execute(conn)
            .map_err(|_| BaseError::DatabaseFatal(None))?;
            Ok(())
        })
    }

    pub fn list_by_provider_id(provider_id: i64) -> DbResult<Vec<ProviderApiKey>> {
        let conn = &mut get_connection();
        let provider_api_keys = db_execute!(conn, {
            provider_api_key::table
                .filter(provider_api_key::dsl::provider_id.eq(provider_id))
                .load::<ProviderApiKeyDb>(conn)
                .map_err(|_| BaseError::DatabaseFatal(None))
                .unwrap()
                .into_iter()
                .map(|db| db.from_db())
                .collect::<Vec<ProviderApiKey>>()
        });

        Ok(provider_api_keys)
    }
}

impl Provider {
    pub fn new(
        id: Option<i64>,
        key: &str,
        name: &str,
        endpoint: &str,
        config: Option<&str>,
        limit_model: bool,
        use_proxy: bool,
    ) -> Self {
        let now = Utc::now().timestamp_millis();
        Self {
            id: id.unwrap_or(ID_GENERATOR.generate_id()),
            provider_key: key.to_string(),
            name: name.to_string(),
            endpoint: endpoint.to_string(),
            omit_config: match config {
                Some(config) => Some(config.to_string()),
                None => None,
            },
            limit_model,
            use_proxy,
            is_deleted: false,
            is_enabled: true,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn full_commit(data: FullCommitData) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            conn.transaction::<(), BaseError, _>(|conn| {
                // Check if provider exists
                let existing_provider = provider::table
                    .filter(provider::dsl::provider_key.eq(&data.provider_key))
                    .first::<ProviderDb>(conn);

                let now = Utc::now().timestamp_millis();
                let provider = match existing_provider {
                    Ok(db) => {
                        // Update existing provider
                        let mut provider = db.from_db();
                        provider.name = data.name;
                        provider.endpoint = data.endpoint;
                        provider.limit_model = data.limit_model;
                        provider.use_proxy = data.use_proxy;
                        provider.updated_at = now;

                        diesel::update(provider::table)
                            .filter(provider::dsl::id.eq(provider.id))
                            .set(ProviderDb::to_db(&provider))
                            .execute(conn)?;
                        provider
                    }
                    Err(_) => {
                        // Create new provider
                        let provider = Provider::new(
                            None,
                            &data.provider_key,
                            &data.name,
                            &data.endpoint,
                            None,
                            data.limit_model,
                            data.use_proxy,
                        );
                        diesel::insert_into(provider::table)
                            .values(ProviderDb::to_db(&provider))
                            .execute(conn);
                        provider
                    }
                };

                // Handle models
                let existing_models = Model::list_by_provider_id(provider.id)?;
                let new_model_names: HashSet<_> =
                    data.models.iter().map(|m| &m.model_name).collect();

                // Delete models not in new data
                for model in existing_models {
                    if !new_model_names.contains(&model.model_name) {
                        Model::delete(model.id).map_err(|_e| BaseError::DatabaseFatal(None))?;
                    }
                }

                // Upsert models from new data
                for model in data.models {
                    Model::upsert_by_provider_and_name(
                        provider.id,
                        &model.model_name,
                        model.real_model_name.as_deref(),
                    )?;
                }

                // Handle provider keys
                let existing_keys = ProviderApiKey::list_by_provider_id(provider.id)
                    .map_err(|_e| BaseError::DatabaseFatal(None))?;
                let new_key_values: HashSet<_> =
                    data.provider_keys.iter().map(|k| &k.api_key).collect();

                // Delete keys not in new data
                for key in existing_keys {
                    if !new_key_values.contains(&key.api_key) {
                        ProviderApiKey::delete_by_provider_and_key(provider.id, &key.api_key)
                            .map_err(|_e| BaseError::DatabaseFatal(None))
                            .map_err(|_e| BaseError::DatabaseFatal(None))?;
                    }
                }

                // Upsert keys from new data
                for key in data.provider_keys {
                    ProviderApiKey::upsert_by_provider_and_key(
                        provider.id,
                        &key.api_key,
                        key.description.as_deref(),
                    )?;
                }

                Ok(())
            })
            .map_err(|_| BaseError::DatabaseFatal(None))
        })
    }

    pub fn insert_one(provider: &Provider, api_keys: Vec<String>) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            conn.transaction::<(), diesel::result::Error, _>(|conn| {
                let data = ProviderDb::to_db(provider);

                diesel::insert_into(provider::table)
                    .values(&data)
                    .execute(conn)
                    .unwrap();

                diesel::insert_into(provider_api_key::table)
                    .values(
                        api_keys
                            .iter()
                            .map(|key| {
                                ProviderApiKeyDb::to_db(&ProviderApiKey::new(
                                    provider.id,
                                    key,
                                    None,
                                ))
                            })
                            .collect::<Vec<ProviderApiKeyDb>>(),
                    )
                    .execute(conn)
                    .unwrap();
                Ok(())
            })
            .map_err(|_| BaseError::DatabaseFatal(None))
        })
    }

    pub fn query_one(id: i64) -> DbResult<Self> {
        let conn = &mut get_connection();
        Ok(db_execute!(conn, {
            provider::table
                .filter(provider::dsl::id.eq(id))
                .first::<ProviderDb>(conn)
                .map_err(|_| BaseError::NotFound(None))
                .unwrap()
                .from_db()
        }))
    }
    pub fn query_by_key(key: &str) -> DbResult<Self> {
        let conn = &mut get_connection();
        Ok(db_execute!(conn, {
            provider::table
                .filter(provider::dsl::provider_key.eq(key))
                .first::<ProviderDb>(conn)
                .map_err(|_| BaseError::NotFound(None))
                .unwrap()
                .from_db()
        }))
    }

    pub fn query_key_by_key(key: &str) -> DbResult<(Provider, Vec<ProviderApiKey>)> {
        let provider = Self::query_by_key(key)?;
        let conn = &mut get_connection();

        let provider_api_keys = db_execute!(conn, {
            provider_api_key::table
                .filter(provider_api_key::dsl::provider_id.eq(provider.id))
                .load::<ProviderApiKeyDb>(conn)
                .map_err(|_| BaseError::DatabaseFatal(None))
                .unwrap()
                .into_iter()
                .map(|db| db.from_db())
                .collect::<Vec<ProviderApiKey>>()
        });

        Ok((provider, provider_api_keys))
    }
    pub fn list() -> DbResult<Vec<Self>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let list = provider::table
                .filter(provider::dsl::is_deleted.eq(false))
                .load::<ProviderDb>(conn)
                .map_err(|_| BaseError::DatabaseFatal(None))
                .unwrap();

            Ok(list
                .into_iter()
                .map(|db| db.from_db())
                .collect::<Vec<Provider>>())
        })
    }

    pub fn delete_one(id: i64) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::update(provider::dsl::provider.filter(provider::dsl::id.eq(id)))
                .set(provider::dsl::is_deleted.eq(true))
                .execute(conn)
                .unwrap();
        });

        Ok(())
    }

    pub fn update_one(provider: &Provider) -> DbResult<Provider> {
        let conn = &mut get_connection();

        db_execute!(conn, {
            let data = ProviderDb::to_db(&provider);

            diesel::update(provider::table)
                .filter(provider::dsl::id.eq(&provider.id))
                .set(data)
                .execute(conn)
                .map_err(|_| BaseError::DatabaseFatal(None))
        })?;

        Self::query_one(provider.id)
    }
}
