use std::{collections::HashMap, sync::Arc};

use axum::{
    body::Body,
    extract::{ConnectInfo, Path, Query, Request, State},
    http::{HeaderValue, header::CACHE_CONTROL},
    routing::{MethodRouter, any, get},
};
use tower_http::cors::{Any, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    schema::enum_def::LlmApiType,
    service::app_state::{AppState, StateRouter, create_state_router},
};

use super::gemini::handle_gemini_request;
use super::handlers::{list_models_handler, openai_utility_handler};
use super::unified::unified_proxy_handler;

type QueryParams = HashMap<String, String>;

fn generation_route(api_type: LlmApiType) -> MethodRouter<Arc<AppState>> {
    any(
        move |State(app_state),
              Query(query_params): Query<QueryParams>,
              ConnectInfo(addr),
              request: Request<Body>| async move {
            unified_proxy_handler(app_state, addr, query_params, api_type, request).await
        },
    )
}

fn openai_utility_route(downstream_path: &'static str) -> MethodRouter<Arc<AppState>> {
    any(
        move |State(app_state),
              Query(params): Query<QueryParams>,
              ConnectInfo(addr),
              request: Request<Body>| async move {
            openai_utility_handler(app_state, addr, params, request, downstream_path).await
        },
    )
}

fn models_route(api_type: LlmApiType) -> MethodRouter<Arc<AppState>> {
    get(
        move |State(app_state), Query(params): Query<QueryParams>, request: Request<Body>| async move {
            list_models_handler(app_state, params, request, api_type).await
        },
    )
}

fn add_generation_routes(
    router: StateRouter,
    api_type: LlmApiType,
    paths: &[&'static str],
) -> StateRouter {
    paths.iter().fold(router, |router, path| {
        router.route(path, generation_route(api_type))
    })
}

fn add_openai_utility_routes(
    router: StateRouter,
    paths: &[(&'static str, &'static str)],
) -> StateRouter {
    paths
        .iter()
        .fold(router, |router, (route_path, downstream_path)| {
            router.route(route_path, openai_utility_route(downstream_path))
        })
}

fn nest_router_variants(
    router: StateRouter,
    include_root_routes: bool,
    version_prefixes: &[&'static str],
) -> StateRouter {
    let router_variants = version_prefixes
        .iter()
        .fold(create_state_router(), |router_variants, version_prefix| {
            router_variants.nest(version_prefix, router.clone())
        });

    if include_root_routes {
        router_variants.merge(router)
    } else {
        router_variants
    }
}

fn create_openai_router() -> StateRouter {
    let router = add_generation_routes(
        create_state_router(),
        LlmApiType::Openai,
        &["/chat/completions"],
    );
    let router = add_openai_utility_routes(
        router,
        &[("/embeddings", "embeddings"), ("/rerank", "rerank")],
    )
    .route("/models", models_route(LlmApiType::Openai));

    nest_router_variants(router, true, &["/v1"])
}

fn create_anthropic_router() -> StateRouter {
    let router =
        add_generation_routes(create_state_router(), LlmApiType::Anthropic, &["/messages"])
            .route("/models", models_route(LlmApiType::Anthropic));

    nest_router_variants(router, true, &["/v1"])
}

fn create_ollama_router() -> StateRouter {
    add_generation_routes(
        create_state_router(),
        LlmApiType::Ollama,
        &["/api/chat", "/api/generate", "/api/embeddings"],
    )
    .route("/api/tags", models_route(LlmApiType::Ollama))
}

fn create_responses_router() -> StateRouter {
    let router = add_generation_routes(
        create_state_router(),
        LlmApiType::Responses,
        &["/responses"],
    )
    .route("/models", models_route(LlmApiType::Responses));

    nest_router_variants(router, true, &["/v1"])
}

fn create_gemini_router() -> StateRouter {
    let router = create_state_router()
        .route("/models", models_route(LlmApiType::Gemini))
        .route(
            "/models/{*model_action_segment}",
            any(
                |Path(path_segment): Path<String>,
                 Query(query_params): Query<QueryParams>,
                 State(app_state),
                 ConnectInfo(addr),
                 request: Request<Body>| async move {
                    handle_gemini_request(app_state, addr, path_segment, query_params, request)
                        .await
                },
            ),
        );

    nest_router_variants(router, false, &["/v1beta", "/v1"])
}

pub fn create_proxy_router() -> StateRouter {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    create_state_router()
        .nest("/openai", create_openai_router())
        .nest("/anthropic", create_anthropic_router())
        .nest("/ollama", create_ollama_router())
        .nest("/responses", create_responses_router())
        .nest("/gemini", create_gemini_router())
        .layer(cors)
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::Body,
        http::{Method, Request, StatusCode, header},
    };
    use tower::ServiceExt;

    use crate::database::TestDbContext;
    use crate::database::api_key::{ApiKey, CreateApiKeyPayload};
    use crate::schema::enum_def::Action;
    use crate::service::app_state::create_test_app_state;

    use super::create_proxy_router;

    fn payload() -> CreateApiKeyPayload {
        CreateApiKeyPayload {
            name: "proxy-no-store".to_string(),
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

    fn request(path: &str, header_name: &str, api_key: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method(Method::GET).uri(path);
        if let Some(api_key) = api_key {
            let value = if header_name == header::AUTHORIZATION.as_str() {
                format!("Bearer {api_key}")
            } else {
                api_key.to_string()
            };
            builder = builder.header(header_name, value);
        }
        builder
            .body(Body::empty())
            .expect("proxy request should build")
    }

    fn assert_no_store(response: &axum::response::Response) {
        assert_eq!(
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("no-store")
        );
    }

    #[tokio::test]
    async fn all_proxy_protocols_override_cache_control_on_success_and_auth_error() {
        let database = TestDbContext::new_sqlite("proxy-api-no-store.sqlite");
        database
            .run_async(async {
                let created = ApiKey::create(&payload()).expect("proxy key should create");
                let api_key = created.reveal.api_key;
                let app_state = create_test_app_state(database.clone()).await;
                let cases = [
                    ("/openai/v1/models", header::AUTHORIZATION.as_str()),
                    ("/responses/v1/models", header::AUTHORIZATION.as_str()),
                    ("/anthropic/v1/models", "x-api-key"),
                    ("/gemini/v1/models", "x-goog-api-key"),
                    ("/ollama/api/tags", header::AUTHORIZATION.as_str()),
                ];

                for (path, header_name) in cases {
                    let success = create_proxy_router()
                        .with_state(Arc::clone(&app_state))
                        .oneshot(request(path, header_name, Some(&api_key)))
                        .await
                        .expect("proxy success should respond");
                    assert_eq!(success.status(), StatusCode::OK, "{path}");
                    assert_no_store(&success);

                    let error = create_proxy_router()
                        .with_state(Arc::clone(&app_state))
                        .oneshot(request(path, header_name, None))
                        .await
                        .expect("proxy auth error should respond");
                    assert_eq!(error.status(), StatusCode::UNAUTHORIZED, "{path}");
                    assert_no_store(&error);
                }
            })
            .await;
    }
}
