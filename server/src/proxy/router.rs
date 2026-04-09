use std::{collections::HashMap, sync::Arc};

use axum::{
    body::Body,
    extract::{ConnectInfo, Path, Query, Request, State},
    routing::{MethodRouter, any, get},
};
use tower_http::cors::{Any, CorsLayer};

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
}
