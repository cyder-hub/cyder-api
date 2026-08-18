use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Path, State},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;

use crate::{
    database::api_key::{
        ApiKeyDetail, ApiKeyDetailWithSecret, ApiKeyReveal, ApiKeySummary, CreateApiKeyPayload,
        UpdateApiKeyMetadataPayload,
    },
    service::app_state::{AppState, StateRouter, create_state_router},
    service::runtime::{ApiKeyBilledAmountSnapshot, ApiKeyGovernanceSnapshot},
    utils::{HttpResult, auth::ManagerAuthContext},
};

use super::{BaseError, auth::authorize_secret_governance_command};

#[derive(Debug, Clone, Serialize)]
struct ApiKeyBilledAmountSnapshotResponse {
    currency: String,
    amount_nanos: i64,
}

#[derive(Debug, Clone, Serialize)]
struct ApiKeyRuntimeSnapshotResponse {
    api_key_id: i64,
    current_concurrency: u32,
    current_minute_bucket: Option<i64>,
    current_minute_request_count: u32,
    day_bucket: Option<i64>,
    daily_request_count: i64,
    daily_token_count: i64,
    month_bucket: Option<i64>,
    monthly_token_count: i64,
    daily_billed_amounts: Vec<ApiKeyBilledAmountSnapshotResponse>,
    monthly_billed_amounts: Vec<ApiKeyBilledAmountSnapshotResponse>,
}

impl From<ApiKeyBilledAmountSnapshot> for ApiKeyBilledAmountSnapshotResponse {
    fn from(value: ApiKeyBilledAmountSnapshot) -> Self {
        Self {
            currency: value.currency,
            amount_nanos: value.amount_nanos,
        }
    }
}

