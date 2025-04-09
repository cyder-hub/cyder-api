use axum::{extract::Query, routing::get, Router};

use crate::{
    database::{ListResult ,record::{Record, RecordQueryPayload}},
    utils::HttpResult,
};

use super::error::BaseError;

async fn list_record(
    Query(payload): Query<RecordQueryPayload>,
) -> Result<HttpResult<ListResult<Record>>, BaseError> {
    match Record::list(payload) {
        Ok(result) => Ok(HttpResult::new(result)),
        Err(_) => Err(BaseError::DatabaseFatal(None)),
    }
}

async fn get_record() {}

pub fn create_record_router() -> Router {
    Router::new().nest(
        "/record",
        Router::new()
            .route("/list", get(list_record))
            .route("/{id}", get(get_record)),
    )
}
