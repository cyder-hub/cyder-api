use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    routing::{delete, get, post, put},
};
use serde::Deserialize;

use crate::controller::BaseError;
use crate::service::app_state::{AppState, StateRouter, create_state_router};
use crate::service::notification::types::{
    CreateNotificationChannelInput, NotificationChannelResponse, NotificationDeliveryListInput,
    NotificationDeliveryListResponse, NotificationWebhookTestResult,
    UpdateNotificationChannelInput,
};
use crate::utils::HttpResult;

#[derive(Debug, Deserialize)]
struct CreateNotificationChannelRequest {
    channel_key: String,
    name: String,
    endpoint_url: String,
    signing_secret: Option<String>,
    headers_json: Option<String>,
    cooldown_seconds: Option<i64>,
    #[serde(default = "default_enabled")]
    is_enabled: bool,
}

#[derive(Debug, Deserialize, Default)]
struct UpdateNotificationChannelRequest {
    name: Option<String>,
    endpoint_url: Option<String>,
    signing_secret: Option<String>,
    #[serde(default)]
    clear_signing_secret: bool,
    headers_json: Option<String>,
    #[serde(default)]
    clear_headers: bool,
    cooldown_seconds: Option<i64>,
    is_enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct NotificationDeliveryListQuery {
    alert_id: Option<i64>,
    channel_id: Option<i64>,
    status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

fn default_enabled() -> bool {
    true
}

async fn list_notification_channels(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<Vec<NotificationChannelResponse>>, BaseError> {
    Ok(HttpResult::new(app_state.notification.list_channels()?))
}

async fn get_notification_channel(
    State(app_state): State<Arc<AppState>>,
    Path(channel_id): Path<i64>,
) -> Result<HttpResult<NotificationChannelResponse>, BaseError> {
    Ok(HttpResult::new(
        app_state.notification.get_channel(channel_id)?,
    ))
}

async fn create_notification_channel(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<CreateNotificationChannelRequest>,
) -> Result<HttpResult<NotificationChannelResponse>, BaseError> {
    Ok(HttpResult::new(app_state.notification.create_channel(
        CreateNotificationChannelInput {
            channel_key: payload.channel_key,
            name: payload.name,
            endpoint_url: payload.endpoint_url,
            signing_secret: payload.signing_secret,
            headers_json: payload.headers_json,
            cooldown_seconds: payload.cooldown_seconds,
            is_enabled: payload.is_enabled,
        },
    )?))
}

async fn update_notification_channel(
    State(app_state): State<Arc<AppState>>,
    Path(channel_id): Path<i64>,
    Json(payload): Json<UpdateNotificationChannelRequest>,
) -> Result<HttpResult<NotificationChannelResponse>, BaseError> {
    Ok(HttpResult::new(app_state.notification.update_channel(
        channel_id,
        UpdateNotificationChannelInput {
            name: payload.name,
            endpoint_url: payload.endpoint_url,
            signing_secret: payload.signing_secret,
            clear_signing_secret: payload.clear_signing_secret,
            headers_json: payload.headers_json,
            clear_headers: payload.clear_headers,
            cooldown_seconds: payload.cooldown_seconds,
            is_enabled: payload.is_enabled,
        },
    )?))
}

async fn delete_notification_channel(
    State(app_state): State<Arc<AppState>>,
    Path(channel_id): Path<i64>,
) -> Result<HttpResult<NotificationChannelResponse>, BaseError> {
    Ok(HttpResult::new(
        app_state.notification.delete_channel(channel_id)?,
    ))
}

async fn test_notification_channel(
    State(app_state): State<Arc<AppState>>,
    Path(channel_id): Path<i64>,
) -> Result<HttpResult<NotificationWebhookTestResult>, BaseError> {
    let client = app_state.infra.client().await;
    Ok(HttpResult::new(
        app_state
            .notification
            .test_webhook_channel(channel_id, client.as_ref())
            .await?,
    ))
}

async fn list_notification_deliveries(
    State(app_state): State<Arc<AppState>>,
    Query(query): Query<NotificationDeliveryListQuery>,
) -> Result<HttpResult<NotificationDeliveryListResponse>, BaseError> {
    Ok(HttpResult::new(app_state.notification.list_deliveries(
        NotificationDeliveryListInput {
            alert_id: query.alert_id,
            channel_id: query.channel_id,
            status: query.status,
            limit: query.limit,
            offset: query.offset,
        },
    )?))
}

pub fn create_notification_router() -> StateRouter {
    create_state_router().nest(
        "/notifications",
        create_state_router()
            .route("/deliveries", get(list_notification_deliveries))
            .route("/channels", get(list_notification_channels))
            .route("/channels", post(create_notification_channel))
            .route("/channels/{id}", get(get_notification_channel))
            .route("/channels/{id}", put(update_notification_channel))
            .route("/channels/{id}", delete(delete_notification_channel))
            .route("/channels/{id}/test", post(test_notification_channel)),
    )
}

#[cfg(test)]
mod tests {
    use super::{CreateNotificationChannelRequest, UpdateNotificationChannelRequest};

    #[test]
    fn create_channel_defaults_to_enabled() {
        let payload: CreateNotificationChannelRequest = serde_json::from_str(
            r#"{"channel_key":"ops","name":"Ops","endpoint_url":"https://example.com"}"#,
        )
        .unwrap();
        assert!(payload.is_enabled);
    }

    #[test]
    fn update_channel_defaults_to_not_clearing_secret() {
        let payload: UpdateNotificationChannelRequest = serde_json::from_str("{}").unwrap();
        assert!(!payload.clear_signing_secret);
        assert!(!payload.clear_headers);
    }
}
