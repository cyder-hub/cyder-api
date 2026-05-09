use diesel::prelude::*;

use crate::database::{
    DbResult,
    model::{Model, NewModel, UpdateModelData},
    provider::{NewProvider, NewProviderApiKey, Provider, ProviderApiKey, UpdateProviderData},
};

use super::{PortableRepositoryConnection, map_write_error};

#[derive(Debug, Clone)]
pub(crate) enum ProviderApiKeyImportOutcome {
    Created(ProviderApiKey),
    Existing(ProviderApiKey),
}

impl ProviderApiKeyImportOutcome {
    #[cfg(test)]
    fn key(&self) -> &ProviderApiKey {
        match self {
            Self::Created(key) | Self::Existing(key) => key,
        }
    }
}

pub(crate) fn list_providers_for_export(
    conn: &mut PortableRepositoryConnection<'_>,
) -> DbResult<Vec<Provider>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::provider;
            use crate::database::provider::_postgres_model::ProviderDb;

            provider::table
                .filter(provider::dsl::deleted_at.is_null())
                .order((provider::dsl::provider_key.asc(), provider::dsl::id.asc()))
                .select(ProviderDb::as_select())
                .load::<ProviderDb>(*conn)
                .map(|rows| rows.into_iter().map(ProviderDb::from_db).collect())
                .map_err(|err| map_write_error("Failed to list providers for portable export", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::provider;
            use crate::database::provider::_sqlite_model::ProviderDb;

            provider::table
                .filter(provider::dsl::deleted_at.is_null())
                .order((provider::dsl::provider_key.asc(), provider::dsl::id.asc()))
                .select(ProviderDb::as_select())
                .load::<ProviderDb>(*conn)
                .map(|rows| rows.into_iter().map(ProviderDb::from_db).collect())
                .map_err(|err| map_write_error("Failed to list providers for portable export", err))
        }
    }
}

pub(crate) fn list_provider_api_keys_for_export(
    conn: &mut PortableRepositoryConnection<'_>,
    provider_id: i64,
) -> DbResult<Vec<ProviderApiKey>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::provider_api_key;
            use crate::database::provider::_postgres_model::ProviderApiKeyDb;

            provider_api_key::table
                .filter(
                    provider_api_key::dsl::provider_id
                        .eq(provider_id)
                        .and(provider_api_key::dsl::deleted_at.is_null()),
                )
                .order((
                    provider_api_key::dsl::created_at.asc(),
                    provider_api_key::dsl::id.asc(),
                ))
                .select(ProviderApiKeyDb::as_select())
                .load::<ProviderApiKeyDb>(*conn)
                .map(|rows| rows.into_iter().map(ProviderApiKeyDb::from_db).collect())
                .map_err(|err| {
                    map_write_error("Failed to list provider api keys for portable export", err)
                })
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::provider_api_key;
            use crate::database::provider::_sqlite_model::ProviderApiKeyDb;

            provider_api_key::table
                .filter(
                    provider_api_key::dsl::provider_id
                        .eq(provider_id)
                        .and(provider_api_key::dsl::deleted_at.is_null()),
                )
                .order((
                    provider_api_key::dsl::created_at.asc(),
                    provider_api_key::dsl::id.asc(),
                ))
                .select(ProviderApiKeyDb::as_select())
                .load::<ProviderApiKeyDb>(*conn)
                .map(|rows| rows.into_iter().map(ProviderApiKeyDb::from_db).collect())
                .map_err(|err| {
                    map_write_error("Failed to list provider api keys for portable export", err)
                })
        }
    }
}

pub(crate) fn list_models_for_provider_export(
    conn: &mut PortableRepositoryConnection<'_>,
    provider_id: i64,
) -> DbResult<Vec<Model>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::model;
            use crate::database::model::_postgres_model::ModelDb;

            model::table
                .filter(
                    model::dsl::provider_id
                        .eq(provider_id)
                        .and(model::dsl::deleted_at.is_null()),
                )
                .order((model::dsl::model_name.asc(), model::dsl::id.asc()))
                .select(ModelDb::as_select())
                .load::<ModelDb>(*conn)
                .map(|rows| rows.into_iter().map(ModelDb::from_db).collect())
                .map_err(|err| map_write_error("Failed to list models for portable export", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::model;
            use crate::database::model::_sqlite_model::ModelDb;

            model::table
                .filter(
                    model::dsl::provider_id
                        .eq(provider_id)
                        .and(model::dsl::deleted_at.is_null()),
                )
                .order((model::dsl::model_name.asc(), model::dsl::id.asc()))
                .select(ModelDb::as_select())
                .load::<ModelDb>(*conn)
                .map(|rows| rows.into_iter().map(ModelDb::from_db).collect())
                .map_err(|err| map_write_error("Failed to list models for portable export", err))
        }
    }
}