impl From<ApiKeyGovernanceSnapshot> for ApiKeyRuntimeSnapshotResponse {
    fn from(value: ApiKeyGovernanceSnapshot) -> Self {
        Self {
            api_key_id: value.api_key_id,
            current_concurrency: value.current_concurrency,
            current_minute_bucket: value.current_minute_bucket,
            current_minute_request_count: value.current_minute_request_count,
            day_bucket: value.day_bucket,
            daily_request_count: value.daily_request_count,
            daily_token_count: value.daily_token_count,
            month_bucket: value.month_bucket,
            monthly_token_count: value.monthly_token_count,
            daily_billed_amounts: value
                .daily_billed_amounts
                .into_iter()
                .map(Into::into)
                .collect(),
            monthly_billed_amounts: value
                .monthly_billed_amounts
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

async fn create_api_key(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_context): Extension<ManagerAuthContext>,
    Json(payload): Json<CreateApiKeyPayload>,
) -> Result<HttpResult<ApiKeyDetailWithSecret>, Response> {
    authorize_secret_governance_command(&app_state, &auth_context)?;
    let created = app_state
        .admin
        .api_key
        .create_api_key(payload)
        .await
        .map_err(IntoResponse::into_response)?;
    Ok(HttpResult::new(created))
}

async fn list_api_keys(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<Vec<ApiKeySummary>>, BaseError> {
    Ok(HttpResult::new(
        app_state.admin.api_key.list_api_keys().await?,
    ))
}

async fn get_api_key_detail(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ApiKeyDetail>, BaseError> {
    Ok(HttpResult::new(
        app_state.admin.api_key.get_api_key_detail(id).await?,
    ))
}

async fn update_api_key(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateApiKeyMetadataPayload>,
) -> Result<HttpResult<ApiKeyDetail>, BaseError> {
    Ok(HttpResult::new(
        app_state.admin.api_key.update_api_key(id, payload).await?,
    ))
}

async fn rotate_api_key(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_context): Extension<ManagerAuthContext>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ApiKeyReveal>, Response> {
    authorize_secret_governance_command(&app_state, &auth_context)?;
    Ok(HttpResult::new(
        app_state
            .admin
            .api_key
            .rotate_api_key(id)
            .await
            .map_err(IntoResponse::into_response)?,
    ))
}

async fn reveal_api_key(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_context): Extension<ManagerAuthContext>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ApiKeyReveal>, Response> {
    authorize_secret_governance_command(&app_state, &auth_context)?;
    Ok(HttpResult::new(
        app_state
            .admin
            .api_key
            .reveal_api_key(id)
            .await
            .map_err(IntoResponse::into_response)?,
    ))
}

async fn delete_api_key(
    State(app_state): State<Arc<AppState>>,
    Extension(auth_context): Extension<ManagerAuthContext>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, Response> {
    authorize_secret_governance_command(&app_state, &auth_context)?;
    app_state
        .admin
        .api_key
        .delete_api_key(id)
        .await
        .map_err(IntoResponse::into_response)?;
    Ok(HttpResult::new(()))
}

async fn get_api_key_runtime_snapshot(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<ApiKeyRuntimeSnapshotResponse>, BaseError> {
    app_state.admin.api_key.ensure_api_key_exists(id).await?;
    let snapshot = app_state
        .api_key_governance
        .get_api_key_governance_snapshot(id)
        .await?;
    Ok(HttpResult::new(snapshot.into()))
}

async fn list_api_key_runtime_snapshots(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<Vec<ApiKeyRuntimeSnapshotResponse>>, BaseError> {
    let snapshots = app_state
        .api_key_governance
        .list_api_key_governance_snapshots()
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(HttpResult::new(snapshots))
}

pub fn create_api_key_management_router() -> StateRouter {
    create_state_router().nest(
        "/api_key",
        create_state_router()
            .route("/", post(create_api_key))
            .route("/list", get(list_api_keys))
            .route("/runtime/list", get(list_api_key_runtime_snapshots))
            .route(
                "/{id}",
                get(get_api_key_detail)
                    .put(update_api_key)
                    .delete(delete_api_key),
            )
            .route("/{id}/rotate", post(rotate_api_key))
            .route("/{id}/reveal", post(reveal_api_key))
            .route("/{id}/runtime", get(get_api_key_runtime_snapshot)),
    )
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, sync::Arc};

    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode},
        response::IntoResponse,
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use crate::config::SecretEncryptionConfig;
    use crate::database::TestDatabase;
    use crate::database::api_key::CreateApiKeyPayload;
    use crate::ingress::client_identity::{ClientIdentity, ClientIdentitySource};
    use crate::schema::enum_def::Action;
    use crate::service::admin::AdminServices;
    use crate::service::app_state::create_test_app_state;
    use crate::service::secret_encryption::SecretEncryptionService;
    use crate::utils::auth::decode_access_token;

    use super::{BaseError, create_api_key_management_router};

    const CURRENT_KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn payload() -> CreateApiKeyPayload {
        CreateApiKeyPayload {
            name: "controller-reveal-contract".to_string(),
            description: None,
            default_action: Some(Action::Allow),
            is_enabled: Some(true),
            expires_at: None,
            rate_limit_rpm: None,
            max_concurrent_requests: None,
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            acl_rules: None,
        }
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        serde_json::from_slice(&bytes).expect("response should be JSON")
    }

    #[tokio::test]
    async fn api_key_reveal_contract_is_post_only_and_returns_recoverable_secret() {
        let database = TestDatabase::new_sqlite_default("controller-api-key-reveal.sqlite").await;
        (async {
            let base = create_test_app_state(database.clone()).await;
            let config: SecretEncryptionConfig = serde_yaml::from_str(&format!(
                "downstream_mode: recoverable\nencryption_key: '{CURRENT_KEY}'\n"
            ))
            .expect("recoverable config should parse");
            let encryption = Arc::new(SecretEncryptionService::from_config(&config));
            let admin = Arc::new(
                AdminServices::new(Arc::clone(&base.catalog), Arc::clone(&encryption)).await,
            );
            let mut configured = (*base).clone();
            configured.admin = admin;
            configured.secret_encryption = encryption;
            let app_state = Arc::new(configured);
            let tokens = app_state
                .admin
                .auth
                .bootstrap("controller api key disabled TOTP password")
                .await
                .expect("manager bootstrap should succeed");
            let auth_context =
                decode_access_token(&tokens.access_token).expect("bootstrap access should decode");

            let created = app_state
                .admin
                .api_key
                .create_api_key(payload())
                .await
                .expect("recoverable API key should create");
            let route = format!("/api_key/{}/reveal", created.detail.id);

            let mut request = Request::builder()
                .method(Method::POST)
                .uri(&route)
                .body(Body::empty())
                .expect("POST request should build");
            request.extensions_mut().insert(auth_context);
            request.extensions_mut().insert(ClientIdentity {
                client_ip: "127.0.0.1".parse().expect("test IP should parse"),
                peer_addr: SocketAddr::from(([127, 0, 0, 1], 31_200)),
                source: ClientIdentitySource::TcpPeer,
                trusted_proxy_hops: 0,
            });
            let response = create_api_key_management_router()
                .with_state(Arc::clone(&app_state))
                .oneshot(request)
                .await
                .expect("POST reveal should respond");
            assert_eq!(response.status(), StatusCode::OK);
            let body = response_json(response).await;
            assert_eq!(body["data"]["api_key"], created.reveal.api_key);
            assert_eq!(body["data"]["can_reveal"], true);

            let response = create_api_key_management_router()
                .with_state(app_state)
                .oneshot(
                    Request::builder()
                        .method(Method::GET)
                        .uri(&route)
                        .body(Body::empty())
                        .expect("GET request should build"),
                )
                .await
                .expect("GET reveal should respond");
            assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        })
        .await;
    }

    #[tokio::test]
    async fn manager_api_key_openapi_matches_routes_safe_dtos_and_errors() {
        let document: serde_yaml::Value = serde_yaml::from_str(include_str!(
            "../../../docs/openapi/manager-api-key.openapi.yaml"
        ))
        .expect("manager API key OpenAPI should parse");
        assert_eq!(document["openapi"].as_str(), Some("3.1.0"));
        assert_eq!(document["info"]["version"].as_str(), Some("1.0.0-pre.3"));
        assert_eq!(
            document["x-cyder-default-cache-control"].as_str(),
            Some("no-store")
        );

        let expected_operations = [
            ("/ai/manager/api/api_key/list", "get"),
            ("/ai/manager/api/api_key", "post"),
            ("/ai/manager/api/api_key/{id}", "get"),
            ("/ai/manager/api/api_key/{id}", "put"),
            ("/ai/manager/api/api_key/{id}", "delete"),
            ("/ai/manager/api/api_key/{id}/rotate", "post"),
            ("/ai/manager/api/api_key/{id}/reveal", "post"),
        ];
        for (path, method) in expected_operations {
            let operation = &document["paths"][path][method];
            assert!(
                operation.is_mapping(),
                "operation should exist for {method} {path}"
            );
            assert!(
                operation["x-cyder-error-codes"].is_sequence(),
                "error contract should exist for {method} {path}"
            );
        }
        for (path, method) in [
            ("/ai/manager/api/api_key", "post"),
            ("/ai/manager/api/api_key/{id}", "delete"),
            ("/ai/manager/api/api_key/{id}/rotate", "post"),
            ("/ai/manager/api/api_key/{id}/reveal", "post"),
        ] {
            assert!(
                document["paths"][path][method]["parameters"].is_null(),
                "{method} {path} must use the session grant instead of a TOTP header"
            );
            let codes = document["paths"][path][method]["x-cyder-error-codes"]
                .as_sequence()
                .expect("secret governance error codes should be a sequence")
                .iter()
                .filter_map(serde_yaml::Value::as_u64)
                .collect::<Vec<_>>();
            for code in [1479, 1480, 1491] {
                assert!(codes.contains(&code), "{method} {path} must include {code}");
            }
            for code in [1471, 1472, 1473, 1474, 1475, 1476, 1484, 1485] {
                assert!(
                    !codes.contains(&code),
                    "{method} {path} must not expose per-command TOTP error {code}"
                );
            }
        }
        for (path, method) in [
            ("/ai/manager/api/api_key/list", "get"),
            ("/ai/manager/api/api_key/{id}", "get"),
            ("/ai/manager/api/api_key/{id}", "put"),
        ] {
            assert!(
                document["paths"][path][method]["parameters"].is_null(),
                "{method} {path} must not require sensitive TOTP"
            );
        }
        assert!(
            document["paths"]["/ai/manager/api/api_key/{id}/reveal"]["get"].is_null(),
            "GET Reveal must not be documented"
        );
        assert!(
            document["paths"]["/ai/manager/api/api_key/{id}/rotate"]["post"]["requestBody"]
                .is_null(),
            "Rotate must have no request body"
        );
        assert!(
            document["paths"]["/ai/manager/api/api_key/{id}/reveal"]["post"]["requestBody"]
                .is_null(),
            "Reveal must have no request body"
        );

        let summary_required = document["components"]["schemas"]["ApiKeySummary"]["required"]
            .as_sequence()
            .expect("API key summary required fields")
            .iter()
            .filter_map(serde_yaml::Value::as_str)
            .collect::<Vec<_>>();
        assert!(summary_required.contains(&"can_reveal"));
        for forbidden in [
            "api_key",
            "api_key_hash",
            "secret_ciphertext",
            "secret_nonce",
            "secret_key_fingerprint",
        ] {
            assert!(!summary_required.contains(&forbidden));
        }

        let reveal_required = document["components"]["schemas"]["ApiKeyReveal"]["required"]
            .as_sequence()
            .expect("API key reveal required fields")
            .iter()
            .filter_map(serde_yaml::Value::as_str)
            .collect::<Vec<_>>();
        assert!(reveal_required.contains(&"api_key"));
        assert!(reveal_required.contains(&"can_reveal"));
        assert_eq!(
            document["components"]["schemas"]["ApiKeySecretUnavailableError"]["properties"]["code"]
                ["const"]
                .as_u64(),
            Some(1004)
        );
        assert_eq!(
            document["components"]["schemas"]["ApiKeySecretUnavailableError"]["properties"]["msg"]
                ["const"]
                .as_str(),
            Some("api key secret is unavailable")
        );

        for (path, method) in expected_operations {
            let success = &document["paths"][path][method]["responses"]["200"];
            assert_eq!(
                success["headers"]["Cache-Control"]["$ref"].as_str(),
                Some("#/components/headers/NoStore"),
                "successful {method} {path} must document no-store"
            );
        }

        let response = BaseError::ApiKeySecretUnavailable.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = response_json(response).await;
        assert_eq!(body["code"].as_u64(), Some(1004));
        assert_eq!(body["msg"].as_str(), Some("api key secret is unavailable"));
    }
}
