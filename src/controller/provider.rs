use crate::database::{
    provider::{CustomField, FullCommitData, Provider},
    DbResult,
};
use axum::{
    extract::Path,
    routing::{delete, get, post, put},
    Json, Router,
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize}; // Added Serialize import

use crate::utils::HttpResult;

use super::BaseError;
use crate::database::model::Model; // Added Model import
use crate::database::provider::ProviderApiKey; // Added ProviderKey import

#[derive(Serialize)]
struct ProviderDetail {
    provider: Provider,
    models: Vec<Model>,
    provider_keys: Vec<ProviderApiKey>,
    custom_fields: Vec<CustomField>,
}

async fn list() -> (StatusCode, HttpResult<Vec<Provider>>) {
    let result = Provider::list().unwrap();
    (StatusCode::OK, HttpResult::new(result))
}

#[derive(Deserialize)]
struct InserPayload {
    pub name: String,
    pub key: String,
    pub endpoint: String,
    pub omit_config: Option<String>,
    pub limit_model: bool,
    pub use_proxy: bool,
    pub api_keys: Vec<String>,
}

async fn insert(Json(payload): Json<InserPayload>) -> DbResult<HttpResult<Provider>> {
    let provider = Provider::new(
        None,
        &payload.key,
        &payload.name,
        &payload.endpoint,
        payload.omit_config.as_deref(),
        payload.limit_model,
        payload.use_proxy,
    );
    Provider::insert_one(&provider, payload.api_keys)?;
    Ok(HttpResult::new(provider))
}

async fn get_provider(Path(id): Path<i64>) -> Result<HttpResult<Provider>, BaseError> {
    match Provider::query_one(id) {
        Ok(pro) => Ok(HttpResult::new(pro)),
        Err(err) => Err(err),
    }
}

async fn update_provider(
    Path(id): Path<i64>,
    Json(payload): Json<InserPayload>,
) -> Result<HttpResult<Provider>, BaseError> {
    let provider = Provider::new(
        Some(id),
        &payload.key,
        &payload.name,
        &payload.endpoint,
        payload.omit_config.as_deref(),
        payload.limit_model,
        payload.use_proxy,
    );
    Provider::update_one(&provider)?;
    Ok(HttpResult::new(provider))
}

async fn delete_provider(Path(id): Path<i64>) -> Result<HttpResult<()>, BaseError> {
    match Provider::delete_one(id) {
        Ok(_) => Ok(HttpResult::new(())),
        Err(err) => Err(err),
    }
}

async fn get_provider_detail(Path(id): Path<i64>) -> Result<HttpResult<ProviderDetail>, BaseError> {
    let provider = Provider::query_one(id)?;
    let models = Model::list_by_provider_id(id)?;
    let provider_keys = ProviderApiKey::list_by_provider_id(id)?;
    let custom_fields = CustomField::list_by_provider_id(id)?;

    let detail = ProviderDetail {
        provider,
        models,
        provider_keys,
        custom_fields,
    };

    Ok(HttpResult::new(detail))
}

async fn full_commit(Json(data): Json<FullCommitData>) -> Result<HttpResult<String>, BaseError> {
    Provider::full_commit(data)?;
    Ok(HttpResult::new("ok".to_string()))
}

async fn list_provider_details() -> Result<(StatusCode, HttpResult<Vec<ProviderDetail>>), BaseError> {
    let providers = Provider::list()?;
    let mut provider_details: Vec<ProviderDetail> = Vec::new();

    for provider in providers {
        let models = Model::list_by_provider_id(provider.id)?;
        let provider_keys = ProviderApiKey::list_by_provider_id(provider.id)?;
        let custom_fields = CustomField::list_by_provider_id(provider.id)?;

        let detail = ProviderDetail {
            provider,
            models,
            provider_keys,
            custom_fields,
        };
        provider_details.push(detail);
    }

    Ok((StatusCode::OK, HttpResult::new(provider_details)))
}

pub fn create_provider_router() -> Router {
    Router::new().nest(
        "/provider",
        Router::new()
            .route("/", post(insert))
            .route("/commit", post(full_commit))
            .route("/list", get(list))
            .route("/detail/list", get(list_provider_details))
            .route("/{id}", get(get_provider))
            .route("/{id}/detail", get(get_provider_detail))
            .route("/{id}", delete(delete_provider))
            .route("/{id}", put(update_provider)),
    )
}