pub(crate) fn find_active_provider_by_key(
    conn: &mut PortableRepositoryConnection<'_>,
    provider_key: &str,
) -> DbResult<Option<Provider>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::provider;
            use crate::database::provider::_postgres_model::ProviderDb;

            provider::table
                .filter(
                    provider::dsl::provider_key
                        .eq(provider_key)
                        .and(provider::dsl::deleted_at.is_null()),
                )
                .select(ProviderDb::as_select())
                .first::<ProviderDb>(*conn)
                .optional()
                .map(|row| row.map(ProviderDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup provider by key", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::provider;
            use crate::database::provider::_sqlite_model::ProviderDb;

            provider::table
                .filter(
                    provider::dsl::provider_key
                        .eq(provider_key)
                        .and(provider::dsl::deleted_at.is_null()),
                )
                .select(ProviderDb::as_select())
                .first::<ProviderDb>(*conn)
                .optional()
                .map(|row| row.map(ProviderDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup provider by key", err))
        }
    }
}

pub(crate) fn find_active_model_for_provider(
    conn: &mut PortableRepositoryConnection<'_>,
    provider_id: i64,
    model_name: &str,
) -> DbResult<Option<Model>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::model;
            use crate::database::model::_postgres_model::ModelDb;

            model::table
                .filter(
                    model::dsl::provider_id
                        .eq(provider_id)
                        .and(model::dsl::model_name.eq(model_name))
                        .and(model::dsl::deleted_at.is_null()),
                )
                .select(ModelDb::as_select())
                .first::<ModelDb>(*conn)
                .optional()
                .map(|row| row.map(ModelDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup model by provider and name", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::model;
            use crate::database::model::_sqlite_model::ModelDb;

            model::table
                .filter(
                    model::dsl::provider_id
                        .eq(provider_id)
                        .and(model::dsl::model_name.eq(model_name))
                        .and(model::dsl::deleted_at.is_null()),
                )
                .select(ModelDb::as_select())
                .first::<ModelDb>(*conn)
                .optional()
                .map(|row| row.map(ModelDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup model by provider and name", err))
        }
    }
}

pub(crate) fn find_active_model_by_ref(
    conn: &mut PortableRepositoryConnection<'_>,
    provider_key: &str,
    model_name: &str,
) -> DbResult<Option<Model>> {
    let Some(provider) = find_active_provider_by_key(conn, provider_key)? else {
        return Ok(None);
    };

    find_active_model_for_provider(conn, provider.id, model_name)
}

pub(crate) fn insert_provider(
    conn: &mut PortableRepositoryConnection<'_>,
    new_provider: &NewProvider,
) -> DbResult<Provider> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::provider;
            use crate::database::provider::_postgres_model::{NewProviderDb, ProviderDb};

            diesel::insert_into(provider::table)
                .values(NewProviderDb::to_db(new_provider))
                .returning(ProviderDb::as_returning())
                .get_result::<ProviderDb>(*conn)
                .map(ProviderDb::from_db)
                .map_err(|err| map_write_error("Failed to import provider", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::provider;
            use crate::database::provider::_sqlite_model::{NewProviderDb, ProviderDb};

            diesel::insert_into(provider::table)
                .values(NewProviderDb::to_db(new_provider))
                .returning(ProviderDb::as_returning())
                .get_result::<ProviderDb>(*conn)
                .map(ProviderDb::from_db)
                .map_err(|err| map_write_error("Failed to import provider", err))
        }
    }
}

pub(crate) fn update_provider(
    conn: &mut PortableRepositoryConnection<'_>,
    provider_id: i64,
    data: &UpdateProviderData,
    updated_at: i64,
) -> DbResult<Provider> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::provider;
            use crate::database::provider::_postgres_model::{ProviderDb, UpdateProviderDataDb};

            diesel::update(provider::table.find(provider_id))
                .set((
                    UpdateProviderDataDb::to_db(data),
                    provider::dsl::updated_at.eq(updated_at),
                ))
                .returning(ProviderDb::as_returning())
                .get_result::<ProviderDb>(*conn)
                .map(ProviderDb::from_db)
                .map_err(|err| map_write_error("Failed to update imported provider", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::provider;
            use crate::database::provider::_sqlite_model::{ProviderDb, UpdateProviderDataDb};

            diesel::update(provider::table.find(provider_id))
                .set((
                    UpdateProviderDataDb::to_db(data),
                    provider::dsl::updated_at.eq(updated_at),
                ))
                .returning(ProviderDb::as_returning())
                .get_result::<ProviderDb>(*conn)
                .map(ProviderDb::from_db)
                .map_err(|err| map_write_error("Failed to update imported provider", err))
        }
    }
}

