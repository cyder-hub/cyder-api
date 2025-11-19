use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{ConnectInfo, Path, Query, Request, State},
    routing::{any, get},
};

use crate::{
    controller::llm_types::LlmApiType,
    service::app_state::{create_state_router, StateRouter},
};

use super::anthropic::handle_anthropic_request;
use super::gemini::handle_gemini_request;
use super::handlers::{list_models_handler, openai_utility_handler};
use super::openai::handle_openai_request;

fn create_openai_router() -> StateRouter {
    create_state_router()
        .route(
            "/chat/completions",
            any(
                |State(app_state),
                 Query(query_params): Query<HashMap<String, String>>,
                 ConnectInfo(addr),
                 request: Request<Body>| async move {
                    handle_openai_request(app_state, addr, query_params, request).await
                },
            ),
        )
        .route(
            "/embeddings",
            any(
                |State(app_state),
                 Query(params): Query<HashMap<String, String>>,
                 ConnectInfo(addr),
                 request: Request<Body>| async move {
                    openai_utility_handler(app_state, addr, params, request, "embeddings")
                        .await
                },
            ),
        )
        .route(
            "/rerank",
            any(
                |State(app_state),
                 Query(params): Query<HashMap<String, String>>,
                 ConnectInfo(addr),
                 request: Request<Body>| async move {
                    openai_utility_handler(app_state, addr, params, request, "rerank").await
                },
            ),
        )
        .route(
            "/models",
            get(
                |State(app_state),
                 Query(params): Query<HashMap<String, String>>,
                 request: Request<Body>| async move {
                    list_models_handler(app_state, params, request, LlmApiType::OpenAI).await
                },
            ),
        )
}

fn create_anthropic_router() -> StateRouter {
    create_state_router()
        .route(
            "/messages",
            any(
                |State(app_state), ConnectInfo(addr), request: Request<Body>| async move {
                    handle_anthropic_request(app_state, addr, request).await
                },
            ),
        )
        .route(
            "/models",
            get(
                |State(app_state),
                 Query(params): Query<HashMap<String, String>>,
                 request: Request<Body>| async move {
                    list_models_handler(app_state, params, request, LlmApiType::Anthropic).await
                },
            ),
        )
}

pub fn create_proxy_router() -> StateRouter {
    let openai_router = create_openai_router();
    let anthropic_router = create_anthropic_router();
    create_state_router()
        .nest("/openai", openai_router.clone())
        .nest("/openai/v1", openai_router)
        .nest("/anthropic", anthropic_router.clone())
        .nest("/anthropic/v1", anthropic_router)
        .route(
            "/gemini/v1beta/models", // Exact match for listing models
            get(
                |State(app_state),
                 Query(params): Query<HashMap<String, String>>,
                 request: Request<Body>| async move {
                    list_models_handler(app_state, params, request, LlmApiType::Gemini).await
                },
            ),
        )
        .route(
            "/gemini/v1beta/models/{*model_action_segment}", // Wildcard for model actions
            any(
                |Path(path_segment): Path<String>,
                 Query(query_params): Query<HashMap<String, String>>,
                 State(app_state),
                 ConnectInfo(addr),
                 request: Request<Body>| async move {
                    handle_gemini_request(app_state, addr, path_segment, query_params, request)
                        .await
                },
            ),
        )
}