pub(crate) fn insert_model(
    conn: &mut PortableRepositoryConnection<'_>,
    new_model: &NewModel,
) -> DbResult<Model> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::model;
            use crate::database::model::_postgres_model::{ModelDb, NewModelDb};

            diesel::insert_into(model::table)
                .values(NewModelDb::to_db(new_model))
                .returning(ModelDb::as_returning())
                .get_result::<ModelDb>(*conn)
                .map(ModelDb::from_db)
                .map_err(|err| map_write_error("Failed to import model", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::model;
            use crate::database::model::_sqlite_model::{ModelDb, NewModelDb};

            diesel::insert_into(model::table)
                .values(NewModelDb::to_db(new_model))
                .returning(ModelDb::as_returning())
                .get_result::<ModelDb>(*conn)
                .map(ModelDb::from_db)
                .map_err(|err| map_write_error("Failed to import model", err))
        }
    }
}

pub(crate) fn update_model(
    conn: &mut PortableRepositoryConnection<'_>,
    model_id: i64,
    data: &UpdateModelData,
    updated_at: i64,
) -> DbResult<Model> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::model;
            use crate::database::model::_postgres_model::{ModelDb, UpdateModelDataDb};

            diesel::update(model::table.find(model_id))
                .set((
                    UpdateModelDataDb::to_db(data),
                    model::dsl::updated_at.eq(updated_at),
                ))
                .returning(ModelDb::as_returning())
                .get_result::<ModelDb>(*conn)
                .map(ModelDb::from_db)
                .map_err(|err| map_write_error("Failed to update imported model", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::model;
            use crate::database::model::_sqlite_model::{ModelDb, UpdateModelDataDb};

            diesel::update(model::table.find(model_id))
                .set((
                    UpdateModelDataDb::to_db(data),
                    model::dsl::updated_at.eq(updated_at),
                ))
                .returning(ModelDb::as_returning())
                .get_result::<ModelDb>(*conn)
                .map(ModelDb::from_db)
                .map_err(|err| map_write_error("Failed to update imported model", err))
        }
    }
}

pub(crate) fn insert_provider_api_key_if_missing_by_raw_key(
    conn: &mut PortableRepositoryConnection<'_>,
    new_key: &NewProviderApiKey,
) -> DbResult<ProviderApiKeyImportOutcome> {
    if let Some(existing) =
        find_provider_api_key_by_raw_key(conn, new_key.provider_id, &new_key.api_key)?
    {
        return Ok(ProviderApiKeyImportOutcome::Existing(existing));
    }

    insert_provider_api_key(conn, new_key).map(ProviderApiKeyImportOutcome::Created)
}

pub(crate) fn find_provider_api_key_by_raw_key(
    conn: &mut PortableRepositoryConnection<'_>,
    provider_id: i64,
    raw_api_key: &str,
) -> DbResult<Option<ProviderApiKey>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::provider_api_key;
            use crate::database::provider::_postgres_model::ProviderApiKeyDb;

            provider_api_key::table
                .filter(
                    provider_api_key::dsl::provider_id
                        .eq(provider_id)
                        .and(provider_api_key::dsl::api_key.eq(raw_api_key))
                        .and(provider_api_key::dsl::deleted_at.is_null()),
                )
                .select(ProviderApiKeyDb::as_select())
                .first::<ProviderApiKeyDb>(*conn)
                .optional()
                .map(|row| row.map(ProviderApiKeyDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup provider api key by raw key", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::provider_api_key;
            use crate::database::provider::_sqlite_model::ProviderApiKeyDb;

            provider_api_key::table
                .filter(
                    provider_api_key::dsl::provider_id
                        .eq(provider_id)
                        .and(provider_api_key::dsl::api_key.eq(raw_api_key))
                        .and(provider_api_key::dsl::deleted_at.is_null()),
                )
                .select(ProviderApiKeyDb::as_select())
                .first::<ProviderApiKeyDb>(*conn)
                .optional()
                .map(|row| row.map(ProviderApiKeyDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup provider api key by raw key", err))
        }
    }
}

fn insert_provider_api_key(
    conn: &mut PortableRepositoryConnection<'_>,
    new_key: &NewProviderApiKey,
) -> DbResult<ProviderApiKey> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::provider_api_key;
            use crate::database::provider::_postgres_model::{
                NewProviderApiKeyDb, ProviderApiKeyDb,
            };

            diesel::insert_into(provider_api_key::table)
                .values(NewProviderApiKeyDb::to_db(new_key))
                .returning(ProviderApiKeyDb::as_returning())
                .get_result::<ProviderApiKeyDb>(*conn)
                .map(ProviderApiKeyDb::from_db)
                .map_err(|err| map_write_error("Failed to import provider api key", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::provider_api_key;
            use crate::database::provider::_sqlite_model::{NewProviderApiKeyDb, ProviderApiKeyDb};

            diesel::insert_into(provider_api_key::table)
                .values(NewProviderApiKeyDb::to_db(new_key))
                .returning(ProviderApiKeyDb::as_returning())
                .get_result::<ProviderApiKeyDb>(*conn)
                .map(ProviderApiKeyDb::from_db)
                .map_err(|err| map_write_error("Failed to import provider api key", err))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        database::{
            TestDbContext, get_connection,
            provider::{NewProvider, NewProviderApiKey, ProviderApiKey},
        },
        schema::enum_def::{ProviderApiKeyMode, ProviderType},
        service::admin::portable_config::repository::with_transaction,
        utils::ID_GENERATOR,
    };

    use super::{
        ProviderApiKeyImportOutcome, insert_provider, insert_provider_api_key_if_missing_by_raw_key,
    };

    #[test]
    fn provider_api_key_import_matches_existing_by_provider_and_raw_key() {
        let test_db_context = TestDbContext::new_sqlite("portable-provider-key-raw-match.sqlite");

        test_db_context.run_sync(|| {
            let provider_id = ID_GENERATOR.generate_id();
            let first_key_id = ID_GENERATOR.generate_id();
            let second_key_id = ID_GENERATOR.generate_id();
            let raw_key = "sk-provider-import-secret";
            let mut conn = get_connection().expect("connection");

            let (first, second) = with_transaction(&mut conn, |tx| {
                let provider = insert_provider(
                    tx,
                    &NewProvider {
                        id: provider_id,
                        provider_key: "openai".to_string(),
                        name: "OpenAI".to_string(),
                        endpoint: "https://api.openai.com/v1".to_string(),
                        use_proxy: false,
                        is_enabled: true,
                        created_at: 1000,
                        updated_at: 1000,
                        provider_type: ProviderType::Openai,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    },
                )?;
                let first = insert_provider_api_key_if_missing_by_raw_key(
                    tx,
                    &NewProviderApiKey {
                        id: first_key_id,
                        provider_id: provider.id,
                        api_key: raw_key.to_string(),
                        description: Some("primary".to_string()),
                        is_enabled: true,
                        created_at: 1000,
                        updated_at: 1000,
                    },
                )?;
                let second = insert_provider_api_key_if_missing_by_raw_key(
                    tx,
                    &NewProviderApiKey {
                        id: second_key_id,
                        provider_id: provider.id,
                        api_key: raw_key.to_string(),
                        description: Some("duplicate import".to_string()),
                        is_enabled: true,
                        created_at: 2000,
                        updated_at: 2000,
                    },
                )?;
                Ok((first, second))
            })
            .expect("provider key import should commit");

            assert!(matches!(first, ProviderApiKeyImportOutcome::Created(_)));
            assert!(matches!(second, ProviderApiKeyImportOutcome::Existing(_)));
            assert_eq!(first.key().id, second.key().id);
            assert_eq!(first.key().id, first_key_id);

            let keys = ProviderApiKey::list_by_provider_id(provider_id)
                .expect("provider keys should load");
            assert_eq!(keys.len(), 1);
            assert_eq!(keys[0].id, first_key_id);
            assert_eq!(keys[0].api_key, raw_key);
            assert_eq!(keys[0].description.as_deref(), Some("primary"));
        });
    }
}
