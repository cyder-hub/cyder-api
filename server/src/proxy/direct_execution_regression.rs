use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    io::Write as _,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    body::{Body, Bytes},
    extract::Request,
    http::{
        HeaderMap, Method, StatusCode,
        header::{CONTENT_TYPE, LOCATION},
    },
    response::Response,
    routing::any,
    serve,
};
use diesel::prelude::*;
use flate2::{Compression, write::GzEncoder};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    net::TcpListener,
    sync::{Mutex as AsyncMutex, Notify, oneshot},
    task::JoinHandle,
    time::timeout,
};
use tower::ServiceExt;

use super::{
    ExecutionStage, ResponseVisibility,
    cancellation::ProxyCancellationContext,
    create_proxy_router,
    logging::{RequestLogPersistedContext, RequestLogPersistedSink},
    request_context::{X_CLIENT_REQUEST_ID, X_REQUEST_ID},
};
use crate::{
    config::{ClientIdentityConfig, OutboundHttpConfig, ProxyRequestConfig},
    cost::{CostSnapshot, MeterKey},
    database::{
        DbConnection, TestDbContext,
        api_key::{ApiKey, CreateApiKeyPayload},
        cost::{
            CostCatalog, CostCatalogVersion, CostComponent, NewCostCatalogPayload,
            NewCostCatalogVersionPayload, NewCostComponentPayload,
        },
        get_connection,
        model::{Model, UpdateModelData},
        model_source_binding::{ModelSourceBindingInput, ModelSourceConfig, replace_for_model},
        provider::Provider,
        request_log::{RequestLog, RequestLogQueryPayload, RequestLogRecord},
        request_patch::{RequestPatchRuleInput, RequestPatchVariantInput},
        upstream_source::{NewUpstreamSource, UpdateUpstreamSourceData, UpstreamSource},
    },
    ingress::client_identity::ClientIdentityResolver,
    schema::enum_def::{
        Action, DownstreamProtocol, ModelKind, ProviderApiKeyMode, RequestPatchOperation,
        RequestPatchPlacement, RequestStatus, UpstreamProfileType, UpstreamProtocol,
    },
    service::{
        admin::provider::{BootstrapProviderCommand, ReplaceProviderApiKeyInput},
        app_state::{AppState, create_test_app_state},
        infra::AppInfra,
        transform::{StreamTransformer, TransformOutcomeKind},
        upstream_profile::upstream_runtime_profile,
        vertex::{cache_vertex_token_for_test, vertex_token_is_cached_for_test},
    },
    utils::{
        ID_GENERATOR,
        sse::{SseEvent, SseFrame, SseParser},
    },
};

const DOWNSTREAM_SECRET_MARKER: &str = "cyder-";
const PROVIDER_SECRET: &str = "provider-baseline-secret";
const UPSTREAM_MODEL: &str = "baseline-upstream-model";
const DIRECT_CLIENT_REQUEST_ID: &str = "direct-execution.client-1";
const WAIT_TIMEOUT: Duration = Duration::from_secs(5);

const FIXTURE_SOURCES: [(&str, &str); 4] = [
    (
        "openai",
        include_str!("../service/transform/testdata/direct_execution/openai.json"),
    ),
    (
        "responses",
        include_str!("../service/transform/testdata/direct_execution/responses.json"),
    ),
    (
        "anthropic",
        include_str!("../service/transform/testdata/direct_execution/anthropic.json"),
    ),
    (
        "gemini",
        include_str!("../service/transform/testdata/direct_execution/gemini.json"),
    ),
];

const RESPONSES_TARGET_SOURCE: &str =
    include_str!("../service/transform/testdata/direct_execution/responses_target.json");
const ANTHROPIC_TARGET_SOURCE: &str =
    include_str!("../service/transform/testdata/direct_execution/anthropic_target.json");
const GEMINI_TARGET_SOURCE: &str =
    include_str!("../service/transform/testdata/direct_execution/gemini_target.json");

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct GoldenEvent {
    #[serde(default)]
    event: Option<String>,
    data: Value,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum DownstreamAuth {
    Bearer,
    XApiKey,
    GeminiQuery,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct RequestGolden {
    pub(super) downstream: Value,
    pub(super) upstream: Value,
    pub(super) upstream_path: String,
    #[serde(default)]
    upstream_query: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct NonStreamGolden {
    pub(super) upstream_response: Value,
    downstream_status: u16,
    downstream_content_type: String,
    downstream_response: Value,
}

#[derive(Clone, Debug, Deserialize)]
struct StreamGolden {
    downstream_request: Value,
    upstream_request: Value,
    upstream_path: String,
    #[serde(default)]
    upstream_query: Option<String>,
    upstream_events: Vec<GoldenEvent>,
    downstream_events: Vec<GoldenEvent>,
    downstream_content_type: String,
    expected_text: String,
}

#[derive(Clone, Debug, Deserialize)]
struct UsageGolden {
    input: i32,
    output: i32,
    total: i32,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct ErrorGolden {
    pub(super) downstream_request: Value,
    pub(super) upstream_status: u16,
    pub(super) upstream_response: Value,
    pub(super) downstream_status: u16,
    pub(super) downstream_response: Value,
}

#[derive(Clone, Debug, Deserialize)]
struct CancellationGolden {
    downstream_request: Value,
    upstream_request: Value,
    upstream_path: String,
    #[serde(default)]
    upstream_query: Option<String>,
    first_upstream_event: GoldenEvent,
    expected_status: RequestStatus,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct DirectExecutionFixture {
    pub(super) protocol: DownstreamProtocol,
    pub(super) profile_type: UpstreamProfileType,
    pub(super) downstream_path: String,
    downstream_stream_path: String,
    pub(super) downstream_auth: DownstreamAuth,
    upstream_headers: BTreeMap<String, String>,
    pub(super) request: RequestGolden,
    pub(super) non_stream: NonStreamGolden,
    stream: StreamGolden,
    usage: UsageGolden,
    pub(super) error: ErrorGolden,
    cancellation: CancellationGolden,
}

#[derive(Clone, Debug, Deserialize)]
struct ResponsesTargetRequestGolden {
    non_stream: Value,
    stream: Value,
}

#[derive(Clone, Debug, Deserialize)]
struct ResponsesHttpErrorGolden {
    status: u16,
    response: Value,
}

#[derive(Clone, Debug, Deserialize)]
struct ResponsesTargetErrorGolden {
    http_429: ResponsesHttpErrorGolden,
    failed_response: Value,
    error_event: GoldenEvent,
}

#[derive(Clone, Debug, Deserialize)]
struct ResponsesTargetCancellationGolden {
    first_upstream_event: GoldenEvent,
}

#[derive(Clone, Debug, Deserialize)]
struct ResponsesTargetGolden {
    profile_type: UpstreamProfileType,
    base_url: String,
    upstream_headers: BTreeMap<String, String>,
    upstream_path: String,
    requests: BTreeMap<String, ResponsesTargetRequestGolden>,
    non_stream_response: Value,
    stream_events: Vec<GoldenEvent>,
    usage: UsageGolden,
    error: ResponsesTargetErrorGolden,
    cancellation: ResponsesTargetCancellationGolden,
}

#[derive(Clone, Debug, Deserialize)]
struct AnthropicTargetRequestGolden {
    downstream: String,
    non_stream: Value,
    stream: Value,
}

#[derive(Clone, Debug, Deserialize)]
struct AnthropicTargetErrorGolden {
    http_429: ResponsesHttpErrorGolden,
    stream_error_event: GoldenEvent,
}

#[derive(Clone, Debug, Deserialize)]
struct AnthropicTargetGolden {
    profile_type: UpstreamProfileType,
    base_url: String,
    upstream_headers: BTreeMap<String, String>,
    upstream_path: String,
    requests: Vec<AnthropicTargetRequestGolden>,
    non_stream_response: Value,
    stream_events: Vec<GoldenEvent>,
    usage: UsageGolden,
    error: AnthropicTargetErrorGolden,
    cancellation: ResponsesTargetCancellationGolden,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum GeminiFixtureAuth {
    XGoogApiKey,
    Bearer,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum GeminiFixtureAction {
    GenerateContent,
    StreamGenerateContent,
    CountTokens,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum GeminiFixtureCapability {
    Tools,
    Reasoning,
    Multimodal,
    StructuredOutput,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum GeminiFixtureCellStatus {
    Full,
    ControlledLoss,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum GeminiFixtureTerminal {
    Stop,
    MaxTokens,
    Safety,
    PromptBlock,
    ApplicationFailure,
    ObservationDegraded,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeminiTargetProfileGolden {
    profile_type: UpstreamProfileType,
    base_url: String,
    auth: GeminiFixtureAuth,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeminiTargetOperationGolden {
    action: GeminiFixtureAction,
    suffix: String,
    #[serde(default)]
    query: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeminiAdvancedRequestGolden {
    capability: GeminiFixtureCapability,
    status: GeminiFixtureCellStatus,
    downstream_request: Value,
    expected_upstream: Value,
    #[serde(default)]
    expected_loss_reason: Option<String>,
    rejection_request: Value,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeminiTargetRequestGolden {
    downstream: DownstreamProtocol,
    non_stream: Value,
    stream: Value,
    advanced: Vec<GeminiAdvancedRequestGolden>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeminiCountTokensGolden {
    contents_request: Value,
    generate_content_request: Value,
    response: Value,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeminiTerminalGolden {
    terminal: GeminiFixtureTerminal,
    response: Value,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeminiOfficialFieldsGolden {
    request_part_tags: Vec<String>,
    finish_reasons: Vec<String>,
    usage_fields: Vec<String>,
    count_tokens_fields: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeminiTargetErrorGolden {
    http_429: ResponsesHttpErrorGolden,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeminiTargetGolden {
    fixture_version: u8,
    model: String,
    profiles: Vec<GeminiTargetProfileGolden>,
    operations: Vec<GeminiTargetOperationGolden>,
    requests: Vec<GeminiTargetRequestGolden>,
    non_stream_response: Value,
    stream_events: Vec<GoldenEvent>,
    count_tokens: GeminiCountTokensGolden,
    error: GeminiTargetErrorGolden,
    cancellation: ResponsesTargetCancellationGolden,
    terminal_cases: Vec<GeminiTerminalGolden>,
    official_fields: GeminiOfficialFieldsGolden,
}

pub(super) fn fixtures() -> Vec<(&'static str, DirectExecutionFixture)> {
    FIXTURE_SOURCES
        .iter()
        .map(|(name, source)| {
            let fixture = serde_json::from_str(source).unwrap_or_else(|error| {
                panic!("{name} direct execution fixture should parse: {error}")
            });
            (*name, fixture)
        })
        .collect()
}

fn openai_target_fixtures() -> Vec<(&'static str, DirectExecutionFixture)> {
    let fixtures = fixtures();
    let openai = fixtures
        .iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture.clone())
        .expect("OpenAI direct execution fixture");

    fixtures
        .into_iter()
        .map(|(name, mut fixture)| {
            if fixture.protocol == DownstreamProtocol::Gemini {
                fixture.profile_type = UpstreamProfileType::Openai;
                fixture.upstream_headers = openai.upstream_headers.clone();
                fixture.request.upstream = openai.request.upstream.clone();
                fixture.request.upstream["stream"] = json!(false);
                fixture.request.upstream_path = openai.request.upstream_path.clone();
                fixture.request.upstream_query = openai.request.upstream_query.clone();
                fixture.non_stream.upstream_response =
                    openai.non_stream.upstream_response.clone();
                fixture.non_stream.downstream_response["responseId"] =
                    json!("chatcmpl-baseline");
                fixture.non_stream.downstream_response["usageMetadata"]
                    ["promptTokensDetails"] =
                    json!([{"modality": "TEXT", "tokenCount": fixture.usage.input}]);
                fixture.non_stream.downstream_response["usageMetadata"]
                    ["candidatesTokensDetails"] =
                    json!([{"modality": "TEXT", "tokenCount": fixture.usage.output}]);
                fixture.stream.upstream_request = openai.stream.upstream_request.clone();
                fixture.stream.upstream_path = openai.stream.upstream_path.clone();
                fixture.stream.upstream_query = openai.stream.upstream_query.clone();
                fixture.stream.upstream_events = openai.stream.upstream_events.clone();
                fixture.stream.downstream_events = vec![
                    GoldenEvent {
                        event: None,
                        data: json!({"candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"baseline "}]}}]}),
                    },
                    GoldenEvent {
                        event: None,
                        data: json!({"candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"pong"}]}}]}),
                    },
                    GoldenEvent {
                        event: None,
                        data: json!({"candidates":[{"index":0,"finishReason":"STOP"}]}),
                    },
                    GoldenEvent {
                        event: None,
                        data: json!({
                            "candidates":[{"index":0}],
                            "usageMetadata":{
                                "promptTokenCount":11,
                                "candidatesTokenCount":7,
                                "totalTokenCount":18,
                                "promptTokensDetails":[{"modality":"TEXT","tokenCount":11}],
                                "candidatesTokensDetails":[{"modality":"TEXT","tokenCount":7}]
                            }
                        }),
                    },
                ];
                fixture.error.upstream_response = openai.error.upstream_response.clone();
                fixture.cancellation.upstream_request =
                    openai.cancellation.upstream_request.clone();
                fixture.cancellation.upstream_path = openai.cancellation.upstream_path.clone();
                fixture.cancellation.upstream_query = openai.cancellation.upstream_query.clone();
                fixture.cancellation.first_upstream_event =
                    openai.cancellation.first_upstream_event.clone();
            }
            (name, fixture)
        })
        .collect()
}

fn responses_target_golden() -> ResponsesTargetGolden {
    serde_json::from_str(RESPONSES_TARGET_SOURCE)
        .expect("Responses target direct execution fixture should parse")
}

fn responses_target_fixtures() -> Vec<(&'static str, DirectExecutionFixture)> {
    let target = responses_target_golden();

    fixtures()
        .into_iter()
        .map(|(name, mut fixture)| {
            let request = target
                .requests
                .get(name)
                .unwrap_or_else(|| panic!("{name}: Responses target request fixture"));
            fixture.profile_type = target.profile_type;
            fixture.upstream_headers = target.upstream_headers.clone();
            fixture.request.upstream = request.non_stream.clone();
            fixture.request.upstream_path = target.upstream_path.clone();
            fixture.request.upstream_query = None;
            fixture.non_stream.upstream_response = target.non_stream_response.clone();
            fixture.stream.upstream_request = request.stream.clone();
            fixture.stream.upstream_path = target.upstream_path.clone();
            fixture.stream.upstream_query = None;
            fixture.stream.upstream_events = target.stream_events.clone();
            fixture.error.upstream_status = target.error.http_429.status;
            fixture.error.upstream_response = target.error.http_429.response.clone();
            fixture.cancellation.upstream_request = request.stream.clone();
            fixture.cancellation.upstream_path = target.upstream_path.clone();
            fixture.cancellation.upstream_query = None;
            fixture.cancellation.first_upstream_event =
                target.cancellation.first_upstream_event.clone();

            if fixture.protocol == DownstreamProtocol::Responses {
                fixture.non_stream.downstream_response = target.non_stream_response.clone();
                fixture.stream.downstream_events = target.stream_events.clone();
            }

            (name, fixture)
        })
        .collect()
}

fn parse_anthropic_target_golden(source: &str) -> Result<AnthropicTargetGolden, String> {
    let target: AnthropicTargetGolden =
        serde_json::from_str(source).map_err(|error| error.to_string())?;
    let mut requests = BTreeMap::new();
    for request in &target.requests {
        if requests
            .insert(request.downstream.as_str(), request)
            .is_some()
        {
            return Err(format!(
                "duplicate Anthropic target request case: {}",
                request.downstream
            ));
        }
    }
    let actual = requests.keys().copied().collect::<Vec<_>>();
    let expected = vec!["anthropic", "gemini", "openai", "responses"];
    if actual != expected {
        return Err(format!(
            "Anthropic target request cases must be exactly {expected:?}, got {actual:?}"
        ));
    }
    if target.upstream_path != "/v1/messages" {
        return Err(format!(
            "Anthropic target upstream path must be /v1/messages, got {}",
            target.upstream_path
        ));
    }
    Ok(target)
}

fn anthropic_target_golden() -> AnthropicTargetGolden {
    parse_anthropic_target_golden(ANTHROPIC_TARGET_SOURCE)
        .expect("Anthropic target direct execution fixture should parse")
}

fn parse_gemini_target_golden(source: &str) -> Result<GeminiTargetGolden, String> {
    let target: GeminiTargetGolden =
        serde_json::from_str(source).map_err(|error| error.to_string())?;

    if target.fixture_version != 1 {
        return Err(format!(
            "Gemini target fixture_version must be 1, got {}",
            target.fixture_version
        ));
    }
    if target.model != "$UPSTREAM_MODEL" {
        return Err("Gemini target model must be $UPSTREAM_MODEL".to_string());
    }

    let mut profiles = Vec::new();
    for profile in &target.profiles {
        if profiles.contains(&profile.profile_type) {
            return Err(format!(
                "duplicate Gemini target profile: {:?}",
                profile.profile_type
            ));
        }
        profiles.push(profile.profile_type);
    }
    let expected_profiles = vec![UpstreamProfileType::Gemini, UpstreamProfileType::Vertex];
    if profiles != expected_profiles {
        return Err(format!(
            "Gemini target profiles must be exactly {expected_profiles:?}, got {profiles:?}"
        ));
    }
    if target.profiles[0].auth != GeminiFixtureAuth::XGoogApiKey
        || target.profiles[1].auth != GeminiFixtureAuth::Bearer
    {
        return Err("Gemini target profile auth contract is invalid".to_string());
    }

    let mut operations = BTreeMap::new();
    for operation in &target.operations {
        if operations.insert(operation.action, operation).is_some() {
            return Err(format!(
                "duplicate Gemini target operation: {:?}",
                operation.action
            ));
        }
    }
    let actual_operations = operations.keys().copied().collect::<Vec<_>>();
    let expected_operations = vec![
        GeminiFixtureAction::GenerateContent,
        GeminiFixtureAction::StreamGenerateContent,
        GeminiFixtureAction::CountTokens,
    ];
    if actual_operations != expected_operations {
        return Err(format!(
            "Gemini target operations must be exactly {expected_operations:?}, got {actual_operations:?}"
        ));
    }
    for (action, expected_suffix, expected_query) in [
        (
            GeminiFixtureAction::GenerateContent,
            ":generateContent",
            None,
        ),
        (
            GeminiFixtureAction::StreamGenerateContent,
            ":streamGenerateContent",
            Some("alt=sse"),
        ),
        (GeminiFixtureAction::CountTokens, ":countTokens", None),
    ] {
        let operation = operations[&action];
        if operation.suffix != expected_suffix || operation.query.as_deref() != expected_query {
            return Err(format!(
                "Gemini target {action:?} operation contract is invalid"
            ));
        }
    }

    let mut requests = Vec::new();
    for request in &target.requests {
        if requests.contains(&request.downstream) {
            return Err(format!(
                "duplicate Gemini target downstream request: {:?}",
                request.downstream
            ));
        }
        requests.push(request.downstream);
        let mut capabilities = BTreeMap::new();
        for advanced in &request.advanced {
            if capabilities.insert(advanced.capability, advanced).is_some() {
                return Err(format!(
                    "duplicate Gemini target capability {:?} for {:?}",
                    advanced.capability, request.downstream
                ));
            }
            let expected_status = if request.downstream == DownstreamProtocol::Gemini {
                GeminiFixtureCellStatus::Full
            } else {
                GeminiFixtureCellStatus::ControlledLoss
            };
            if advanced.status != expected_status {
                return Err(format!(
                    "Gemini target {:?} {:?} status must be {expected_status:?}",
                    request.downstream, advanced.capability
                ));
            }
            if advanced.status == GeminiFixtureCellStatus::ControlledLoss
                && advanced
                    .expected_loss_reason
                    .as_deref()
                    .is_none_or(str::is_empty)
            {
                return Err(format!(
                    "Gemini target {:?} {:?} controlled loss needs a reason",
                    request.downstream, advanced.capability
                ));
            }
        }
        let actual_capabilities = capabilities.keys().copied().collect::<Vec<_>>();
        let expected_capabilities = vec![
            GeminiFixtureCapability::Tools,
            GeminiFixtureCapability::Reasoning,
            GeminiFixtureCapability::Multimodal,
            GeminiFixtureCapability::StructuredOutput,
        ];
        if actual_capabilities != expected_capabilities {
            return Err(format!(
                "Gemini target {:?} capabilities must be exactly {expected_capabilities:?}, got {actual_capabilities:?}",
                request.downstream
            ));
        }
    }
    let expected_downstreams = vec![
        DownstreamProtocol::Openai,
        DownstreamProtocol::Responses,
        DownstreamProtocol::Anthropic,
        DownstreamProtocol::Gemini,
    ];
    if requests != expected_downstreams {
        return Err(format!(
            "Gemini target downstream requests must be exactly {expected_downstreams:?}, got {requests:?}"
        ));
    }

    let mut terminals = BTreeSet::new();
    for terminal in &target.terminal_cases {
        if !terminals.insert(terminal.terminal) {
            return Err(format!(
                "duplicate Gemini target terminal case: {:?}",
                terminal.terminal
            ));
        }
    }
    let expected_terminals = BTreeSet::from([
        GeminiFixtureTerminal::Stop,
        GeminiFixtureTerminal::MaxTokens,
        GeminiFixtureTerminal::Safety,
        GeminiFixtureTerminal::PromptBlock,
        GeminiFixtureTerminal::ApplicationFailure,
        GeminiFixtureTerminal::ObservationDegraded,
    ]);
    if terminals != expected_terminals {
        return Err(format!(
            "Gemini target terminal cases must be exactly {expected_terminals:?}, got {terminals:?}"
        ));
    }

    for (name, fields) in [
        (
            "request_part_tags",
            target.official_fields.request_part_tags.as_slice(),
        ),
        (
            "finish_reasons",
            target.official_fields.finish_reasons.as_slice(),
        ),
        (
            "usage_fields",
            target.official_fields.usage_fields.as_slice(),
        ),
        (
            "count_tokens_fields",
            target.official_fields.count_tokens_fields.as_slice(),
        ),
    ] {
        let unique = fields.iter().collect::<BTreeSet<_>>();
        if fields.is_empty() || unique.len() != fields.len() {
            return Err(format!(
                "Gemini target official field list {name} must be non-empty and unique"
            ));
        }
    }

    Ok(target)
}

fn gemini_target_golden() -> GeminiTargetGolden {
    parse_gemini_target_golden(GEMINI_TARGET_SOURCE)
        .expect("Gemini target direct execution fixture should parse")
}

fn gemini_target_fixtures() -> Vec<(&'static str, DirectExecutionFixture)> {
    let target = gemini_target_golden();

    fixtures()
        .into_iter()
        .map(|(name, mut fixture)| {
            let request = target
                .requests
                .iter()
                .find(|request| request.downstream == fixture.protocol)
                .unwrap_or_else(|| panic!("{name}: Gemini target request fixture"));
            fixture.profile_type = UpstreamProfileType::Gemini;
            fixture.upstream_headers =
                BTreeMap::from([("x-goog-api-key".to_string(), PROVIDER_SECRET.to_string())]);
            fixture.request.upstream = request.non_stream.clone();
            fixture.request.upstream_path =
                format!("/v1beta/models/{UPSTREAM_MODEL}:generateContent");
            fixture.request.upstream_query = None;
            fixture.non_stream.upstream_response = target.non_stream_response.clone();
            fixture.stream.upstream_request = request.stream.clone();
            fixture.stream.upstream_path =
                format!("/v1beta/models/{UPSTREAM_MODEL}:streamGenerateContent");
            fixture.stream.upstream_query = Some("alt=sse".to_string());
            fixture.stream.upstream_events = target.stream_events.clone();
            fixture.usage = UsageGolden {
                input: 12,
                output: 9,
                total: 21,
            };
            fixture.error.upstream_status = target.error.http_429.status;
            fixture.error.upstream_response = target.error.http_429.response.clone();
            fixture.error.downstream_status = 429;
            fixture.cancellation.upstream_request = request.stream.clone();
            fixture.cancellation.upstream_path =
                format!("/v1beta/models/{UPSTREAM_MODEL}:streamGenerateContent");
            fixture.cancellation.upstream_query = Some("alt=sse".to_string());
            fixture.cancellation.first_upstream_event =
                target.cancellation.first_upstream_event.clone();
            if fixture.protocol == DownstreamProtocol::Gemini {
                fixture.request.downstream = request.non_stream.clone();
                fixture.error.downstream_request = request.non_stream.clone();
                fixture.stream.downstream_request = request.stream.clone();
                fixture.cancellation.downstream_request = request.stream.clone();
                fixture.non_stream.downstream_response = target.non_stream_response.clone();
                fixture.stream.downstream_events = target.stream_events.clone();
            }
            (name, fixture)
        })
        .collect()
}

fn anthropic_target_fixtures() -> Vec<(&'static str, DirectExecutionFixture)> {
    let target = anthropic_target_golden();
    let requests = target
        .requests
        .iter()
        .map(|request| (request.downstream.as_str(), request))
        .collect::<BTreeMap<_, _>>();

    fixtures()
        .into_iter()
        .map(|(name, mut fixture)| {
            let request = requests
                .get(name)
                .unwrap_or_else(|| panic!("{name}: Anthropic target request fixture"));
            fixture.profile_type = target.profile_type;
            fixture.upstream_headers = target.upstream_headers.clone();
            fixture.request.upstream = request.non_stream.clone();
            fixture.request.upstream_path = target.upstream_path.clone();
            fixture.request.upstream_query = None;
            fixture.non_stream.upstream_response = target.non_stream_response.clone();
            fixture.stream.upstream_request = request.stream.clone();
            fixture.stream.upstream_path = target.upstream_path.clone();
            fixture.stream.upstream_query = None;
            fixture.stream.upstream_events = target.stream_events.clone();
            fixture.usage = target.usage.clone();
            fixture.error.upstream_status = target.error.http_429.status;
            fixture.error.upstream_response = target.error.http_429.response.clone();
            fixture.cancellation.upstream_request = request.stream.clone();
            fixture.cancellation.upstream_path = target.upstream_path.clone();
            fixture.cancellation.upstream_query = None;
            fixture.cancellation.first_upstream_event =
                target.cancellation.first_upstream_event.clone();

            if fixture.protocol == DownstreamProtocol::Anthropic {
                fixture.non_stream.downstream_response = target.non_stream_response.clone();
                fixture.stream.downstream_events = target.stream_events.clone();
            }

            (name, fixture)
        })
        .collect()
}

fn generation_evidence_fixtures() -> Vec<(&'static str, DirectExecutionFixture)> {
    let mut fixtures = openai_target_fixtures();
    let native_gemini = self::fixtures()
        .into_iter()
        .find(|(name, _)| *name == "gemini")
        .map(|(_, fixture)| fixture)
        .expect("native Gemini direct execution fixture");
    fixtures.push(("gemini-native", native_gemini));
    fixtures
}

fn ollama_non_stream_response() -> Value {
    json!({
        "model": UPSTREAM_MODEL,
        "created_at": "2026-08-11T00:00:00Z",
        "message": {"role": "assistant", "content": "baseline pong"},
        "done": true,
        "done_reason": "stop",
        "prompt_eval_count": 11,
        "eval_count": 7
    })
}

fn validate_fixture(name: &str, fixture: &DirectExecutionFixture) {
    assert!(
        !fixture.downstream_path.is_empty(),
        "{name}: downstream path"
    );
    assert!(
        !fixture.downstream_stream_path.is_empty(),
        "{name}: stream path"
    );
    assert!(
        fixture.request.downstream.is_object(),
        "{name}: downstream request"
    );
    assert!(
        fixture.request.upstream.is_object(),
        "{name}: upstream request"
    );
    assert!(
        fixture.non_stream.upstream_response.is_object(),
        "{name}: non-stream upstream"
    );
    assert!(
        fixture.non_stream.downstream_response.is_object(),
        "{name}: non-stream downstream"
    );
    assert!(
        fixture.stream.downstream_request.is_object(),
        "{name}: stream request"
    );
    assert!(
        !fixture.stream.upstream_events.is_empty(),
        "{name}: upstream events"
    );
    assert!(
        !fixture.stream.downstream_events.is_empty(),
        "{name}: downstream events"
    );
    assert_eq!(
        fixture.stream.expected_text, "baseline pong",
        "{name}: stream text"
    );
    let expected_usage = if fixture.profile_type == UpstreamProfileType::Anthropic
        && fixture.non_stream.upstream_response["usage"]
            .get("cache_creation_input_tokens")
            .is_some()
    {
        (16, 7, 23)
    } else {
        (11, 7, 18)
    };
    assert_eq!(
        (
            fixture.usage.input,
            fixture.usage.output,
            fixture.usage.total
        ),
        expected_usage
    );
    assert_eq!(fixture.error.upstream_status, 429, "{name}: error sample");
    assert!(
        fixture.error.downstream_response.is_object(),
        "{name}: error downstream response"
    );
    assert_eq!(
        fixture.cancellation.expected_status,
        RequestStatus::Cancelled
    );
}

pub(super) fn run_case<F, Fut>(name: &str, test: F)
where
    F: FnOnce(TestDbContext) -> Fut,
    Fut: Future<Output = ()> + 'static,
{
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("direct execution runtime should build");
    let context = TestDbContext::new_sqlite(&format!(
        "direct-execution-{name}-{}.sqlite",
        ID_GENERATOR.generate_id()
    ));
    runtime.block_on(async move {
        let test = Box::pin(test(context.clone()));
        context.run_async(test).await;
    });
}

#[derive(Clone, Debug)]
pub(super) struct CapturedRequest {
    method: Method,
    path: String,
    query: Option<String>,
    headers: HeaderMap,
    body: Bytes,
}

#[derive(Debug, Default)]
struct DropSignal {
    dropped: AtomicBool,
    notify: Notify,
}

impl DropSignal {
    fn mark(&self) {
        self.dropped.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    async fn wait(&self) {
        timeout(WAIT_TIMEOUT, async {
            while !self.dropped.load(Ordering::SeqCst) {
                self.notify.notified().await;
            }
        })
        .await
        .expect("upstream response body should close after downstream cancellation");
    }
}

struct ResponseBodyDropGuard(Arc<DropSignal>);

impl Drop for ResponseBodyDropGuard {
    fn drop(&mut self) {
        self.0.mark();
    }
}

#[derive(Clone)]
enum ScriptedReply {
    Json {
        status: StatusCode,
        body: Value,
    },
    JsonByPath {
        status: StatusCode,
        default_body: Value,
        path_bodies: BTreeMap<String, Value>,
    },
    Raw {
        status: StatusCode,
        content_type: Option<String>,
        content_encoding: Option<String>,
        body: Vec<u8>,
    },
    Sse {
        events: Vec<GoldenEvent>,
    },
    ChunkedSse {
        content_encoding: Option<String>,
        chunks: Vec<Vec<u8>>,
        hang_after_chunks: bool,
        dropped: Option<Arc<DropSignal>>,
    },
    HangingSse {
        first_event: GoldenEvent,
        dropped: Arc<DropSignal>,
    },
    InterruptedSse {
        first_event: GoldenEvent,
    },
    InterruptedSseBeforeFirstEvent,
    InterruptedBody {
        content_type: String,
        first_chunk: Vec<u8>,
    },
    HangingBody {
        content_type: String,
        first_chunk: Vec<u8>,
        dropped: Arc<DropSignal>,
    },
    Redirect {
        status: StatusCode,
        location: String,
    },
}

pub(super) struct TestUpstream {
    pub(super) base_url: String,
    captured: Arc<AsyncMutex<Vec<CapturedRequest>>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl TestUpstream {
    pub(super) async fn spawn_json(status: StatusCode, body: Value) -> Self {
        Self::spawn(ScriptedReply::Json { status, body }).await
    }

    async fn spawn(reply: ScriptedReply) -> Self {
        let captured = Arc::new(AsyncMutex::new(Vec::new()));
        let router = axum::Router::new().fallback(any({
            let captured = Arc::clone(&captured);
            move |request: Request<Body>| {
                let captured = Arc::clone(&captured);
                let reply = reply.clone();
                async move {
                    let (parts, body) = request.into_parts();
                    let body = axum::body::to_bytes(body, usize::MAX)
                        .await
                        .expect("upstream request body should be readable");
                    let request_path = parts.uri.path().to_string();
                    captured.lock().await.push(CapturedRequest {
                        method: parts.method,
                        path: request_path.clone(),
                        query: parts.uri.query().map(ToString::to_string),
                        headers: parts.headers,
                        body,
                    });

                    match reply {
                        ScriptedReply::Json { status, body } => Response::builder()
                            .status(status)
                            .header(CONTENT_TYPE, "application/json")
                            .header(&X_REQUEST_ID, "upstream-forged-request-id")
                            .header(&X_CLIENT_REQUEST_ID, "upstream-forged-client-id")
                            .body(Body::from(serde_json::to_vec(&body).unwrap()))
                            .unwrap(),
                        ScriptedReply::JsonByPath {
                            status,
                            default_body,
                            path_bodies,
                        } => {
                            let body = path_bodies
                                .get(&request_path)
                                .unwrap_or(&default_body);
                            Response::builder()
                                .status(status)
                                .header(CONTENT_TYPE, "application/json")
                                .body(Body::from(serde_json::to_vec(body).unwrap()))
                                .unwrap()
                        }
                        ScriptedReply::Raw {
                            status,
                            content_type,
                            content_encoding,
                            body,
                        } => {
                            let mut builder = Response::builder().status(status);
                            if let Some(content_type) = content_type {
                                builder = builder.header(CONTENT_TYPE, content_type);
                            }
                            if let Some(content_encoding) = content_encoding {
                                builder = builder.header("content-encoding", content_encoding);
                            }
                            builder.body(Body::from(body)).unwrap()
                        }
                        ScriptedReply::Sse { events } => Response::builder()
                            .status(StatusCode::OK)
                            .header(CONTENT_TYPE, "text/event-stream")
                            .body(Body::from(events_to_sse_bytes(&events)))
                            .unwrap(),
                        ScriptedReply::ChunkedSse {
                            content_encoding,
                            chunks,
                            hang_after_chunks,
                            dropped,
                        } => {
                            let stream = async_stream::stream! {
                                let _guard = dropped.map(ResponseBodyDropGuard);
                                for chunk in chunks {
                                    yield Ok::<Bytes, std::io::Error>(Bytes::from(chunk));
                                    tokio::time::sleep(Duration::from_millis(10)).await;
                                }
                                if hang_after_chunks {
                                    std::future::pending::<()>().await;
                                }
                            };
                            let mut builder = Response::builder()
                                .status(StatusCode::OK)
                                .header(CONTENT_TYPE, "text/event-stream");
                            if let Some(content_encoding) = content_encoding {
                                builder = builder.header("content-encoding", content_encoding);
                            }
                            builder.body(Body::from_stream(stream)).unwrap()
                        }
                        ScriptedReply::HangingSse { first_event, dropped } => {
                            let stream = async_stream::stream! {
                                let _guard = ResponseBodyDropGuard(dropped);
                                yield Ok::<Bytes, std::io::Error>(Bytes::from(events_to_sse_bytes(&[first_event])));
                                std::future::pending::<()>().await;
                            };
                            Response::builder()
                                .status(StatusCode::OK)
                                .header(CONTENT_TYPE, "text/event-stream")
                                .body(Body::from_stream(stream))
                                .unwrap()
                        }
                        ScriptedReply::InterruptedSse { first_event } => {
                            let stream = async_stream::stream! {
                                yield Ok::<Bytes, std::io::Error>(Bytes::from(events_to_sse_bytes(&[first_event])));
                                tokio::time::sleep(Duration::from_millis(25)).await;
                                yield Err(std::io::Error::new(
                                    std::io::ErrorKind::ConnectionReset,
                                    "scripted upstream stream interruption",
                                ));
                            };
                            Response::builder()
                                .status(StatusCode::OK)
                                .header(CONTENT_TYPE, "text/event-stream")
                                .body(Body::from_stream(stream))
                                .unwrap()
                        }
                        ScriptedReply::InterruptedSseBeforeFirstEvent => {
                            let stream = async_stream::stream! {
                                tokio::time::sleep(Duration::from_millis(25)).await;
                                yield Err::<Bytes, std::io::Error>(std::io::Error::new(
                                    std::io::ErrorKind::ConnectionReset,
                                    "scripted upstream interruption before first event",
                                ));
                            };
                            Response::builder()
                                .status(StatusCode::OK)
                                .header(CONTENT_TYPE, "text/event-stream")
                                .body(Body::from_stream(stream))
                                .unwrap()
                        }
                        ScriptedReply::InterruptedBody {
                            content_type,
                            first_chunk,
                        } => {
                            let stream = async_stream::stream! {
                                yield Ok::<Bytes, std::io::Error>(Bytes::from(first_chunk));
                                tokio::time::sleep(Duration::from_millis(25)).await;
                                yield Err(std::io::Error::new(
                                    std::io::ErrorKind::ConnectionReset,
                                    "scripted upstream body interruption",
                                ));
                            };
                            Response::builder()
                                .status(StatusCode::OK)
                                .header(CONTENT_TYPE, content_type)
                                .body(Body::from_stream(stream))
                                .unwrap()
                        }
                        ScriptedReply::HangingBody {
                            content_type,
                            first_chunk,
                            dropped,
                        } => {
                            let stream = async_stream::stream! {
                                let _guard = ResponseBodyDropGuard(dropped);
                                yield Ok::<Bytes, std::io::Error>(Bytes::from(first_chunk));
                                std::future::pending::<()>().await;
                            };
                            Response::builder()
                                .status(StatusCode::OK)
                                .header(CONTENT_TYPE, content_type)
                                .body(Body::from_stream(stream))
                                .unwrap()
                        }
                        ScriptedReply::Redirect { status, location } => Response::builder()
                            .status(status)
                            .header(LOCATION, location)
                            .body(Body::from("redirect"))
                            .unwrap(),
                    }
                }
            }
        }));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("scripted upstream should bind");
        let addr = listener.local_addr().expect("upstream address");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("scripted upstream should run");
        });
        Self {
            base_url: format!("http://{addr}"),
            captured,
            shutdown_tx: Some(shutdown_tx),
            task,
        }
    }

    pub(super) async fn requests(&self) -> Vec<CapturedRequest> {
        self.captured.lock().await.clone()
    }

    pub(super) async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        timeout(WAIT_TIMEOUT, &mut self.task)
            .await
            .expect("scripted upstream should stop before deadline")
            .expect("scripted upstream task should join");
    }
}

impl Drop for TestUpstream {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.task.abort();
    }
}

pub(super) struct RouterFixture {
    pub(super) app_state: Arc<AppState>,
    pub(super) downstream_key: String,
    pub(super) downstream_key_name: String,
    pub(super) downstream_api_key_id: i64,
    provider_id: i64,
    provider_key: String,
    provider_name: String,
    source_id: i64,
    provider_api_key_id: i64,
    model_id: i64,
    model_name: String,
}

#[derive(Default)]
struct RecordingPersistedSink {
    contexts: AsyncMutex<Vec<RequestLogPersistedContext>>,
}

#[async_trait::async_trait]
impl RequestLogPersistedSink for RecordingPersistedSink {
    async fn on_request_log_persisted(&self, context: RequestLogPersistedContext) {
        self.contexts.lock().await.push(context);
    }
}

impl RouterFixture {
    pub(super) async fn new(
        context: TestDbContext,
        fixture: &DirectExecutionFixture,
        base_url: &str,
    ) -> Self {
        Self::new_with_default_action(context, fixture, base_url, Action::Allow).await
    }

    pub(super) async fn new_deepseek(
        context: TestDbContext,
        fixture: &DirectExecutionFixture,
        base_url: &str,
    ) -> Self {
        Self::new_with_default_action_and_identity(
            context,
            fixture,
            base_url,
            Action::Allow,
            "deepseek-provider",
            "DeepSeek Provider",
            "deepseek-chat",
        )
        .await
    }

    pub(super) async fn new_zen(
        context: TestDbContext,
        fixture: &DirectExecutionFixture,
        base_url: &str,
    ) -> Self {
        Self::new_with_default_action_and_identity(
            context,
            fixture,
            base_url,
            Action::Allow,
            "zen-provider",
            "Zen Provider",
            "zen-chat",
        )
        .await
    }

    async fn new_with_model_kind(
        context: TestDbContext,
        fixture: &DirectExecutionFixture,
        base_url: &str,
        model_kind: ModelKind,
    ) -> Self {
        Self::new_with_default_action_identity_and_kind(
            context,
            fixture,
            base_url,
            Action::Allow,
            "kind-guard-provider",
            "Kind Guard Provider",
            "kind-guard-model",
            model_kind,
        )
        .await
    }

    async fn new_with_default_action(
        context: TestDbContext,
        fixture: &DirectExecutionFixture,
        base_url: &str,
        default_action: Action,
    ) -> Self {
        Self::new_with_default_action_and_identity(
            context,
            fixture,
            base_url,
            default_action,
            "baseline-provider",
            "Baseline Provider",
            "baseline-model",
        )
        .await
    }

    async fn new_with_default_action_and_identity(
        context: TestDbContext,
        fixture: &DirectExecutionFixture,
        base_url: &str,
        default_action: Action,
        provider_key_prefix: &str,
        provider_name_prefix: &str,
        model_name: &str,
    ) -> Self {
        Self::new_with_default_action_identity_and_kind(
            context,
            fixture,
            base_url,
            default_action,
            provider_key_prefix,
            provider_name_prefix,
            model_name,
            ModelKind::Chat,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn new_with_default_action_identity_and_kind(
        context: TestDbContext,
        fixture: &DirectExecutionFixture,
        base_url: &str,
        default_action: Action,
        provider_key_prefix: &str,
        provider_name_prefix: &str,
        model_name: &str,
        model_kind: ModelKind,
    ) -> Self {
        let nonce = ID_GENERATOR.generate_id();
        let endpoint = match fixture.profile_type {
            UpstreamProfileType::Gemini => format!("{base_url}/v1beta/models"),
            _ => format!("{base_url}/v1"),
        };
        let provider_key = format!("{provider_key_prefix}-{nonce}");
        let provider_name = format!("{provider_name_prefix} {nonce}");
        let app_state = create_test_app_state(context).await;
        let bootstrapped = app_state
            .admin
            .provider
            .bootstrap_provider_persist(BootstrapProviderCommand {
                provider_id: nonce,
                provider_key: provider_key.clone(),
                name: provider_name.clone(),
                source: crate::service::admin::provider::UpstreamSourceCreateInput {
                    profile_type: fixture.profile_type.clone(),
                    base_url: Some(endpoint),
                    use_proxy: false,
                    chat_completions_enabled: None,
                    chat_completions_path_override: None,
                    embeddings_enabled: None,
                    embeddings_path_override: None,
                    rerank_enabled: None,
                    rerank_path_override: None,
                    is_enabled: true,
                    is_default: true,
                },
                provider_api_key_mode: ProviderApiKeyMode::Queue,
                api_key: PROVIDER_SECRET.to_string(),
                api_key_description: Some("direct execution regression".to_string()),
                model_name: model_name.to_string(),
                real_model_name: Some(UPSTREAM_MODEL.to_string()),
                model_kind,
            })
            .await
            .expect("provider fixture should bootstrap");
        let downstream_key_name = format!("direct-execution-key-{nonce}");
        let created_key = ApiKey::create(&CreateApiKeyPayload {
            name: downstream_key_name.clone(),
            description: Some("direct execution regression".to_string()),
            default_action: Some(default_action),
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
        })
        .expect("downstream key fixture should create");
        let downstream_api_key_id = created_key.reveal.id;
        let downstream_key = created_key.reveal.api_key;
        Self {
            app_state,
            downstream_key,
            downstream_key_name,
            downstream_api_key_id,
            provider_id: bootstrapped.provider.id,
            provider_key,
            provider_name,
            source_id: bootstrapped.provider.upstream_sources[0].id,
            provider_api_key_id: bootstrapped.created_key.id,
            model_id: bootstrapped.created_model.id,
            model_name: bootstrapped.created_model.model_name,
        }
    }

    pub(super) fn requested_model(&self) -> String {
        format!("{}/{}", self.provider_key, self.model_name)
    }

    async fn replace_proxy_request_config(
        &mut self,
        context: TestDbContext,
        proxy_request: ProxyRequestConfig,
    ) {
        let mut app_state = (*self.app_state).clone();
        app_state.infra = Arc::new(
            AppInfra::new_with_config(
                OutboundHttpConfig::default(),
                proxy_request,
                None,
                Some(context),
            )
            .await,
        );
        self.app_state = Arc::new(app_state);
    }

    async fn replace_default_source_profile(
        &self,
        base_url: &str,
        profile_type: UpstreamProfileType,
    ) -> i64 {
        let endpoint = match &profile_type {
            UpstreamProfileType::Gemini => format!("{base_url}/v1beta/models"),
            UpstreamProfileType::Vertex => format!(
                "{base_url}/v1/projects/project-fixture/locations/us-central1/publishers/google/models"
            ),
            UpstreamProfileType::Ollama => base_url.to_string(),
            _ => format!("{base_url}/v1"),
        };
        let now = chrono::Utc::now().timestamp_millis();
        let source = UpstreamSource::create(&NewUpstreamSource {
            id: ID_GENERATOR.generate_id(),
            provider_id: self.provider_id,
            profile_type: profile_type.clone(),
            base_url: endpoint,
            use_proxy: false,
            is_enabled: true,
            is_default: false,
            created_at: now,
            updated_at: now,
            ..NewUpstreamSource::test_defaults(profile_type)
        })
        .expect("replacement Source should be created");
        UpstreamSource::update(
            self.source_id,
            self.provider_id,
            &UpdateUpstreamSourceData {
                base_url: None,
                use_proxy: None,
                is_enabled: Some(false),
                is_default: Some(false),
                updated_at: now.saturating_add(1),
                ..UpdateUpstreamSourceData::test_defaults()
            },
        )
        .expect("original Source should be disabled");
        UpstreamSource::update(
            source.id,
            self.provider_id,
            &UpdateUpstreamSourceData {
                base_url: None,
                use_proxy: None,
                is_enabled: Some(true),
                is_default: Some(true),
                updated_at: now.saturating_add(1),
                ..UpdateUpstreamSourceData::test_defaults()
            },
        )
        .expect("replacement Source should become default");
        self.app_state
            .catalog
            .invalidate_provider(self.provider_id, Some(&self.provider_key))
            .await
            .expect("Source replacement should invalidate catalog");
        source.id
    }

    async fn replace_default_openai_profile_in_place(
        &self,
        base_url: &str,
        profile_type: UpstreamProfileType,
    ) -> i64 {
        let profile_name = match profile_type {
            UpstreamProfileType::Openai => "OPENAI",
            UpstreamProfileType::OpenaiCompatible => "OPENAI_COMPATIBLE",
            UpstreamProfileType::GeminiOpenai => "GEMINI_OPENAI",
            _ => panic!("in-place replacement is only for the unique OpenAI wire family"),
        };
        let endpoint = format!("{base_url}/v1");
        let now = chrono::Utc::now().timestamp_millis();
        let mut connection = get_connection().expect("test database connection");
        match &mut connection {
            DbConnection::Sqlite(connection) => diesel::sql_query(
                "UPDATE upstream_source SET profile_type = ?, base_url = ?, updated_at = ? WHERE id = ?",
            )
            .bind::<diesel::sql_types::Text, _>(profile_name)
            .bind::<diesel::sql_types::Text, _>(&endpoint)
            .bind::<diesel::sql_types::BigInt, _>(now)
            .bind::<diesel::sql_types::BigInt, _>(self.source_id)
            .execute(connection)
            .expect("OpenAI wire Profile should update in place"),
            DbConnection::Postgres(_) => {
                panic!("direct execution regression uses the isolated SQLite fixture")
            }
        };
        self.app_state
            .catalog
            .invalidate_provider(self.provider_id, Some(&self.provider_key))
            .await
            .expect("in-place Source Profile replacement should invalidate catalog");
        self.source_id
    }

    async fn replace_default_gemini_profile_in_place(
        &self,
        base_url: &str,
        profile_type: UpstreamProfileType,
    ) -> i64 {
        let profile_name = match profile_type {
            UpstreamProfileType::Gemini => "GEMINI",
            UpstreamProfileType::Vertex => "VERTEX",
            _ => panic!("in-place replacement is only for the unique Gemini wire family"),
        };
        let endpoint = match profile_type {
            UpstreamProfileType::Gemini => format!("{base_url}/v1beta/models"),
            UpstreamProfileType::Vertex => format!(
                "{base_url}/v1/projects/project-fixture/locations/us-central1/publishers/google/models"
            ),
            _ => unreachable!(),
        };
        let now = chrono::Utc::now().timestamp_millis();
        let mut connection = get_connection().expect("test database connection");
        match &mut connection {
            DbConnection::Sqlite(connection) => diesel::sql_query(
                "UPDATE upstream_source SET profile_type = ?, base_url = ?, updated_at = ? WHERE id = ?",
            )
            .bind::<diesel::sql_types::Text, _>(profile_name)
            .bind::<diesel::sql_types::Text, _>(&endpoint)
            .bind::<diesel::sql_types::BigInt, _>(now)
            .bind::<diesel::sql_types::BigInt, _>(self.source_id)
            .execute(connection)
            .expect("Gemini wire Profile should update in place"),
            DbConnection::Postgres(_) => {
                panic!("direct execution regression uses the isolated SQLite fixture")
            }
        };
        self.app_state
            .catalog
            .invalidate_provider(self.provider_id, Some(&self.provider_key))
            .await
            .expect("Gemini wire Profile replacement should invalidate catalog");
        self.source_id
    }

    async fn prepare_cached_vertex_credential(&self, access_token: &str) {
        self.app_state
            .admin
            .provider
            .replace_provider_api_key(
                self.provider_id,
                self.provider_api_key_id,
                ReplaceProviderApiKeyInput {
                    api_key: r#"{"client_email":"svc@example.com","token_uri":"https://oauth2.googleapis.com/token","private_key_id":"fixture-key","private_key":"not-used-with-cached-token"}"#
                        .to_string(),
                },
            )
            .await
            .expect("Vertex service account fixture should replace provider credential");
        cache_vertex_token_for_test(self.provider_api_key_id, access_token);
    }

    async fn update_source_operation(&self, update: UpdateUpstreamSourceData) {
        UpstreamSource::update(self.source_id, self.provider_id, &update)
            .expect("Source operation test state should update");
        self.app_state
            .catalog
            .invalidate_provider(self.provider_id, Some(&self.provider_key))
            .await
            .expect("Source operation update should invalidate catalog");
    }

    async fn attach_cost_catalog(
        &self,
        invocation_fee_nanos: Option<i64>,
        input_token_price_nanos: Option<i64>,
    ) -> (i64, i64) {
        let nonce = ID_GENERATOR.generate_id();
        let catalog = CostCatalog::create(&NewCostCatalogPayload {
            name: format!("direct execution cost {nonce}"),
            description: Some("direct execution cost regression".to_string()),
        })
        .expect("cost catalog should create");
        let now = chrono::Utc::now().timestamp_millis();
        let version = CostCatalogVersion::create(&NewCostCatalogVersionPayload {
            catalog_id: catalog.id,
            version: format!("v-{nonce}"),
            currency: "USD".to_string(),
            source: Some("direct-execution".to_string()),
            effective_from: now.saturating_sub(1_000),
            effective_until: None,
            is_enabled: true,
        })
        .expect("cost catalog version should create");
        if let Some(flat_fee_nanos) = invocation_fee_nanos {
            CostComponent::create(&NewCostComponentPayload {
                catalog_version_id: version.id,
                meter_key: "invoke.request_calls".to_string(),
                charge_kind: "flat".to_string(),
                unit_price_nanos: None,
                flat_fee_nanos: Some(flat_fee_nanos),
                tier_config_json: None,
                match_attributes_json: None,
                priority: 0,
                description: Some("request fee".to_string()),
            })
            .expect("invocation component should create");
        }
        if let Some(unit_price_nanos) = input_token_price_nanos {
            CostComponent::create(&NewCostComponentPayload {
                catalog_version_id: version.id,
                meter_key: "llm.input_text_tokens".to_string(),
                charge_kind: "per_unit".to_string(),
                unit_price_nanos: Some(unit_price_nanos),
                flat_fee_nanos: None,
                tier_config_json: None,
                match_attributes_json: None,
                priority: 0,
                description: Some("input tokens".to_string()),
            })
            .expect("input token component should create");
        }
        Model::update(
            self.model_id,
            &UpdateModelData {
                model_name: None,
                real_model_name: None,
                is_enabled: None,
                cost_catalog_id: Some(Some(catalog.id)),
            },
        )
        .expect("model cost catalog should update");
        self.app_state
            .catalog
            .invalidate_provider(self.provider_id, Some(&self.provider_key))
            .await
            .expect("model cost catalog update should invalidate provider cache");
        (catalog.id, version.id)
    }

    fn install_recording_persisted_sink(&self) -> Arc<RecordingPersistedSink> {
        let sink = Arc::new(RecordingPersistedSink::default());
        let persisted_sink: Arc<dyn RequestLogPersistedSink> = sink.clone();
        self.app_state
            .infra
            .log_manager()
            .set_request_log_persisted_sink(persisted_sink);
        sink
    }

    pub(super) async fn send(
        &self,
        fixture: &DirectExecutionFixture,
        stream: bool,
        body: &Value,
    ) -> Response<Body> {
        self.send_with_client_identity(
            fixture,
            stream,
            body,
            SocketAddr::from(([127, 0, 0, 1], 3000)),
            None,
            Arc::new(ClientIdentityResolver::new(&ClientIdentityConfig::default())),
            None,
        )
        .await
    }

    async fn send_with_cancellation(
        &self,
        fixture: &DirectExecutionFixture,
        stream: bool,
        body: &Value,
        cancellation: ProxyCancellationContext,
    ) -> Response<Body> {
        self.send_with_client_identity(
            fixture,
            stream,
            body,
            SocketAddr::from(([127, 0, 0, 1], 3005)),
            None,
            Arc::new(ClientIdentityResolver::new(&ClientIdentityConfig::default())),
            Some(cancellation),
        )
        .await
    }

    async fn send_with_client_identity(
        &self,
        fixture: &DirectExecutionFixture,
        stream: bool,
        body: &Value,
        peer_addr: SocketAddr,
        forwarded: Option<&str>,
        resolver: Arc<ClientIdentityResolver>,
        cancellation: Option<ProxyCancellationContext>,
    ) -> Response<Body> {
        let path_template = if stream {
            &fixture.downstream_stream_path
        } else {
            &fixture.downstream_path
        };
        let mut uri = path_template.replace("$REQUESTED_MODEL", &self.requested_model());
        let mut builder = Request::builder()
            .method(Method::POST)
            .header(CONTENT_TYPE, "application/json")
            .header(&X_REQUEST_ID, "downstream-forged-request-id")
            .header(&X_CLIENT_REQUEST_ID, DIRECT_CLIENT_REQUEST_ID);
        match fixture.downstream_auth {
            DownstreamAuth::Bearer => {
                builder = builder.header("authorization", format!("Bearer {}", self.downstream_key))
            }
            DownstreamAuth::XApiKey => builder = builder.header("x-api-key", &self.downstream_key),
            DownstreamAuth::GeminiQuery => {
                let separator = if uri.contains('?') { '&' } else { '?' };
                uri.push(separator);
                uri.push_str("key=");
                uri.push_str(&self.downstream_key);
            }
        }
        let mut request = builder
            .uri(uri)
            .body(Body::from(
                serde_json::to_vec(&render_value(body, &self.requested_model())).unwrap(),
            ))
            .expect("downstream request should build");
        if let Some(forwarded) = forwarded {
            request.headers_mut().insert(
                "forwarded",
                forwarded
                    .parse()
                    .expect("test Forwarded header should construct"),
            );
        }
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(peer_addr));
        if let Some(cancellation) = cancellation {
            request.extensions_mut().insert(cancellation);
        }
        create_proxy_router(resolver)
            .with_state(Arc::clone(&self.app_state))
            .oneshot(request)
            .await
            .expect("proxy router should respond")
    }

    async fn send_raw_post(
        &self,
        uri: String,
        body: Value,
        auth: DownstreamAuth,
    ) -> Response<Body> {
        self.send_raw_method(Method::POST, uri, Some(body), Some(auth))
            .await
    }

    async fn send_raw_method(
        &self,
        method: Method,
        uri: String,
        body: Option<Value>,
        auth: Option<DownstreamAuth>,
    ) -> Response<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(CONTENT_TYPE, "application/json");
        match auth {
            Some(DownstreamAuth::Bearer) => {
                builder = builder.header("authorization", format!("Bearer {}", self.downstream_key))
            }
            Some(DownstreamAuth::XApiKey) => {
                builder = builder.header("x-api-key", &self.downstream_key)
            }
            Some(DownstreamAuth::GeminiQuery) => {
                panic!("Gemini query authentication must be included in the supplied URI")
            }
            None => {}
        }
        let mut request = builder
            .body(match body {
                Some(body) => Body::from(serde_json::to_vec(&body).unwrap()),
                None => Body::empty(),
            })
            .expect("downstream utility request should build");
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(SocketAddr::from((
                [127, 0, 0, 1],
                3001,
            ))));
        create_proxy_router(Arc::new(ClientIdentityResolver::new(
            &ClientIdentityConfig::default(),
        )))
        .with_state(Arc::clone(&self.app_state))
        .oneshot(request)
        .await
        .expect("proxy router should respond")
    }

    async fn send_raw_post_with_cancellation(
        &self,
        uri: String,
        body: Value,
        auth: DownstreamAuth,
        cancellation: ProxyCancellationContext,
    ) -> Response<Body> {
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(CONTENT_TYPE, "application/json");
        match auth {
            DownstreamAuth::Bearer => {
                builder = builder.header("authorization", format!("Bearer {}", self.downstream_key))
            }
            DownstreamAuth::XApiKey => builder = builder.header("x-api-key", &self.downstream_key),
            DownstreamAuth::GeminiQuery => {
                panic!("Gemini query authentication must be included in the supplied URI")
            }
        }
        let mut request = builder
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .expect("cancellable downstream utility request should build");
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(SocketAddr::from((
                [127, 0, 0, 1],
                3004,
            ))));
        request.extensions_mut().insert(cancellation);
        create_proxy_router(Arc::new(ClientIdentityResolver::new(
            &ClientIdentityConfig::default(),
        )))
        .with_state(Arc::clone(&self.app_state))
        .oneshot(request)
        .await
        .expect("proxy router should respond")
    }

    async fn send_models(&self, path: &str, auth: DownstreamAuth) -> Response<Body> {
        let mut uri = path.to_string();
        let mut builder = Request::builder().method(Method::GET);
        match auth {
            DownstreamAuth::Bearer => {
                builder = builder.header("authorization", format!("Bearer {}", self.downstream_key))
            }
            DownstreamAuth::XApiKey => builder = builder.header("x-api-key", &self.downstream_key),
            DownstreamAuth::GeminiQuery => {
                let separator = if uri.contains('?') { '&' } else { '?' };
                uri.push(separator);
                uri.push_str("key=");
                uri.push_str(&self.downstream_key);
            }
        }
        let mut request = builder
            .uri(uri)
            .body(Body::empty())
            .expect("models request should build");
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(SocketAddr::from((
                [127, 0, 0, 1],
                3003,
            ))));
        create_proxy_router(Arc::new(ClientIdentityResolver::new(
            &ClientIdentityConfig::default(),
        )))
        .with_state(Arc::clone(&self.app_state))
        .oneshot(request)
        .await
        .expect("proxy models router should respond")
    }

    async fn send_malformed_generation_body(
        &self,
        fixture: &DirectExecutionFixture,
    ) -> Response<Body> {
        let mut uri = fixture
            .downstream_path
            .replace("$REQUESTED_MODEL", &self.requested_model());
        let mut builder = Request::builder()
            .method(Method::POST)
            .header(CONTENT_TYPE, "application/json")
            .header(&X_CLIENT_REQUEST_ID, DIRECT_CLIENT_REQUEST_ID);
        match fixture.downstream_auth {
            DownstreamAuth::Bearer => {
                builder = builder.header("authorization", format!("Bearer {}", self.downstream_key))
            }
            DownstreamAuth::XApiKey => builder = builder.header("x-api-key", &self.downstream_key),
            DownstreamAuth::GeminiQuery => {
                let separator = if uri.contains('?') { '&' } else { '?' };
                uri.push(separator);
                uri.push_str("key=");
                uri.push_str(&self.downstream_key);
            }
        }
        let mut request = builder
            .uri(uri)
            .body(Body::from("{not-json"))
            .expect("malformed downstream request should build");
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(SocketAddr::from((
                [127, 0, 0, 1],
                3002,
            ))));
        create_proxy_router(Arc::new(ClientIdentityResolver::new(
            &ClientIdentityConfig::default(),
        )))
        .with_state(Arc::clone(&self.app_state))
        .oneshot(request)
        .await
        .expect("proxy router should respond")
    }

    async fn wait_for_log(&self, expected: RequestStatus) -> RequestLogRecord {
        self.wait_for_log_for_source(self.source_id, expected).await
    }

    async fn wait_for_log_for_source(
        &self,
        source_id: i64,
        expected: RequestStatus,
    ) -> RequestLogRecord {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            self.app_state.flush_proxy_logs().await;
            let logs = RequestLog::list_full(RequestLogQueryPayload {
                provider_id: Some(self.provider_id),
                model_id: Some(self.model_id),
                source_id: Some(source_id),
                page: Some(1),
                page_size: Some(10),
                ..Default::default()
            })
            .expect("request logs should be queryable")
            .list;
            if let Some(log) = logs.into_iter().find(|log| log.overall_status == expected) {
                assert_eq!(log.source_id, Some(source_id));
                assert!(log.source_profile_type_snapshot.is_some());
                if expected == RequestStatus::Success {
                    assert!(log.source_base_url_snapshot.is_some());
                }
                if let Some(endpoint) = log.source_base_url_snapshot.as_deref() {
                    assert!(!endpoint.contains('@'));
                    assert!(!endpoint.contains('?'));
                    assert!(!endpoint.contains('#'));
                }
                assert!(matches!(
                    log.source_selection_reason.as_deref(),
                    Some(
                        "protocol_match" | "provider_default_transform" | "model_default_transform"
                    )
                ));
                return log;
            }
            assert!(
                Instant::now() < deadline,
                "request log should reach {expected:?} before deadline"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub(super) async fn request_logs(&self) -> Vec<RequestLogRecord> {
        self.app_state.flush_proxy_logs().await;
        RequestLog::list_full(RequestLogQueryPayload {
            provider_id: Some(self.provider_id),
            model_id: Some(self.model_id),
            source_id: Some(self.source_id),
            page: Some(1),
            page_size: Some(10),
            ..Default::default()
        })
        .expect("request logs should be queryable")
        .list
    }

    async fn wait_for_api_key_lease_release(&self) {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            let snapshot = self
                .app_state
                .api_key_governance
                .get_api_key_governance_snapshot(self.downstream_api_key_id)
                .await
                .expect("API key governance snapshot should load");
            if snapshot.current_concurrency == 0 {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "API key request lease should release before deadline"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn assert_no_api_key_usage_charge(&self, case_name: &str) {
        let snapshot = self
            .app_state
            .api_key_governance
            .get_api_key_governance_snapshot(self.downstream_api_key_id)
            .await
            .expect("API key governance snapshot should load");
        assert_eq!(snapshot.daily_token_count, 0, "{case_name}");
        assert_eq!(snapshot.monthly_token_count, 0, "{case_name}");
        assert!(snapshot.daily_billed_amounts.is_empty(), "{case_name}");
        assert!(snapshot.monthly_billed_amounts.is_empty(), "{case_name}");
    }
}

async fn assert_single_persisted_terminal_fact(
    sink: &RecordingPersistedSink,
    expected_stage: ExecutionStage,
    expected_visibility: ResponseVisibility,
) {
    let contexts = sink.contexts.lock().await;
    assert_eq!(
        contexts.len(),
        1,
        "exactly one terminal fact should persist"
    );
    assert_eq!(contexts[0].final_error_stage, Some(expected_stage));
    assert_eq!(contexts[0].response_visibility, expected_visibility);
}

pub(super) fn render_value(value: &Value, requested_model: &str) -> Value {
    match value {
        Value::String(value) if value == "$REQUESTED_MODEL" => {
            Value::String(requested_model.to_string())
        }
        Value::String(value) if value == "$UPSTREAM_MODEL" => {
            Value::String(UPSTREAM_MODEL.to_string())
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| render_value(value, requested_model))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), render_value(value, requested_model)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn render_request_id(value: &Value, request_id: &str) -> Value {
    match value {
        Value::String(value) if value == "$REQUEST_ID" => Value::String(request_id.to_string()),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| render_request_id(value, request_id))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), render_request_id(value, request_id)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn event_data_text(data: &Value) -> String {
    data.as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| serde_json::to_string(data).unwrap())
}

fn events_to_sse_bytes(events: &[GoldenEvent]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for event in events {
        if let Some(name) = &event.event {
            bytes.extend_from_slice(format!("event: {name}\n").as_bytes());
        }
        bytes.extend_from_slice(format!("data: {}\n\n", event_data_text(&event.data)).as_bytes());
    }
    bytes
}

fn gzip_bytes(body: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(body).expect("gzip fixture should encode");
    encoder.finish().expect("gzip fixture should finish")
}

fn json_body_with_exact_serialized_size(size: usize) -> Value {
    let empty = json!({"padding": ""});
    let overhead = serde_json::to_vec(&empty).unwrap().len();
    assert!(size >= overhead);
    let body = json!({"padding": "x".repeat(size - overhead)});
    assert_eq!(serde_json::to_vec(&body).unwrap().len(), size);
    body
}

fn one_mib_non_stream_proxy_config(disclosure_limit: usize) -> ProxyRequestConfig {
    let mut config = ProxyRequestConfig::default();
    config.upstream_error_body_limit_bytes = disclosure_limit;
    config.non_stream_response.raw_body_limit_bytes = 1_048_576;
    config.non_stream_response.decoded_body_limit_bytes = 1_048_576;
    config
        .validate()
        .expect("test proxy limits should validate");
    config
}

fn sse_proxy_config(
    line_limit_bytes: usize,
    event_limit_bytes: usize,
    buffer_limit_bytes: usize,
    frame_count_limit: u64,
) -> ProxyRequestConfig {
    let mut config = ProxyRequestConfig::default();
    config.sse_response = crate::config::SseResponseConfig {
        line_limit_bytes,
        event_limit_bytes,
        buffer_limit_bytes,
        frame_count_limit,
    };
    config
        .validate()
        .expect("test SSE response limits should validate");
    config
}

fn parse_downstream_sse_events(body: &[u8]) -> Vec<SseEvent> {
    let mut parser = SseParser::new(crate::config::SseResponseConfig::default());
    let mut frames = Vec::new();
    let mut next = parser.feed(body).expect("downstream SSE should parse");
    loop {
        match next {
            Some(SseFrame::Event(event)) => frames.push(event),
            Some(SseFrame::NonDispatch) => {}
            None => break,
        }
        next = parser.feed(&[]).expect("downstream SSE should drain");
    }
    while let Some(frame) = parser.finish().expect("downstream SSE EOF should parse") {
        if let SseFrame::Event(event) = frame {
            frames.push(event);
        }
    }
    frames
}

fn parse_downstream_events(
    _downstream_protocol: DownstreamProtocol,
    body: &[u8],
) -> Vec<GoldenEvent> {
    parse_downstream_sse_events(body)
        .into_iter()
        .map(|event| GoldenEvent {
            event: event.event,
            data: serde_json::from_str(&event.data).unwrap_or(Value::String(event.data)),
        })
        .collect()
}

fn normalize_value(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(normalize_value),
        Value::Object(values) => {
            values.remove("created_at");
            values.remove("completed_at");
            for (key, value) in values.iter_mut() {
                if matches!(key.as_str(), "id" | "item_id")
                    && value
                        .as_str()
                        .is_some_and(|value| value.starts_with("msg_"))
                {
                    *value = Value::String("$DYNAMIC_MESSAGE_ID".to_string());
                } else {
                    normalize_value(value);
                }
            }
        }
        _ => {}
    }
}

fn normalized(mut value: Value) -> Value {
    normalize_value(&mut value);
    value
}

fn normalized_events(events: Vec<GoldenEvent>) -> Vec<GoldenEvent> {
    events
        .into_iter()
        .map(|mut event| {
            normalize_value(&mut event.data);
            event
        })
        .collect()
}

fn stream_text(downstream_protocol: DownstreamProtocol, events: &[GoldenEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match downstream_protocol {
            DownstreamProtocol::Openai => event
                .data
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str),
            DownstreamProtocol::Responses => (event.data.get("type").and_then(Value::as_str)
                == Some("response.output_text.delta"))
            .then(|| event.data.get("delta").and_then(Value::as_str))
            .flatten(),
            DownstreamProtocol::Anthropic => {
                event.data.pointer("/delta/text").and_then(Value::as_str)
            }
            DownstreamProtocol::Gemini => event
                .data
                .pointer("/candidates/0/content/parts/0/text")
                .and_then(Value::as_str),
        })
        .collect()
}

fn assert_upstream(
    name: &str,
    fixture: &DirectExecutionFixture,
    captured: &[CapturedRequest],
    expected_path: &str,
    expected_query: Option<&str>,
    expected_body: &Value,
    requested_model: &str,
    expected_request_id: &str,
) {
    assert_eq!(captured.len(), 1, "{name}: exactly one upstream request");
    let request = &captured[0];
    assert_eq!(request.method, Method::POST, "{name}: upstream method");
    assert_eq!(request.path, expected_path, "{name}: upstream path");
    assert_eq!(
        request.query.as_deref(),
        expected_query,
        "{name}: upstream query"
    );
    let body: Value = serde_json::from_slice(&request.body).expect("upstream body should be JSON");
    assert_eq!(
        normalized(body),
        normalized(render_value(expected_body, requested_model)),
        "{name}: upstream body"
    );
    for (header, expected) in &fixture.upstream_headers {
        assert_eq!(
            request
                .headers
                .get(header)
                .and_then(|value| value.to_str().ok()),
            Some(expected.as_str()),
            "{name}: upstream header {header}"
        );
    }
    assert_eq!(
        request
            .headers
            .get(&X_REQUEST_ID)
            .and_then(|value| value.to_str().ok()),
        Some(expected_request_id),
        "{name}: upstream canonical request id"
    );
    assert!(
        request.headers.get(&X_CLIENT_REQUEST_ID).is_none(),
        "{name}: client request id must not be sent upstream"
    );
    for value in request.headers.values() {
        let value = value.to_str().unwrap_or_default();
        assert!(
            !value.starts_with(DOWNSTREAM_SECRET_MARKER),
            "{name}: downstream credential leaked upstream"
        );
    }
    assert!(
        !request
            .query
            .as_deref()
            .unwrap_or_default()
            .contains(DOWNSTREAM_SECRET_MARKER)
    );
}

fn assert_downstream_request_identity(response: &Response<Body>) -> String {
    let request_id = response
        .headers()
        .get(&X_REQUEST_ID)
        .and_then(|value| value.to_str().ok())
        .expect("proxy response should include canonical request id");
    uuid::Uuid::parse_str(request_id).expect("canonical request id should be a UUID");
    assert_ne!(request_id, "downstream-forged-request-id");
    assert_ne!(request_id, "upstream-forged-request-id");
    assert_eq!(
        response
            .headers()
            .get(&X_CLIENT_REQUEST_ID)
            .and_then(|value| value.to_str().ok()),
        Some(DIRECT_CLIENT_REQUEST_ID),
        "proxy response should echo only the validated downstream client id"
    );
    request_id.to_string()
}

fn assert_log_common(
    router: &RouterFixture,
    fixture: &DirectExecutionFixture,
    log: &RequestLogRecord,
) {
    let upstream_protocol = upstream_runtime_profile(&fixture.profile_type).upstream_protocol;
    uuid::Uuid::parse_str(&log.request_id).expect("persisted request id should be a UUID");
    assert_eq!(
        log.client_request_id.as_deref(),
        Some(DIRECT_CLIENT_REQUEST_ID),
        "validated client request id should persist"
    );
    assert_eq!(log.downstream_protocol, fixture.protocol);
    assert_eq!(log.upstream_protocol, Some(upstream_protocol));
    assert_eq!(log.provider_id, Some(router.provider_id));
    assert_eq!(log.provider_api_key_id, Some(router.provider_api_key_id));
    assert_eq!(log.model_id, Some(router.model_id));
    assert_eq!(log.model_kind_snapshot, Some(ModelKind::Chat));
    assert_eq!(log.source_id, Some(router.source_id));
    assert_eq!(
        log.source_profile_type_snapshot,
        Some(fixture.profile_type.clone())
    );
    let expected_selection_reason = match (fixture.protocol, upstream_protocol) {
        (DownstreamProtocol::Openai, UpstreamProtocol::Openai)
        | (DownstreamProtocol::Responses, UpstreamProtocol::Responses)
        | (DownstreamProtocol::Anthropic, UpstreamProtocol::Anthropic)
        | (DownstreamProtocol::Gemini, UpstreamProtocol::Gemini) => "protocol_match",
        _ => "provider_default_transform",
    };
    assert_eq!(
        log.source_selection_reason.as_deref(),
        Some(expected_selection_reason)
    );
    assert_eq!(
        log.provider_key_snapshot.as_deref(),
        Some(router.provider_key.as_str())
    );
    assert_eq!(
        log.provider_name_snapshot.as_deref(),
        Some(router.provider_name.as_str())
    );
    assert_eq!(
        log.model_name_snapshot.as_deref(),
        Some(router.model_name.as_str())
    );
    assert_eq!(
        log.real_model_name_snapshot.as_deref(),
        Some(UPSTREAM_MODEL)
    );
    assert_eq!(log.client_ip.as_deref(), Some("127.0.0.1"));
}

fn assert_log_timing_order(log: &RequestLogRecord) {
    let sent = log
        .upstream_request_sent_at
        .expect("upstream request timing should be persisted");
    let headers = log
        .upstream_response_headers_at
        .expect("upstream response headers timing should be persisted");
    assert!(headers >= sent);
    if let Some(raw) = log.upstream_first_body_chunk_at {
        assert!(raw >= headers);
    }
    if let Some(first_response_body) = log.first_response_body_at {
        assert!(first_response_body >= sent);
        if let Some(raw) = log.upstream_first_body_chunk_at {
            assert!(first_response_body >= raw);
        }
    }
    if let Some(first_token) = log.first_token_at {
        assert!(log.is_stream);
        let raw = log
            .upstream_first_body_chunk_at
            .expect("TTFT requires an upstream raw body timestamp");
        assert!(first_token >= raw);
    }
    if let Some(max_idle) = log.max_upstream_response_idle_ms {
        assert!(max_idle >= 0);
    }
    let completed = log
        .completed_at
        .expect("completed timing should be persisted");
    assert!(completed >= sent);
    for stage in [
        log.upstream_response_headers_at,
        log.upstream_first_body_chunk_at,
        log.first_response_body_at,
        log.first_token_at,
    ]
    .into_iter()
    .flatten()
    {
        assert!(completed >= stage);
    }
}

fn assert_usage(log: &RequestLogRecord, usage: &UsageGolden) {
    assert_eq!(log.total_input_tokens, Some(usage.input));
    assert_eq!(log.total_output_tokens, Some(usage.output));
    assert_eq!(log.total_tokens, Some(usage.total));
}

fn downstream_error_code(body: &Value, protocol: DownstreamProtocol) -> Option<&str> {
    assert!(
        body.get("code").is_none(),
        "legacy top-level code must be absent"
    );
    assert!(
        body.get("message").is_none(),
        "legacy top-level message must be absent"
    );
    match protocol {
        DownstreamProtocol::Openai
        | DownstreamProtocol::Responses
        | DownstreamProtocol::Anthropic => body["error"]["code"].as_str(),
        DownstreamProtocol::Gemini => {
            body["error"]["details"][0]["metadata"]["cyder_code"].as_str()
        }
    }
}

fn downstream_error_message(body: &Value) -> Option<&str> {
    body["error"]["message"].as_str()
}

fn assert_no_public_transform_diagnostics(response: &Response<Body>) {
    assert!(
        response
            .headers()
            .keys()
            .all(|name| !name.as_str().starts_with("x-cyder-")),
        "internal transform facts must not use downstream headers"
    );
}

fn assert_payload_free_transform_bytes(bytes: &[u8], private_marker: &str) {
    let rendered = String::from_utf8_lossy(bytes);
    for forbidden in [
        private_marker,
        "transform_diagnostic",
        "safe_summary",
        "sha256",
        "outcome_counts",
        "action_counts",
        "semantic_counts",
        "failure_origin",
        "reason_code",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "downstream bytes must not expose {forbidden}"
        );
    }
}

fn assert_native_fatal_stream_event(
    protocol: DownstreamProtocol,
    event: &GoldenEvent,
    request_id: &str,
) {
    const CODE: &str = "upstream_response_error";
    const MESSAGE: &str = "The gateway could not read a valid response from the upstream provider.";
    match protocol {
        DownstreamProtocol::Openai => {
            assert_eq!(event.event, None);
            assert_eq!(event.data["error"]["code"], CODE);
            assert_eq!(event.data["error"]["type"], "server_error");
            assert_eq!(event.data["error"]["message"], MESSAGE);
            assert_eq!(event.data["request_id"], request_id);
        }
        DownstreamProtocol::Responses => {
            assert_eq!(event.event, None);
            assert_eq!(event.data["type"], "response.error");
            assert_eq!(event.data["error"]["code"], CODE);
            assert_eq!(event.data["error"]["type"], "server_error");
            assert_eq!(event.data["error"]["message"], MESSAGE);
            assert_eq!(event.data["request_id"], request_id);
        }
        DownstreamProtocol::Anthropic => {
            assert_eq!(event.event.as_deref(), Some("error"));
            assert_eq!(event.data["type"], "error");
            assert_eq!(event.data["error"]["code"], CODE);
            assert_eq!(event.data["error"]["type"], "api_error");
            assert_eq!(event.data["error"]["message"], MESSAGE);
            assert_eq!(event.data["request_id"], request_id);
        }
        DownstreamProtocol::Gemini => {
            assert_eq!(event.event, None);
            assert_eq!(event.data["error"]["code"], 502);
            assert_eq!(event.data["error"]["status"], "UNKNOWN");
            assert_eq!(event.data["error"]["message"], MESSAGE);
            assert_eq!(
                event.data["error"]["details"][0]["reason"],
                "UPSTREAM_RESPONSE_ERROR"
            );
            assert_eq!(
                event.data["error"]["details"][0]["metadata"]["request_id"],
                request_id
            );
        }
    }

    let encoded = serde_json::to_string(&event.data).expect("terminal event should serialize");
    for forbidden in [
        "[DONE]",
        "message_stop",
        "response.completed",
        "transform_diagnostic",
        "operator_message",
        "safe_summary",
        "sha256",
        "malformed-upstream-private-marker",
    ] {
        assert!(
            !encoded.contains(forbidden),
            "forbidden terminal field: {forbidden}"
        );
    }
}

#[test]
fn direct_execution_regression_fixtures_define_four_complete_protocols() {
    let fixtures = openai_target_fixtures();
    assert_eq!(fixtures.len(), 4);
    for (name, fixture) in fixtures {
        validate_fixture(name, &fixture);
        assert_eq!(
            fixture.profile_type,
            UpstreamProfileType::Openai,
            "{name}: R3.16 baseline evidence must execute against OpenAI Chat Completions"
        );
    }
}

#[test]
fn responses_target_fixtures_define_four_complete_protocols_and_native_evidence() {
    let target = responses_target_golden();
    let fixtures = responses_target_fixtures();

    assert_eq!(target.base_url, "/v1");
    assert_eq!(target.profile_type, UpstreamProfileType::Responses);
    assert_eq!(target.upstream_path, "/v1/responses");
    assert_eq!(
        fixtures.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        vec!["openai", "responses", "anthropic", "gemini"]
    );
    assert_eq!(target.requests.len(), 4);

    for (name, fixture) in fixtures {
        validate_fixture(name, &fixture);
        assert_eq!(
            fixture.profile_type,
            UpstreamProfileType::Responses,
            "{name}"
        );
        assert_eq!(fixture.request.upstream_path, "/v1/responses", "{name}");
        assert_eq!(fixture.stream.upstream_path, "/v1/responses", "{name}");
        assert_eq!(
            fixture.cancellation.upstream_path, "/v1/responses",
            "{name}"
        );
        assert_eq!(
            fixture
                .upstream_headers
                .get("authorization")
                .map(String::as_str),
            Some("Bearer provider-baseline-secret"),
            "{name}"
        );
        for request in [&fixture.request.upstream, &fixture.stream.upstream_request] {
            assert_eq!(request["model"], "$UPSTREAM_MODEL", "{name}");
            assert_eq!(request["store"], false, "{name}");
        }
        assert_eq!(fixture.request.upstream["stream"], false, "{name}");
        assert_eq!(fixture.stream.upstream_request["stream"], true, "{name}");
        assert_eq!(
            fixture.non_stream.upstream_response["status"], "completed",
            "{name}"
        );
        assert_eq!(
            fixture.non_stream.upstream_response["usage"]["input_tokens"], 11,
            "{name}"
        );
        assert_eq!(
            fixture.non_stream.upstream_response["usage"]["output_tokens"], 7,
            "{name}"
        );
    }

    assert_eq!(
        (target.usage.input, target.usage.output, target.usage.total),
        (11, 7, 18)
    );
    assert!(
        target
            .stream_events
            .iter()
            .any(|event| event.data["type"] == "response.output_text.done")
    );
    assert!(
        target
            .stream_events
            .iter()
            .any(|event| event.data["type"] == "response.completed")
    );
    assert_eq!(target.error.http_429.status, 429);
    assert_eq!(target.error.failed_response["status"], "failed");
    assert_eq!(target.error.error_event.event.as_deref(), Some("error"));
    assert_eq!(target.error.error_event.data["type"], "error");
    assert_eq!(
        target.cancellation.first_upstream_event.data["response"]["status"],
        "in_progress"
    );
}

#[test]
fn anthropic_target_fixtures_define_four_complete_protocols_and_reject_bad_overlays() {
    let target = anthropic_target_golden();
    let fixtures = anthropic_target_fixtures();

    assert_eq!(target.base_url, "/v1");
    assert_eq!(target.profile_type, UpstreamProfileType::Anthropic);
    assert_eq!(target.upstream_path, "/v1/messages");
    assert_eq!(
        fixtures.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        vec!["openai", "responses", "anthropic", "gemini"]
    );
    assert_eq!(target.requests.len(), 4);

    for (name, fixture) in fixtures {
        validate_fixture(name, &fixture);
        assert_eq!(
            fixture.profile_type,
            UpstreamProfileType::Anthropic,
            "{name}"
        );
        assert_eq!(fixture.request.upstream_path, "/v1/messages", "{name}");
        assert_eq!(fixture.stream.upstream_path, "/v1/messages", "{name}");
        assert_eq!(fixture.cancellation.upstream_path, "/v1/messages", "{name}");
        assert_eq!(
            fixture
                .upstream_headers
                .get("anthropic-version")
                .map(String::as_str),
            Some("2023-06-01"),
            "{name}"
        );
        assert_eq!(
            fixture
                .upstream_headers
                .get("x-api-key")
                .map(String::as_str),
            Some(PROVIDER_SECRET),
            "{name}"
        );
        for request in [&fixture.request.upstream, &fixture.stream.upstream_request] {
            assert_eq!(request["model"], "$UPSTREAM_MODEL", "{name}");
            assert_eq!(
                request["max_tokens"],
                if name == "anthropic" { 64 } else { 4096 },
                "{name}"
            );
        }
        if name == "anthropic" {
            assert!(fixture.request.upstream.get("stream").is_none(), "{name}");
        } else {
            assert_eq!(fixture.request.upstream["stream"], false, "{name}");
        }
        assert_eq!(fixture.stream.upstream_request["stream"], true, "{name}");
        assert_eq!(
            fixture.non_stream.upstream_response["stop_reason"],
            "end_turn"
        );
    }

    assert_eq!(
        (target.usage.input, target.usage.output, target.usage.total),
        (16, 7, 23)
    );
    assert_eq!(
        target.error.stream_error_event.event.as_deref(),
        Some("error")
    );

    let source: Value = serde_json::from_str(ANTHROPIC_TARGET_SOURCE).expect("valid source JSON");

    let mut missing = source.clone();
    missing["requests"]
        .as_array_mut()
        .expect("request cases")
        .pop();
    let error = parse_anthropic_target_golden(&missing.to_string()).expect_err("missing case");
    assert!(error.contains("must be exactly"));

    let mut duplicate = source.clone();
    let duplicate_case = duplicate["requests"][0].clone();
    duplicate["requests"]
        .as_array_mut()
        .expect("request cases")
        .push(duplicate_case);
    let error = parse_anthropic_target_golden(&duplicate.to_string()).expect_err("duplicate case");
    assert!(error.contains("duplicate Anthropic target request case"));

    let mut bad_path = source;
    bad_path["upstream_path"] = json!("/messages");
    let error = parse_anthropic_target_golden(&bad_path.to_string()).expect_err("bad path");
    assert!(error.contains("must be /v1/messages"));
}

#[test]
fn r3_19_gemini_target_fixtures_define_complete_protocol_contract() {
    let target = gemini_target_golden();

    assert_eq!(target.fixture_version, 1);
    assert_eq!(target.model, "$UPSTREAM_MODEL");
    assert_eq!(target.profiles.len(), 2);
    assert_eq!(target.operations.len(), 3);
    assert_eq!(target.requests.len(), 4);
    assert_eq!(
        target
            .requests
            .iter()
            .map(|request| request.advanced.len())
            .sum::<usize>(),
        16
    );
    assert_eq!(target.terminal_cases.len(), 6);

    let gemini = target
        .profiles
        .iter()
        .find(|profile| profile.profile_type == UpstreamProfileType::Gemini)
        .expect("GEMINI profile fixture");
    assert_eq!(
        gemini.base_url,
        "https://generativelanguage.googleapis.com/v1beta/models"
    );
    assert_eq!(gemini.auth, GeminiFixtureAuth::XGoogApiKey);

    let vertex = target
        .profiles
        .iter()
        .find(|profile| profile.profile_type == UpstreamProfileType::Vertex)
        .expect("VERTEX profile fixture");
    assert_eq!(
        vertex.base_url,
        "https://us-central1-aiplatform.googleapis.com/v1/projects/project-fixture/locations/us-central1/publishers/google/models"
    );
    assert_eq!(vertex.auth, GeminiFixtureAuth::Bearer);

    for request in &target.requests {
        assert!(request.non_stream.is_object(), "{:?}", request.downstream);
        assert!(request.stream.is_object(), "{:?}", request.downstream);
        for advanced in &request.advanced {
            assert!(
                advanced.downstream_request.is_object(),
                "{:?} {:?}",
                request.downstream,
                advanced.capability
            );
            assert!(
                advanced.expected_upstream.is_object(),
                "{:?} {:?}",
                request.downstream,
                advanced.capability
            );
            assert!(
                advanced.rejection_request.is_object(),
                "{:?} {:?}",
                request.downstream,
                advanced.capability
            );
        }
    }

    assert_eq!(
        target.non_stream_response["candidates"][0]["finishReason"],
        "STOP"
    );
    assert_eq!(target.stream_events.len(), 3);
    assert_eq!(target.count_tokens.response["totalTokens"], json!(23));
    assert_eq!(target.error.http_429.status, 429);
    assert_eq!(
        target.error.http_429.response["error"]["status"],
        "RESOURCE_EXHAUSTED"
    );
    assert_eq!(
        target
            .cancellation
            .first_upstream_event
            .data
            .pointer("/candidates/0/content/parts/0/text"),
        Some(&json!("baseline "))
    );
    assert!(
        target
            .count_tokens
            .contents_request
            .get("contents")
            .is_some()
    );
    assert!(
        target
            .count_tokens
            .generate_content_request
            .get("generateContentRequest")
            .is_some()
    );

    for required in [
        "functionCall",
        "functionResponse",
        "thoughtSignature",
        "inlineData",
        "fileData",
    ] {
        assert!(
            target
                .official_fields
                .request_part_tags
                .iter()
                .any(|field| field == required),
            "missing official Part field {required}"
        );
    }
    for required in [
        "promptTokenCount",
        "candidatesTokenCount",
        "cachedContentTokenCount",
        "thoughtsTokenCount",
        "toolUsePromptTokenCount",
        "totalTokenCount",
    ] {
        assert!(
            target
                .official_fields
                .usage_fields
                .iter()
                .any(|field| field == required),
            "missing official usage field {required}"
        );
    }
}

#[test]
fn r3_19_gemini_target_fixture_loader_rejects_missing_duplicate_and_unknown_tags() {
    let source: Value = serde_json::from_str(GEMINI_TARGET_SOURCE).expect("valid source JSON");

    let mut missing = source.clone();
    missing
        .as_object_mut()
        .expect("fixture object")
        .remove("count_tokens");
    let error = parse_gemini_target_golden(&missing.to_string()).expect_err("missing section");
    assert!(error.contains("missing field `count_tokens`"), "{error}");

    let mut duplicate = source.clone();
    let duplicate_request = duplicate["requests"][0].clone();
    duplicate["requests"]
        .as_array_mut()
        .expect("request cases")
        .push(duplicate_request);
    let error = parse_gemini_target_golden(&duplicate.to_string()).expect_err("duplicate case");
    assert!(
        error.contains("duplicate Gemini target downstream request"),
        "{error}"
    );

    let mut illegal_protocol = source.clone();
    illegal_protocol["requests"][0]["downstream"] = json!("UNSUPPORTED");
    let error =
        parse_gemini_target_golden(&illegal_protocol.to_string()).expect_err("illegal protocol");
    assert!(error.contains("unknown variant"), "{error}");

    let mut illegal_capability = source.clone();
    illegal_capability["requests"][0]["advanced"][0]["capability"] = json!("web_search");
    let error = parse_gemini_target_golden(&illegal_capability.to_string())
        .expect_err("illegal capability");
    assert!(error.contains("unknown variant"), "{error}");

    let mut illegal_terminal = source.clone();
    illegal_terminal["terminal_cases"][0]["terminal"] = json!("future_unknown");
    let error =
        parse_gemini_target_golden(&illegal_terminal.to_string()).expect_err("illegal terminal");
    assert!(error.contains("unknown variant"), "{error}");

    let mut unknown_fixture_tag = source;
    unknown_fixture_tag["future_fixture_contract"] = json!(true);
    let error = parse_gemini_target_golden(&unknown_fixture_tag.to_string())
        .expect_err("unknown fixture tag");
    assert!(error.contains("unknown field"), "{error}");
}

#[test]
fn r3_19_gemini_query_auth_and_patch_boundary_is_enforced_end_to_end() {
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "gemini")
        .expect("Gemini fixture");
    run_case("r3-19-gemini-query-boundary", move |context| async move {
        let upstream =
            TestUpstream::spawn_json(StatusCode::OK, fixture.non_stream.upstream_response.clone())
                .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        router
            .app_state
            .admin
            .request_patch
            .create_source_variant(
                router.source_id,
                RequestPatchVariantInput {
                    source_id: router.source_id,
                    model_id: None,
                    suffix: None,
                    enabled: true,
                    expose_in_models: false,
                    rules: vec![RequestPatchRuleInput {
                        placement: RequestPatchPlacement::Query,
                        target: "trace".to_string(),
                        operation: RequestPatchOperation::Set,
                        value_json: Some(Some(json!("manager-safe"))),
                        description: Some("R3.19 safe Gemini Query Patch".to_string()),
                    }],
                },
            )
            .await
            .expect("safe Query Patch should save");
        router
            .app_state
            .catalog
            .invalidate_models_catalog()
            .await
            .expect("Patch catalog should invalidate");

        let uri = format!(
            "/gemini/v1beta/models/{}:generateContent?key={}&alt=client-owned&trace=downstream-owned&custom=discarded",
            router.requested_model(),
            router.downstream_key
        );
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(CONTENT_TYPE, "application/json")
            .header("authorization", "Bearer downstream-auth-private-marker")
            .header("x-api-key", "downstream-x-api-key-private-marker")
            .header("x-goog-api-key", &router.downstream_key)
            .header("cookie", "session=downstream-cookie-private-marker")
            .header(
                "proxy-authorization",
                "Basic downstream-proxy-private-marker",
            )
            .body(Body::from(
                serde_json::to_vec(&fixture.request.downstream).unwrap(),
            ))
            .expect("Gemini request");
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(SocketAddr::from((
                [127, 0, 0, 1],
                3019,
            ))));
        let response = create_proxy_router(Arc::new(ClientIdentityResolver::new(
            &ClientIdentityConfig::default(),
        )))
        .with_state(Arc::clone(&router.app_state))
        .oneshot(request)
        .await
        .expect("proxy router response");

        assert_eq!(response.status(), StatusCode::OK);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Gemini response body");
        let captured = upstream.requests().await;
        assert_eq!(captured.len(), 1);
        assert_eq!(
            captured[0].path,
            "/v1beta/models/baseline-upstream-model:generateContent"
        );
        assert_eq!(captured[0].query.as_deref(), Some("trace=manager-safe"));
        assert_eq!(
            captured[0]
                .headers
                .get("x-goog-api-key")
                .and_then(|value| value.to_str().ok()),
            Some(PROVIDER_SECRET)
        );
        for forbidden in [
            "authorization",
            "x-api-key",
            "cookie",
            "proxy-authorization",
        ] {
            assert!(!captured[0].headers.contains_key(forbidden), "{forbidden}");
        }
        assert_eq!(
            ["authorization", "x-api-key", "x-goog-api-key"]
                .into_iter()
                .map(|name| captured[0].headers.get_all(name).iter().count())
                .sum::<usize>(),
            1
        );
        router.wait_for_log(RequestStatus::Success).await;
        upstream.shutdown().await;
    });
}

#[test]
fn r3_19_gemini_invalid_model_fails_before_credentials_and_network() {
    const SENTINEL: &str = "gemini-invalid-model-private-marker";
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "gemini")
        .expect("Gemini fixture");
    for profile_type in [UpstreamProfileType::Gemini, UpstreamProfileType::Vertex] {
        let fixture = fixture.clone();
        let case_name = format!("r3-19-gemini-invalid-model-{profile_type:?}");
        run_case(&case_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                fixture.non_stream.upstream_response.clone(),
            )
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            if profile_type == UpstreamProfileType::Vertex {
                router
                    .replace_default_gemini_profile_in_place(&upstream.base_url, profile_type)
                    .await;
            }
            Model::update(
                router.model_id,
                &UpdateModelData {
                    model_name: None,
                    real_model_name: Some(Some(format!("models/{SENTINEL}"))),
                    is_enabled: None,
                    cost_catalog_id: None,
                },
            )
            .expect("invalid legacy model state should seed");
            router
                .app_state
                .catalog
                .invalidate_provider(router.provider_id, Some(&router.provider_key))
                .await
                .expect("provider cache should invalidate");
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("invalid model response");
            assert!(!String::from_utf8_lossy(&body).contains(SENTINEL));
            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                0,
                "credential decryption precedes Vertex OAuth, so zero decrypt also proves zero OAuth"
            );
            assert!(upstream.requests().await.is_empty());
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("provider_configuration_error")
            );
            assert!(
                !log.final_error_message
                    .unwrap_or_default()
                    .contains(SENTINEL)
            );
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_gemini_post_patch_target_validation_precedes_credentials_and_network() {
    const SENTINEL: &str = "gemini-invalid-body-private-marker";
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "gemini")
        .expect("Gemini fixture");
    for profile_type in [UpstreamProfileType::Gemini, UpstreamProfileType::Vertex] {
        let fixture = fixture.clone();
        let case_name = format!("r3-19-gemini-post-patch-final-validation-{profile_type:?}");
        run_case(&case_name.clone(), move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                fixture.non_stream.upstream_response.clone(),
            )
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            if profile_type == UpstreamProfileType::Vertex {
                router
                    .replace_default_gemini_profile_in_place(&upstream.base_url, profile_type)
                    .await;
            }
            router
                .app_state
                .admin
                .request_patch
                .create_source_variant(
                    router.source_id,
                    RequestPatchVariantInput {
                        source_id: router.source_id,
                        model_id: None,
                        suffix: None,
                        enabled: true,
                        expose_in_models: false,
                        rules: vec![RequestPatchRuleInput {
                            placement: RequestPatchPlacement::Body,
                            target: "/contents/0/parts".to_string(),
                            operation: RequestPatchOperation::Set,
                            value_json: Some(Some(json!(SENTINEL))),
                            description: Some("R3.19 post-Patch body validation".to_string()),
                        }],
                    },
                )
                .await
                .expect("ordinary body Patch should save");
            router
                .app_state
                .catalog
                .invalidate_models_catalog()
                .await
                .expect("Patch catalog should invalidate");
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("invalid target response body");
            assert!(!String::from_utf8_lossy(&body).contains(SENTINEL));
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty());
            assert!(
                profile_type != UpstreamProfileType::Vertex
                    || !vertex_token_is_cached_for_test(router.provider_api_key_id),
                "{case_name}: Vertex OAuth must not begin"
            );
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("provider_configuration_error")
            );
            assert!(
                !log.final_error_message
                    .unwrap_or_default()
                    .contains(SENTINEL)
            );
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_four_downstreams_materialize_native_gemini_nonstream_and_stream_once() {
    for (name, fixture) in gemini_target_fixtures() {
        for is_stream in [false, true] {
            let fixture = fixture.clone();
            let case_name = format!(
                "r3-19-{name}-to-gemini-materializer-{}",
                if is_stream { "stream" } else { "nonstream" }
            );
            run_case(&case_name, move |context| async move {
                let upstream = if is_stream {
                    TestUpstream::spawn(ScriptedReply::Sse {
                        events: fixture.stream.upstream_events.clone(),
                    })
                    .await
                } else {
                    TestUpstream::spawn_json(
                        StatusCode::OK,
                        fixture.non_stream.upstream_response.clone(),
                    )
                    .await
                };
                let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
                router
                    .app_state
                    .admin
                    .request_patch
                    .create_source_variant(
                        router.source_id,
                        RequestPatchVariantInput {
                            source_id: router.source_id,
                            model_id: None,
                            suffix: None,
                            enabled: true,
                            expose_in_models: false,
                            rules: vec![
                                RequestPatchRuleInput {
                                    placement: RequestPatchPlacement::Query,
                                    target: "trace".to_string(),
                                    operation: RequestPatchOperation::Set,
                                    value_json: Some(Some(json!("materializer-safe"))),
                                    description: Some(
                                        "Gemini materializer query capture".to_string(),
                                    ),
                                },
                                RequestPatchRuleInput {
                                    placement: RequestPatchPlacement::Header,
                                    target: "x-r3-19-materializer".to_string(),
                                    operation: RequestPatchOperation::Set,
                                    value_json: Some(Some(json!("enabled"))),
                                    description: Some(
                                        "Gemini materializer header capture".to_string(),
                                    ),
                                },
                                RequestPatchRuleInput {
                                    placement: RequestPatchPlacement::Body,
                                    target: "/requestTag".to_string(),
                                    operation: RequestPatchOperation::Set,
                                    value_json: Some(Some(json!("patched"))),
                                    description: Some(
                                        "Gemini materializer body capture".to_string(),
                                    ),
                                },
                            ],
                        },
                    )
                    .await
                    .expect("safe generation Patch should save");
                router
                    .app_state
                    .catalog
                    .invalidate_models_catalog()
                    .await
                    .expect("Patch catalog should invalidate");
                router
                    .app_state
                    .secret_encryption
                    .reset_decrypt_call_count();

                let downstream_body = if is_stream {
                    &fixture.stream.downstream_request
                } else {
                    &fixture.request.downstream
                };
                let response = router.send(&fixture, is_stream, downstream_body).await;

                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{name} stream={is_stream}"
                );
                axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("downstream response should complete");
                let captured = upstream.requests().await;
                assert_eq!(captured.len(), 1, "{name} stream={is_stream}");
                let request = &captured[0];
                assert_eq!(request.method, Method::POST);
                assert_eq!(
                    request.path,
                    if is_stream {
                        format!("/v1beta/models/{UPSTREAM_MODEL}:streamGenerateContent")
                    } else {
                        format!("/v1beta/models/{UPSTREAM_MODEL}:generateContent")
                    }
                );
                let query = request
                    .query
                    .as_deref()
                    .map(|query| {
                        query
                            .split('&')
                            .map(|pair| pair.split_once('=').unwrap_or((pair, "")))
                            .map(|(key, value)| (key.to_string(), value.to_string()))
                            .collect::<BTreeMap<_, _>>()
                    })
                    .unwrap_or_default();
                assert_eq!(
                    query.get("trace").map(String::as_str),
                    Some("materializer-safe")
                );
                assert_eq!(
                    query.get("alt").map(String::as_str),
                    is_stream.then_some("sse")
                );
                assert_eq!(query.len(), if is_stream { 2 } else { 1 });
                assert_eq!(
                    request
                        .headers
                        .get("x-goog-api-key")
                        .and_then(|value| value.to_str().ok()),
                    Some(PROVIDER_SECRET)
                );
                assert_eq!(
                    request
                        .headers
                        .get("x-r3-19-materializer")
                        .and_then(|value| value.to_str().ok()),
                    Some("enabled")
                );
                assert!(request.headers.contains_key(CONTENT_TYPE));
                assert!(request.headers.contains_key(&X_REQUEST_ID));
                let actual_body: Value = serde_json::from_slice(&request.body)
                    .expect("captured Gemini request should be JSON");
                let mut expected_body = render_value(
                    if is_stream {
                        &fixture.stream.upstream_request
                    } else {
                        &fixture.request.upstream
                    },
                    &router.requested_model(),
                );
                expected_body["requestTag"] = json!("patched");
                assert_eq!(actual_body, expected_body, "{name} stream={is_stream}");
                if fixture.protocol == DownstreamProtocol::Gemini {
                    assert_eq!(actual_body["futureRoot"], json!({"preserve":true}));
                    assert_eq!(actual_body["generationConfig"]["candidateCount"], 2);
                } else {
                    assert_eq!(actual_body["generationConfig"]["candidateCount"], 1);
                }

                let deadline = Instant::now() + WAIT_TIMEOUT;
                let log = loop {
                    if let Some(log) = router.request_logs().await.into_iter().next() {
                        break log;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "{name} stream={is_stream}: request log should persist"
                    );
                    tokio::time::sleep(Duration::from_millis(10)).await;
                };
                assert_log_common(&router, &fixture, &log);
                assert_eq!(log.source_id, Some(router.source_id));
                assert_eq!(
                    log.source_profile_type_snapshot,
                    Some(UpstreamProfileType::Gemini)
                );
                upstream.shutdown().await;
            });
        }
    }
}

#[test]
fn r3_19_three_cross_wire_requests_capture_ordered_roles_and_generation_controls_once() {
    let response = gemini_target_golden().non_stream_response;
    for (case_name, protocol, downstream_request, expected_upstream) in [
        (
            "openai",
            DownstreamProtocol::Openai,
            json!({
                "model":"$REQUESTED_MODEL",
                "messages":[
                    {"role":"system","content":"system-one"},
                    {"role":"developer","content":"developer-two"},
                    {"role":"user","content":"user-one"},
                    {"role":"user","content":"user-two"},
                    {"role":"assistant","content":"assistant-three"}
                ],
                "temperature":0.7,
                "max_completion_tokens":64,
                "top_p":0.8,
                "stop":["STOP"],
                "seed":7,
                "presence_penalty":0.25,
                "frequency_penalty":-0.5,
                "n":1
            }),
            json!({
                "systemInstruction":{"parts":[{"text":"system-one"},{"text":"developer-two"}]},
                "contents":[
                    {"role":"user","parts":[{"text":"user-one"},{"text":"user-two"}]},
                    {"role":"model","parts":[{"text":"assistant-three"}]}
                ],
                "generationConfig":{
                    "candidateCount":1,"temperature":0.7,"maxOutputTokens":64,"topP":0.8,
                    "seed":7,"presencePenalty":0.25,"frequencyPenalty":-0.5,
                    "stopSequences":["STOP"]
                }
            }),
        ),
        (
            "responses",
            DownstreamProtocol::Responses,
            json!({
                "model":"$REQUESTED_MODEL",
                "instructions":"system-one",
                "input":[
                    {"role":"developer","content":"developer-two"},
                    {"role":"user","content":"user-one"},
                    {"role":"user","content":"user-two"},
                    {"role":"assistant","content":"assistant-three"}
                ],
                "temperature":0.6,"max_output_tokens":63,"top_p":0.75
            }),
            json!({
                "systemInstruction":{"parts":[{"text":"system-one"},{"text":"developer-two"}]},
                "contents":[
                    {"role":"user","parts":[{"text":"user-one"},{"text":"user-two"}]},
                    {"role":"model","parts":[{"text":"assistant-three"}]}
                ],
                "generationConfig":{"candidateCount":1,"temperature":0.6,"maxOutputTokens":63,"topP":0.75}
            }),
        ),
        (
            "anthropic",
            DownstreamProtocol::Anthropic,
            json!({
                "model":"$REQUESTED_MODEL",
                "system":"system-one",
                "messages":[
                    {"role":"user","content":"user-one"},
                    {"role":"user","content":"user-two"},
                    {"role":"assistant","content":"assistant-three"}
                ],
                "temperature":0.5,"max_tokens":62,"top_p":0.7,"top_k":31,
                "stop_sequences":["STOP"]
            }),
            json!({
                "systemInstruction":{"parts":[{"text":"system-one"}]},
                "contents":[
                    {"role":"user","parts":[{"text":"user-one"},{"text":"user-two"}]},
                    {"role":"model","parts":[{"text":"assistant-three"}]}
                ],
                "generationConfig":{
                    "candidateCount":1,"temperature":0.5,"maxOutputTokens":62,"topP":0.7,
                    "topK":31,"stopSequences":["STOP"]
                }
            }),
        ),
    ] {
        let (_, fixture) = gemini_target_fixtures()
            .into_iter()
            .find(|(_, fixture)| fixture.protocol == protocol)
            .expect("Gemini target fixture for downstream protocol");
        let upstream_response = response.clone();
        let runtime_name = format!("r3-19-gemini-portable-request-{case_name}");
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_response).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let response = router.send(&fixture, false, &downstream_request).await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("portable response should complete");
            let requests = upstream.requests().await;
            assert_eq!(requests.len(), 1, "{case_name}");
            let captured: Value = serde_json::from_slice(&requests[0].body)
                .expect("captured Gemini request should be JSON");
            assert_eq!(captured, expected_upstream, "{case_name}");
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 1);
            router.wait_for_log(RequestStatus::Success).await;
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_unrepresentable_gemini_request_controls_are_precredential_zero_call() {
    let (_, fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Openai)
        .expect("OpenAI to Gemini target fixture");
    for (case_name, field) in [
        ("multiple-candidates", json!({"n":2})),
        ("logprobs", json!({"logprobs":true})),
        ("logit-bias", json!({"logit_bias":{"1":1}})),
        (
            "prediction",
            json!({"prediction":{"type":"content","content":"private-marker"}}),
        ),
        ("output-audio", json!({"modalities":["text","audio"]})),
        ("web-search", json!({"web_search_options":{}})),
        ("seed-overflow", json!({"seed":2147483648_i64})),
    ] {
        let fixture = fixture.clone();
        let runtime_name = format!("r3-19-gemini-zero-call-{case_name}");
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                json!({"candidates":[{"index":0,"finishReason":"STOP"}]}),
            )
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();
            let mut request = json!({
                "model":"$REQUESTED_MODEL",
                "messages":[{"role":"user","content":"hello"}]
            });
            request
                .as_object_mut()
                .expect("request object")
                .extend(field.as_object().expect("field object").clone());

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case_name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("rejection response should read");
            assert!(!String::from_utf8_lossy(&body).contains("private-marker"));
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty(), "{case_name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert!(matches!(
                log.final_error_code.as_deref(),
                Some("invalid_request_error" | "unsupported_capability_error")
            ));
            assert_eq!(log.total_tokens, None);
            assert_eq!(log.estimated_cost_nanos, None);
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_four_tool_cells_capture_native_gemini_requests_once() {
    let target = gemini_target_golden();
    let upstream_response = target.non_stream_response.clone();
    for request_case in target.requests {
        let advanced = request_case
            .advanced
            .iter()
            .find(|advanced| advanced.capability == GeminiFixtureCapability::Tools)
            .expect("Gemini tools fixture")
            .clone();
        let (_, fixture) = gemini_target_fixtures()
            .into_iter()
            .find(|(_, fixture)| fixture.protocol == request_case.downstream)
            .expect("Gemini target fixture for tools cell");
        let upstream_response = upstream_response.clone();
        let case_name = format!("r3-19-gemini-tools-{:?}", request_case.downstream);
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_response).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let response = router
                .send(&fixture, false, &advanced.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("tools response should complete");
            let requests = upstream.requests().await;
            assert_eq!(requests.len(), 1, "{case_name}");
            let body: Value = serde_json::from_slice(&requests[0].body)
                .expect("captured Gemini tools request should be JSON");
            assert_eq!(
                body,
                render_value(&advanced.expected_upstream, &router.requested_model()),
                "{case_name}"
            );
            router.wait_for_log(RequestStatus::Success).await;
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_invalid_tools_and_unsigned_history_are_precredential_zero_call() {
    let (_, fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Openai)
        .expect("OpenAI to Gemini target fixture");
    for (case_name, request) in [
        (
            "built-in",
            json!({
                "model":"$REQUESTED_MODEL","messages":[{"role":"user","content":"search"}],
                "tools":[{"type":"web_search"}],
                "tool_choice":"required"
            }),
        ),
        (
            "orphan-result",
            json!({
                "model":"$REQUESTED_MODEL","messages":[
                    {"role":"tool","tool_call_id":"orphan","content":"private-tool-marker"}
                ]
            }),
        ),
        (
            "duplicate-call-id",
            json!({
                "model":"$REQUESTED_MODEL","messages":[
                    {"role":"assistant","tool_calls":[
                        {"id":"duplicate","type":"function","function":{"name":"weather","arguments":"{}"}},
                        {"id":"duplicate","type":"function","function":{"name":"time","arguments":"{}"}}
                    ]}
                ]
            }),
        ),
        (
            "mismatched-result",
            json!({
                "model":"$REQUESTED_MODEL","messages":[
                    {"role":"assistant","tool_calls":[
                        {"id":"call-weather","type":"function","function":{"name":"weather","arguments":"{}"}}
                    ]},
                    {"role":"tool","tool_call_id":"call-other","content":"private-tool-marker"}
                ]
            }),
        ),
        (
            "missing-thought-signature",
            json!({
                "model":"$REQUESTED_MODEL","messages":[
                    {"role":"assistant","tool_calls":[
                        {"id":"call-weather","type":"function","function":{"name":"weather","arguments":"{}"}}
                    ]},
                    {"role":"tool","tool_call_id":"call-weather","content":"private-tool-marker"}
                ]
            }),
        ),
    ] {
        let fixture = fixture.clone();
        let runtime_name = format!("r3-19-gemini-tools-zero-call-{case_name}");
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                json!({"candidates":[{"index":0,"finishReason":"STOP"}]}),
            )
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case_name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("tools rejection should read");
            assert!(!String::from_utf8_lossy(&body).contains("private-tool-marker"));
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty(), "{case_name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert!(matches!(
                log.final_error_code.as_deref(),
                Some("invalid_request_error" | "unsupported_capability_error")
            ));
            assert_eq!(log.total_tokens, None);
            assert_eq!(log.estimated_cost_nanos, None);
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_four_reasoning_cells_capture_native_gemini_requests_once() {
    let target = gemini_target_golden();
    let upstream_response = target.non_stream_response.clone();
    for request_case in target.requests {
        let advanced = request_case
            .advanced
            .iter()
            .find(|advanced| advanced.capability == GeminiFixtureCapability::Reasoning)
            .expect("Gemini reasoning fixture")
            .clone();
        let (_, fixture) = gemini_target_fixtures()
            .into_iter()
            .find(|(_, fixture)| fixture.protocol == request_case.downstream)
            .expect("Gemini target fixture for reasoning cell");
        let upstream_response = upstream_response.clone();
        let case_name = format!("r3-19-gemini-reasoning-{:?}", request_case.downstream);
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_response).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let response = router
                .send(&fixture, false, &advanced.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("reasoning response should complete");
            let requests = upstream.requests().await;
            assert_eq!(requests.len(), 1, "{case_name}");
            let body: Value = serde_json::from_slice(&requests[0].body)
                .expect("captured Gemini reasoning request should be JSON");
            assert_eq!(
                body,
                render_value(&advanced.expected_upstream, &router.requested_model()),
                "{case_name}"
            );
            router.wait_for_log(RequestStatus::Success).await;
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_invalid_reasoning_controls_are_precredential_zero_call() {
    let fixtures = gemini_target_fixtures();
    for (case_name, protocol, request) in [
        (
            "openai-unknown-effort",
            DownstreamProtocol::Openai,
            json!({"model":"$REQUESTED_MODEL","messages":[{"role":"user","content":"private-reasoning-marker"}],"reasoning_effort":"future"}),
        ),
        (
            "responses-unknown-effort",
            DownstreamProtocol::Responses,
            json!({"model":"$REQUESTED_MODEL","input":"private-reasoning-marker","reasoning":{"effort":"future"}}),
        ),
        (
            "anthropic-zero-budget",
            DownstreamProtocol::Anthropic,
            json!({"model":"$REQUESTED_MODEL","max_tokens":64,"messages":[{"role":"user","content":"private-reasoning-marker"}],"thinking":{"type":"enabled","budget_tokens":0}}),
        ),
        (
            "anthropic-negative-budget",
            DownstreamProtocol::Anthropic,
            json!({"model":"$REQUESTED_MODEL","max_tokens":64,"messages":[{"role":"user","content":"private-reasoning-marker"}],"thinking":{"type":"enabled","budget_tokens":-1}}),
        ),
        (
            "anthropic-budget-overflow",
            DownstreamProtocol::Anthropic,
            json!({"model":"$REQUESTED_MODEL","max_tokens":64,"messages":[{"role":"user","content":"private-reasoning-marker"}],"thinking":{"type":"enabled","budget_tokens":4294967296_u64}}),
        ),
        (
            "anthropic-budget-effort-conflict",
            DownstreamProtocol::Anthropic,
            json!({"model":"$REQUESTED_MODEL","max_tokens":64,"messages":[{"role":"user","content":"private-reasoning-marker"}],"thinking":{"type":"enabled","budget_tokens":1024},"output_config":{"effort":"high"}}),
        ),
    ] {
        let fixture = fixtures
            .iter()
            .find(|(_, fixture)| fixture.protocol == protocol)
            .map(|(_, fixture)| fixture.clone())
            .expect("Gemini target fixture for invalid reasoning case");
        let runtime_name = format!("r3-19-gemini-reasoning-zero-call-{case_name}");
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                json!({"candidates":[{"index":0,"finishReason":"STOP"}]}),
            )
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case_name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("reasoning rejection should read");
            assert!(!String::from_utf8_lossy(&body).contains("private-reasoning-marker"));
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty(), "{case_name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert!(matches!(
                log.final_error_code.as_deref(),
                Some("invalid_request_error" | "unsupported_capability_error")
            ));
            assert_eq!(log.total_tokens, None);
            assert_eq!(log.estimated_cost_nanos, None);
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_valid_reasoning_model_capability_error_is_one_upstream_call() {
    let (_, fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Openai)
        .expect("OpenAI to Gemini target fixture");
    run_case(
        "r3-19-gemini-reasoning-provider-reject",
        move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::BAD_REQUEST,
                content_type: Some("application/json".to_string()),
                content_encoding: None,
                body:
                    br#"{"error":{"code":400,"message":"thinking is not supported by this model"}}"#
                        .to_vec(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let response = router
                .send(
                    &fixture,
                    false,
                    &json!({
                        "model":"$REQUESTED_MODEL",
                        "messages":[{"role":"user","content":"reason"}],
                        "reasoning_effort":"high"
                    }),
                )
                .await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("provider capability error should read");
            assert_eq!(upstream.requests().await.len(), 1);
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(log.upstream_http_status, Some(400));
            assert_eq!(log.total_tokens, None);
            assert_eq!(log.estimated_cost_nanos, None);
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        },
    );
}

#[test]
fn r3_19_four_multimodal_cells_capture_only_native_gemini_inline_data_once() {
    let target = gemini_target_golden();
    let upstream_response = target.non_stream_response.clone();
    for request_case in target.requests {
        let advanced = request_case
            .advanced
            .iter()
            .find(|advanced| advanced.capability == GeminiFixtureCapability::Multimodal)
            .expect("Gemini multimodal fixture")
            .clone();
        let (_, fixture) = gemini_target_fixtures()
            .into_iter()
            .find(|(_, fixture)| fixture.protocol == request_case.downstream)
            .expect("Gemini target fixture for multimodal cell");
        let upstream_response = upstream_response.clone();
        let case_name = format!("r3-19-gemini-multimodal-{:?}", request_case.downstream);
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_response).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let transformed = crate::service::transform::transform_request_data(
                advanced.downstream_request.clone(),
                fixture.protocol,
                UpstreamProtocol::Gemini,
                false,
            )
            .expect("multimodal cell must be sendable");
            match advanced.status {
                GeminiFixtureCellStatus::ControlledLoss => {
                    let expected_reason = advanced
                        .expected_loss_reason
                        .as_deref()
                        .expect("controlled-loss media fixture reason");
                    assert!(transformed.summary.facts.iter().any(|fact| {
                        fact.reason_code.as_str() == expected_reason
                            && fact.outcome == TransformOutcomeKind::ControlledLossMinor
                            && fact.safe_summary.is_none()
                    }));
                }
                GeminiFixtureCellStatus::Full => {
                    assert!(transformed.summary.facts.iter().all(|fact| !matches!(
                        fact.outcome,
                        TransformOutcomeKind::ControlledLossMinor
                            | TransformOutcomeKind::ControlledLossMajor
                    )))
                }
            }

            let response = router
                .send(&fixture, false, &advanced.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("multimodal response should complete");
            let requests = upstream.requests().await;
            assert_eq!(requests.len(), 1, "{case_name}: no upload, probe, or retry");
            let body: Value = serde_json::from_slice(&requests[0].body)
                .expect("captured Gemini multimodal request should be JSON");
            assert_eq!(
                body,
                render_value(&advanced.expected_upstream, &router.requested_model()),
                "{case_name}"
            );
            if fixture.protocol != DownstreamProtocol::Gemini {
                for inline_data in body["contents"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .flat_map(|content| content["parts"].as_array().into_iter().flatten())
                    .filter_map(|part| part.get("inlineData"))
                {
                    assert!(inline_data.as_object().is_some_and(|object| {
                        object.len() == 2
                            && object.contains_key("mimeType")
                            && object.contains_key("data")
                    }));
                }
                assert!(!body.to_string().contains("fileData"));
            }
            router.wait_for_log(RequestStatus::Success).await;
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_invalid_multimodal_inputs_are_precredential_zero_call() {
    let fixtures = gemini_target_fixtures();
    for (case_name, protocol, request) in [
        (
            "remote-image",
            DownstreamProtocol::Openai,
            json!({"model":"$REQUESTED_MODEL","messages":[{"role":"user","content":[
                {"type":"image_url","image_url":{"url":"https://private.invalid/gemini-media-private-marker.png"}}
            ]}]}),
        ),
        (
            "provider-file-id",
            DownstreamProtocol::Responses,
            json!({"model":"$REQUESTED_MODEL","input":[{"role":"user","content":[
                {"type":"input_file","filename":"report.pdf","file_id":"gemini-media-private-marker"}
            ]}]}),
        ),
        (
            "video",
            DownstreamProtocol::Responses,
            json!({"model":"$REQUESTED_MODEL","input":[{"role":"user","content":[
                {"type":"input_video","video_url":"https://private.invalid/gemini-media-private-marker.mp4"}
            ]}]}),
        ),
        (
            "bad-base64-padding",
            DownstreamProtocol::Openai,
            json!({"model":"$REQUESTED_MODEL","messages":[{"role":"user","content":[
                {"type":"image_url","image_url":{"url":"data:image/png;base64,ZmFrZQ="}}
            ]}]}),
        ),
        (
            "assistant-role",
            DownstreamProtocol::Anthropic,
            json!({"model":"$REQUESTED_MODEL","max_tokens":64,"messages":[{"role":"assistant","content":[
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"AA=="}}
            ]}]}),
        ),
    ] {
        let fixture = fixtures
            .iter()
            .find(|(_, fixture)| fixture.protocol == protocol)
            .map(|(_, fixture)| fixture.clone())
            .expect("Gemini target fixture for invalid multimodal case");
        let runtime_name = format!("r3-19-gemini-multimodal-zero-call-{case_name}");
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                json!({"candidates":[{"index":0,"finishReason":"STOP"}]}),
            )
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case_name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("multimodal rejection should read");
            assert!(!String::from_utf8_lossy(&body).contains("gemini-media-private-marker"));
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty(), "{case_name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("gemini-media-private-marker")
            );
            assert_eq!(log.total_tokens, None);
            assert_eq!(log.estimated_cost_nanos, None);
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_four_structured_output_cells_capture_native_gemini_requests_once() {
    let target = gemini_target_golden();
    let upstream_response = target.non_stream_response.clone();
    for request_case in target.requests {
        let advanced = request_case
            .advanced
            .iter()
            .find(|advanced| advanced.capability == GeminiFixtureCapability::StructuredOutput)
            .expect("Gemini structured output fixture")
            .clone();
        let (_, fixture) = gemini_target_fixtures()
            .into_iter()
            .find(|(_, fixture)| fixture.protocol == request_case.downstream)
            .expect("Gemini target fixture for structured output cell");
        let upstream_response = upstream_response.clone();
        let case_name = format!(
            "r3-19-gemini-structured-output-{:?}",
            request_case.downstream
        );
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_response).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let transformed = crate::service::transform::transform_request_data(
                advanced.downstream_request.clone(),
                fixture.protocol,
                UpstreamProtocol::Gemini,
                false,
            )
            .expect("structured output cell must be sendable");
            match advanced.status {
                GeminiFixtureCellStatus::ControlledLoss => {
                    let expected_reason = advanced
                        .expected_loss_reason
                        .as_deref()
                        .expect("controlled-loss structured output reason");
                    assert!(transformed.summary.facts.iter().any(|fact| {
                        fact.reason_code.as_str() == expected_reason
                            && fact.outcome == TransformOutcomeKind::ControlledLossMinor
                            && fact.safe_summary.is_none()
                    }));
                }
                GeminiFixtureCellStatus::Full => {
                    assert!(transformed.summary.facts.iter().all(|fact| !matches!(
                        fact.outcome,
                        TransformOutcomeKind::ControlledLossMinor
                            | TransformOutcomeKind::ControlledLossMajor
                    )))
                }
            }

            let response = router
                .send(&fixture, false, &advanced.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("structured output response should complete");
            let requests = upstream.requests().await;
            assert_eq!(requests.len(), 1, "{case_name}");
            let body: Value = serde_json::from_slice(&requests[0].body)
                .expect("captured Gemini structured output request should be JSON");
            assert_eq!(
                body,
                render_value(&advanced.expected_upstream, &router.requested_model()),
                "{case_name}"
            );
            if fixture.protocol != DownstreamProtocol::Gemini {
                let config = &body["generationConfig"];
                assert_eq!(config["responseMimeType"], "application/json");
                assert!(config["responseJsonSchema"].is_object());
                assert!(config.get("responseSchema").is_none());
                assert!(config.get("responseFormat").is_none());
            }
            router.wait_for_log(RequestStatus::Success).await;
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_invalid_structured_outputs_are_precredential_zero_call() {
    let target = gemini_target_golden();
    let fixtures = gemini_target_fixtures();
    let mut cases = target
        .requests
        .into_iter()
        .map(|request_case| {
            let rejection = request_case
                .advanced
                .into_iter()
                .find(|advanced| advanced.capability == GeminiFixtureCapability::StructuredOutput)
                .expect("structured output fixture rejection")
                .rejection_request;
            ("fixture-reject", request_case.downstream, rejection)
        })
        .collect::<Vec<_>>();
    cases.extend([
        (
            "schema-array",
            DownstreamProtocol::Openai,
            json!({"model":"$REQUESTED_MODEL","messages":[{"role":"user","content":"private-structured-marker"}],
                "response_format":{"type":"json_schema","json_schema":{
                    "name":"answer","schema":[],"strict":true
                }}}),
        ),
        (
            "unknown-outer-metadata",
            DownstreamProtocol::Responses,
            json!({"model":"$REQUESTED_MODEL","input":"private-structured-marker","text":{"format":{
                "type":"json_schema","name":"answer","schema":{"type":"object"},
                "strict":true,"future":"private-structured-marker"
            }}}),
        ),
    ]);

    for (case_name, protocol, request) in cases {
        let fixture = fixtures
            .iter()
            .find(|(_, fixture)| fixture.protocol == protocol)
            .map(|(_, fixture)| fixture.clone())
            .expect("Gemini target fixture for invalid structured output case");
        let runtime_name = format!("r3-19-gemini-structured-zero-call-{protocol:?}-{case_name}");
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                json!({"candidates":[{"index":0,"finishReason":"STOP"}]}),
            )
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{protocol:?}/{case_name}"
            );
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("structured output rejection should read");
            assert!(!String::from_utf8_lossy(&body).contains("private-structured-marker"));
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(
                upstream.requests().await.is_empty(),
                "{protocol:?}/{case_name}"
            );
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("private-structured-marker")
            );
            assert_eq!(log.total_tokens, None);
            assert_eq!(log.estimated_cost_nanos, None);
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_structured_output_combines_with_tools_reasoning_media_and_stream_once() {
    let (_, fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Openai)
        .expect("OpenAI to Gemini target fixture");
    run_case(
        "r3-19-gemini-structured-combined-stream",
        move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Sse {
                events: vec![GoldenEvent {
                    event: None,
                    data: json!({
                        "responseId":"gemini-combined-response",
                        "candidates":[{"index":0,"content":{"role":"model","parts":[
                            {"text":"{\"answer\":\"ok\"}"}
                        ]},"finishReason":"STOP"}],
                        "usageMetadata":{
                            "promptTokenCount":5,"candidatesTokenCount":3,"totalTokenCount":8
                        }
                    }),
                }],
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let request = json!({
                "model":"$REQUESTED_MODEL","stream":true,
                "messages":[{"role":"user","content":[
                    {"type":"text","text":"inspect"},
                    {"type":"image_url","image_url":{"url":"data:image/png;base64,ZmFrZQ=="}}
                ]}],
                "tools":[{"type":"function","function":{
                    "name":"lookup","parameters":{"type":"object"},"strict":true
                }}],
                "reasoning_effort":"high",
                "response_format":{"type":"json_schema","json_schema":{
                    "name":"answer","schema":{"type":"object","properties":{
                        "answer":{"type":"string","x-future":true}
                    }},"strict":true
                }}
            });

            let response = router.send(&fixture, true, &request).await;

            assert_eq!(response.status(), StatusCode::OK);
            let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("combined stream response should complete");
            assert!(
                !String::from_utf8_lossy(&response_body).contains("error"),
                "{}",
                String::from_utf8_lossy(&response_body)
            );
            let requests = upstream.requests().await;
            assert_eq!(requests.len(), 1);
            let body: Value = serde_json::from_slice(&requests[0].body)
                .expect("combined Gemini request should be JSON");
            assert_eq!(
                body["generationConfig"]["responseMimeType"],
                "application/json"
            );
            assert_eq!(
                body["generationConfig"]["responseJsonSchema"]["properties"]["answer"]["x-future"],
                true
            );
            assert_eq!(
                body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
                "high"
            );
            assert!(body["tools"].is_array());
            assert!(
                body["contents"][0]["parts"]
                    .as_array()
                    .is_some_and(|parts| {
                        parts.iter().any(|part| part.get("inlineData").is_some())
                    })
            );
            router.wait_for_log(RequestStatus::Success).await;
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        },
    );
}

fn assert_gemini_advanced_cell(
    test_name: &'static str,
    protocol: DownstreamProtocol,
    capability: GeminiFixtureCapability,
) {
    let target = gemini_target_golden();
    let request_case = target
        .requests
        .into_iter()
        .find(|request| request.downstream == protocol)
        .expect("Gemini advanced request fixture for protocol");
    let advanced = request_case
        .advanced
        .into_iter()
        .find(|advanced| advanced.capability == capability)
        .expect("Gemini advanced capability fixture");
    let (_, fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == protocol)
        .expect("Gemini target fixture for advanced cell");
    let upstream_response = target.non_stream_response;

    run_case(test_name, move |context| async move {
        let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_response).await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let transformed = crate::service::transform::transform_request_data(
            advanced.downstream_request.clone(),
            protocol,
            UpstreamProtocol::Gemini,
            false,
        )
        .expect("Gemini advanced cell must be sendable");
        let controlled_facts = transformed
            .summary
            .facts
            .iter()
            .filter(|fact| {
                matches!(
                    fact.outcome,
                    TransformOutcomeKind::ControlledLossMinor
                        | TransformOutcomeKind::ControlledLossMajor
                )
            })
            .collect::<Vec<_>>();
        match advanced.status {
            GeminiFixtureCellStatus::ControlledLoss => {
                let expected_reason = advanced
                    .expected_loss_reason
                    .as_deref()
                    .expect("controlled-loss cell reason");
                assert!(
                    controlled_facts.iter().any(|fact| {
                        fact.reason_code.as_str() == expected_reason
                            && fact.outcome == TransformOutcomeKind::ControlledLossMinor
                    }),
                    "{test_name}: expected typed controlled loss {expected_reason}"
                );
                assert!(
                    controlled_facts
                        .iter()
                        .all(|fact| fact.safe_summary.is_none()),
                    "{test_name}: controlled-loss facts must be payload-free"
                );
            }
            GeminiFixtureCellStatus::Full => {
                assert!(
                    controlled_facts.is_empty(),
                    "{test_name}: same-wire full cell must not manufacture loss"
                );
                assert!(
                    transformed
                        .summary
                        .facts
                        .iter()
                        .all(|fact| fact.safe_summary.is_none()),
                    "{test_name}: same-wire facts remain payload-free"
                );
            }
        }

        let response = router
            .send(&fixture, false, &advanced.downstream_request)
            .await;

        assert_eq!(response.status(), StatusCode::OK, "{test_name}");
        assert_no_public_transform_diagnostics(&response);
        let request_id = assert_downstream_request_identity(&response);
        let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Gemini advanced response should complete");
        let response_body: Value = serde_json::from_slice(&response_body)
            .expect("Gemini advanced downstream response should be JSON");
        assert_eq!(
            gemini_non_stream_text(protocol, &response_body),
            Some("baseline pong"),
            "{test_name}: response path executes"
        );
        let captured = upstream.requests().await;
        assert_upstream(
            test_name,
            &fixture,
            &captured,
            &fixture.request.upstream_path,
            None,
            &advanced.expected_upstream,
            &router.requested_model(),
            &request_id,
        );
        assert_eq!(
            router.app_state.secret_encryption.decrypt_call_count(),
            1,
            "{test_name}: one credential resolution"
        );
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_eq!(log.request_id, request_id, "{test_name}");
        assert_log_common(&router, &fixture, &log);
        assert_usage(&log, &fixture.usage);
        assert!(log.final_error_code.is_none(), "{test_name}");
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1, "{test_name}");
        assert_eq!(captured.len(), 1, "{test_name}: no retry/probe/upload");
        upstream.shutdown().await;
    });
}

macro_rules! gemini_advanced_cell_test {
    ($name:ident, $protocol:expr, $capability:expr) => {
        #[test]
        fn $name() {
            assert_gemini_advanced_cell(stringify!($name), $protocol, $capability);
        }
    };
}

gemini_advanced_cell_test!(
    openai_to_gemini_tools_cell_has_typed_controlled_loss,
    DownstreamProtocol::Openai,
    GeminiFixtureCapability::Tools
);
gemini_advanced_cell_test!(
    responses_to_gemini_tools_cell_has_typed_controlled_loss,
    DownstreamProtocol::Responses,
    GeminiFixtureCapability::Tools
);
gemini_advanced_cell_test!(
    anthropic_to_gemini_tools_cell_has_typed_controlled_loss,
    DownstreamProtocol::Anthropic,
    GeminiFixtureCapability::Tools
);
gemini_advanced_cell_test!(
    gemini_to_gemini_tools_cell_is_full,
    DownstreamProtocol::Gemini,
    GeminiFixtureCapability::Tools
);
gemini_advanced_cell_test!(
    openai_to_gemini_reasoning_cell_has_typed_controlled_loss,
    DownstreamProtocol::Openai,
    GeminiFixtureCapability::Reasoning
);
gemini_advanced_cell_test!(
    responses_to_gemini_reasoning_cell_has_typed_controlled_loss,
    DownstreamProtocol::Responses,
    GeminiFixtureCapability::Reasoning
);
gemini_advanced_cell_test!(
    anthropic_to_gemini_reasoning_cell_has_typed_controlled_loss,
    DownstreamProtocol::Anthropic,
    GeminiFixtureCapability::Reasoning
);
gemini_advanced_cell_test!(
    gemini_to_gemini_reasoning_cell_is_full,
    DownstreamProtocol::Gemini,
    GeminiFixtureCapability::Reasoning
);
gemini_advanced_cell_test!(
    openai_to_gemini_multimodal_cell_has_typed_controlled_loss,
    DownstreamProtocol::Openai,
    GeminiFixtureCapability::Multimodal
);
gemini_advanced_cell_test!(
    responses_to_gemini_multimodal_cell_has_typed_controlled_loss,
    DownstreamProtocol::Responses,
    GeminiFixtureCapability::Multimodal
);
gemini_advanced_cell_test!(
    anthropic_to_gemini_multimodal_cell_has_typed_controlled_loss,
    DownstreamProtocol::Anthropic,
    GeminiFixtureCapability::Multimodal
);
gemini_advanced_cell_test!(
    gemini_to_gemini_multimodal_cell_is_full,
    DownstreamProtocol::Gemini,
    GeminiFixtureCapability::Multimodal
);
gemini_advanced_cell_test!(
    openai_to_gemini_structured_output_cell_has_typed_controlled_loss,
    DownstreamProtocol::Openai,
    GeminiFixtureCapability::StructuredOutput
);
gemini_advanced_cell_test!(
    responses_to_gemini_structured_output_cell_has_typed_controlled_loss,
    DownstreamProtocol::Responses,
    GeminiFixtureCapability::StructuredOutput
);
gemini_advanced_cell_test!(
    anthropic_to_gemini_structured_output_cell_has_typed_controlled_loss,
    DownstreamProtocol::Anthropic,
    GeminiFixtureCapability::StructuredOutput
);
gemini_advanced_cell_test!(
    gemini_to_gemini_structured_output_cell_is_full,
    DownstreamProtocol::Gemini,
    GeminiFixtureCapability::StructuredOutput
);

#[test]
fn r3_19_gemini_advanced_evidence_registry_has_16_unique_cells_and_4_full_12_loss() {
    const EVIDENCE: [(&str, &str, &str, &str); 16] = [
        (
            "openai",
            "tools",
            "controlled_loss",
            "proxy::direct_execution_regression::openai_to_gemini_tools_cell_has_typed_controlled_loss",
        ),
        (
            "responses",
            "tools",
            "controlled_loss",
            "proxy::direct_execution_regression::responses_to_gemini_tools_cell_has_typed_controlled_loss",
        ),
        (
            "anthropic",
            "tools",
            "controlled_loss",
            "proxy::direct_execution_regression::anthropic_to_gemini_tools_cell_has_typed_controlled_loss",
        ),
        (
            "gemini",
            "tools",
            "full",
            "proxy::direct_execution_regression::gemini_to_gemini_tools_cell_is_full",
        ),
        (
            "openai",
            "reasoning",
            "controlled_loss",
            "proxy::direct_execution_regression::openai_to_gemini_reasoning_cell_has_typed_controlled_loss",
        ),
        (
            "responses",
            "reasoning",
            "controlled_loss",
            "proxy::direct_execution_regression::responses_to_gemini_reasoning_cell_has_typed_controlled_loss",
        ),
        (
            "anthropic",
            "reasoning",
            "controlled_loss",
            "proxy::direct_execution_regression::anthropic_to_gemini_reasoning_cell_has_typed_controlled_loss",
        ),
        (
            "gemini",
            "reasoning",
            "full",
            "proxy::direct_execution_regression::gemini_to_gemini_reasoning_cell_is_full",
        ),
        (
            "openai",
            "multimodal",
            "controlled_loss",
            "proxy::direct_execution_regression::openai_to_gemini_multimodal_cell_has_typed_controlled_loss",
        ),
        (
            "responses",
            "multimodal",
            "controlled_loss",
            "proxy::direct_execution_regression::responses_to_gemini_multimodal_cell_has_typed_controlled_loss",
        ),
        (
            "anthropic",
            "multimodal",
            "controlled_loss",
            "proxy::direct_execution_regression::anthropic_to_gemini_multimodal_cell_has_typed_controlled_loss",
        ),
        (
            "gemini",
            "multimodal",
            "full",
            "proxy::direct_execution_regression::gemini_to_gemini_multimodal_cell_is_full",
        ),
        (
            "openai",
            "structured_output",
            "controlled_loss",
            "proxy::direct_execution_regression::openai_to_gemini_structured_output_cell_has_typed_controlled_loss",
        ),
        (
            "responses",
            "structured_output",
            "controlled_loss",
            "proxy::direct_execution_regression::responses_to_gemini_structured_output_cell_has_typed_controlled_loss",
        ),
        (
            "anthropic",
            "structured_output",
            "controlled_loss",
            "proxy::direct_execution_regression::anthropic_to_gemini_structured_output_cell_has_typed_controlled_loss",
        ),
        (
            "gemini",
            "structured_output",
            "full",
            "proxy::direct_execution_regression::gemini_to_gemini_structured_output_cell_is_full",
        ),
    ];

    let mut cells = BTreeSet::new();
    let mut references = BTreeSet::new();
    let mut full = 0;
    let mut controlled_loss = 0;
    for (downstream, capability, status, reference) in EVIDENCE {
        assert!(cells.insert((downstream, capability)));
        assert!(references.insert(reference));
        assert!(reference.starts_with("proxy::direct_execution_regression::"));
        match status {
            "full" => {
                assert_eq!(downstream, "gemini");
                full += 1;
            }
            "controlled_loss" => {
                assert_ne!(downstream, "gemini");
                controlled_loss += 1;
            }
            unexpected => panic!("unexpected Gemini advanced status: {unexpected}"),
        }
    }
    assert_eq!(cells.len(), 16);
    assert_eq!(references.len(), 16);
    assert_eq!((full, controlled_loss), (4, 12));
}

#[test]
fn r3_19_gemini_tool_and_reasoning_streams_preserve_ids_signatures_and_native_deltas() {
    let (_, fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Openai)
        .expect("OpenAI to Gemini advanced stream fixture");
    let events = vec![
        GoldenEvent {
            event: None,
            data: json!({
                "responseId":"gemini-advanced-stream-response",
                "candidates":[{"index":0,"content":{"role":"model","parts":[
                    {"text":"plan","thought":true,"thoughtSignature":"sig-reasoning"},
                    {"functionCall":{"id":"call-provider","name":"lookup","args":{"city":"Shanghai"}},"thoughtSignature":"sig-tool"}
                ]}}]
            }),
        },
        GoldenEvent {
            event: None,
            data: json!({
                "responseId":"gemini-advanced-stream-response",
                "candidates":[{"index":0,"content":{"role":"model","parts":[
                    {"text":" now","thought":true,"thoughtSignature":"sig-reasoning-tail"}
                ]},"finishReason":"STOP"}],
                "usageMetadata":{"promptTokenCount":4,"candidatesTokenCount":1,"thoughtsTokenCount":2,"totalTokenCount":7}
            }),
        },
    ];
    run_case(
        "r3-19-gemini-tool-reasoning-stream-ids-signatures",
        move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Sse {
                events: events.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let request = json!({
                "model":"$REQUESTED_MODEL","stream":true,
                "messages":[{"role":"user","content":"plan and call"}],
                "tools":[{"type":"function","function":{
                    "name":"lookup","parameters":{"type":"object"},"strict":true
                }}],
                "reasoning_effort":"high"
            });

            let response = router.send(&fixture, true, &request).await;

            assert_eq!(response.status(), StatusCode::OK);
            let request_id = assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("advanced Gemini stream response should complete");
            let downstream_events = parse_downstream_events(fixture.protocol, &body);
            let rendered = downstream_events
                .iter()
                .map(|event| event.data.to_string())
                .collect::<String>();
            assert!(rendered.contains("plan"));
            assert!(rendered.contains(" now"));
            assert!(rendered.contains("call-provider"));
            assert!(rendered.contains("lookup"));
            assert!(rendered.contains("Shanghai"));
            assert!(!rendered.contains("sig-reasoning"));
            assert!(!rendered.contains("sig-reasoning-tail"));
            assert!(!rendered.contains("sig-tool"));
            assert_eq!(
                downstream_events
                    .iter()
                    .filter(|event| event.data == "[DONE]")
                    .count(),
                1
            );
            assert_eq!(
                downstream_events
                    .iter()
                    .filter(|event| {
                        event.data.pointer("/choices/0/finish_reason") == Some(&json!("tool_calls"))
                    })
                    .count(),
                1
            );

            let captured = upstream.requests().await;
            assert_upstream(
                "r3-19-gemini-tool-reasoning-stream-ids-signatures",
                &fixture,
                &captured,
                &fixture.stream.upstream_path,
                fixture.stream.upstream_query.as_deref(),
                &json!({
                    "contents":[{"role":"user","parts":[{"text":"plan and call"}]}],
                    "generationConfig":{"candidateCount":1,"thinkingConfig":{"thinkingLevel":"high","includeThoughts":true}},
                    "tools":[{"functionDeclarations":[{"name":"lookup","parameters":{"type":"object"}}]}]
                }),
                &router.requested_model(),
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_log_common(&router, &fixture, &log);
            assert_eq!(log.total_input_tokens, Some(4));
            assert_eq!(log.total_output_tokens, Some(3));
            assert_eq!(log.total_tokens, Some(7));
            assert_eq!(log.reasoning_tokens, Some(2));
            assert!(log.final_error_code.is_none());
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1);
            assert_eq!(captured.len(), 1);
            upstream.shutdown().await;
        },
    );
}

#[test]
fn r3_19_gemini_nonstream_terminal_families_preserve_same_wire_bytes_and_cost_policy() {
    let (_, fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Gemini)
        .expect("native Gemini target fixture");
    let target = gemini_target_golden();

    for terminal_case in target.terminal_cases {
        let fixture = fixture.clone();
        let mut upstream_body = terminal_case.response;
        upstream_body["usageMetadata"] = json!({
            "promptTokenCount":11,
            "candidatesTokenCount":7,
            "totalTokenCount":18
        });
        upstream_body["futureResponseField"] = json!({"preserve":true});
        let raw = serde_json::to_vec_pretty(&upstream_body)
            .expect("Gemini terminal fixture should serialize");
        let expected_status = if matches!(
            terminal_case.terminal,
            GeminiFixtureTerminal::Stop
                | GeminiFixtureTerminal::MaxTokens
                | GeminiFixtureTerminal::Safety
                | GeminiFixtureTerminal::PromptBlock
        ) {
            RequestStatus::Success
        } else {
            RequestStatus::Error
        };
        let case_name = format!("r3-19-gemini-same-wire-{:?}", terminal_case.terminal);
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("application/json".to_string()),
                content_encoding: None,
                body: raw.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("same-wire Gemini terminal response should read");
            assert_eq!(response_body.as_ref(), raw.as_slice(), "{case_name}");
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            let log = router.wait_for_log(expected_status.clone()).await;
            if expected_status == RequestStatus::Success {
                assert_eq!(log.total_tokens, Some(18), "{case_name}");
                assert!(log.estimated_cost_nanos.is_some(), "{case_name}");
            } else {
                assert_eq!(log.total_input_tokens, None, "{case_name}");
                assert_eq!(log.total_output_tokens, None, "{case_name}");
                assert_eq!(log.total_tokens, None, "{case_name}");
                assert_eq!(log.estimated_cost_nanos, None, "{case_name}");
                assert_eq!(log.cost_snapshot_json, None, "{case_name}");
                assert_eq!(
                    log.final_error_code.as_deref(),
                    Some("upstream_response_error"),
                    "{case_name}"
                );
            }
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }

    let case_name = "r3-19-gemini-same-wire-malformed-json";
    run_case(case_name, move |context| async move {
        let raw = b"{private-gemini-malformed}".to_vec();
        let upstream = TestUpstream::spawn(ScriptedReply::Raw {
            status: StatusCode::OK,
            content_type: Some("application/json".to_string()),
            content_encoding: None,
            body: raw.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        router.attach_cost_catalog(Some(100), Some(2)).await;

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("malformed same-wire Gemini response should read");
        assert_eq!(response_body.as_ref(), raw.as_slice());
        assert_eq!(upstream.requests().await.len(), 1);
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(log.total_tokens, None);
        assert_eq!(log.estimated_cost_nanos, None);
        assert_eq!(log.cost_snapshot_json, None);
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("upstream_response_error")
        );
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn r3_19_gemini_cross_wire_terminal_mappings_are_committed_once() {
    for (protocol, upstream_body, pointer, expected) in [
        (
            DownstreamProtocol::Openai,
            json!({
                "responseId":"gemini-tool-terminal",
                "candidates":[{"index":0,"content":{"role":"model","parts":[
                    {"text":"thinking","thought":true,"thoughtSignature":"sig-thinking"},
                    {"functionCall":{"name":"lookup","args":{"q":"safe"}},
                     "thoughtSignature":"sig-tool"}
                ]},"finishReason":"STOP"}]
            }),
            "/choices/0/finish_reason",
            "tool_calls",
        ),
        (
            DownstreamProtocol::Responses,
            json!({"promptFeedback":{"blockReason":"JAILBREAK"}}),
            "/incomplete_details/reason",
            "content_filter",
        ),
        (
            DownstreamProtocol::Anthropic,
            json!({"candidates":[{"index":0,"finishReason":"MODEL_ARMOR"}]}),
            "/stop_reason",
            "refusal",
        ),
    ] {
        let (_, fixture) = gemini_target_fixtures()
            .into_iter()
            .find(|(_, fixture)| fixture.protocol == protocol)
            .expect("cross-wire Gemini fixture");
        let case_name = format!("r3-19-gemini-terminal-mapping-{protocol:?}");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_body).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("mapped Gemini response should read");
            let body: Value = serde_json::from_slice(&body).expect("mapped response JSON");
            assert_eq!(
                body.pointer(pointer).and_then(Value::as_str),
                Some(expected),
                "{case_name}"
            );
            if protocol == DownstreamProtocol::Openai {
                assert_eq!(
                    body.pointer("/choices/0/message/reasoning_content"),
                    Some(&json!("thinking"))
                );
                assert_eq!(
                    body.pointer("/choices/0/message/tool_calls/0/function/name"),
                    Some(&json!("lookup"))
                );
                assert!(
                    body.pointer("/choices/0/message/tool_calls/0/id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| id.starts_with("gemini-call-"))
                );
            }
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            router.wait_for_log(RequestStatus::Success).await;
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_gemini_cross_wire_failed_and_unconfirmed_terminals_are_preheader_502_without_cost() {
    const PRIVATE_MARKER: &str = "private-gemini-terminal-marker";
    let cases = [
        (
            "application-failure",
            Some(json!({"candidates":[{"index":0,
                "finishReason":"MALFORMED_FUNCTION_CALL"}],
                "usageMetadata":{"promptTokenCount":11,"candidatesTokenCount":7,
                    "totalTokenCount":18},
                "futureResponseField":PRIVATE_MARKER})),
        ),
        (
            "unknown",
            Some(
                json!({"candidates":[{"index":0,"finishReason":"FUTURE_REASON"}],
                "usageMetadata":{"promptTokenCount":11,"candidatesTokenCount":7,
                    "totalTokenCount":18},
                "futureResponseField":PRIVATE_MARKER}),
            ),
        ),
        (
            "missing",
            Some(json!({"candidates":[{"index":0}],
                "usageMetadata":{"promptTokenCount":11,"candidatesTokenCount":7,
                    "totalTokenCount":18},
                "futureResponseField":PRIVATE_MARKER})),
        ),
        (
            "multi-candidate",
            Some(json!({"candidates":[
                {"index":0,"finishReason":"STOP"},
                {"index":1,"finishReason":"STOP"}],
                "futureResponseField":PRIVATE_MARKER})),
        ),
        (
            "invalid-index",
            Some(json!({"candidates":[{"index":7,"finishReason":"STOP"}],
                "futureResponseField":PRIVATE_MARKER})),
        ),
        ("malformed-json", None),
    ];

    for (name, fixture) in gemini_target_fixtures()
        .into_iter()
        .filter(|(_, fixture)| fixture.protocol != DownstreamProtocol::Gemini)
    {
        for (terminal_name, upstream_body) in cases.clone() {
            let fixture = fixture.clone();
            let case_name = format!("r3-19-gemini-{name}-{terminal_name}-preheader");
            let runtime_name = case_name.clone();
            run_case(&runtime_name, move |context| async move {
                let upstream = match upstream_body {
                    Some(body) => TestUpstream::spawn_json(StatusCode::OK, body).await,
                    None => {
                        TestUpstream::spawn(ScriptedReply::Raw {
                            status: StatusCode::OK,
                            content_type: Some("application/json".to_string()),
                            content_encoding: None,
                            body: format!("{{{PRIVATE_MARKER}").into_bytes(),
                        })
                        .await
                    }
                };
                let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
                router.attach_cost_catalog(Some(100), Some(2)).await;

                let response = router
                    .send(&fixture, false, &fixture.request.downstream)
                    .await;

                assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{case_name}");
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("Gemini terminal error envelope should read");
                assert_payload_free_transform_bytes(&body, PRIVATE_MARKER);
                let body: Value = serde_json::from_slice(&body).expect("error envelope JSON");
                assert_eq!(
                    downstream_error_code(&body, fixture.protocol),
                    Some("upstream_response_error"),
                    "{case_name}"
                );
                assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
                let log = router.wait_for_log(RequestStatus::Error).await;
                assert_eq!(log.upstream_http_status, Some(200), "{case_name}");
                assert_eq!(
                    log.final_error_code.as_deref(),
                    Some("upstream_response_error"),
                    "{case_name}"
                );
                assert!(
                    !log.final_error_message
                        .as_deref()
                        .unwrap_or_default()
                        .contains(PRIVATE_MARKER),
                    "{case_name}"
                );
                assert_eq!(log.total_input_tokens, None, "{case_name}");
                assert_eq!(log.total_output_tokens, None, "{case_name}");
                assert_eq!(log.total_tokens, None, "{case_name}");
                assert_eq!(log.estimated_cost_nanos, None, "{case_name}");
                assert_eq!(log.cost_snapshot_json, None, "{case_name}");
                router.wait_for_api_key_lease_release().await;
                assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
                upstream.shutdown().await;
            });
        }
    }
}

#[test]
fn r3_19_gemini_nonstream_and_stream_usage_normalize_to_same_cost_once() {
    let target = gemini_target_golden();
    let (_, fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Openai)
        .expect("OpenAI to Gemini usage fixture");

    for is_stream in [false, true] {
        let fixture = fixture.clone();
        let non_stream_response = target.non_stream_response.clone();
        let stream_events = target.stream_events.clone();
        let case_name = format!(
            "r3-19-gemini-usage-{}",
            if is_stream { "stream" } else { "nonstream" }
        );
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = if is_stream {
                TestUpstream::spawn(ScriptedReply::Sse {
                    events: stream_events,
                })
                .await
            } else {
                TestUpstream::spawn_json(StatusCode::OK, non_stream_response).await
            };
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let (catalog_id, catalog_version_id) =
                router.attach_cost_catalog(Some(100), Some(2)).await;
            let request = if is_stream {
                &fixture.stream.downstream_request
            } else {
                &fixture.request.downstream
            };

            let response = router.send(&fixture, is_stream, request).await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("Gemini usage response should complete");
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(log.total_input_tokens, Some(12), "{case_name}");
            assert_eq!(log.total_output_tokens, Some(9), "{case_name}");
            assert_eq!(log.total_tokens, Some(21), "{case_name}");
            assert_eq!(log.input_text_tokens, Some(9), "{case_name}");
            assert_eq!(log.output_text_tokens, Some(7), "{case_name}");
            assert_eq!(log.input_image_tokens, Some(0), "{case_name}");
            assert_eq!(log.output_image_tokens, Some(0), "{case_name}");
            assert_eq!(log.cache_read_tokens, Some(3), "{case_name}");
            assert_eq!(log.reasoning_tokens, Some(2), "{case_name}");
            assert_eq!(log.estimated_cost_nanos, Some(124), "{case_name}");
            assert_eq!(log.cost_catalog_id, Some(catalog_id), "{case_name}");
            assert_eq!(
                log.cost_catalog_version_id,
                Some(catalog_version_id),
                "{case_name}"
            );
            let snapshot: CostSnapshot = serde_json::from_str(
                log.cost_snapshot_json
                    .as_deref()
                    .expect("Gemini usage cost snapshot should persist"),
            )
            .expect("Gemini usage cost snapshot should parse");
            assert_eq!(snapshot.total_cost_nanos, 124, "{case_name}");
            assert_eq!(snapshot.detail_lines.len(), 3, "{case_name}");
            assert_eq!(
                snapshot.detail_lines[0].meter_key,
                MeterKey::LlmInputTextTokens
            );
            assert_eq!(snapshot.detail_lines[0].quantity, 9);
            assert_eq!(
                snapshot.detail_lines[1].meter_key,
                MeterKey::LlmCacheReadTokens
            );
            assert_eq!(snapshot.detail_lines[1].quantity, 3);
            assert_eq!(
                snapshot.detail_lines[2].meter_key,
                MeterKey::InvokeRequestCalls
            );
            assert_eq!(snapshot.detail_lines[2].quantity, 1);
            assert_eq!(
                snapshot.unmatched_items,
                vec![
                    MeterKey::LlmOutputTextTokens.to_string(),
                    MeterKey::LlmReasoningTokens.to_string(),
                ],
                "{case_name}"
            );
            assert_eq!(snapshot.warnings.len(), 2, "{case_name}");
            assert!(
                snapshot
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("llm.output_text_tokens")),
                "{case_name}"
            );
            assert!(
                snapshot
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("llm.reasoning_tokens")),
                "{case_name}"
            );
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_gemini_invalid_usage_is_raw_same_wire_cross_wire_502_and_never_billed() {
    const PRIVATE_MARKER: &str = "private-gemini-usage-marker";
    for protocol in [DownstreamProtocol::Gemini, DownstreamProtocol::Openai] {
        let (_, fixture) = gemini_target_fixtures()
            .into_iter()
            .find(|(_, fixture)| fixture.protocol == protocol)
            .expect("Gemini usage boundary fixture");
        let upstream_body = json!({
            "candidates":[{"index":0,"content":{"role":"model","parts":[
                {"text":"ok"}
            ]},"finishReason":"STOP"}],
            "usageMetadata":{
                "promptTokenCount":3,
                "candidatesTokenCount":0,
                "cachedContentTokenCount":4,
                "totalTokenCount":3
            },
            "futureResponseField":PRIVATE_MARKER
        });
        let raw = serde_json::to_vec_pretty(&upstream_body)
            .expect("invalid Gemini usage fixture serializes");
        let case_name = format!("r3-19-gemini-invalid-usage-{protocol:?}");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("application/json".to_string()),
                content_encoding: None,
                body: raw.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            if protocol == DownstreamProtocol::Gemini {
                assert_eq!(response.status(), StatusCode::OK, "{case_name}");
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("same-wire invalid usage should read");
                assert_eq!(body.as_ref(), raw.as_slice(), "{case_name}");
            } else {
                assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{case_name}");
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("cross-wire invalid usage error should read");
                assert_payload_free_transform_bytes(&body, PRIVATE_MARKER);
                let body: Value = serde_json::from_slice(&body).expect("error envelope JSON");
                assert_eq!(
                    downstream_error_code(&body, protocol),
                    Some("upstream_response_error")
                );
            }
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            let expected_status = if protocol == DownstreamProtocol::Gemini {
                RequestStatus::Success
            } else {
                RequestStatus::Error
            };
            let log = router.wait_for_log(expected_status).await;
            assert_eq!(log.total_input_tokens, None, "{case_name}");
            assert_eq!(log.total_output_tokens, None, "{case_name}");
            assert_eq!(log.total_tokens, None, "{case_name}");
            assert_eq!(log.estimated_cost_nanos, None, "{case_name}");
            assert_eq!(log.cost_catalog_version_id, None, "{case_name}");
            assert_eq!(log.cost_snapshot_json, None, "{case_name}");
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains(PRIVATE_MARKER),
                "{case_name}"
            );
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_gemini_success_without_usage_charges_only_one_invocation() {
    let (_, fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Openai)
        .expect("OpenAI to Gemini missing usage fixture");
    run_case(
        "r3-19-gemini-missing-usage-invocation",
        move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                json!({"candidates":[{"index":0,"content":{"role":"model","parts":[
                {"text":"partial"}
            ]},"finishReason":"MAX_TOKENS"}]}),
            )
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let (catalog_id, catalog_version_id) =
                router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), StatusCode::OK);
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("missing usage response should read");
            assert_eq!(upstream.requests().await.len(), 1);
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(log.total_tokens, None);
            assert_eq!(log.estimated_cost_nanos, Some(100));
            assert_eq!(log.cost_catalog_id, Some(catalog_id));
            assert_eq!(log.cost_catalog_version_id, Some(catalog_version_id));
            let snapshot: CostSnapshot = serde_json::from_str(
                log.cost_snapshot_json
                    .as_deref()
                    .expect("invocation-only snapshot should persist"),
            )
            .expect("invocation-only snapshot should parse");
            assert_eq!(snapshot.total_cost_nanos, 100);
            assert_eq!(snapshot.detail_lines.len(), 1);
            assert_eq!(
                snapshot.detail_lines[0].meter_key,
                MeterKey::InvokeRequestCalls
            );
            assert_eq!(snapshot.detail_lines[0].quantity, 1);
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        },
    );
}

#[test]
fn r3_19_gemini_reported_total_mismatch_is_preserved_warned_and_costed_from_components() {
    let (_, fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Openai)
        .expect("OpenAI to Gemini total mismatch fixture");
    run_case("r3-19-gemini-total-mismatch", move |context| async move {
        let upstream = TestUpstream::spawn_json(
            StatusCode::OK,
            json!({
                "candidates":[{"index":0,"content":{"role":"model","parts":[
                    {"text":"ok"}
                ]},"finishReason":"STOP"}],
                "usageMetadata":{
                    "promptTokenCount":11,
                    "candidatesTokenCount":7,
                    "thoughtsTokenCount":2,
                    "toolUsePromptTokenCount":1,
                    "totalTokenCount":999
                }
            }),
        )
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        router.attach_cost_catalog(Some(100), Some(2)).await;

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("total mismatch response should read");
        let body: Value = serde_json::from_slice(&body).expect("OpenAI response JSON");
        assert_eq!(body.pointer("/usage/total_tokens"), Some(&json!(999)));
        assert_eq!(upstream.requests().await.len(), 1);
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_eq!(log.total_input_tokens, Some(12));
        assert_eq!(log.total_output_tokens, Some(9));
        assert_eq!(log.total_tokens, Some(21));
        assert_eq!(log.estimated_cost_nanos, Some(124));
        let snapshot: CostSnapshot = serde_json::from_str(
            log.cost_snapshot_json
                .as_deref()
                .expect("mismatch cost snapshot should persist"),
        )
        .expect("mismatch cost snapshot should parse");
        assert_eq!(snapshot.total_cost_nanos, 124);
        assert_eq!(snapshot.warnings.len(), 3);
        assert!(snapshot.warnings.iter().any(|warning| {
            warning.contains("reported total_tokens 999") && warning.contains("21")
        }));
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn r3_19_gemini_same_wire_stream_usage_regression_is_raw_and_unbillable() {
    const PRIVATE_MARKER: &str = "private-gemini-regressed-usage";
    let (_, fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Gemini)
        .expect("native Gemini stream usage fixture");
    run_case(
        "r3-19-gemini-stream-usage-regression",
        move |context| async move {
            let events = vec![
                GoldenEvent {
                    event: None,
                    data: json!({
                        "responseId":"gemini-regression",
                        "candidates":[{"index":0,"content":{"role":"model","parts":[
                            {"text":"ok"}
                        ]},"finishReason":"STOP"}],
                        "usageMetadata":{
                            "promptTokenCount":11,"candidatesTokenCount":7,
                            "thoughtsTokenCount":2,"toolUsePromptTokenCount":1,
                            "totalTokenCount":21
                        }
                    }),
                },
                GoldenEvent {
                    event: None,
                    data: json!({
                        "usageMetadata":{
                            "promptTokenCount":11,"candidatesTokenCount":6,
                            "thoughtsTokenCount":2,"toolUsePromptTokenCount":1,
                            "totalTokenCount":20,
                            "futureUsageField":PRIVATE_MARKER
                        }
                    }),
                },
            ];
            let upstream = TestUpstream::spawn(ScriptedReply::Sse { events }).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("same-wire regressed usage stream should complete");
            let body = String::from_utf8_lossy(&body);
            assert!(body.contains(PRIVATE_MARKER));
            assert!(!body.contains("[DONE]"));
            assert_eq!(upstream.requests().await.len(), 1);
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(log.total_input_tokens, None);
            assert_eq!(log.total_output_tokens, None);
            assert_eq!(log.total_tokens, None);
            assert_eq!(log.estimated_cost_nanos, None);
            assert_eq!(log.cost_catalog_version_id, None);
            assert_eq!(log.cost_snapshot_json, None);
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        },
    );
}

#[test]
fn r3_19_gemini_stream_state_machine_completes_four_targets_once_after_chunk_split_eof() {
    let events = vec![
        GoldenEvent {
            event: None,
            data: json!({
                "responseId":"gemini-direct-state-machine",
                "candidates":[{"index":0,"content":{"role":"model","parts":[
                    {"text":"hello "},{"text":"world"}
                ]}}]
            }),
        },
        GoldenEvent {
            event: None,
            data: json!({
                "responseId":"gemini-direct-state-machine",
                "candidates":[{"index":0,"finishReason":"STOP"}]
            }),
        },
        GoldenEvent {
            event: None,
            data: json!({
                "responseId":"gemini-direct-state-machine",
                "usageMetadata":{
                    "promptTokenCount":3,
                    "candidatesTokenCount":2,
                    "totalTokenCount":5
                }
            }),
        },
    ];

    for (name, fixture) in gemini_target_fixtures() {
        let case_name = format!("r3-19-gemini-stream-state-machine-{name}");
        let runtime_name = case_name.clone();
        let events = events.clone();
        run_case(&runtime_name, move |context| async move {
            let wire = events_to_sse_bytes(&events);
            let chunks = wire.chunks(7).map(<[u8]>::to_vec).collect::<Vec<_>>();
            let upstream = TestUpstream::spawn(ScriptedReply::ChunkedSse {
                content_encoding: None,
                chunks,
                hang_after_chunks: false,
                dropped: None,
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let request_id = assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("legal Gemini stream should complete after EOF");
            let downstream_events = parse_downstream_events(fixture.protocol, &body);
            match fixture.protocol {
                DownstreamProtocol::Openai => {
                    assert_eq!(
                        downstream_events
                            .iter()
                            .filter(|event| event.data == json!("[DONE]"))
                            .count(),
                        1,
                        "{case_name}: exactly one OpenAI done marker"
                    );
                    assert_eq!(
                        downstream_events
                            .iter()
                            .filter(|event| {
                                event.data.pointer("/choices/0/finish_reason")
                                    == Some(&json!("stop"))
                            })
                            .count(),
                        1,
                        "{case_name}: exactly one OpenAI stop"
                    );
                }
                DownstreamProtocol::Responses => assert_eq!(
                    downstream_events
                        .iter()
                        .filter(|event| event.data["type"] == "response.completed")
                        .count(),
                    1,
                    "{case_name}: exactly one Responses terminal"
                ),
                DownstreamProtocol::Anthropic => assert_eq!(
                    downstream_events
                        .iter()
                        .filter(|event| event.event.as_deref() == Some("message_stop"))
                        .count(),
                    1,
                    "{case_name}: exactly one Anthropic terminal"
                ),
                DownstreamProtocol::Gemini => {
                    assert_eq!(downstream_events, events, "{case_name}: raw Gemini events");
                    assert!(
                        !String::from_utf8_lossy(&body).contains("[DONE]"),
                        "{case_name}: Gemini must not receive an OpenAI marker"
                    );
                }
            }
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(log.request_id, request_id, "{case_name}");
            assert_eq!(log.total_input_tokens, Some(3), "{case_name}");
            assert_eq!(log.total_output_tokens, Some(2), "{case_name}");
            assert_eq!(log.total_tokens, Some(5), "{case_name}");
            assert!(log.final_error_code.is_none(), "{case_name}");
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_gemini_stream_prompt_block_completes_four_targets_with_one_safety_terminal() {
    let prompt_block = GoldenEvent {
        event: None,
        data: json!({
            "responseId":"gemini-direct-prompt-block",
            "promptFeedback":{"blockReason":"SAFETY","safetyRatings":[]},
            "usageMetadata":{"promptTokenCount":3,"totalTokenCount":3}
        }),
    };

    for (name, fixture) in gemini_target_fixtures() {
        let case_name = format!("r3-19-gemini-stream-prompt-block-{name}");
        let runtime_name = case_name.clone();
        let prompt_block = prompt_block.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Sse {
                events: vec![prompt_block.clone()],
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("Gemini prompt block stream should complete at EOF");
            let downstream_events = parse_downstream_events(fixture.protocol, &body);
            match fixture.protocol {
                DownstreamProtocol::Openai => {
                    assert_eq!(
                        downstream_events
                            .iter()
                            .filter(|event| {
                                event.data.pointer("/choices/0/finish_reason")
                                    == Some(&json!("content_filter"))
                            })
                            .count(),
                        1,
                        "{case_name}"
                    );
                    assert_eq!(
                        downstream_events
                            .iter()
                            .filter(|event| event.data == "[DONE]")
                            .count(),
                        1,
                        "{case_name}"
                    );
                }
                DownstreamProtocol::Responses => assert_eq!(
                    downstream_events
                        .iter()
                        .filter(|event| event.data["type"] == "response.incomplete")
                        .count(),
                    1,
                    "{case_name}"
                ),
                DownstreamProtocol::Anthropic => {
                    assert_eq!(
                        downstream_events
                            .iter()
                            .filter(|event| event.event.as_deref() == Some("message_stop"))
                            .count(),
                        1,
                        "{case_name}"
                    );
                    assert!(
                        downstream_events
                            .iter()
                            .any(|event| event.data["delta"]["stop_reason"] == "refusal"),
                        "{case_name}"
                    );
                }
                DownstreamProtocol::Gemini => {
                    assert_eq!(downstream_events, vec![prompt_block], "{case_name}");
                    assert!(!String::from_utf8_lossy(&body).contains("[DONE]"));
                }
            }
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(log.total_input_tokens, Some(3), "{case_name}");
            assert_eq!(log.total_output_tokens, Some(0), "{case_name}");
            assert_eq!(log.total_tokens, Some(3), "{case_name}");
            assert!(log.final_error_code.is_none(), "{case_name}");
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_gemini_cross_wire_illegal_stream_sequences_emit_one_fatal_without_success_terminal() {
    const PRIVATE_MARKER: &str = "private-gemini-illegal-stream-marker";
    let (_, fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Openai)
        .expect("OpenAI to Gemini stream state fixture");
    let finish = GoldenEvent {
        event: None,
        data: json!({"candidates":[{"index":0,"finishReason":"STOP"}]}),
    };
    let content = GoldenEvent {
        event: None,
        data: json!({"candidates":[{"index":0,"content":{"role":"model","parts":[
            {"text":"partial"}
        ]}}]}),
    };
    let cases = vec![
        ("empty-stream", Vec::new()),
        ("missing-terminal-eof", vec![content.clone()]),
        ("duplicate-terminal", vec![finish.clone(), finish.clone()]),
        ("post-terminal-content", vec![finish.clone(), content]),
        (
            "multiple-candidates",
            vec![GoldenEvent {
                event: None,
                data: json!({"candidates":[{"index":0},{"index":1}]}),
            }],
        ),
        (
            "candidate-index-conflict",
            vec![GoldenEvent {
                event: None,
                data: json!({"candidates":[{"index":1,"finishReason":"STOP"}]}),
            }],
        ),
        (
            "unknown-finish",
            vec![GoldenEvent {
                event: None,
                data: json!({"candidates":[{"index":0,"finishReason":"FUTURE_STOP"}]}),
            }],
        ),
        (
            "unknown-part",
            vec![GoldenEvent {
                event: None,
                data: json!({"candidates":[{"index":0,"content":{"role":"model","parts":[
                    {"futurePart":{"private":PRIVATE_MARKER}}
                ]}}]}),
            }],
        ),
        (
            "malformed-json",
            vec![GoldenEvent {
                event: None,
                data: json!("{not-json}"),
            }],
        ),
        (
            "openai-done-marker",
            vec![GoldenEvent {
                event: None,
                data: json!("[DONE]"),
            }],
        ),
        (
            "usage-decrease",
            vec![
                GoldenEvent {
                    event: None,
                    data: json!({"usageMetadata":{
                        "promptTokenCount":3,"candidatesTokenCount":2,"totalTokenCount":5
                    }}),
                },
                GoldenEvent {
                    event: None,
                    data: json!({"usageMetadata":{
                        "promptTokenCount":3,"candidatesTokenCount":1,"totalTokenCount":4
                    }}),
                },
            ],
        ),
    ];

    for (case, events) in cases {
        let fixture = fixture.clone();
        let case_name = format!("r3-19-gemini-illegal-stream-{case}");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Sse { events }).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let request_id = assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("guarded stream failure should close the body");
            assert_payload_free_transform_bytes(&body, PRIVATE_MARKER);
            let downstream_events = parse_downstream_events(fixture.protocol, &body);
            let terminal = downstream_events.last().expect("native fatal terminal");
            assert_native_fatal_stream_event(fixture.protocol, terminal, &request_id);
            assert_eq!(
                downstream_events
                    .iter()
                    .filter(|event| event.data["error"]["code"] == "upstream_response_error")
                    .count(),
                1,
                "{case_name}: exactly one failure terminal"
            );
            assert!(
                downstream_events.iter().all(|event| event.data != "[DONE]"
                    && event
                        .data
                        .pointer("/choices/0/finish_reason")
                        .is_none_or(Value::is_null)),
                "{case_name}: illegal sequence must not commit a success terminal"
            );
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_stream_failure_has_no_usage_or_cost(&log, &case_name);
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains(PRIVATE_MARKER),
                "{case_name}"
            );
            router.wait_for_api_key_lease_release().await;
            router.assert_no_api_key_usage_charge(&case_name).await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_gemini_same_wire_unknown_stream_is_raw_then_fails_once_without_cost() {
    const PRIVATE_MARKER: &str = "private-gemini-same-wire-stream-marker";
    let (_, fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Gemini)
        .expect("native Gemini stream state fixture");
    run_case(
        "r3-19-gemini-same-wire-unknown-stream",
        move |context| async move {
            let events = vec![
                GoldenEvent {
                    event: None,
                    data: json!({"candidates":[{"index":0,"content":{"role":"model","parts":[
                        {"futurePart":{"private":PRIVATE_MARKER}}
                    ]}}]}),
                },
                GoldenEvent {
                    event: None,
                    data: json!({"candidates":[{"index":0,"finishReason":"STOP"}]}),
                },
            ];
            let upstream = TestUpstream::spawn(ScriptedReply::Sse {
                events: events.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK);
            let request_id = assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("same-wire degraded stream should close with one native error");
            let downstream_events = parse_downstream_events(fixture.protocol, &body);
            assert_eq!(&downstream_events[..2], events.as_slice());
            assert!(String::from_utf8_lossy(&body).contains(PRIVATE_MARKER));
            assert!(!String::from_utf8_lossy(&body).contains("[DONE]"));
            assert_eq!(downstream_events.len(), 3);
            assert_native_fatal_stream_event(
                fixture.protocol,
                downstream_events.last().expect("one fatal tail"),
                &request_id,
            );
            assert_eq!(upstream.requests().await.len(), 1);
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_stream_failure_has_no_usage_or_cost(
                &log,
                "r3-19-gemini-same-wire-unknown-stream",
            );
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains(PRIVATE_MARKER)
            );
            router.wait_for_api_key_lease_release().await;
            router
                .assert_no_api_key_usage_charge("same-wire unknown Gemini stream")
                .await;
            assert_eq!(router.request_logs().await.len(), 1);
            upstream.shutdown().await;
        },
    );
}

#[test]
fn r3_19_gemini_stream_application_failures_are_raw_same_wire_and_safe_cross_wire() {
    const PRIVATE_MARKER: &str = "private-gemini-stream-application-marker";
    for (name, fixture) in gemini_target_fixtures() {
        let case_name = format!("r3-19-gemini-stream-application-failure-{name}");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let failed = GoldenEvent {
                event: None,
                data: json!({
                    "responseId":"gemini-stream-application-failure",
                    "candidates":[{"index":0,"finishReason":"OTHER"}],
                    "private":PRIVATE_MARKER
                }),
            };
            let upstream = TestUpstream::spawn(ScriptedReply::Sse {
                events: vec![failed.clone()],
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let request_id = assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("application failure stream should close");
            let downstream_events = parse_downstream_events(fixture.protocol, &body);
            if fixture.protocol == DownstreamProtocol::Gemini {
                assert_eq!(
                    downstream_events,
                    vec![failed],
                    "{case_name}: raw same-wire"
                );
                assert!(String::from_utf8_lossy(&body).contains(PRIVATE_MARKER));
            } else {
                assert_eq!(downstream_events.len(), 1, "{case_name}");
                assert_native_fatal_stream_event(
                    fixture.protocol,
                    &downstream_events[0],
                    &request_id,
                );
                assert_payload_free_transform_bytes(&body, PRIVATE_MARKER);
            }
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_stream_failure_has_no_usage_or_cost(&log, &case_name);
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains(PRIVATE_MARKER),
                "{case_name}"
            );
            router.wait_for_api_key_lease_release().await;
            router.assert_no_api_key_usage_charge(&case_name).await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

fn gemini_non_stream_text(protocol: DownstreamProtocol, body: &Value) -> Option<&str> {
    let pointer = match protocol {
        DownstreamProtocol::Openai => "/choices/0/message/content",
        DownstreamProtocol::Responses => "/output/0/content/0/text",
        DownstreamProtocol::Anthropic => "/content/0/text",
        DownstreamProtocol::Gemini => "/candidates/0/content/parts/0/text",
    };
    body.pointer(pointer).and_then(Value::as_str)
}

fn assert_gemini_non_stream_usage(protocol: DownstreamProtocol, body: &Value, case_name: &str) {
    match protocol {
        DownstreamProtocol::Openai => {
            assert_eq!(
                body.pointer("/usage/prompt_tokens"),
                Some(&json!(12)),
                "{case_name}"
            );
            assert_eq!(
                body.pointer("/usage/completion_tokens"),
                Some(&json!(9)),
                "{case_name}"
            );
            assert_eq!(
                body.pointer("/usage/total_tokens"),
                Some(&json!(21)),
                "{case_name}"
            );
        }
        DownstreamProtocol::Responses => {
            assert_eq!(
                body.pointer("/usage/input_tokens"),
                Some(&json!(12)),
                "{case_name}"
            );
            assert_eq!(
                body.pointer("/usage/output_tokens"),
                Some(&json!(9)),
                "{case_name}"
            );
            assert_eq!(
                body.pointer("/usage/total_tokens"),
                Some(&json!(21)),
                "{case_name}"
            );
        }
        DownstreamProtocol::Anthropic => {
            assert_eq!(
                body.pointer("/usage/input_tokens"),
                Some(&json!(9)),
                "{case_name}"
            );
            assert_eq!(
                body.pointer("/usage/output_tokens"),
                Some(&json!(9)),
                "{case_name}"
            );
            assert_eq!(
                body.pointer("/usage/cache_read_input_tokens"),
                Some(&json!(3)),
                "{case_name}"
            );
        }
        DownstreamProtocol::Gemini => {
            assert_eq!(
                body.pointer("/usageMetadata/promptTokenCount"),
                Some(&json!(11)),
                "{case_name}"
            );
            assert_eq!(
                body.pointer("/usageMetadata/toolUsePromptTokenCount"),
                Some(&json!(1)),
                "{case_name}"
            );
            assert_eq!(
                body.pointer("/usageMetadata/candidatesTokenCount"),
                Some(&json!(7)),
                "{case_name}"
            );
            assert_eq!(
                body.pointer("/usageMetadata/thoughtsTokenCount"),
                Some(&json!(2)),
                "{case_name}"
            );
            assert_eq!(
                body.pointer("/usageMetadata/totalTokenCount"),
                Some(&json!(21)),
                "{case_name}"
            );
        }
    }
}

fn assert_gemini_usage_cost(
    log: &RequestLogRecord,
    catalog_id: i64,
    catalog_version_id: i64,
    case_name: &str,
) {
    assert_eq!(log.total_input_tokens, Some(12), "{case_name}");
    assert_eq!(log.total_output_tokens, Some(9), "{case_name}");
    assert_eq!(log.total_tokens, Some(21), "{case_name}");
    assert_eq!(log.input_text_tokens, Some(9), "{case_name}");
    assert_eq!(log.output_text_tokens, Some(7), "{case_name}");
    assert_eq!(log.input_image_tokens, Some(0), "{case_name}");
    assert_eq!(log.output_image_tokens, Some(0), "{case_name}");
    assert_eq!(log.cache_read_tokens, Some(3), "{case_name}");
    assert_eq!(log.reasoning_tokens, Some(2), "{case_name}");
    assert_eq!(log.cost_catalog_id, Some(catalog_id), "{case_name}");
    assert_eq!(
        log.cost_catalog_version_id,
        Some(catalog_version_id),
        "{case_name}"
    );
    assert_eq!(log.estimated_cost_nanos, Some(124), "{case_name}");
    let snapshot: CostSnapshot = serde_json::from_str(
        log.cost_snapshot_json
            .as_deref()
            .expect("Gemini base-cell cost snapshot should persist"),
    )
    .expect("Gemini base-cell cost snapshot should parse");
    assert_eq!(snapshot.total_cost_nanos, 124, "{case_name}");
}

fn assert_gemini_success_stream_terminal(
    protocol: DownstreamProtocol,
    events: &[GoldenEvent],
    raw_body: &[u8],
    expected_gemini_events: &[GoldenEvent],
    case_name: &str,
) {
    match protocol {
        DownstreamProtocol::Openai => {
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.data == json!("[DONE]"))
                    .count(),
                1,
                "{case_name}: one OpenAI done"
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|event| {
                        event.data.pointer("/choices/0/finish_reason") == Some(&json!("stop"))
                    })
                    .count(),
                1,
                "{case_name}: one OpenAI stop"
            );
        }
        DownstreamProtocol::Responses => {
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.data["type"] == "response.completed")
                    .count(),
                1,
                "{case_name}: one Responses completion"
            );
            assert!(
                events
                    .iter()
                    .all(|event| event.data["type"] != "response.error"),
                "{case_name}"
            );
        }
        DownstreamProtocol::Anthropic => {
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.event.as_deref() == Some("message_start"))
                    .count(),
                1,
                "{case_name}: one Anthropic start"
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.event.as_deref() == Some("message_stop"))
                    .count(),
                1,
                "{case_name}: one Anthropic stop"
            );
            assert!(
                events
                    .iter()
                    .any(|event| event.data.pointer("/delta/stop_reason")
                        == Some(&json!("end_turn"))),
                "{case_name}"
            );
        }
        DownstreamProtocol::Gemini => {
            assert_eq!(
                events, expected_gemini_events,
                "{case_name}: raw Gemini stream"
            );
            assert!(
                !String::from_utf8_lossy(raw_body).contains("[DONE]"),
                "{case_name}: Gemini has no done marker"
            );
        }
    }
}

fn assert_gemini_base_cell(test_name: &'static str, protocol: DownstreamProtocol) {
    let (_, fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == protocol)
        .expect("Gemini target fixture for base cell");

    let non_stream_fixture = fixture.clone();
    let case_name = format!("{test_name}-non-stream");
    let runtime_name = case_name.clone();
    run_case(&runtime_name, move |context| async move {
        let upstream = TestUpstream::spawn_json(
            StatusCode::OK,
            non_stream_fixture.non_stream.upstream_response.clone(),
        )
        .await;
        let router = RouterFixture::new(context, &non_stream_fixture, &upstream.base_url).await;
        let (catalog_id, catalog_version_id) = router.attach_cost_catalog(Some(100), Some(2)).await;
        let persisted_sink = router.install_recording_persisted_sink();
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(
                &non_stream_fixture,
                false,
                &non_stream_fixture.request.downstream,
            )
            .await;

        assert_eq!(response.status(), StatusCode::OK, "{case_name}");
        assert_no_public_transform_diagnostics(&response);
        let request_id = assert_downstream_request_identity(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Gemini base non-stream response should read");
        let body: Value =
            serde_json::from_slice(&body).expect("Gemini base non-stream response should be JSON");
        assert_eq!(
            gemini_non_stream_text(protocol, &body),
            Some("baseline pong"),
            "{case_name}"
        );
        let (pointer, terminal) = match protocol {
            DownstreamProtocol::Openai => ("/choices/0/finish_reason", "stop"),
            DownstreamProtocol::Responses => ("/status", "completed"),
            DownstreamProtocol::Anthropic => ("/stop_reason", "end_turn"),
            DownstreamProtocol::Gemini => ("/candidates/0/finishReason", "STOP"),
        };
        assert_eq!(
            body.pointer(pointer).and_then(Value::as_str),
            Some(terminal),
            "{case_name}: normal terminal"
        );
        assert_gemini_non_stream_usage(protocol, &body, &case_name);
        let captured = upstream.requests().await;
        assert_upstream(
            test_name,
            &non_stream_fixture,
            &captured,
            &non_stream_fixture.request.upstream_path,
            None,
            &non_stream_fixture.request.upstream,
            &router.requested_model(),
            &request_id,
        );
        assert_eq!(
            router.app_state.secret_encryption.decrypt_call_count(),
            1,
            "{case_name}: one credential resolution"
        );
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_eq!(log.request_id, request_id, "{case_name}");
        assert_log_common(&router, &non_stream_fixture, &log);
        assert_log_timing_order(&log);
        assert!(!log.is_stream, "{case_name}");
        assert_eq!(log.upstream_http_status, Some(200), "{case_name}");
        assert!(log.final_error_code.is_none(), "{case_name}");
        assert_gemini_usage_cost(&log, catalog_id, catalog_version_id, &case_name);
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
        assert_eq!(persisted_sink.contexts.lock().await.len(), 1, "{case_name}");
        assert_eq!(captured.len(), 1, "{case_name}: no retry");
        upstream.shutdown().await;
    });

    let stream_fixture = fixture.clone();
    let case_name = format!("{test_name}-stream");
    let runtime_name = case_name.clone();
    run_case(&runtime_name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Sse {
            events: stream_fixture.stream.upstream_events.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &stream_fixture, &upstream.base_url).await;
        let (catalog_id, catalog_version_id) = router.attach_cost_catalog(Some(100), Some(2)).await;
        let persisted_sink = router.install_recording_persisted_sink();
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(
                &stream_fixture,
                true,
                &stream_fixture.stream.downstream_request,
            )
            .await;

        assert_eq!(response.status(), StatusCode::OK, "{case_name}");
        assert_no_public_transform_diagnostics(&response);
        let request_id = assert_downstream_request_identity(&response);
        let body = timeout(
            WAIT_TIMEOUT,
            axum::body::to_bytes(response.into_body(), usize::MAX),
        )
        .await
        .expect("Gemini base stream should terminate")
        .expect("Gemini base stream should read");
        let events = parse_downstream_events(protocol, &body);
        assert_eq!(
            stream_text(protocol, &events),
            "baseline pong",
            "{case_name}"
        );
        assert_gemini_success_stream_terminal(
            protocol,
            &events,
            &body,
            &stream_fixture.stream.upstream_events,
            &case_name,
        );
        let captured = upstream.requests().await;
        assert_upstream(
            test_name,
            &stream_fixture,
            &captured,
            &stream_fixture.stream.upstream_path,
            stream_fixture.stream.upstream_query.as_deref(),
            &stream_fixture.stream.upstream_request,
            &router.requested_model(),
            &request_id,
        );
        assert_eq!(
            router.app_state.secret_encryption.decrypt_call_count(),
            1,
            "{case_name}: one credential resolution"
        );
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_eq!(log.request_id, request_id, "{case_name}");
        assert_log_common(&router, &stream_fixture, &log);
        assert_log_timing_order(&log);
        assert!(log.is_stream, "{case_name}");
        assert_eq!(log.upstream_http_status, Some(200), "{case_name}");
        assert!(log.final_error_code.is_none(), "{case_name}");
        assert_gemini_usage_cost(&log, catalog_id, catalog_version_id, &case_name);
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
        assert_eq!(persisted_sink.contexts.lock().await.len(), 1, "{case_name}");
        assert_eq!(captured.len(), 1, "{case_name}: no retry");
        upstream.shutdown().await;
    });

    let error_fixture = fixture.clone();
    let error_response = gemini_target_golden().error.http_429.response;
    let case_name = format!("{test_name}-upstream-error");
    let runtime_name = case_name.clone();
    run_case(&runtime_name, move |context| async move {
        let upstream =
            TestUpstream::spawn_json(StatusCode::TOO_MANY_REQUESTS, error_response.clone()).await;
        let router = RouterFixture::new(context, &error_fixture, &upstream.base_url).await;
        router.attach_cost_catalog(Some(100), Some(2)).await;
        let persisted_sink = router.install_recording_persisted_sink();
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(
                &error_fixture,
                false,
                &error_fixture.error.downstream_request,
            )
            .await;

        assert_eq!(
            response.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "{case_name}"
        );
        assert_no_public_transform_diagnostics(&response);
        let request_id = assert_downstream_request_identity(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Gemini 429 envelope should read");
        let body: Value = serde_json::from_slice(&body).expect("Gemini 429 envelope JSON");
        assert_eq!(
            downstream_error_code(&body, protocol),
            Some("upstream_rate_limit_error"),
            "{case_name}"
        );
        assert_eq!(
            body.pointer("/upstream_error/body"),
            Some(&error_response),
            "{case_name}: authentic bounded Gemini error"
        );
        let captured = upstream.requests().await;
        assert_upstream(
            test_name,
            &error_fixture,
            &captured,
            &error_fixture.request.upstream_path,
            None,
            &error_fixture.request.upstream,
            &router.requested_model(),
            &request_id,
        );
        assert_eq!(
            router.app_state.secret_encryption.decrypt_call_count(),
            1,
            "{case_name}: one credential resolution"
        );
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(log.request_id, request_id, "{case_name}");
        assert_log_common(&router, &error_fixture, &log);
        assert_log_timing_order(&log);
        assert_eq!(log.upstream_http_status, Some(429), "{case_name}");
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("upstream_rate_limit_error"),
            "{case_name}"
        );
        assert_stream_failure_has_no_usage_or_cost(&log, &case_name);
        router.wait_for_api_key_lease_release().await;
        router.assert_no_api_key_usage_charge(&case_name).await;
        assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
        assert_eq!(persisted_sink.contexts.lock().await.len(), 1, "{case_name}");
        assert_eq!(captured.len(), 1, "{case_name}: no retry");
        upstream.shutdown().await;
    });

    let cancellation_fixture = fixture;
    let case_name = format!("{test_name}-cancellation");
    let runtime_name = case_name.clone();
    run_case(&runtime_name, move |context| async move {
        let dropped = Arc::new(DropSignal::default());
        let upstream = TestUpstream::spawn(ScriptedReply::HangingSse {
            first_event: cancellation_fixture
                .cancellation
                .first_upstream_event
                .clone(),
            dropped: Arc::clone(&dropped),
        })
        .await;
        let router = RouterFixture::new(context, &cancellation_fixture, &upstream.base_url).await;
        router.attach_cost_catalog(Some(100), Some(2)).await;
        let persisted_sink = router.install_recording_persisted_sink();
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(
                &cancellation_fixture,
                true,
                &cancellation_fixture.cancellation.downstream_request,
            )
            .await;

        assert_eq!(response.status(), StatusCode::OK, "{case_name}");
        let request_id = assert_downstream_request_identity(&response);
        let mut body = response.into_body().into_data_stream();
        let first = timeout(WAIT_TIMEOUT, body.next())
            .await
            .expect("Gemini cancellation first frame deadline")
            .expect("Gemini cancellation first frame")
            .expect("Gemini cancellation first frame should read");
        assert!(!first.is_empty(), "{case_name}");
        drop(body);
        dropped.wait().await;
        let captured = upstream.requests().await;
        assert_upstream(
            test_name,
            &cancellation_fixture,
            &captured,
            &cancellation_fixture.cancellation.upstream_path,
            cancellation_fixture.cancellation.upstream_query.as_deref(),
            &cancellation_fixture.cancellation.upstream_request,
            &router.requested_model(),
            &request_id,
        );
        assert_eq!(
            router.app_state.secret_encryption.decrypt_call_count(),
            1,
            "{case_name}: one credential resolution"
        );
        let log = router.wait_for_log(RequestStatus::Cancelled).await;
        assert_eq!(log.request_id, request_id, "{case_name}");
        assert_log_common(&router, &cancellation_fixture, &log);
        assert_log_timing_order(&log);
        assert_eq!(log.upstream_http_status, Some(200), "{case_name}");
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("client_cancelled_error"),
            "{case_name}"
        );
        assert_stream_failure_has_no_usage_or_cost(&log, &case_name);
        router.wait_for_api_key_lease_release().await;
        router.assert_no_api_key_usage_charge(&case_name).await;
        assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
        assert_eq!(captured.len(), 1, "{case_name}: no retry");
        assert_single_persisted_terminal_fact(
            &persisted_sink,
            ExecutionStage::DownstreamSend,
            ResponseVisibility::BodyStarted,
        )
        .await;
        upstream.shutdown().await;
    });
}

macro_rules! gemini_base_cell_test {
    ($name:ident, $protocol:expr) => {
        #[test]
        fn $name() {
            assert_gemini_base_cell(stringify!($name), $protocol);
        }
    };
}

gemini_base_cell_test!(
    openai_to_gemini_base_cell_is_verified,
    DownstreamProtocol::Openai
);
gemini_base_cell_test!(
    responses_to_gemini_base_cell_is_verified,
    DownstreamProtocol::Responses
);
gemini_base_cell_test!(
    anthropic_to_gemini_base_cell_is_verified,
    DownstreamProtocol::Anthropic
);
gemini_base_cell_test!(
    gemini_to_gemini_base_cell_is_verified,
    DownstreamProtocol::Gemini
);

#[test]
fn r3_19_gemini_target_evidence_registry_covers_exactly_24_base_dimensions() {
    const DIMENSIONS: [&str; 6] = [
        "non_stream_text",
        "stream_text",
        "usage",
        "normal_termination",
        "upstream_error",
        "cancellation",
    ];
    const EVIDENCE: [(&str, &str); 4] = [
        (
            "openai",
            "proxy::direct_execution_regression::openai_to_gemini_base_cell_is_verified",
        ),
        (
            "responses",
            "proxy::direct_execution_regression::responses_to_gemini_base_cell_is_verified",
        ),
        (
            "anthropic",
            "proxy::direct_execution_regression::anthropic_to_gemini_base_cell_is_verified",
        ),
        (
            "gemini",
            "proxy::direct_execution_regression::gemini_to_gemini_base_cell_is_verified",
        ),
    ];

    let mut cells = BTreeSet::new();
    let mut references = BTreeSet::new();
    for (downstream, reference) in EVIDENCE {
        assert!(reference.starts_with("proxy::direct_execution_regression::"));
        assert!(references.insert(reference));
        for dimension in DIMENSIONS {
            assert!(cells.insert((downstream, dimension)));
        }
    }
    assert_eq!(cells.len(), 24);
    assert_eq!(references.len(), 4);
}

#[test]
fn r3_19_gemini_repeated_http_errors_do_not_gate_the_next_request() {
    for (name, fixture) in gemini_target_fixtures() {
        let case_name = format!("r3-19-gemini-repeated-http-error-{name}");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::TOO_MANY_REQUESTS,
                fixture.error.upstream_response.clone(),
            )
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            for attempt in 0..2 {
                let response = router
                    .send(&fixture, false, &fixture.error.downstream_request)
                    .await;
                assert_eq!(
                    response.status(),
                    StatusCode::TOO_MANY_REQUESTS,
                    "{case_name}: attempt {attempt}"
                );
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("repeated Gemini HTTP error should read");
                let body: Value = serde_json::from_slice(&body)
                    .expect("repeated Gemini HTTP error should be JSON");
                assert_eq!(
                    downstream_error_code(&body, fixture.protocol),
                    Some("upstream_rate_limit_error"),
                    "{case_name}: attempt {attempt}"
                );
                router.wait_for_api_key_lease_release().await;

                let deadline = Instant::now() + WAIT_TIMEOUT;
                loop {
                    let logs = router.request_logs().await;
                    if logs.len() == attempt + 1 {
                        assert!(logs.iter().all(|log| {
                            log.overall_status == RequestStatus::Error
                                && log.upstream_http_status == Some(429)
                                && log.final_error_code.as_deref()
                                    == Some("upstream_rate_limit_error")
                                && log.total_tokens.is_none()
                                && log.estimated_cost_nanos.is_none()
                        }));
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "{case_name}: one log per admitted request"
                    );
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                assert_eq!(
                    upstream.requests().await.len(),
                    attempt + 1,
                    "{case_name}: no hidden retry and next request admitted"
                );
            }

            router.assert_no_api_key_usage_charge(&case_name).await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_gemini_base_cells_body_and_stream_reader_errors_release_once_without_cost() {
    for (name, fixture) in gemini_target_fixtures() {
        let non_stream_fixture = fixture.clone();
        let case_name = format!("r3-19-gemini-body-interruption-{name}");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::InterruptedBody {
                content_type: "application/json".to_string(),
                first_chunk: br#"{"responseId":"private-partial""#.to_vec(),
            })
            .await;
            let router = RouterFixture::new(context, &non_stream_fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(
                    &non_stream_fixture,
                    false,
                    &non_stream_fixture.request.downstream,
                )
                .await;

            assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{case_name}");
            let request_id = assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("Gemini body interruption envelope should read");
            let body: Value = serde_json::from_slice(&body)
                .expect("Gemini body interruption envelope should be JSON");
            assert_eq!(
                downstream_error_code(&body, non_stream_fixture.protocol),
                Some("upstream_response_error"),
                "{case_name}"
            );
            assert!(!body.to_string().contains("private-partial"), "{case_name}");
            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &non_stream_fixture,
                &captured,
                &non_stream_fixture.request.upstream_path,
                None,
                &non_stream_fixture.request.upstream,
                &router.requested_model(),
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(log.request_id, request_id, "{case_name}");
            assert_log_common(&router, &non_stream_fixture, &log);
            assert_eq!(log.upstream_http_status, Some(200), "{case_name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error"),
                "{case_name}"
            );
            assert_stream_failure_has_no_usage_or_cost(&log, &case_name);
            router.wait_for_api_key_lease_release().await;
            router.assert_no_api_key_usage_charge(&case_name).await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            assert_eq!(captured.len(), 1, "{case_name}: no retry");
            upstream.shutdown().await;
        });

        let case_name = format!("r3-19-gemini-stream-reader-interruption-{name}");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::InterruptedSse {
                first_event: fixture.cancellation.first_upstream_event.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;
            let persisted_sink = router.install_recording_persisted_sink();

            let response = router
                .send(&fixture, true, &fixture.cancellation.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let request_id = assert_downstream_request_identity(&response);
            let mut body = response.into_body().into_data_stream();
            let mut successful = Vec::new();
            let mut saw_body_error = false;
            loop {
                match timeout(WAIT_TIMEOUT, body.next())
                    .await
                    .expect("Gemini stream interruption deadline")
                {
                    Some(Ok(chunk)) => successful.extend_from_slice(&chunk),
                    Some(Err(_)) => {
                        saw_body_error = true;
                        break;
                    }
                    None => break,
                }
            }
            assert!(saw_body_error, "{case_name}: reader error reaches Body");
            let events = parse_downstream_events(fixture.protocol, &successful);
            assert_eq!(
                stream_text(fixture.protocol, &events),
                "baseline ",
                "{case_name}: already committed text remains visible"
            );
            assert!(
                events.iter().all(|event| {
                    event.data != "[DONE]"
                        && event.data["type"] != "response.completed"
                        && event.event.as_deref() != Some("message_stop")
                        && event
                            .data
                            .pointer("/choices/0/finish_reason")
                            .is_none_or(Value::is_null)
                        && event.data.pointer("/candidates/0/finishReason").is_none()
                }),
                "{case_name}: interrupted stream has no success terminal"
            );
            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.cancellation.upstream_path,
                fixture.cancellation.upstream_query.as_deref(),
                &fixture.cancellation.upstream_request,
                &router.requested_model(),
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(log.request_id, request_id, "{case_name}");
            assert_log_common(&router, &fixture, &log);
            assert_eq!(log.upstream_http_status, Some(200), "{case_name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error"),
                "{case_name}"
            );
            assert_stream_failure_has_no_usage_or_cost(&log, &case_name);
            router.wait_for_api_key_lease_release().await;
            router.assert_no_api_key_usage_charge(&case_name).await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            assert_eq!(captured.len(), 1, "{case_name}: no retry");
            assert_single_persisted_terminal_fact(
                &persisted_sink,
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::BodyStarted,
            )
            .await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_gemini_and_vertex_profiles_execute_equivalent_generate_and_stream_contracts() {
    let (_, native_fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Gemini)
        .expect("native Gemini profile-equivalence fixture");

    for (profile_type, profile_name, access_token) in [
        (UpstreamProfileType::Gemini, "gemini", PROVIDER_SECRET),
        (
            UpstreamProfileType::Vertex,
            "vertex",
            "vertex-profile-equivalence-token",
        ),
    ] {
        for is_stream in [false, true] {
            let mut fixture = native_fixture.clone();
            fixture.profile_type = profile_type.clone();
            fixture.upstream_headers = match profile_type {
                UpstreamProfileType::Gemini => {
                    BTreeMap::from([("x-goog-api-key".to_string(), PROVIDER_SECRET.to_string())])
                }
                UpstreamProfileType::Vertex => BTreeMap::from([(
                    "authorization".to_string(),
                    format!("Bearer {access_token}"),
                )]),
                _ => unreachable!(),
            };
            let case_name = format!(
                "r3-19-profile-equivalence-{profile_name}-{}",
                if is_stream { "stream" } else { "generate" }
            );
            let runtime_name = case_name.clone();
            run_case(&runtime_name, move |context| async move {
                let upstream = if is_stream {
                    TestUpstream::spawn(ScriptedReply::Sse {
                        events: fixture.stream.upstream_events.clone(),
                    })
                    .await
                } else {
                    TestUpstream::spawn_json(
                        StatusCode::OK,
                        fixture.non_stream.upstream_response.clone(),
                    )
                    .await
                };
                let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
                if profile_type == UpstreamProfileType::Vertex {
                    router
                        .replace_default_gemini_profile_in_place(
                            &upstream.base_url,
                            UpstreamProfileType::Vertex,
                        )
                        .await;
                    router.prepare_cached_vertex_credential(access_token).await;
                }
                let (catalog_id, catalog_version_id) =
                    router.attach_cost_catalog(Some(100), Some(2)).await;
                router
                    .app_state
                    .admin
                    .request_patch
                    .create_source_variant(
                        router.source_id,
                        RequestPatchVariantInput {
                            source_id: router.source_id,
                            model_id: None,
                            suffix: None,
                            enabled: true,
                            expose_in_models: false,
                            rules: vec![
                                RequestPatchRuleInput {
                                    placement: RequestPatchPlacement::Query,
                                    target: "trace".to_string(),
                                    operation: RequestPatchOperation::Set,
                                    value_json: Some(Some(json!("profile-equivalent"))),
                                    description: Some(
                                        "Gemini/Vertex profile-equivalence query".to_string(),
                                    ),
                                },
                                RequestPatchRuleInput {
                                    placement: RequestPatchPlacement::Header,
                                    target: "x-r3-19-profile".to_string(),
                                    operation: RequestPatchOperation::Set,
                                    value_json: Some(Some(json!("equivalent"))),
                                    description: Some(
                                        "Gemini/Vertex profile-equivalence header".to_string(),
                                    ),
                                },
                                RequestPatchRuleInput {
                                    placement: RequestPatchPlacement::Body,
                                    target: "/profileEvidence".to_string(),
                                    operation: RequestPatchOperation::Set,
                                    value_json: Some(Some(json!(true))),
                                    description: Some(
                                        "Gemini/Vertex profile-equivalence body".to_string(),
                                    ),
                                },
                            ],
                        },
                    )
                    .await
                    .expect("profile-equivalence Patch should save");
                router
                    .app_state
                    .catalog
                    .invalidate_models_catalog()
                    .await
                    .expect("profile-equivalence Patch cache should invalidate");
                router
                    .app_state
                    .secret_encryption
                    .reset_decrypt_call_count();
                let request = if is_stream {
                    &fixture.stream.downstream_request
                } else {
                    &fixture.request.downstream
                };

                let response = router.send(&fixture, is_stream, request).await;

                assert_eq!(response.status(), StatusCode::OK, "{case_name}");
                assert_no_public_transform_diagnostics(&response);
                let request_id = assert_downstream_request_identity(&response);
                let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("profile-equivalence response should complete");
                if is_stream {
                    assert_eq!(
                        parse_downstream_events(fixture.protocol, &response_body),
                        fixture.stream.upstream_events,
                        "{case_name}: same Gemini SSE semantics"
                    );
                    assert!(!String::from_utf8_lossy(&response_body).contains("[DONE]"));
                } else {
                    assert_eq!(
                        serde_json::from_slice::<Value>(&response_body)
                            .expect("profile-equivalence non-stream JSON"),
                        fixture.non_stream.upstream_response,
                        "{case_name}: same Gemini response semantics"
                    );
                }

                let captured = upstream.requests().await;
                assert_eq!(captured.len(), 1, "{case_name}: exactly one call");
                let request = &captured[0];
                assert_eq!(request.method, Method::POST, "{case_name}");
                assert_eq!(
                    request.path,
                    match (profile_type.clone(), is_stream) {
                        (UpstreamProfileType::Gemini, false) => {
                            format!("/v1beta/models/{UPSTREAM_MODEL}:generateContent")
                        }
                        (UpstreamProfileType::Gemini, true) => {
                            format!("/v1beta/models/{UPSTREAM_MODEL}:streamGenerateContent")
                        }
                        (UpstreamProfileType::Vertex, false) => format!(
                            "/v1/projects/project-fixture/locations/us-central1/publishers/google/models/{UPSTREAM_MODEL}:generateContent"
                        ),
                        (UpstreamProfileType::Vertex, true) => format!(
                            "/v1/projects/project-fixture/locations/us-central1/publishers/google/models/{UPSTREAM_MODEL}:streamGenerateContent"
                        ),
                        _ => unreachable!(),
                    },
                    "{case_name}: collection URL is the only path difference"
                );
                let query = request
                    .query
                    .as_deref()
                    .unwrap_or_default()
                    .split('&')
                    .filter(|pair| !pair.is_empty())
                    .map(|pair| pair.split_once('=').unwrap_or((pair, "")))
                    .collect::<BTreeMap<_, _>>();
                assert_eq!(query.get("trace"), Some(&"profile-equivalent"));
                assert_eq!(query.get("alt"), is_stream.then_some(&"sse"));
                assert_eq!(query.len(), if is_stream { 2 } else { 1 });
                assert_eq!(
                    request
                        .headers
                        .get("x-r3-19-profile")
                        .and_then(|value| value.to_str().ok()),
                    Some("equivalent"),
                    "{case_name}"
                );
                match profile_type {
                    UpstreamProfileType::Gemini => {
                        assert_eq!(
                            request
                                .headers
                                .get("x-goog-api-key")
                                .and_then(|value| value.to_str().ok()),
                            Some(PROVIDER_SECRET),
                            "{case_name}"
                        );
                        assert!(!request.headers.contains_key("authorization"));
                    }
                    UpstreamProfileType::Vertex => {
                        assert_eq!(
                            request
                                .headers
                                .get("authorization")
                                .and_then(|value| value.to_str().ok()),
                            Some("Bearer vertex-profile-equivalence-token"),
                            "{case_name}"
                        );
                        assert!(!request.headers.contains_key("x-goog-api-key"));
                        assert!(vertex_token_is_cached_for_test(router.provider_api_key_id));
                    }
                    _ => unreachable!(),
                }
                assert_eq!(
                    ["authorization", "x-goog-api-key"]
                        .into_iter()
                        .map(|name| request.headers.get_all(name).iter().count())
                        .sum::<usize>(),
                    1,
                    "{case_name}: exactly one auth header"
                );
                assert_eq!(
                    request
                        .headers
                        .get(&X_REQUEST_ID)
                        .and_then(|value| value.to_str().ok()),
                    Some(request_id.as_str()),
                    "{case_name}"
                );
                let mut expected_body = render_value(
                    if is_stream {
                        &fixture.stream.upstream_request
                    } else {
                        &fixture.request.upstream
                    },
                    &router.requested_model(),
                );
                expected_body["profileEvidence"] = json!(true);
                assert_eq!(
                    serde_json::from_slice::<Value>(&request.body)
                        .expect("profile-equivalence upstream body JSON"),
                    expected_body,
                    "{case_name}: identical Gemini wire body"
                );
                assert_eq!(
                    router.app_state.secret_encryption.decrypt_call_count(),
                    1,
                    "{case_name}: one credential resolution"
                );
                let log = router.wait_for_log(RequestStatus::Success).await;
                assert_eq!(log.request_id, request_id, "{case_name}");
                assert_log_common(&router, &fixture, &log);
                assert_eq!(log.source_profile_type_snapshot, Some(profile_type));
                assert_gemini_usage_cost(&log, catalog_id, catalog_version_id, &case_name);
                assert!(log.final_error_code.is_none(), "{case_name}");
                router.wait_for_api_key_lease_release().await;
                assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
                upstream.shutdown().await;
            });
        }
    }
}

#[test]
fn r3_19_gemini_and_vertex_unsafe_collection_query_is_precredential_zero_call() {
    const SENTINEL: &str = "profile-query-private-marker";
    let (_, native_fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Gemini)
        .expect("native Gemini unsafe-profile fixture");

    for profile_type in [UpstreamProfileType::Gemini, UpstreamProfileType::Vertex] {
        let mut fixture = native_fixture.clone();
        fixture.profile_type = profile_type.clone();
        let case_name = format!("r3-19-unsafe-profile-query-{profile_type:?}");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                fixture.non_stream.upstream_response.clone(),
            )
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            if profile_type == UpstreamProfileType::Vertex {
                router
                    .replace_default_gemini_profile_in_place(
                        &upstream.base_url,
                        UpstreamProfileType::Vertex,
                    )
                    .await;
            }
            let unsafe_base = match profile_type {
                UpstreamProfileType::Gemini => {
                    format!("{}/v1beta/models?key={SENTINEL}", upstream.base_url)
                }
                UpstreamProfileType::Vertex => format!(
                    "{}/v1/projects/project-fixture/locations/us-central1/publishers/google/models?trace={SENTINEL}",
                    upstream.base_url
                ),
                _ => unreachable!(),
            };
            UpstreamSource::update(
                router.source_id,
                router.provider_id,
                &UpdateUpstreamSourceData {
                    base_url: Some(unsafe_base),
                    updated_at: chrono::Utc::now().timestamp_millis(),
                    ..UpdateUpstreamSourceData::test_defaults()
                },
            )
            .expect("unsafe legacy collection URL should seed");
            router
                .app_state
                .catalog
                .invalidate_provider(router.provider_id, Some(&router.provider_key))
                .await
                .expect("unsafe Source cache should invalidate");
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("unsafe profile rejection should read");
            assert!(!String::from_utf8_lossy(&body).contains(SENTINEL));
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty(), "{case_name}");
            assert!(
                profile_type != UpstreamProfileType::Vertex
                    || !vertex_token_is_cached_for_test(router.provider_api_key_id),
                "{case_name}: zero Vertex OAuth"
            );
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("provider_configuration_error"),
                "{case_name}"
            );
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains(SENTINEL),
                "{case_name}"
            );
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_vertex_extended_safety_finish_and_usage_use_the_shared_gemini_observer() {
    let (_, base_fixture) = gemini_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Openai)
        .expect("OpenAI to Vertex extension fixture");

    for is_stream in [false, true] {
        let mut fixture = base_fixture.clone();
        fixture.profile_type = UpstreamProfileType::Vertex;
        fixture.upstream_headers = BTreeMap::from([(
            "authorization".to_string(),
            "Bearer vertex-extension-token".to_string(),
        )]);
        let upstream_response = json!({
            "responseId":"vertex-extension-response",
            "candidates":[{"index":0,"finishReason":"MODEL_ARMOR"}],
            "usageMetadata":{"promptTokenCount":3,"totalTokenCount":3}
        });
        let upstream_events = vec![GoldenEvent {
            event: None,
            data: json!({
                "responseId":"vertex-extension-stream",
                "promptFeedback":{"blockReason":"JAILBREAK"},
                "usageMetadata":{"promptTokenCount":3,"totalTokenCount":3}
            }),
        }];
        let case_name = format!(
            "r3-19-vertex-extension-{}",
            if is_stream {
                "stream-jailbreak"
            } else {
                "model-armor"
            }
        );
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = if is_stream {
                TestUpstream::spawn(ScriptedReply::Sse {
                    events: upstream_events,
                })
                .await
            } else {
                TestUpstream::spawn_json(StatusCode::OK, upstream_response).await
            };
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .replace_default_gemini_profile_in_place(
                    &upstream.base_url,
                    UpstreamProfileType::Vertex,
                )
                .await;
            router
                .prepare_cached_vertex_credential("vertex-extension-token")
                .await;
            let (catalog_id, catalog_version_id) =
                router.attach_cost_catalog(Some(100), Some(2)).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();
            let request_body = if is_stream {
                &fixture.stream.downstream_request
            } else {
                &fixture.request.downstream
            };

            let response = router.send(&fixture, is_stream, request_body).await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let request_id = assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("Vertex extension response should read");
            if is_stream {
                let events = parse_downstream_events(fixture.protocol, &body);
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| {
                            event.data.pointer("/choices/0/finish_reason")
                                == Some(&json!("content_filter"))
                        })
                        .count(),
                    1,
                    "{case_name}"
                );
                assert_eq!(
                    events.iter().filter(|event| event.data == "[DONE]").count(),
                    1,
                    "{case_name}"
                );
            } else {
                let body: Value = serde_json::from_slice(&body)
                    .expect("Vertex MODEL_ARMOR response should be JSON");
                assert_eq!(
                    body.pointer("/choices/0/finish_reason"),
                    Some(&json!("content_filter")),
                    "{case_name}"
                );
            }

            let captured = upstream.requests().await;
            assert_upstream(
                &case_name,
                &fixture,
                &captured,
                &format!(
                    "/v1/projects/project-fixture/locations/us-central1/publishers/google/models/{UPSTREAM_MODEL}:{}",
                    if is_stream {
                        "streamGenerateContent"
                    } else {
                        "generateContent"
                    }
                ),
                is_stream.then_some("alt=sse"),
                if is_stream {
                    &fixture.stream.upstream_request
                } else {
                    &fixture.request.upstream
                },
                &router.requested_model(),
                &request_id,
            );
            assert!(vertex_token_is_cached_for_test(router.provider_api_key_id));
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 1);
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_log_common(&router, &fixture, &log);
            assert_eq!(
                log.source_profile_type_snapshot,
                Some(UpstreamProfileType::Vertex)
            );
            assert_eq!(log.total_input_tokens, Some(3), "{case_name}");
            assert_eq!(log.total_output_tokens, Some(0), "{case_name}");
            assert_eq!(log.total_tokens, Some(3), "{case_name}");
            assert_eq!(log.estimated_cost_nanos, Some(106), "{case_name}");
            assert_eq!(log.cost_catalog_id, Some(catalog_id), "{case_name}");
            assert_eq!(
                log.cost_catalog_version_id,
                Some(catalog_version_id),
                "{case_name}"
            );
            assert!(log.final_error_code.is_none(), "{case_name}");
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            assert_eq!(captured.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn r3_19_gemini_vertex_profile_evidence_registry_covers_all_four_operations() {
    const EVIDENCE: [(&str, &str); 5] = [
        (
            "generate_and_stream",
            "proxy::direct_execution_regression::r3_19_gemini_and_vertex_profiles_execute_equivalent_generate_and_stream_contracts",
        ),
        (
            "count_tokens",
            "proxy::direct_execution_regression::r3_19_count_tokens_two_legal_shapes_use_gemini_and_vertex_once_without_usage_or_cost",
        ),
        (
            "source_check",
            "controller::provider::tests::gemini_and_vertex_source_check_send_shared_minimal_contract_once",
        ),
        (
            "precredential_safety",
            "proxy::direct_execution_regression::r3_19_gemini_and_vertex_unsafe_collection_query_is_precredential_zero_call",
        ),
        (
            "vertex_extensions",
            "proxy::direct_execution_regression::r3_19_vertex_extended_safety_finish_and_usage_use_the_shared_gemini_observer",
        ),
    ];
    let registry = EVIDENCE.into_iter().collect::<BTreeMap<_, _>>();
    assert_eq!(registry.len(), EVIDENCE.len());
    assert_eq!(
        registry.keys().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "generate_and_stream",
            "count_tokens",
            "source_check",
            "precredential_safety",
            "vertex_extensions",
        ])
    );
    assert!(
        registry
            .values()
            .all(|reference| reference.contains("::") && !reference.contains("transform::"))
    );
}

#[test]
fn r3_19_count_tokens_two_legal_shapes_use_gemini_and_vertex_once_without_usage_or_cost() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "gemini")
        .map(|(_, fixture)| fixture)
        .expect("Gemini fixture");
    let target = gemini_target_golden();
    for (profile_type, profile_name) in [
        (UpstreamProfileType::Gemini, "gemini"),
        (UpstreamProfileType::Vertex, "vertex"),
    ] {
        for (shape_name, version, request_body) in [
            (
                "contents",
                "v1beta",
                target.count_tokens.contents_request.clone(),
            ),
            (
                "generate-content-request",
                "v1",
                target.count_tokens.generate_content_request.clone(),
            ),
        ] {
            let fixture = fixture.clone();
            let upstream_response = serde_json::to_vec(&target.count_tokens.response)
                .expect("CountTokens fixture response should serialize");
            let case_name = format!("r3-19-count-tokens-{profile_name}-{shape_name}");
            let runtime_name = case_name.clone();
            run_case(&runtime_name, move |context| async move {
                let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                    status: StatusCode::OK,
                    content_type: Some("application/json; charset=utf-8".to_string()),
                    content_encoding: None,
                    body: upstream_response.clone(),
                })
                .await;
                let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
                if profile_type == UpstreamProfileType::Vertex {
                    router
                        .replace_default_gemini_profile_in_place(
                            &upstream.base_url,
                            UpstreamProfileType::Vertex,
                        )
                        .await;
                    router
                        .prepare_cached_vertex_credential("vertex-count-tokens-access-token")
                        .await;
                }
                router.attach_cost_catalog(Some(101), Some(3)).await;
                router
                    .app_state
                    .admin
                    .request_patch
                    .create_source_variant(
                        router.source_id,
                        RequestPatchVariantInput {
                            source_id: router.source_id,
                            model_id: None,
                            suffix: None,
                            enabled: true,
                            expose_in_models: false,
                            rules: vec![
                                RequestPatchRuleInput {
                                    placement: RequestPatchPlacement::Query,
                                    target: "trace".to_string(),
                                    operation: RequestPatchOperation::Set,
                                    value_json: Some(Some(json!("must-not-apply"))),
                                    description: Some(
                                        "CountTokens generation-only Patch boundary".to_string(),
                                    ),
                                },
                                RequestPatchRuleInput {
                                    placement: RequestPatchPlacement::Body,
                                    target: "/patchMustNotApply".to_string(),
                                    operation: RequestPatchOperation::Set,
                                    value_json: Some(Some(json!(true))),
                                    description: Some(
                                        "CountTokens body Patch boundary".to_string(),
                                    ),
                                },
                            ],
                        },
                    )
                    .await
                    .expect("generation Patch fixture should save");
                router
                    .app_state
                    .catalog
                    .invalidate_models_catalog()
                    .await
                    .expect("Patch catalog should invalidate");
                router
                    .app_state
                    .secret_encryption
                    .reset_decrypt_call_count();

                let uri = format!(
                    "/gemini/{version}/models/{}:countTokens?key={}&alt=client-owned&trace=client-owned",
                    router.requested_model(),
                    router.downstream_key
                );
                let response = router
                    .send_raw_post(uri, request_body.clone(), DownstreamAuth::Bearer)
                    .await;

                assert_eq!(response.status(), StatusCode::OK, "{case_name}");
                let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("CountTokens response body should read");
                assert_eq!(
                    response_body.as_ref(),
                    upstream_response.as_slice(),
                    "{case_name}"
                );
                let decoded: Value = serde_json::from_slice(&response_body)
                    .expect("CountTokens raw response should remain JSON");
                assert_eq!(decoded["futureCountField"], json!(true), "{case_name}");

                let captured = upstream.requests().await;
                assert_eq!(captured.len(), 1, "{case_name}");
                assert_eq!(captured[0].method, Method::POST, "{case_name}");
                assert_eq!(
                    captured[0].path,
                    if profile_type == UpstreamProfileType::Vertex {
                        format!(
                            "/v1/projects/project-fixture/locations/us-central1/publishers/google/models/{UPSTREAM_MODEL}:countTokens"
                        )
                    } else {
                        format!("/v1beta/models/{UPSTREAM_MODEL}:countTokens")
                    },
                    "{case_name}"
                );
                assert_eq!(captured[0].query, None, "{case_name}");
                let captured_body: Value = serde_json::from_slice(&captured[0].body)
                    .expect("captured CountTokens request should be JSON");
                assert_eq!(captured_body, request_body, "{case_name}");
                if profile_type == UpstreamProfileType::Vertex {
                    assert_eq!(
                        captured[0]
                            .headers
                            .get("authorization")
                            .and_then(|value| value.to_str().ok()),
                        Some("Bearer vertex-count-tokens-access-token"),
                        "{case_name}"
                    );
                    assert!(!captured[0].headers.contains_key("x-goog-api-key"));
                    assert!(
                        vertex_token_is_cached_for_test(router.provider_api_key_id),
                        "{case_name}: cached OAuth token remains the only token source"
                    );
                } else {
                    assert_eq!(
                        captured[0]
                            .headers
                            .get("x-goog-api-key")
                            .and_then(|value| value.to_str().ok()),
                        Some(PROVIDER_SECRET),
                        "{case_name}"
                    );
                    assert!(!captured[0].headers.contains_key("authorization"));
                }
                assert_eq!(
                    router.app_state.secret_encryption.decrypt_call_count(),
                    1,
                    "{case_name}: one Provider credential resolution"
                );

                let log = router.wait_for_log(RequestStatus::Success).await;
                assert_eq!(log.source_id, Some(router.source_id), "{case_name}");
                assert_eq!(
                    log.source_profile_type_snapshot,
                    Some(profile_type),
                    "{case_name}"
                );
                assert_eq!(log.upstream_http_status, Some(200), "{case_name}");
                assert_eq!(log.total_input_tokens, None, "{case_name}");
                assert_eq!(log.total_output_tokens, None, "{case_name}");
                assert_eq!(log.total_tokens, None, "{case_name}");
                assert_eq!(log.cost_catalog_id, None, "{case_name}");
                assert_eq!(log.cost_catalog_version_id, None, "{case_name}");
                assert_eq!(log.estimated_cost_nanos, None, "{case_name}");
                assert_eq!(log.cost_snapshot_json, None, "{case_name}");
                router.wait_for_api_key_lease_release().await;
                router.assert_no_api_key_usage_charge(&case_name).await;
                assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
                upstream.shutdown().await;
            });
        }
    }
}

#[test]
fn r3_19_count_tokens_invalid_shapes_and_incompatible_source_are_precredential_zero_call() {
    let gemini_fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "gemini")
        .map(|(_, fixture)| fixture)
        .expect("Gemini fixture");
    for (case_name, invalid_body) in [
        (
            "both",
            json!({
                "contents": [{"parts": [{"text": "private-both"}]}],
                "generateContentRequest": {"contents": [{"parts": [{"text": "private-both"}]}]}
            }),
        ),
        ("none", json!({"private": "none"})),
        ("contents-type", json!({"contents": "private-type"})),
        ("empty-contents", json!({"contents": []})),
        (
            "nested-model",
            json!({
                "generateContentRequest": {
                    "model": "models/private-conflict",
                    "contents": [{"parts": [{"text": "count"}]}]
                }
            }),
        ),
    ] {
        let fixture = gemini_fixture.clone();
        let runtime_name = format!("r3-19-count-tokens-invalid-{case_name}");
        run_case(&runtime_name, move |context| async move {
            let upstream =
                TestUpstream::spawn_json(StatusCode::OK, json!({"totalTokens": 1})).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();
            let uri = format!(
                "/gemini/v1beta/models/{}:countTokens?key={}",
                router.requested_model(),
                router.downstream_key
            );

            let response = router
                .send_raw_post(uri, invalid_body, DownstreamAuth::Bearer)
                .await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case_name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("invalid CountTokens response should read");
            assert!(!String::from_utf8_lossy(&body).contains("private"));
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty(), "{case_name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("invalid_request_error")
            );
            assert_eq!(log.cost_catalog_id, None);
            assert_eq!(log.estimated_cost_nanos, None);
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }

    let openai_fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("OpenAI fixture");
    run_case(
        "r3-19-count-tokens-incompatible-source",
        move |context| async move {
            let upstream =
                TestUpstream::spawn_json(StatusCode::OK, json!({"totalTokens": 1})).await;
            let router = RouterFixture::new(context, &openai_fixture, &upstream.base_url).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();
            let uri = format!(
                "/gemini/models/{}:countTokens?key={}",
                router.requested_model(),
                router.downstream_key
            );

            let response = router
                .send_raw_post(
                    uri,
                    json!({"contents": [{"parts": [{"text": "count"}]}]}),
                    DownstreamAuth::Bearer,
                )
                .await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty());
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("unsupported_capability_error")
            );
            assert_eq!(log.cost_catalog_id, None);
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        },
    );
}

#[test]
fn r3_19_count_tokens_malformed_2xx_is_raw_observation_degraded_without_usage_or_cost() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "gemini")
        .map(|(_, fixture)| fixture)
        .expect("Gemini fixture");
    run_case(
        "r3-19-count-tokens-malformed-observation",
        move |context| async move {
            let upstream_response =
                br#"{"totalTokens":"private-invalid-count","futureCountField":true}"#.to_vec();
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("application/json".to_string()),
                content_encoding: None,
                body: upstream_response.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(101), Some(3)).await;
            let uri = format!(
                "/gemini/v1beta/models/{}:countTokens?key={}",
                router.requested_model(),
                router.downstream_key
            );

            let response = router
                .send_raw_post(
                    uri,
                    json!({"contents": [{"parts": [{"text": "count"}]}]}),
                    DownstreamAuth::Bearer,
                )
                .await;

            assert_eq!(response.status(), StatusCode::OK);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("malformed CountTokens response should read");
            assert_eq!(body.as_ref(), upstream_response.as_slice());
            assert_eq!(upstream.requests().await.len(), 1);
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(log.total_tokens, None);
            assert_eq!(log.cost_catalog_id, None);
            assert_eq!(log.cost_catalog_version_id, None);
            assert_eq!(log.estimated_cost_nanos, None);
            assert_eq!(log.cost_snapshot_json, None);
            router.wait_for_api_key_lease_release().await;
            router
                .assert_no_api_key_usage_charge("CountTokens malformed 2xx")
                .await;
            upstream.shutdown().await;
        },
    );
}

#[test]
fn r3_19_count_tokens_http_error_and_cancellation_are_single_call_and_release_resources() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "gemini")
        .map(|(_, fixture)| fixture)
        .expect("Gemini fixture");
    let error_fixture = fixture.clone();
    run_case("r3-19-count-tokens-http-error", move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Raw {
            status: StatusCode::TOO_MANY_REQUESTS,
            content_type: Some("application/json".to_string()),
            content_encoding: None,
            body: br#"{"error":{"message":"rate limited"}}"#.to_vec(),
        })
        .await;
        let router = RouterFixture::new(context, &error_fixture, &upstream.base_url).await;
        router.attach_cost_catalog(Some(101), Some(3)).await;
        let uri = format!(
            "/gemini/v1/models/{}:countTokens?key={}",
            router.requested_model(),
            router.downstream_key
        );

        let response = router
            .send_raw_post(
                uri,
                json!({"contents": [{"parts": [{"text": "count"}]}]}),
                DownstreamAuth::Bearer,
            )
            .await;

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("CountTokens HTTP error should read");
        assert_eq!(upstream.requests().await.len(), 1);
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(log.upstream_http_status, Some(429));
        assert_eq!(log.cost_catalog_id, None);
        assert_eq!(log.estimated_cost_nanos, None);
        router.wait_for_api_key_lease_release().await;
        router
            .assert_no_api_key_usage_charge("CountTokens HTTP error")
            .await;
        upstream.shutdown().await;
    });

    run_case(
        "r3-19-count-tokens-cancellation",
        move |context| async move {
            let dropped = Arc::new(DropSignal::default());
            let upstream = TestUpstream::spawn(ScriptedReply::HangingBody {
                content_type: "application/json".to_string(),
                first_chunk: br#"{"totalTokens":"#.to_vec(),
                dropped: Arc::clone(&dropped),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(101), Some(3)).await;
            let cancellation = ProxyCancellationContext::new();
            let cancellation_trigger = cancellation.clone();
            let captured = Arc::clone(&upstream.captured);
            let cancel_task = tokio::spawn(async move {
                let deadline = Instant::now() + WAIT_TIMEOUT;
                loop {
                    if !captured.lock().await.is_empty() {
                        cancellation_trigger.cancel_now("CountTokens client disconnected");
                        return;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "CountTokens should reach upstream"
                    );
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            });
            let uri = format!(
                "/gemini/v1beta/models/{}:countTokens?key={}",
                router.requested_model(),
                router.downstream_key
            );

            let response = timeout(
                WAIT_TIMEOUT,
                router.send_raw_post_with_cancellation(
                    uri,
                    json!({"contents": [{"parts": [{"text": "count"}]}]}),
                    DownstreamAuth::Bearer,
                    cancellation,
                ),
            )
            .await
            .expect("cancelled CountTokens request should finish");
            cancel_task.await.expect("cancellation trigger should join");

            assert_eq!(response.status().as_u16(), 499);
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("CountTokens cancellation response should read");
            dropped.wait().await;
            assert_eq!(upstream.requests().await.len(), 1);
            let log = router.wait_for_log(RequestStatus::Cancelled).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("client_cancelled_error")
            );
            assert_eq!(log.upstream_http_status, Some(200));
            assert_eq!(log.cost_catalog_id, None);
            assert_eq!(log.estimated_cost_nanos, None);
            router.wait_for_api_key_lease_release().await;
            router
                .assert_no_api_key_usage_charge("CountTokens cancellation")
                .await;
            assert_eq!(router.request_logs().await.len(), 1);
            upstream.shutdown().await;
        },
    );
}

#[test]
fn anthropic_target_evidence_registry_covers_24_base_dimensions_and_16_advanced_cells() {
    const BASE_DIMENSIONS: [&str; 6] = [
        "non_stream_text",
        "stream_text",
        "usage",
        "normal_termination",
        "upstream_error",
        "cancellation",
    ];
    const DOWNSTREAMS: [&str; 4] = ["openai", "responses", "anthropic", "gemini"];
    const BASE_EVIDENCE: [(&str, &str); 9] = [
        (
            "r3-18-openai-base",
            "proxy::direct_execution_regression::openai_to_anthropic_base_cell_success_is_verified",
        ),
        (
            "r3-18-responses-base",
            "proxy::direct_execution_regression::responses_to_anthropic_base_cell_success_is_verified",
        ),
        (
            "r3-18-anthropic-base",
            "proxy::direct_execution_regression::anthropic_to_anthropic_base_cell_success_is_verified",
        ),
        (
            "r3-18-gemini-base",
            "proxy::direct_execution_regression::gemini_to_anthropic_base_cell_success_is_verified",
        ),
        (
            "r3-18-openai-upstream-error",
            "proxy::direct_execution_regression::openai_to_anthropic_base_request_is_verified",
        ),
        (
            "r3-18-responses-upstream-error",
            "proxy::direct_execution_regression::responses_to_anthropic_base_request_is_verified",
        ),
        (
            "r3-18-anthropic-upstream-error",
            "proxy::direct_execution_regression::anthropic_to_anthropic_base_request_is_verified",
        ),
        (
            "r3-18-gemini-upstream-error",
            "proxy::direct_execution_regression::gemini_to_anthropic_base_request_is_verified",
        ),
        (
            "r3-18-cancellation",
            "proxy::direct_execution_regression::anthropic_target_precommit_client_cancellation_returns_499_and_releases_once",
        ),
    ];
    const ADVANCED_EVIDENCE: [(&str, &str, &str, &str, &str); 16] = [
        (
            "openai",
            "tools",
            "full",
            "r3-18-openai-tools-cell",
            "proxy::direct_execution_regression::openai_to_anthropic_tools_cell_is_full",
        ),
        (
            "responses",
            "tools",
            "full",
            "r3-18-responses-tools-cell",
            "proxy::direct_execution_regression::responses_to_anthropic_tools_cell_is_full",
        ),
        (
            "anthropic",
            "tools",
            "full",
            "r3-18-anthropic-tools-cell",
            "proxy::direct_execution_regression::anthropic_to_anthropic_tools_cell_is_full",
        ),
        (
            "gemini",
            "tools",
            "controlled_loss",
            "r3-18-gemini-tools-cell",
            "proxy::direct_execution_regression::gemini_to_anthropic_tools_cell_has_typed_controlled_loss",
        ),
        (
            "openai",
            "reasoning",
            "controlled_loss",
            "r3-18-openai-reasoning-cell",
            "proxy::direct_execution_regression::openai_to_anthropic_reasoning_cell_has_typed_controlled_loss",
        ),
        (
            "responses",
            "reasoning",
            "controlled_loss",
            "r3-18-responses-reasoning-cell",
            "proxy::direct_execution_regression::responses_to_anthropic_reasoning_cell_has_typed_controlled_loss",
        ),
        (
            "anthropic",
            "reasoning",
            "full",
            "r3-18-anthropic-reasoning-cell",
            "proxy::direct_execution_regression::anthropic_to_anthropic_reasoning_cell_is_full",
        ),
        (
            "gemini",
            "reasoning",
            "controlled_loss",
            "r3-18-gemini-reasoning-cell",
            "proxy::direct_execution_regression::gemini_to_anthropic_reasoning_cell_has_typed_controlled_loss",
        ),
        (
            "openai",
            "multimodal",
            "controlled_loss",
            "r3-18-openai-multimodal-cell",
            "proxy::direct_execution_regression::openai_to_anthropic_multimodal_cell_has_typed_controlled_loss",
        ),
        (
            "responses",
            "multimodal",
            "controlled_loss",
            "r3-18-responses-multimodal-cell",
            "proxy::direct_execution_regression::responses_to_anthropic_multimodal_cell_has_typed_controlled_loss",
        ),
        (
            "anthropic",
            "multimodal",
            "full",
            "r3-18-anthropic-multimodal-cell",
            "proxy::direct_execution_regression::anthropic_to_anthropic_multimodal_cell_is_full",
        ),
        (
            "gemini",
            "multimodal",
            "controlled_loss",
            "r3-18-gemini-multimodal-cell",
            "proxy::direct_execution_regression::gemini_to_anthropic_multimodal_cell_has_typed_controlled_loss",
        ),
        (
            "openai",
            "structured_output",
            "controlled_loss",
            "r3-18-openai-structured-cell",
            "proxy::direct_execution_regression::openai_to_anthropic_structured_output_cell_has_typed_controlled_loss",
        ),
        (
            "responses",
            "structured_output",
            "controlled_loss",
            "r3-18-responses-structured-cell",
            "proxy::direct_execution_regression::responses_to_anthropic_structured_output_cell_has_typed_controlled_loss",
        ),
        (
            "anthropic",
            "structured_output",
            "full",
            "r3-18-anthropic-structured-cell",
            "proxy::direct_execution_regression::anthropic_to_anthropic_structured_output_cell_is_full",
        ),
        (
            "gemini",
            "structured_output",
            "controlled_loss",
            "r3-18-gemini-structured-cell",
            "proxy::direct_execution_regression::gemini_to_anthropic_structured_output_cell_has_typed_controlled_loss",
        ),
    ];

    let base_catalog = BASE_EVIDENCE.into_iter().collect::<BTreeMap<_, _>>();
    assert_eq!(base_catalog.len(), BASE_EVIDENCE.len());
    assert!(
        base_catalog
            .values()
            .all(|reference| reference.starts_with("proxy::direct_execution_regression::"))
    );

    let mut base_cells = BTreeSet::new();
    let mut used_base_evidence = BTreeSet::new();
    for downstream in DOWNSTREAMS {
        let success = format!("r3-18-{downstream}-base");
        let upstream_error = format!("r3-18-{downstream}-upstream-error");
        for (dimension, evidence_id) in [
            ("non_stream_text", success.as_str()),
            ("stream_text", success.as_str()),
            ("usage", success.as_str()),
            ("normal_termination", success.as_str()),
            ("upstream_error", upstream_error.as_str()),
            ("cancellation", "r3-18-cancellation"),
        ] {
            assert!(BASE_DIMENSIONS.contains(&dimension));
            assert!(base_catalog.contains_key(evidence_id));
            assert!(base_cells.insert((downstream, dimension)));
            used_base_evidence.insert(evidence_id.to_string());
        }
    }
    assert_eq!(base_cells.len(), DOWNSTREAMS.len() * BASE_DIMENSIONS.len());
    assert_eq!(used_base_evidence.len(), BASE_EVIDENCE.len());

    let mut advanced_cells = BTreeSet::new();
    let mut advanced_ids = BTreeSet::new();
    let mut advanced_references = BTreeSet::new();
    let mut full = 0;
    let mut controlled_loss = 0;
    for (downstream, capability, status, evidence_id, reference) in ADVANCED_EVIDENCE {
        assert!(DOWNSTREAMS.contains(&downstream));
        assert!(["tools", "reasoning", "multimodal", "structured_output"].contains(&capability));
        assert!(advanced_cells.insert((downstream, capability)));
        assert!(advanced_ids.insert(evidence_id));
        assert!(advanced_references.insert(reference));
        assert!(reference.starts_with("proxy::direct_execution_regression::"));
        match status {
            "full" => full += 1,
            "controlled_loss" => controlled_loss += 1,
            unexpected => panic!("unexpected Anthropic advanced status: {unexpected}"),
        }
    }
    assert_eq!(advanced_cells.len(), DOWNSTREAMS.len() * 4);
    assert_eq!((full, controlled_loss), (6, 10));
}

#[test]
fn anthropic_target_materializes_native_requests_for_all_public_downstreams_and_modes() {
    for (name, fixture) in anthropic_target_fixtures() {
        for is_stream in [false, true] {
            let fixture = fixture.clone();
            let case_name = format!(
                "anthropic-materialize-{name}-{}",
                if is_stream { "stream" } else { "non-stream" }
            );
            let runtime_name = case_name.clone();
            run_case(&runtime_name, move |context| async move {
                let upstream = TestUpstream::spawn(ScriptedReply::Json {
                    status: StatusCode::TOO_MANY_REQUESTS,
                    body: fixture.error.upstream_response.clone(),
                })
                .await;
                let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
                router
                    .app_state
                    .secret_encryption
                    .reset_decrypt_call_count();
                let downstream_request = if is_stream {
                    &fixture.stream.downstream_request
                } else {
                    &fixture.request.downstream
                };

                let response = router.send(&fixture, is_stream, downstream_request).await;

                assert_eq!(
                    response.status(),
                    StatusCode::TOO_MANY_REQUESTS,
                    "{case_name}"
                );
                let request_id = assert_downstream_request_identity(&response);
                axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("upstream error response should be consumed");
                let captured = upstream.requests().await;
                let (expected_body, expected_path) = if is_stream {
                    (
                        &fixture.stream.upstream_request,
                        &fixture.stream.upstream_path,
                    )
                } else {
                    (&fixture.request.upstream, &fixture.request.upstream_path)
                };
                assert_upstream(
                    name,
                    &fixture,
                    &captured,
                    expected_path,
                    None,
                    expected_body,
                    &router.requested_model(),
                    &request_id,
                );
                assert_eq!(
                    captured[0]
                        .headers
                        .get("accept-encoding")
                        .and_then(|value| value.to_str().ok()),
                    Some(if is_stream {
                        "identity"
                    } else {
                        "gzip, identity"
                    }),
                    "{case_name}"
                );
                assert_eq!(
                    captured[0]
                        .headers
                        .get("anthropic-version")
                        .and_then(|value| value.to_str().ok()),
                    Some("2023-06-01"),
                    "{case_name}"
                );
                assert_eq!(
                    captured[0]
                        .headers
                        .get("x-api-key")
                        .and_then(|value| value.to_str().ok()),
                    Some(PROVIDER_SECRET),
                    "{case_name}"
                );
                assert!(
                    !captured[0].headers.contains_key("authorization"),
                    "{case_name}"
                );
                assert!(
                    !captured[0].headers.contains_key("x-goog-api-key"),
                    "{case_name}"
                );
                assert!(
                    !captured[0].headers.contains_key("anthropic-beta"),
                    "{case_name}"
                );
                assert_eq!(
                    router.app_state.secret_encryption.decrypt_call_count(),
                    1,
                    "{case_name}: one request resolves one Provider Key"
                );
                let log = router.wait_for_log(RequestStatus::Error).await;
                assert_log_common(&router, &fixture, &log);
                assert_eq!(log.upstream_http_status, Some(429), "{case_name}");
                assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
                router.wait_for_api_key_lease_release().await;
                upstream.shutdown().await;
            });
        }
    }
}

fn assert_anthropic_base_request_cell(fixture_name: &'static str) {
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(name, _)| *name == fixture_name)
        .unwrap_or_else(|| panic!("{fixture_name}: Anthropic target fixture"));

    for is_stream in [false, true] {
        let fixture = fixture.clone();
        let case_name = format!(
            "{fixture_name}-to-anthropic-base-request-{}",
            if is_stream { "stream" } else { "non-stream" }
        );
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::TOO_MANY_REQUESTS,
                body: fixture.error.upstream_response.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .admin
                .request_patch
                .create_source_variant(
                    router.source_id,
                    RequestPatchVariantInput {
                        source_id: router.source_id,
                        model_id: None,
                        suffix: None,
                        enabled: true,
                        expose_in_models: false,
                        rules: vec![RequestPatchRuleInput {
                            placement: RequestPatchPlacement::Header,
                            target: "anthropic-beta".to_string(),
                            operation: RequestPatchOperation::Set,
                            value_json: Some(Some(json!("direct-execution-evidence"))),
                            description: Some(
                                "Anthropic request evidence Source Patch".to_string(),
                            ),
                        }],
                    },
                )
                .await
                .expect("Anthropic evidence Patch should save");
            router
                .app_state
                .catalog
                .invalidate_models_catalog()
                .await
                .expect("Anthropic evidence Patch should invalidate catalog");
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let mut downstream_request = if is_stream {
                fixture.stream.downstream_request.clone()
            } else {
                fixture.request.downstream.clone()
            };
            let mut expected_body = if is_stream {
                fixture.stream.upstream_request.clone()
            } else {
                fixture.request.upstream.clone()
            };
            if fixture.protocol == DownstreamProtocol::Anthropic {
                let extension = json!({"preserved": true});
                downstream_request["vendor_extension"] = extension.clone();
                expected_body["vendor_extension"] = extension;
            }

            let response = router.send(&fixture, is_stream, &downstream_request).await;

            assert_eq!(
                response.status(),
                StatusCode::TOO_MANY_REQUESTS,
                "{case_name}"
            );
            let request_id = assert_downstream_request_identity(&response);
            let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("Anthropic upstream error response should be consumed");
            let response_body: Value = serde_json::from_slice(&response_body)
                .expect("Anthropic downstream error should remain JSON");
            assert_eq!(
                downstream_error_code(&response_body, fixture.protocol),
                Some("upstream_rate_limit_error"),
                "{case_name}: external response"
            );

            let captured = upstream.requests().await;
            let expected_path = if is_stream {
                &fixture.stream.upstream_path
            } else {
                &fixture.request.upstream_path
            };
            assert_upstream(
                fixture_name,
                &fixture,
                &captured,
                expected_path,
                None,
                &expected_body,
                &router.requested_model(),
                &request_id,
            );
            let request = &captured[0];
            assert_eq!(
                request
                    .headers
                    .get("anthropic-version")
                    .and_then(|value| value.to_str().ok()),
                Some("2023-06-01"),
                "{case_name}: fixed version"
            );
            assert_eq!(
                request
                    .headers
                    .get("anthropic-beta")
                    .and_then(|value| value.to_str().ok()),
                Some("direct-execution-evidence"),
                "{case_name}: Source Patch"
            );
            assert_eq!(
                request
                    .headers
                    .get("x-api-key")
                    .and_then(|value| value.to_str().ok()),
                Some(PROVIDER_SECRET),
                "{case_name}: Anthropic credential"
            );
            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                1,
                "{case_name}: exactly one Provider Key resolution"
            );

            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_log_common(&router, &fixture, &log);
            assert_eq!(log.upstream_http_status, Some(429), "{case_name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_rate_limit_error")
            );
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn openai_to_anthropic_base_request_is_verified() {
    assert_anthropic_base_request_cell("openai");
}

#[test]
fn responses_to_anthropic_base_request_is_verified() {
    assert_anthropic_base_request_cell("responses");
}

#[test]
fn anthropic_to_anthropic_base_request_is_verified() {
    assert_anthropic_base_request_cell("anthropic");
}

#[test]
fn gemini_to_anthropic_base_request_is_verified() {
    assert_anthropic_base_request_cell("gemini");
}

fn portable_tools_downstream_request(protocol: DownstreamProtocol, requested_model: &str) -> Value {
    match protocol {
        DownstreamProtocol::Openai => json!({
            "model":requested_model,
            "messages":[
                {"role":"assistant","tool_calls":[
                    {"id":"call-weather","type":"function","function":{"name":"weather","arguments":"{\"city\":\"Paris\"}"}},
                    {"id":"call-time","type":"function","function":{"name":"time","arguments":"{\"zone\":\"UTC\"}"}}
                ]},
                {"role":"tool","tool_call_id":"call-weather","content":"21"},
                {"role":"tool","tool_call_id":"call-time","content":"12:00"}
            ],
            "tools":[
                {"type":"function","function":{"name":"weather","description":"weather lookup","parameters":{"type":"object"},"strict":true}},
                {"type":"function","function":{"name":"time","description":"time lookup","parameters":{"type":"object"},"strict":false}}
            ],
            "tool_choice":{"type":"function","function":{"name":"weather"}},
            "parallel_tool_calls":false
        }),
        DownstreamProtocol::Responses => json!({
            "model":requested_model,
            "input":[
                {"type":"function_call","id":"fc-weather","call_id":"response-weather","name":"weather","arguments":"{\"city\":\"Paris\"}"},
                {"type":"function_call","id":"fc-time","call_id":"response-time","name":"time","arguments":"{\"zone\":\"UTC\"}"},
                {"type":"function_call_output","id":"fco-weather","call_id":"response-weather","output":{"temp":21}},
                {"type":"function_call_output","id":"fco-time","call_id":"response-time","output":"12:00"}
            ],
            "tools":[
                {"type":"function","name":"weather","description":"weather lookup","parameters":{"type":"object"},"strict":true},
                {"type":"function","name":"time","description":"time lookup","parameters":{"type":"object"},"strict":false}
            ],
            "tool_choice":{"type":"allowed_tools","mode":"required","tools":[
                {"type":"function","name":"weather"},{"type":"function","name":"time"}
            ]},
            "parallel_tool_calls":false
        }),
        DownstreamProtocol::Anthropic => json!({
            "model":requested_model,"max_tokens":64,
            "messages":[
                {"role":"assistant","content":[
                    {"type":"tool_use","id":"anthropic-weather","name":"weather","input":{"city":"Paris"}},
                    {"type":"tool_use","id":"anthropic-time","name":"time","input":{"zone":"UTC"}}
                ]},
                {"role":"user","content":[
                    {"type":"tool_result","tool_use_id":"anthropic-weather","content":"{\"temp\":21}"},
                    {"type":"tool_result","tool_use_id":"anthropic-time","content":"12:00"},
                    {"type":"text","text":"summarize"}
                ]}
            ],
            "tools":[
                {"name":"weather","description":"weather lookup","input_schema":{"type":"object"},"strict":true},
                {"name":"time","description":"time lookup","input_schema":{"type":"object"},"strict":false}
            ],
            "tool_choice":{"type":"any","disable_parallel_tool_use":true}
        }),
        DownstreamProtocol::Gemini => json!({
            "contents":[
                {"role":"model","parts":[
                    {"functionCall":{"name":"weather","args":{"city":"Paris"}}},
                    {"functionCall":{"name":"time","args":{"zone":"UTC"}}}
                ]},
                {"role":"user","parts":[
                    {"text":"summarize"},
                    {"functionResponse":{"name":"weather","response":{"temp":21}}},
                    {"functionResponse":{"name":"time","response":{"result":"12:00"}}}
                ]}
            ],
            "tools":[{"functionDeclarations":[
                {"name":"weather","description":"weather lookup","parameters":{"type":"object"}},
                {"name":"time","description":"time lookup","parameters":{"type":"object"}}
            ]}],
            "toolConfig":{"functionCallingConfig":{"mode":"ANY","allowedFunctionNames":["weather","time"]}}
        }),
    }
}

fn anthropic_portable_tools_response() -> Value {
    json!({
        "id":"msg_portable_tools",
        "type":"message",
        "role":"assistant",
        "content":[
            {"type":"text","text":"calling tools"},
            {"type":"tool_use","id":"up-call-weather","name":"weather","input":{"city":"Berlin"}},
            {"type":"tool_use","id":"up-call-time","name":"time","input":{"zone":"UTC"}}
        ],
        "model":UPSTREAM_MODEL,
        "stop_reason":"tool_use",
        "stop_sequence":null,
        "usage":{"input_tokens":11,"output_tokens":7}
    })
}

fn assert_portable_tool_response(protocol: DownstreamProtocol, body: &Value, case_name: &str) {
    match protocol {
        DownstreamProtocol::Openai => {
            assert_eq!(
                body["choices"][0]["finish_reason"], "tool_calls",
                "{case_name}"
            );
            assert_eq!(
                body["choices"][0]["message"]["tool_calls"][0]["id"],
                "up-call-weather"
            );
            assert_eq!(
                body["choices"][0]["message"]["tool_calls"][1]["id"],
                "up-call-time"
            );
        }
        DownstreamProtocol::Responses => {
            let output = body["output"].as_array().expect("Responses tool output");
            let call_ids = output
                .iter()
                .filter(|item| item["type"] == "function_call")
                .filter_map(|item| item["call_id"].as_str())
                .collect::<Vec<_>>();
            assert_eq!(
                call_ids,
                vec!["up-call-weather", "up-call-time"],
                "{case_name}"
            );
        }
        DownstreamProtocol::Anthropic => {
            assert_eq!(body["stop_reason"], "tool_use", "{case_name}");
            assert_eq!(body["content"][1]["id"], "up-call-weather");
            assert_eq!(body["content"][2]["id"], "up-call-time");
        }
        DownstreamProtocol::Gemini => {
            assert_eq!(body["candidates"][0]["finishReason"], "STOP", "{case_name}");
            assert_eq!(
                body["candidates"][0]["content"]["parts"][1]["functionCall"]["name"],
                "weather"
            );
            assert_eq!(
                body["candidates"][0]["content"]["parts"][2]["functionCall"]["name"],
                "time"
            );
        }
    }
}

fn assert_anthropic_portable_tools_cell(test_name: &'static str, protocol: DownstreamProtocol) {
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == protocol)
        .expect("Anthropic target fixture for portable tools");
    run_case(test_name, move |context| async move {
        let upstream =
            TestUpstream::spawn_json(StatusCode::OK, anthropic_portable_tools_response()).await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let request = portable_tools_downstream_request(protocol, &router.requested_model());

        let response = router.send(&fixture, false, &request).await;

        assert_eq!(response.status(), StatusCode::OK, "{test_name}");
        let request_id = assert_downstream_request_identity(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("portable tools response should read");
        let body: Value = serde_json::from_slice(&body).expect("portable tools response JSON");
        assert_portable_tool_response(protocol, &body, test_name);

        let captured = upstream.requests().await;
        assert_eq!(captured.len(), 1, "{test_name}: exactly one upstream call");
        assert_eq!(captured[0].method, Method::POST);
        assert_eq!(captured[0].path, "/v1/messages");
        assert_eq!(captured[0].query, None);
        assert_eq!(
            captured[0]
                .headers
                .get("anthropic-version")
                .and_then(|value| value.to_str().ok()),
            Some("2023-06-01")
        );
        assert_eq!(
            captured[0]
                .headers
                .get(&X_REQUEST_ID)
                .and_then(|value| value.to_str().ok()),
            Some(request_id.as_str())
        );
        let target: Value =
            serde_json::from_slice(&captured[0].body).expect("portable Anthropic target body");
        assert_eq!(target["model"], UPSTREAM_MODEL);
        assert_eq!(
            target["max_tokens"],
            if protocol == DownstreamProtocol::Anthropic {
                64
            } else {
                4096
            }
        );
        assert!(target.get("stream").is_none_or(|stream| stream == false));
        assert_eq!(target["tools"].as_array().map(Vec::len), Some(2));
        assert_eq!(target["tools"][0]["name"], "weather");
        assert_eq!(target["tools"][0]["description"], "weather lookup");
        assert_eq!(target["tools"][0]["input_schema"], json!({"type":"object"}));
        assert_eq!(target["tools"][0]["strict"], true);
        assert_eq!(target["tools"][1]["name"], "time");
        assert_eq!(
            target["tool_choice"]["type"],
            if protocol == DownstreamProtocol::Openai {
                "tool"
            } else {
                "any"
            }
        );
        if protocol == DownstreamProtocol::Openai {
            assert_eq!(target["tool_choice"]["name"], "weather");
        }
        if protocol == DownstreamProtocol::Gemini {
            assert!(
                target["tool_choice"]
                    .get("disable_parallel_tool_use")
                    .is_none()
            );
        } else {
            assert_eq!(target["tool_choice"]["disable_parallel_tool_use"], true);
        }

        let messages = target["messages"]
            .as_array()
            .expect("portable target messages");
        let call_ids = messages
            .iter()
            .flat_map(|message| message["content"].as_array().into_iter().flatten())
            .filter(|block| block["type"] == "tool_use")
            .filter_map(|block| block["id"].as_str())
            .collect::<Vec<_>>();
        let result_ids = messages
            .iter()
            .flat_map(|message| message["content"].as_array().into_iter().flatten())
            .filter(|block| block["type"] == "tool_result")
            .filter_map(|block| block["tool_use_id"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            call_ids, result_ids,
            "{test_name}: stable call/result correlation"
        );
        for message in messages {
            if let Some(blocks) = message["content"].as_array() {
                let first_text = blocks.iter().position(|block| block["type"] == "text");
                let last_result = blocks
                    .iter()
                    .rposition(|block| block["type"] == "tool_result");
                assert!(
                    first_text
                        .zip(last_result)
                        .is_none_or(|(text, result)| result < text)
                );
            }
        }

        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_log_common(&router, &fixture, &log);
        assert!(!log.is_stream);
        assert_eq!(log.upstream_http_status, Some(200));
        assert_eq!(
            (
                log.total_input_tokens,
                log.total_output_tokens,
                log.total_tokens
            ),
            (Some(11), Some(7), Some(18))
        );
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        upstream.shutdown().await;
    });
}

macro_rules! anthropic_portable_tools_cell_test {
    ($name:ident, $protocol:expr) => {
        #[test]
        fn $name() {
            assert_anthropic_portable_tools_cell(stringify!($name), $protocol);
        }
    };
}

anthropic_portable_tools_cell_test!(
    openai_to_anthropic_tools_cell_is_full,
    DownstreamProtocol::Openai
);
anthropic_portable_tools_cell_test!(
    responses_to_anthropic_tools_cell_is_full,
    DownstreamProtocol::Responses
);
anthropic_portable_tools_cell_test!(
    anthropic_to_anthropic_tools_cell_is_full,
    DownstreamProtocol::Anthropic
);
anthropic_portable_tools_cell_test!(
    gemini_to_anthropic_tools_cell_has_typed_controlled_loss,
    DownstreamProtocol::Gemini
);

fn anthropic_multimodal_downstream_request(
    protocol: DownstreamProtocol,
    requested_model: &str,
) -> Value {
    match protocol {
        DownstreamProtocol::Openai => json!({
            "model":requested_model,
            "messages":[{"role":"user","content":[
                {"type":"image_url","image_url":{"url":"https://images.example.com/photo.webp","detail":"high"}},
                {"type":"image_url","image_url":{"url":"data:image/gif;base64,ZmFrZQ=="}},
                {"type":"file","file":{"filename":"report.pdf","file_data":"JVBERi0="}},
                {"type":"file","file":{"filename":"notes.md","file_data":"IyBub3Rlcw=="}}
            ]}]
        }),
        DownstreamProtocol::Responses => json!({
            "model":requested_model,
            "input":[{"type":"message","role":"user","content":[
                {"type":"input_image","image_url":"data:image/webp;base64,ZmFrZQ==","detail":"low"},
                {"type":"input_file","filename":"remote.pdf","file_url":"https://files.example.com/remote.pdf"},
                {"type":"input_file","filename":"data.json","file_data":"eyJvayI6dHJ1ZX0="}
            ]}]
        }),
        DownstreamProtocol::Anthropic => json!({
            "model":requested_model,
            "max_tokens":64,
            "messages":[{"role":"user","content":[
                {"type":"image","source":{"type":"url","url":"https://images.example.com/photo.png"}},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"ZmFrZQ=="}},
                {"type":"document","title":"report.pdf","source":{"type":"base64","media_type":"application/pdf","data":"JVBERi0="}},
                {"type":"document","title":"notes.txt","source":{"type":"text","media_type":"text/plain","data":"native text"}}
            ]}]
        }),
        DownstreamProtocol::Gemini => json!({
            "contents":[{"role":"user","parts":[
                {"inlineData":{"mimeType":"image/jpeg","data":"ZmFrZQ=="}},
                {"inlineData":{"mimeType":"application/pdf","data":"JVBERi0=","displayName":"inline.pdf"}},
                {"inlineData":{"mimeType":"text/markdown","data":"IyBub3Rlcw==","displayName":"notes.md"}},
                {"fileData":{"mimeType":"application/pdf","fileUri":"https://files.example.com/remote.pdf","displayName":"remote.pdf"}}
            ]}]
        }),
    }
}

fn anthropic_multimodal_response() -> Value {
    json!({
        "id":"msg_multimodal",
        "type":"message",
        "role":"assistant",
        "content":[{"type":"text","text":"media received"}],
        "model":UPSTREAM_MODEL,
        "stop_reason":"end_turn",
        "stop_sequence":null,
        "usage":{"input_tokens":11,"output_tokens":7}
    })
}

fn assert_anthropic_multimodal_cell(test_name: &'static str, protocol: DownstreamProtocol) {
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == protocol)
        .expect("Anthropic target fixture for multimodal input");
    run_case(test_name, move |context| async move {
        let upstream =
            TestUpstream::spawn_json(StatusCode::OK, anthropic_multimodal_response()).await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let request = anthropic_multimodal_downstream_request(protocol, &router.requested_model());

        let response = router.send(&fixture, false, &request).await;

        assert_eq!(response.status(), StatusCode::OK, "{test_name}");
        assert_no_public_transform_diagnostics(&response);
        let request_id = assert_downstream_request_identity(&response);
        let _body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("multimodal response should read");

        let captured = upstream.requests().await;
        assert_eq!(captured.len(), 1, "{test_name}: exactly one upstream call");
        assert_eq!(captured[0].path, "/v1/messages");
        assert_eq!(
            captured[0]
                .headers
                .get(&X_REQUEST_ID)
                .and_then(|value| value.to_str().ok()),
            Some(request_id.as_str())
        );
        let target: Value =
            serde_json::from_slice(&captured[0].body).expect("multimodal Anthropic target body");
        assert_eq!(target["model"], UPSTREAM_MODEL, "{test_name}");
        assert_eq!(
            target["max_tokens"],
            if protocol == DownstreamProtocol::Anthropic {
                json!(64)
            } else {
                json!(4096)
            },
            "{test_name}"
        );
        let blocks = target["messages"]
            .as_array()
            .expect("Anthropic messages")
            .iter()
            .flat_map(|message| message["content"].as_array().into_iter().flatten())
            .collect::<Vec<_>>();
        assert!(
            blocks.iter().any(|block| block["type"] == "image"),
            "{test_name}"
        );
        assert!(
            blocks.iter().any(|block| block["type"] == "document"),
            "{test_name}"
        );
        assert!(blocks.iter().all(|block| {
            block["type"] != "image"
                || matches!(block["source"]["type"].as_str(), Some("url" | "base64"))
        }));
        assert!(blocks.iter().all(|block| {
            block["type"] != "document"
                || matches!(
                    block["source"]["type"].as_str(),
                    Some("url" | "base64" | "text")
                )
        }));
        let serialized = target.to_string();
        assert!(!serialized.contains("image_url:"), "{test_name}");
        assert!(!serialized.contains("file_url:"), "{test_name}");
        assert!(!serialized.contains("file_data:"), "{test_name}");
        assert!(!serialized.contains("detail"), "{test_name}");

        match protocol {
            DownstreamProtocol::Openai => {
                assert!(blocks.iter().any(|block| {
                    block["type"] == "image" && block["source"]["media_type"] == "image/gif"
                }));
                assert!(blocks.iter().any(|block| {
                    block["type"] == "document"
                        && block["title"] == "notes.md"
                        && block["source"]["type"] == "text"
                        && block["source"]["media_type"] == "text/plain"
                        && block["source"]["data"] == "# notes"
                }));
            }
            DownstreamProtocol::Responses => {
                assert!(blocks.iter().any(|block| {
                    block["type"] == "document"
                        && block["title"] == "remote.pdf"
                        && block["source"]
                            == json!({"type":"url","url":"https://files.example.com/remote.pdf"})
                }));
                assert!(blocks.iter().any(|block| {
                    block["type"] == "document"
                        && block["title"] == "data.json"
                        && block["source"]["data"] == "{\"ok\":true}"
                }));
            }
            DownstreamProtocol::Anthropic => {
                assert!(blocks.iter().any(|block| {
                    block["type"] == "document"
                        && block["source"]["type"] == "text"
                        && block["source"]["data"] == "native text"
                }));
            }
            DownstreamProtocol::Gemini => {
                assert!(blocks.iter().any(|block| {
                    block["type"] == "document"
                        && block["title"] == "notes.md"
                        && block["source"]["data"] == "# notes"
                }));
                assert!(blocks.iter().any(|block| {
                    block["type"] == "document"
                        && block["title"] == "remote.pdf"
                        && block["source"]["type"] == "url"
                }));
            }
        }

        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_log_common(&router, &fixture, &log);
        assert!(!log.is_stream);
        assert_eq!(log.upstream_http_status, Some(200));
        assert_eq!(
            (
                log.total_input_tokens,
                log.total_output_tokens,
                log.total_tokens
            ),
            (Some(11), Some(7), Some(18))
        );
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        upstream.shutdown().await;
    });
}

macro_rules! anthropic_multimodal_cell_test {
    ($name:ident, $protocol:expr) => {
        #[test]
        fn $name() {
            assert_anthropic_multimodal_cell(stringify!($name), $protocol);
        }
    };
}

anthropic_multimodal_cell_test!(
    openai_to_anthropic_multimodal_cell_has_typed_controlled_loss,
    DownstreamProtocol::Openai
);
anthropic_multimodal_cell_test!(
    responses_to_anthropic_multimodal_cell_has_typed_controlled_loss,
    DownstreamProtocol::Responses
);
anthropic_multimodal_cell_test!(
    anthropic_to_anthropic_multimodal_cell_is_full,
    DownstreamProtocol::Anthropic
);
anthropic_multimodal_cell_test!(
    gemini_to_anthropic_multimodal_cell_has_typed_controlled_loss,
    DownstreamProtocol::Gemini
);

#[test]
fn unportable_multimodal_inputs_to_anthropic_are_zero_call() {
    for (case_name, protocol) in [
        (
            "openai-anthropic-audio-zero-call",
            DownstreamProtocol::Openai,
        ),
        (
            "responses-anthropic-invalid-text-zero-call",
            DownstreamProtocol::Responses,
        ),
        (
            "gemini-anthropic-executable-zero-call",
            DownstreamProtocol::Gemini,
        ),
    ] {
        let (_, fixture) = anthropic_target_fixtures()
            .into_iter()
            .find(|(_, fixture)| fixture.protocol == protocol)
            .expect("Anthropic target fixture for rejected multimodal input");
        run_case(case_name, move |context| async move {
            let upstream =
                TestUpstream::spawn_json(StatusCode::OK, anthropic_multimodal_response()).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();
            let model = router.requested_model();
            let request = match protocol {
                DownstreamProtocol::Openai => json!({
                    "model":model,"messages":[{"role":"user","content":[
                        {"type":"input_audio","input_audio":{"data":"UklGRg==","format":"wav"}}
                    ]}]
                }),
                DownstreamProtocol::Responses => json!({
                    "model":model,"input":[{"type":"message","role":"user","content":[
                        {"type":"input_file","filename":"invalid.txt","file_data":"/w=="}
                    ]}]
                }),
                DownstreamProtocol::Gemini => json!({
                    "contents":[{"role":"user","parts":[
                        {"executableCode":{"language":"python","code":"private-media-marker"}}
                    ]}]
                }),
                DownstreamProtocol::Anthropic => unreachable!(),
            };

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case_name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("multimodal rejection response should read");
            assert!(!String::from_utf8_lossy(&body).contains("private-media-marker"));
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty(), "{case_name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert!(
                matches!(
                    log.final_error_code.as_deref(),
                    Some("invalid_request_error" | "unsupported_capability_error")
                ),
                "{case_name}: {:?}",
                log.final_error_code
            );
            assert_eq!(log.total_tokens, None);
            assert_eq!(log.estimated_cost_nanos, None);
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1);
            upstream.shutdown().await;
        });
    }
}

fn anthropic_portable_tool_stream_events() -> Vec<GoldenEvent> {
    vec![
        GoldenEvent {
            event: Some("message_start".to_string()),
            data: json!({
                "type":"message_start",
                "message":{
                    "id":"msg_portable_tool_stream","type":"message","role":"assistant",
                    "content":[],"model":UPSTREAM_MODEL,"stop_reason":null,"stop_sequence":null,
                    "usage":{"input_tokens":11,"output_tokens":0}
                }
            }),
        },
        GoldenEvent {
            event: Some("content_block_start".to_string()),
            data: json!({
                "type":"content_block_start","index":0,
                "content_block":{"type":"tool_use","id":"up-stream-weather","name":"weather","input":{}}
            }),
        },
        GoldenEvent {
            event: Some("content_block_delta".to_string()),
            data: json!({
                "type":"content_block_delta","index":0,
                "delta":{"type":"input_json_delta","partial_json":"{\"city\":\"Berlin\"}"}
            }),
        },
        GoldenEvent {
            event: Some("content_block_stop".to_string()),
            data: json!({"type":"content_block_stop","index":0}),
        },
        GoldenEvent {
            event: Some("message_delta".to_string()),
            data: json!({
                "type":"message_delta",
                "delta":{"stop_reason":"tool_use","stop_sequence":null},
                "usage":{"output_tokens":7}
            }),
        },
        GoldenEvent {
            event: Some("message_stop".to_string()),
            data: json!({"type":"message_stop"}),
        },
    ]
}

fn assert_anthropic_portable_tools_stream_cell(
    test_name: &'static str,
    protocol: DownstreamProtocol,
) {
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == protocol)
        .expect("Anthropic target fixture for portable tool stream");
    run_case(test_name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Sse {
            events: anthropic_portable_tool_stream_events(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let mut request = portable_tools_downstream_request(protocol, &router.requested_model());
        if protocol != DownstreamProtocol::Gemini {
            request["stream"] = Value::Bool(true);
        }

        let response = router.send(&fixture, true, &request).await;

        assert_eq!(response.status(), StatusCode::OK, "{test_name}");
        let request_id = assert_downstream_request_identity(&response);
        let body = timeout(
            WAIT_TIMEOUT,
            axum::body::to_bytes(response.into_body(), usize::MAX),
        )
        .await
        .expect("portable tool stream should terminate")
        .expect("portable tool stream should read");
        let stream_body = String::from_utf8_lossy(&body);
        assert!(stream_body.contains("weather"), "{test_name}: tool name");
        assert!(
            stream_body.matches("up-stream-weather").count() >= 1,
            "{test_name}: tool stream keeps the source correlation ID"
        );

        let captured = upstream.requests().await;
        assert_eq!(captured.len(), 1, "{test_name}: exactly one upstream call");
        assert_eq!(captured[0].path, "/v1/messages");
        assert_eq!(
            captured[0]
                .headers
                .get(&X_REQUEST_ID)
                .and_then(|value| value.to_str().ok()),
            Some(request_id.as_str())
        );
        let target: Value =
            serde_json::from_slice(&captured[0].body).expect("portable tool stream target body");
        assert_eq!(target["model"], UPSTREAM_MODEL);
        assert_eq!(target["stream"], true);
        assert_eq!(target["tools"].as_array().map(Vec::len), Some(2));

        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_log_common(&router, &fixture, &log);
        assert!(log.is_stream);
        assert_eq!(log.upstream_http_status, Some(200));
        assert_eq!(
            (
                log.total_input_tokens,
                log.total_output_tokens,
                log.total_tokens
            ),
            (Some(11), Some(7), Some(18))
        );
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        upstream.shutdown().await;
    });
}

macro_rules! anthropic_portable_tools_stream_cell_test {
    ($name:ident, $protocol:expr) => {
        #[test]
        fn $name() {
            assert_anthropic_portable_tools_stream_cell(stringify!($name), $protocol);
        }
    };
}

anthropic_portable_tools_stream_cell_test!(
    openai_to_anthropic_portable_tools_stream_is_verified,
    DownstreamProtocol::Openai
);
anthropic_portable_tools_stream_cell_test!(
    responses_to_anthropic_portable_tools_stream_is_verified,
    DownstreamProtocol::Responses
);
anthropic_portable_tools_stream_cell_test!(
    anthropic_to_anthropic_portable_tools_stream_is_verified,
    DownstreamProtocol::Anthropic
);
anthropic_portable_tools_stream_cell_test!(
    gemini_to_anthropic_portable_tools_stream_is_verified,
    DownstreamProtocol::Gemini
);

#[test]
fn anthropic_tool_invalid_references_and_forced_nonportable_are_zero_call() {
    for (case_name, protocol) in [
        ("anthropic-tool-dangling", DownstreamProtocol::Openai),
        ("anthropic-tool-duplicate", DownstreamProtocol::Openai),
        (
            "anthropic-tool-missing-reference",
            DownstreamProtocol::Openai,
        ),
        ("anthropic-tool-missing-id", DownstreamProtocol::Anthropic),
        ("anthropic-tool-bad-order", DownstreamProtocol::Anthropic),
        (
            "anthropic-tool-forced-nonportable",
            DownstreamProtocol::Responses,
        ),
    ] {
        let (_, fixture) = anthropic_target_fixtures()
            .into_iter()
            .find(|(_, fixture)| fixture.protocol == protocol)
            .expect("Anthropic target fixture for invalid tools");
        run_case(case_name, move |context| async move {
            let upstream =
                TestUpstream::spawn_json(StatusCode::OK, anthropic_portable_tools_response()).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();
            let model = router.requested_model();
            let request = match case_name {
                "anthropic-tool-dangling" => json!({
                    "model":model,
                    "messages":[{"role":"assistant","tool_calls":[
                        {"id":"dangling","type":"function","function":{"name":"lookup","arguments":"{}"}}
                    ]}]
                }),
                "anthropic-tool-duplicate" => json!({
                    "model":model,
                    "messages":[
                        {"role":"assistant","tool_calls":[
                            {"id":"duplicate","type":"function","function":{"name":"lookup","arguments":"{}"}},
                            {"id":"duplicate","type":"function","function":{"name":"lookup","arguments":"{}"}}
                        ]},
                        {"role":"tool","tool_call_id":"duplicate","content":"ok"}
                    ]
                }),
                "anthropic-tool-missing-reference" => json!({
                    "model":model,
                    "messages":[{"role":"tool","tool_call_id":"missing","content":"ok"}]
                }),
                "anthropic-tool-missing-id" => json!({
                    "model":model,"max_tokens":64,
                    "messages":[
                        {"role":"assistant","content":[{"type":"tool_use","name":"lookup","input":{}}]},
                        {"role":"user","content":[{"type":"tool_result","tool_use_id":"missing","content":"ok"}]}
                    ]
                }),
                "anthropic-tool-bad-order" => json!({
                    "model":model,"max_tokens":64,
                    "messages":[
                        {"role":"assistant","content":[{"type":"tool_use","id":"ordered","name":"lookup","input":{}}]},
                        {"role":"user","content":[
                            {"type":"text","text":"too early"},
                            {"type":"tool_result","tool_use_id":"ordered","content":"ok"}
                        ]}
                    ]
                }),
                "anthropic-tool-forced-nonportable" => json!({
                    "model":model,"input":"search",
                    "tools":[{"type":"web_search_preview"}],
                    "tool_choice":"required"
                }),
                _ => unreachable!("registered invalid portable tool case"),
            };

            let response = router.send(&fixture, false, &request).await;

            assert!(
                matches!(
                    response.status(),
                    StatusCode::BAD_REQUEST | StatusCode::INTERNAL_SERVER_ERROR
                ),
                "{case_name}: deterministic pre-transport rejection"
            );
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("invalid portable tool rejection should read");
            let body: Value = serde_json::from_slice(&body)
                .expect("invalid portable tool rejection should be JSON");
            assert!(
                downstream_error_code(&body, protocol).is_some(),
                "{case_name}"
            );
            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                0,
                "{case_name}: reject before Provider Key resolution"
            );
            assert!(upstream.requests().await.is_empty(), "{case_name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert!(log.final_error_code.is_some(), "{case_name}");
            assert_eq!(log.total_tokens, None, "{case_name}");
            assert_eq!(log.estimated_cost_nanos, None, "{case_name}");
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

fn reasoning_downstream_request(
    protocol: DownstreamProtocol,
    requested_model: &str,
    is_stream: bool,
) -> Value {
    match protocol {
        DownstreamProtocol::Openai => json!({
            "model":requested_model,"messages":[{"role":"user","content":"reason"}],
            "reasoning_effort":"minimal","stream":is_stream
        }),
        DownstreamProtocol::Responses => json!({
            "model":requested_model,"input":"reason",
            "reasoning":{"effort":"high","summary":"detailed"},"stream":is_stream
        }),
        DownstreamProtocol::Anthropic => json!({
            "model":requested_model,"max_tokens":2048,
            "messages":[{"role":"user","content":"reason"}],
            "thinking":{"type":"enabled","budget_tokens":1024},
            "output_config":{"effort":"max"},"stream":is_stream
        }),
        DownstreamProtocol::Gemini => json!({
            "contents":[{"role":"user","parts":[{"text":"reason"}]}],
            "generationConfig":{"thinkingConfig":{
                "thinkingLevel":"medium","includeThoughts":true
            }}
        }),
    }
}

fn anthropic_reasoning_response() -> Value {
    json!({
        "id":"msg_reasoning","type":"message","role":"assistant",
        "content":[
            {"type":"thinking","thinking":"private direct reasoning","signature":"direct-signature-secret"},
            {"type":"text","text":"public answer"}
        ],
        "model":UPSTREAM_MODEL,"stop_reason":"end_turn","stop_sequence":null,
        "usage":{"input_tokens":11,"output_tokens":7}
    })
}

fn assert_reasoning_target(
    protocol: DownstreamProtocol,
    target: &Value,
    is_stream: bool,
    case_name: &str,
) {
    assert_eq!(target["model"], UPSTREAM_MODEL, "{case_name}");
    assert_eq!(target["stream"], is_stream, "{case_name}");
    match protocol {
        DownstreamProtocol::Openai => {
            assert_eq!(target["thinking"], json!({"type":"adaptive"}));
            assert_eq!(target["output_config"], json!({"effort":"low"}));
        }
        DownstreamProtocol::Responses => {
            assert_eq!(target["thinking"], json!({"type":"adaptive"}));
            assert_eq!(target["output_config"], json!({"effort":"high"}));
        }
        DownstreamProtocol::Anthropic => {
            assert_eq!(
                target["thinking"],
                json!({"type":"enabled","budget_tokens":1024})
            );
            assert_eq!(target["output_config"], json!({"effort":"max"}));
        }
        DownstreamProtocol::Gemini => {
            assert_eq!(target["thinking"], json!({"type":"adaptive"}));
            assert_eq!(target["output_config"], json!({"effort":"medium"}));
        }
    }
}

fn assert_anthropic_reasoning_cell(test_name: &'static str, protocol: DownstreamProtocol) {
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == protocol)
        .expect("Anthropic target fixture for reasoning");
    run_case(test_name, move |context| async move {
        let upstream =
            TestUpstream::spawn_json(StatusCode::OK, anthropic_reasoning_response()).await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let request = reasoning_downstream_request(protocol, &router.requested_model(), false);

        let response = router.send(&fixture, false, &request).await;

        assert_eq!(response.status(), StatusCode::OK, "{test_name}");
        assert_no_public_transform_diagnostics(&response);
        let request_id = assert_downstream_request_identity(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("reasoning response should read");
        let body: Value = serde_json::from_slice(&body).expect("reasoning response JSON");
        let encoded = body.to_string();
        assert!(encoded.contains("public answer"), "{test_name}");
        if protocol == DownstreamProtocol::Anthropic {
            assert!(encoded.contains("private direct reasoning"), "{test_name}");
            assert!(encoded.contains("direct-signature-secret"), "{test_name}");
        } else if protocol == DownstreamProtocol::Openai {
            assert_eq!(
                body.pointer("/choices/0/message/reasoning_content"),
                Some(&json!("private direct reasoning")),
                "{test_name}: reasoning uses the dedicated OpenAI field"
            );
            assert!(!encoded.contains("direct-signature-secret"), "{test_name}");
        } else {
            assert!(encoded.contains("private direct reasoning"), "{test_name}");
            assert!(!encoded.contains("direct-signature-secret"), "{test_name}");
        }

        let captured = upstream.requests().await;
        assert_eq!(captured.len(), 1, "{test_name}: exactly one upstream call");
        assert_eq!(captured[0].path, "/v1/messages");
        assert_eq!(
            captured[0]
                .headers
                .get(&X_REQUEST_ID)
                .and_then(|value| value.to_str().ok()),
            Some(request_id.as_str())
        );
        let target: Value =
            serde_json::from_slice(&captured[0].body).expect("reasoning Anthropic target body");
        assert_reasoning_target(protocol, &target, false, test_name);

        let log = router.wait_for_log(RequestStatus::Success).await;
        assert!(!format!("{log:?}").contains("direct-signature-secret"));
        assert_log_common(&router, &fixture, &log);
        assert!(!log.is_stream);
        assert_eq!(log.upstream_http_status, Some(200));
        assert_eq!(
            (
                log.total_input_tokens,
                log.total_output_tokens,
                log.total_tokens
            ),
            (Some(11), Some(7), Some(18))
        );
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        upstream.shutdown().await;
    });
}

macro_rules! anthropic_reasoning_cell_test {
    ($name:ident, $protocol:expr) => {
        #[test]
        fn $name() {
            assert_anthropic_reasoning_cell(stringify!($name), $protocol);
        }
    };
}

anthropic_reasoning_cell_test!(
    openai_to_anthropic_reasoning_cell_has_typed_controlled_loss,
    DownstreamProtocol::Openai
);
anthropic_reasoning_cell_test!(
    responses_to_anthropic_reasoning_cell_has_typed_controlled_loss,
    DownstreamProtocol::Responses
);
anthropic_reasoning_cell_test!(
    anthropic_to_anthropic_reasoning_cell_is_full,
    DownstreamProtocol::Anthropic
);
anthropic_reasoning_cell_test!(
    gemini_to_anthropic_reasoning_cell_has_typed_controlled_loss,
    DownstreamProtocol::Gemini
);

fn anthropic_reasoning_stream_events() -> Vec<GoldenEvent> {
    vec![
        GoldenEvent {
            event: Some("message_start".to_string()),
            data: json!({
                "type":"message_start","message":{
                    "id":"msg_reasoning_stream","type":"message","role":"assistant",
                    "content":[],"model":UPSTREAM_MODEL,"stop_reason":null,"stop_sequence":null,
                    "usage":{"input_tokens":11,"output_tokens":0}
                }
            }),
        },
        GoldenEvent {
            event: Some("content_block_start".to_string()),
            data: json!({
                "type":"content_block_start","index":0,
                "content_block":{"type":"thinking","thinking":"","signature":null}
            }),
        },
        GoldenEvent {
            event: Some("content_block_delta".to_string()),
            data: json!({
                "type":"content_block_delta","index":0,
                "delta":{"type":"thinking_delta","thinking":"private direct stream reasoning"}
            }),
        },
        GoldenEvent {
            event: Some("content_block_delta".to_string()),
            data: json!({
                "type":"content_block_delta","index":0,
                "delta":{"type":"signature_delta","signature":"direct-stream-signature-secret"}
            }),
        },
        GoldenEvent {
            event: Some("content_block_stop".to_string()),
            data: json!({"type":"content_block_stop","index":0}),
        },
        GoldenEvent {
            event: Some("content_block_start".to_string()),
            data: json!({
                "type":"content_block_start","index":1,
                "content_block":{"type":"text","text":""}
            }),
        },
        GoldenEvent {
            event: Some("content_block_delta".to_string()),
            data: json!({
                "type":"content_block_delta","index":1,
                "delta":{"type":"text_delta","text":"public answer"}
            }),
        },
        GoldenEvent {
            event: Some("content_block_stop".to_string()),
            data: json!({"type":"content_block_stop","index":1}),
        },
        GoldenEvent {
            event: Some("message_delta".to_string()),
            data: json!({
                "type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},
                "usage":{"output_tokens":7}
            }),
        },
        GoldenEvent {
            event: Some("message_stop".to_string()),
            data: json!({"type":"message_stop"}),
        },
    ]
}

fn assert_anthropic_reasoning_stream_cell(test_name: &'static str, protocol: DownstreamProtocol) {
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == protocol)
        .expect("Anthropic target fixture for reasoning stream");
    run_case(test_name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Sse {
            events: anthropic_reasoning_stream_events(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let request = reasoning_downstream_request(protocol, &router.requested_model(), true);

        let response = router.send(&fixture, true, &request).await;

        assert_eq!(response.status(), StatusCode::OK, "{test_name}");
        let request_id = assert_downstream_request_identity(&response);
        let body = timeout(
            WAIT_TIMEOUT,
            axum::body::to_bytes(response.into_body(), usize::MAX),
        )
        .await
        .expect("reasoning stream should terminate")
        .expect("reasoning stream should read");
        let encoded = String::from_utf8_lossy(&body);
        assert!(encoded.contains("public answer"), "{test_name}");
        if protocol == DownstreamProtocol::Anthropic {
            assert!(
                encoded.contains("private direct stream reasoning"),
                "{test_name}"
            );
            assert!(
                encoded.contains("direct-stream-signature-secret"),
                "{test_name}"
            );
        } else if protocol == DownstreamProtocol::Openai {
            assert!(
                encoded.contains("private direct stream reasoning"),
                "{test_name}: reasoning uses the dedicated OpenAI delta field"
            );
            assert!(
                !encoded.contains("direct-stream-signature-secret"),
                "{test_name}"
            );
        } else {
            assert!(
                encoded.contains("private direct stream reasoning"),
                "{test_name}"
            );
            assert!(
                !encoded.contains("direct-stream-signature-secret"),
                "{test_name}"
            );
        }

        let captured = upstream.requests().await;
        assert_eq!(captured.len(), 1, "{test_name}: exactly one upstream call");
        assert_eq!(captured[0].path, "/v1/messages");
        assert_eq!(
            captured[0]
                .headers
                .get(&X_REQUEST_ID)
                .and_then(|value| value.to_str().ok()),
            Some(request_id.as_str())
        );
        let target: Value = serde_json::from_slice(&captured[0].body)
            .expect("reasoning stream Anthropic target body");
        assert_reasoning_target(protocol, &target, true, test_name);

        let log = router.wait_for_log(RequestStatus::Success).await;
        assert!(!format!("{log:?}").contains("direct-stream-signature-secret"));
        assert_log_common(&router, &fixture, &log);
        assert!(log.is_stream);
        assert_eq!(log.upstream_http_status, Some(200));
        assert_eq!(
            (
                log.total_input_tokens,
                log.total_output_tokens,
                log.total_tokens
            ),
            (Some(11), Some(7), Some(18))
        );
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        upstream.shutdown().await;
    });
}

macro_rules! anthropic_reasoning_stream_cell_test {
    ($name:ident, $protocol:expr) => {
        #[test]
        fn $name() {
            assert_anthropic_reasoning_stream_cell(stringify!($name), $protocol);
        }
    };
}

anthropic_reasoning_stream_cell_test!(
    openai_to_anthropic_reasoning_stream_is_verified,
    DownstreamProtocol::Openai
);
anthropic_reasoning_stream_cell_test!(
    responses_to_anthropic_reasoning_stream_is_verified,
    DownstreamProtocol::Responses
);
anthropic_reasoning_stream_cell_test!(
    anthropic_to_anthropic_reasoning_stream_is_verified,
    DownstreamProtocol::Anthropic
);
anthropic_reasoning_stream_cell_test!(
    gemini_to_anthropic_reasoning_stream_is_verified,
    DownstreamProtocol::Gemini
);

#[test]
fn gemini_positive_thinking_budget_to_anthropic_is_zero_call() {
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Gemini)
        .expect("Gemini to Anthropic target fixture");
    run_case(
        "gemini-positive-thinking-budget-anthropic",
        move |context| async move {
            let upstream =
                TestUpstream::spawn_json(StatusCode::OK, anthropic_reasoning_response()).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();
            let request = json!({
                "contents":[{"role":"user","parts":[{"text":"reason"}]}],
                "generationConfig":{"thinkingConfig":{"thinkingBudget":1024}}
            });

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("positive thinking budget rejection should read");
            let body: Value =
                serde_json::from_slice(&body).expect("positive thinking budget rejection JSON");
            assert!(downstream_error_code(&body, DownstreamProtocol::Gemini).is_some());
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty());
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("invalid_request_error")
            );
            assert_eq!(log.total_tokens, None);
            assert_eq!(log.estimated_cost_nanos, None);
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1);
            upstream.shutdown().await;
        },
    );
}

fn anthropic_structured_downstream_request(
    protocol: DownstreamProtocol,
    requested_model: &str,
) -> Value {
    let schema = json!({
        "type":"object",
        "properties":{"answer":{"type":"string","minLength":2,"x-opaque":{"keep":true}}},
        "required":["answer"],
        "additionalProperties":false
    });
    match protocol {
        DownstreamProtocol::Openai => json!({
            "model":requested_model,"messages":[{"role":"user","content":"answer as JSON"}],
            "max_tokens":128,"reasoning_effort":"low",
            "tools":[{"type":"function","function":{
                "name":"lookup","parameters":{"type":"object"},"strict":true
            }}],
            "response_format":{"type":"json_schema","json_schema":{
                "name":"answer_contract","description":"A stable answer",
                "schema":schema,"strict":true
            }}
        }),
        DownstreamProtocol::Responses => json!({
            "model":requested_model,"input":"answer as JSON","max_output_tokens":128,
            "reasoning":{"effort":"high"},
            "tools":[{"type":"function","name":"lookup",
                "parameters":{"type":"object"},"strict":true}],
            "text":{"format":{"type":"json_schema","name":"answer_contract",
                "description":"A stable answer","schema":schema,"strict":true}}
        }),
        DownstreamProtocol::Anthropic => json!({
            "model":requested_model,"max_tokens":128,
            "messages":[{"role":"user","content":"answer as JSON"}],
            "thinking":{"type":"adaptive"},
            "tools":[{"name":"lookup","input_schema":{"type":"object"},"strict":true}],
            "output_config":{"effort":"max","format":{
                "type":"json_schema","schema":schema
            }}
        }),
        DownstreamProtocol::Gemini => json!({
            "contents":[{"role":"user","parts":[{"text":"answer as JSON"}]}],
            "tools":[{"functionDeclarations":[{
                "name":"lookup","parameters":{"type":"object"}
            }]}],
            "generationConfig":{
                "maxOutputTokens":128,
                "thinkingConfig":{"thinkingLevel":"medium"},
                "responseMimeType":"application/json",
                "responseJsonSchema":{
                    "type":"object","propertyOrdering":["answer"],
                    "properties":{"answer":{"type":"string","minLength":2,
                        "propertyOrdering":[],"x-opaque":{"keep":true}}},
                    "required":["answer"],"additionalProperties":false
                }
            }
        }),
    }
}

fn assert_anthropic_structured_output_cell(test_name: &'static str, protocol: DownstreamProtocol) {
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == protocol)
        .expect("Anthropic target fixture for structured output");
    run_case(test_name, move |context| async move {
        let upstream =
            TestUpstream::spawn_json(StatusCode::OK, anthropic_multimodal_response()).await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let request = anthropic_structured_downstream_request(protocol, &router.requested_model());

        let transformed = crate::service::transform::transform_request_data(
            request.clone(),
            protocol,
            UpstreamProtocol::Anthropic,
            false,
        )
        .expect("structured direct cell must transform");
        let has_controlled_loss = transformed.summary.facts.iter().any(|fact| {
            matches!(
                fact.outcome,
                TransformOutcomeKind::ControlledLossMinor
                    | TransformOutcomeKind::ControlledLossMajor
            )
        });
        assert_eq!(
            has_controlled_loss,
            protocol != DownstreamProtocol::Anthropic,
            "{test_name}"
        );
        assert!(
            transformed
                .summary
                .facts
                .iter()
                .all(|fact| fact.safe_summary.is_none())
        );

        let response = router.send(&fixture, false, &request).await;

        assert_eq!(response.status(), StatusCode::OK, "{test_name}");
        assert_no_public_transform_diagnostics(&response);
        let request_id = assert_downstream_request_identity(&response);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("structured response should complete");
        let captured = upstream.requests().await;
        assert_eq!(captured.len(), 1, "{test_name}: exactly one upstream call");
        assert_eq!(captured[0].path, "/v1/messages");
        assert!(captured[0].headers.get("anthropic-beta").is_none());
        assert_eq!(
            captured[0]
                .headers
                .get(&X_REQUEST_ID)
                .and_then(|value| value.to_str().ok()),
            Some(request_id.as_str())
        );
        let target: Value =
            serde_json::from_slice(&captured[0].body).expect("structured Anthropic target body");
        assert_eq!(target["model"], UPSTREAM_MODEL, "{test_name}");
        if protocol == DownstreamProtocol::Anthropic {
            assert!(target.get("stream").is_none(), "{test_name}");
        } else {
            assert_eq!(target["stream"], false, "{test_name}");
        }
        assert_eq!(target["tools"][0]["name"], "lookup", "{test_name}");
        assert_eq!(target["thinking"]["type"], "adaptive", "{test_name}");
        let format = &target["output_config"]["format"];
        assert_eq!(format["type"], "json_schema", "{test_name}");
        assert_eq!(
            format["schema"]["properties"]["answer"]["minLength"], 2,
            "{test_name}"
        );
        assert_eq!(
            format["schema"]["properties"]["answer"]["x-opaque"]["keep"], true,
            "{test_name}"
        );
        assert!(!format["schema"].to_string().contains("propertyOrdering"));
        assert!(format.get("name").is_none());
        assert!(format.get("description").is_none());
        assert!(format.get("strict").is_none());

        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_log_common(&router, &fixture, &log);
        assert!(!log.is_stream);
        assert_eq!(log.upstream_http_status, Some(200));
        assert_eq!(
            (
                log.total_input_tokens,
                log.total_output_tokens,
                log.total_tokens
            ),
            (Some(11), Some(7), Some(18))
        );
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        upstream.shutdown().await;
    });
}

macro_rules! anthropic_structured_output_cell_test {
    ($name:ident, $protocol:expr) => {
        #[test]
        fn $name() {
            assert_anthropic_structured_output_cell(stringify!($name), $protocol);
        }
    };
}

anthropic_structured_output_cell_test!(
    openai_to_anthropic_structured_output_cell_has_typed_controlled_loss,
    DownstreamProtocol::Openai
);
anthropic_structured_output_cell_test!(
    responses_to_anthropic_structured_output_cell_has_typed_controlled_loss,
    DownstreamProtocol::Responses
);
anthropic_structured_output_cell_test!(
    anthropic_to_anthropic_structured_output_cell_is_full,
    DownstreamProtocol::Anthropic
);
anthropic_structured_output_cell_test!(
    gemini_to_anthropic_structured_output_cell_has_typed_controlled_loss,
    DownstreamProtocol::Gemini
);

#[test]
fn openai_to_anthropic_structured_tools_thinking_stream_is_verified() {
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Openai)
        .expect("OpenAI to Anthropic target fixture");
    run_case(
        "openai-anthropic-structured-stream-combination",
        move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Sse {
                events: anthropic_reasoning_stream_events(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let mut request = anthropic_structured_downstream_request(
                DownstreamProtocol::Openai,
                &router.requested_model(),
            );
            request["stream"] = json!(true);

            let response = router.send(&fixture, true, &request).await;

            assert_eq!(response.status(), StatusCode::OK);
            let body = timeout(
                WAIT_TIMEOUT,
                axum::body::to_bytes(response.into_body(), usize::MAX),
            )
            .await
            .expect("structured stream should terminate")
            .expect("structured stream should read");
            assert!(String::from_utf8_lossy(&body).contains("public answer"));
            let captured = upstream.requests().await;
            assert_eq!(captured.len(), 1);
            let target: Value =
                serde_json::from_slice(&captured[0].body).expect("structured stream target JSON");
            assert_eq!(target["stream"], true);
            assert_eq!(target["tools"][0]["name"], "lookup");
            assert_eq!(target["thinking"], json!({"type":"adaptive"}));
            assert_eq!(target["output_config"]["effort"], "low");
            assert_eq!(target["output_config"]["format"]["type"], "json_schema");
            assert!(captured[0].headers.get("anthropic-beta").is_none());
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert!(log.is_stream);
            assert_eq!(log.total_tokens, Some(18));
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1);
            upstream.shutdown().await;
        },
    );
}

#[test]
fn invalid_structured_outputs_to_anthropic_are_zero_call() {
    for (case_name, protocol) in [
        (
            "openai-anthropic-json-object-zero-call",
            DownstreamProtocol::Openai,
        ),
        (
            "responses-anthropic-grammar-zero-call",
            DownstreamProtocol::Responses,
        ),
        (
            "anthropic-invalid-schema-zero-call",
            DownstreamProtocol::Anthropic,
        ),
        (
            "gemini-structured-conflict-zero-call",
            DownstreamProtocol::Gemini,
        ),
    ] {
        let (_, fixture) = anthropic_target_fixtures()
            .into_iter()
            .find(|(_, fixture)| fixture.protocol == protocol)
            .expect("Anthropic target fixture for invalid structured output");
        run_case(case_name, move |context| async move {
            let upstream =
                TestUpstream::spawn_json(StatusCode::OK, anthropic_multimodal_response()).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();
            let model = router.requested_model();
            let request = match protocol {
                DownstreamProtocol::Openai => json!({
                    "model":model,"messages":[{"role":"user","content":"json"}],
                    "response_format":{"type":"json_object"}
                }),
                DownstreamProtocol::Responses => json!({
                    "model":model,"input":"json","text":{"format":{
                        "type":"grammar","grammar":"private-schema-marker"
                    }}
                }),
                DownstreamProtocol::Anthropic => json!({
                    "model":model,"max_tokens":64,
                    "messages":[{"role":"user","content":"json"}],
                    "output_config":{"format":{"type":"json_schema",
                        "schema":["private-schema-marker"]}}
                }),
                DownstreamProtocol::Gemini => json!({
                    "contents":[{"role":"user","parts":[{"text":"json"}]}],
                    "generationConfig":{
                        "responseFormat":{"text":{"mimeType":"application/json",
                            "schema":{"type":"object"}}},
                        "responseJsonSchema":{"private":"private-schema-marker"}
                    }
                }),
            };

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case_name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("structured rejection should read");
            assert!(!String::from_utf8_lossy(&body).contains("private-schema-marker"));
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty(), "{case_name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(log.total_tokens, None);
            assert_eq!(log.estimated_cost_nanos, None);
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1);
            upstream.shutdown().await;
        });
    }
}

#[test]
fn cross_wire_unknown_anthropic_target_field_is_zero_call() {
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("OpenAI to Anthropic target fixture");
    run_case(
        "openai-anthropic-unknown-field",
        move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                fixture.non_stream.upstream_response.clone(),
            )
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .admin
                .request_patch
                .create_source_variant(
                    router.source_id,
                    RequestPatchVariantInput {
                        source_id: router.source_id,
                        model_id: None,
                        suffix: None,
                        enabled: true,
                        expose_in_models: false,
                        rules: vec![RequestPatchRuleInput {
                            placement: RequestPatchPlacement::Body,
                            target: "/messages/0/content".to_string(),
                            operation: RequestPatchOperation::Set,
                            value_json: Some(Some(json!([{
                                "type": "server_tool_use",
                                "name": "payload-secret-marker"
                            }]))),
                            description: Some(
                                "Cross-Wire unknown Anthropic field rejection".to_string(),
                            ),
                        }],
                    },
                )
                .await
                .expect("unknown Anthropic target Patch should save for runtime validation");
            router
                .app_state
                .catalog
                .invalidate_models_catalog()
                .await
                .expect("unknown Anthropic target Patch should invalidate catalog");
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("unknown target field rejection should read");
            let body: Value = serde_json::from_slice(&body).expect("rejection should be JSON");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("provider_configuration_error")
            );
            assert!(!body.to_string().contains("payload-secret-marker"));
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty());
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("provider_configuration_error")
            );
            assert!(
                !log.final_error_message
                    .unwrap_or_default()
                    .contains("payload-secret-marker")
            );
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        },
    );
}

fn assert_anthropic_non_stream_base_cell(test_name: &'static str, protocol: DownstreamProtocol) {
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == protocol)
        .expect("Anthropic target fixture for protocol");
    run_case(test_name, move |context| async move {
        let upstream =
            TestUpstream::spawn_json(StatusCode::OK, fixture.non_stream.upstream_response.clone())
                .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let (catalog_id, catalog_version_id) = router.attach_cost_catalog(Some(100), Some(2)).await;

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::OK, "{protocol:?}");
        assert_no_public_transform_diagnostics(&response);
        let request_id = assert_downstream_request_identity(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Anthropic non-stream response should complete");
        let body: Value =
            serde_json::from_slice(&body).expect("downstream response should be JSON");
        assert!(body.to_string().contains("baseline pong"), "{protocol:?}");
        let (pointer, expected) = match protocol {
            DownstreamProtocol::Openai => ("/choices/0/finish_reason", "stop"),
            DownstreamProtocol::Responses => ("/status", "completed"),
            DownstreamProtocol::Anthropic => ("/stop_reason", "end_turn"),
            DownstreamProtocol::Gemini => ("/candidates/0/finishReason", "STOP"),
        };
        assert_eq!(
            body.pointer(pointer).and_then(Value::as_str),
            Some(expected),
            "{protocol:?}: terminal mapping"
        );
        let captured = upstream.requests().await;
        assert_upstream(
            test_name,
            &fixture,
            &captured,
            &fixture.request.upstream_path,
            None,
            &fixture.request.upstream,
            &router.requested_model(),
            &request_id,
        );
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_log_common(&router, &fixture, &log);
        assert_log_timing_order(&log);
        assert!(!log.is_stream);
        assert_eq!(log.upstream_http_status, Some(200));
        assert_usage(&log, &fixture.usage);
        assert_eq!(log.input_text_tokens, Some(11));
        assert_eq!(log.cache_read_tokens, Some(3));
        assert_eq!(log.cache_write_tokens, Some(2));
        assert_eq!(log.cost_catalog_id, Some(catalog_id));
        assert_eq!(log.cost_catalog_version_id, Some(catalog_version_id));
        assert_eq!(log.estimated_cost_nanos, Some(128));
        let snapshot: CostSnapshot = serde_json::from_str(
            log.cost_snapshot_json
                .as_deref()
                .expect("Anthropic usage cost snapshot should persist"),
        )
        .expect("Anthropic usage cost snapshot should parse");
        assert_eq!(
            snapshot.unmatched_items,
            vec![
                MeterKey::LlmOutputTextTokens.to_string(),
                MeterKey::LlmCacheWriteTokens.to_string(),
            ]
        );
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        assert_eq!(upstream.requests().await.len(), 1);
        upstream.shutdown().await;
    });
}

fn assert_anthropic_stream_cache_usage_cell(test_name: &'static str, protocol: DownstreamProtocol) {
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == protocol)
        .expect("Anthropic target fixture for protocol");
    run_case(test_name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Sse {
            events: fixture.stream.upstream_events.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        router.attach_cost_catalog(Some(100), Some(2)).await;

        let response = router
            .send(&fixture, true, &fixture.stream.downstream_request)
            .await;

        assert_eq!(response.status(), StatusCode::OK, "{protocol:?}");
        let request_id = assert_downstream_request_identity(&response);
        let body = timeout(
            WAIT_TIMEOUT,
            axum::body::to_bytes(response.into_body(), usize::MAX),
        )
        .await
        .expect("Anthropic stream usage response should terminate")
        .expect("Anthropic stream usage response should read");
        let events = parse_downstream_events(protocol, &body);
        assert_eq!(
            stream_text(protocol, &events),
            "baseline pong",
            "{protocol:?}"
        );
        let captured = upstream.requests().await;
        assert_upstream(
            test_name,
            &fixture,
            &captured,
            &fixture.stream.upstream_path,
            None,
            &fixture.stream.upstream_request,
            &router.requested_model(),
            &request_id,
        );
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_log_common(&router, &fixture, &log);
        assert!(log.is_stream);
        assert_usage(&log, &fixture.usage);
        assert_eq!(log.input_text_tokens, Some(11));
        assert_eq!(log.cache_read_tokens, Some(3));
        assert_eq!(log.cache_write_tokens, Some(2));
        assert_eq!(log.estimated_cost_nanos, Some(128));
        let snapshot: CostSnapshot = serde_json::from_str(
            log.cost_snapshot_json
                .as_deref()
                .expect("Anthropic stream cache cost snapshot should persist"),
        )
        .expect("Anthropic stream cache cost snapshot should parse");
        assert!(
            snapshot
                .unmatched_items
                .contains(&MeterKey::LlmCacheWriteTokens.to_string())
        );
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        assert_eq!(upstream.requests().await.len(), 1);
        upstream.shutdown().await;
    });
}

macro_rules! anthropic_base_cell_test {
    ($name:ident, $protocol:expr) => {
        #[test]
        fn $name() {
            assert_anthropic_non_stream_base_cell(stringify!($name), $protocol);
            assert_anthropic_stream_cache_usage_cell(stringify!($name), $protocol);
        }
    };
}

anthropic_base_cell_test!(
    openai_to_anthropic_base_cell_success_is_verified,
    DownstreamProtocol::Openai
);
anthropic_base_cell_test!(
    responses_to_anthropic_base_cell_success_is_verified,
    DownstreamProtocol::Responses
);
anthropic_base_cell_test!(
    anthropic_to_anthropic_base_cell_success_is_verified,
    DownstreamProtocol::Anthropic
);
anthropic_base_cell_test!(
    gemini_to_anthropic_base_cell_success_is_verified,
    DownstreamProtocol::Gemini
);

fn assert_stream_failure_has_no_usage_or_cost(log: &RequestLogRecord, case_name: &str) {
    assert_eq!(log.total_input_tokens, None, "{case_name}");
    assert_eq!(log.total_output_tokens, None, "{case_name}");
    assert_eq!(log.total_tokens, None, "{case_name}");
    assert_eq!(log.input_text_tokens, None, "{case_name}");
    assert_eq!(log.cache_read_tokens, None, "{case_name}");
    assert_eq!(log.cache_write_tokens, None, "{case_name}");
    assert_eq!(log.estimated_cost_nanos, None, "{case_name}");
    assert_eq!(log.cost_snapshot_json, None, "{case_name}");
}

#[test]
fn anthropic_stream_message_stop_closes_hanging_upstream_for_all_downstreams() {
    for (name, fixture) in anthropic_target_fixtures() {
        let case_name = format!("anthropic-stream-success-terminal-{name}");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let dropped = Arc::new(DropSignal::default());
            let upstream = TestUpstream::spawn(ScriptedReply::ChunkedSse {
                content_encoding: None,
                chunks: vec![events_to_sse_bytes(&fixture.stream.upstream_events)],
                hang_after_chunks: true,
                dropped: Some(Arc::clone(&dropped)),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let request_id = assert_downstream_request_identity(&response);
            let body = timeout(
                WAIT_TIMEOUT,
                axum::body::to_bytes(response.into_body(), usize::MAX),
            )
            .await
            .expect("message_stop should close the downstream Body")
            .expect("successful Anthropic stream should remain readable");
            let events = parse_downstream_events(fixture.protocol, &body);
            assert_eq!(
                stream_text(fixture.protocol, &events),
                "baseline pong",
                "{case_name}"
            );
            dropped.wait().await;

            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.stream.upstream_path,
                None,
                &fixture.stream.upstream_request,
                &router.requested_model(),
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_log_common(&router, &fixture, &log);
            assert_usage(&log, &fixture.usage);
            assert_eq!(log.estimated_cost_nanos, Some(128), "{case_name}");
            assert!(log.final_error_code.is_none(), "{case_name}");
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            assert_eq!(captured.len(), 1, "{case_name}: no retry");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn anthropic_stream_error_is_raw_same_wire_and_one_native_terminal_cross_wire() {
    let stream_error_event = anthropic_target_golden().error.stream_error_event;
    for (name, fixture) in anthropic_target_fixtures() {
        let case_name = format!("anthropic-stream-error-{name}");
        let runtime_name = case_name.clone();
        let stream_error_event = stream_error_event.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Sse {
                events: vec![stream_error_event.clone()],
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;
            let persisted_sink = router.install_recording_persisted_sink();

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let request_id = assert_downstream_request_identity(&response);
            let body = timeout(
                WAIT_TIMEOUT,
                axum::body::to_bytes(response.into_body(), usize::MAX),
            )
            .await
            .expect("Anthropic error event should terminate the Body")
            .expect("protocol terminal errors should close normally");
            let events = parse_downstream_events(fixture.protocol, &body);
            assert_eq!(events.len(), 1, "{case_name}: exactly one terminal event");
            if fixture.protocol == DownstreamProtocol::Anthropic {
                assert_eq!(events[0], stream_error_event, "{case_name}: raw same-wire");
            } else {
                assert_native_fatal_stream_event(fixture.protocol, &events[0], &request_id);
                assert!(
                    !body
                        .windows("baseline stream failure".len())
                        .any(|window| window == b"baseline stream failure"),
                    "{case_name}: upstream payload must not cross Wire"
                );
            }

            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_log_common(&router, &fixture, &log);
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error"),
                "{case_name}"
            );
            assert_stream_failure_has_no_usage_or_cost(&log, &case_name);
            router.wait_for_api_key_lease_release().await;
            router.assert_no_api_key_usage_charge(&case_name).await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            assert_eq!(persisted_sink.contexts.lock().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn anthropic_stream_eof_emits_one_native_error_and_clears_partial_usage() {
    for (name, fixture) in anthropic_target_fixtures() {
        let case_name = format!("anthropic-stream-eof-{name}");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Sse {
                events: vec![fixture.stream.upstream_events[0].clone()],
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let request_id = assert_downstream_request_identity(&response);
            let body = timeout(
                WAIT_TIMEOUT,
                axum::body::to_bytes(response.into_body(), usize::MAX),
            )
            .await
            .expect("Anthropic EOF should terminate the downstream Body")
            .expect("EOF transform error should use a native terminal event");
            let events = parse_downstream_events(fixture.protocol, &body);
            let terminal = events.last().expect("EOF native terminal event");
            assert_native_fatal_stream_event(fixture.protocol, terminal, &request_id);
            assert_eq!(
                events
                    .iter()
                    .filter(|event| match fixture.protocol {
                        DownstreamProtocol::Openai => {
                            event.data["error"]["code"] == "upstream_response_error"
                        }
                        DownstreamProtocol::Responses => {
                            event.data["type"] == "response.error"
                                && event.data["error"]["code"] == "upstream_response_error"
                        }
                        DownstreamProtocol::Anthropic => {
                            event.event.as_deref() == Some("error")
                                && event.data["error"]["code"] == "upstream_response_error"
                        }
                        DownstreamProtocol::Gemini => {
                            event.data["error"]["details"][0]["reason"] == "UPSTREAM_RESPONSE_ERROR"
                        }
                    })
                    .count(),
                1,
                "{case_name}: exactly one target error terminal"
            );

            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_log_common(&router, &fixture, &log);
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error"),
                "{case_name}"
            );
            assert_stream_failure_has_no_usage_or_cost(&log, &case_name);
            router.wait_for_api_key_lease_release().await;
            router.assert_no_api_key_usage_charge(&case_name).await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn anthropic_stream_client_cancellation_logs_cancelled_and_releases_once_for_all_downstreams() {
    for (name, fixture) in anthropic_target_fixtures() {
        let case_name = format!("anthropic-stream-cancel-{name}");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let dropped = Arc::new(DropSignal::default());
            let upstream_prefix = fixture.stream.upstream_events[..4].to_vec();
            let upstream = TestUpstream::spawn(ScriptedReply::ChunkedSse {
                content_encoding: None,
                chunks: vec![events_to_sse_bytes(&upstream_prefix)],
                hang_after_chunks: true,
                dropped: Some(Arc::clone(&dropped)),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;
            let persisted_sink = router.install_recording_persisted_sink();

            let response = router
                .send(&fixture, true, &fixture.cancellation.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let request_id = assert_downstream_request_identity(&response);
            let mut body = response.into_body().into_data_stream();
            let first = timeout(WAIT_TIMEOUT, body.next())
                .await
                .expect("first transformed frame deadline")
                .expect("first transformed frame")
                .expect("first transformed frame should be readable");
            assert!(!first.is_empty(), "{case_name}");
            drop(body);
            dropped.wait().await;

            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.cancellation.upstream_path,
                None,
                &fixture.cancellation.upstream_request,
                &router.requested_model(),
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Cancelled).await;
            assert_log_common(&router, &fixture, &log);
            assert_eq!(log.overall_status, RequestStatus::Cancelled, "{case_name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("client_cancelled_error"),
                "{case_name}"
            );
            assert_stream_failure_has_no_usage_or_cost(&log, &case_name);
            router.wait_for_api_key_lease_release().await;
            router.assert_no_api_key_usage_charge(&case_name).await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            assert_eq!(captured.len(), 1, "{case_name}: no retry");
            assert_single_persisted_terminal_fact(
                &persisted_sink,
                ExecutionStage::DownstreamSend,
                ResponseVisibility::BodyStarted,
            )
            .await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn anthropic_target_precommit_client_cancellation_returns_499_and_releases_once() {
    for (name, fixture) in anthropic_target_fixtures() {
        let case_name = format!("anthropic-target-precommit-cancel-{name}");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let dropped = Arc::new(DropSignal::default());
            let upstream = TestUpstream::spawn(ScriptedReply::HangingBody {
                content_type: "application/json".to_string(),
                first_chunk: br#"{"#.to_vec(),
                dropped: Arc::clone(&dropped),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;
            let cancellation = ProxyCancellationContext::new();
            let cancellation_trigger = cancellation.clone();
            let captured_requests = Arc::clone(&upstream.captured);
            let cancel_task = tokio::spawn(async move {
                let deadline = Instant::now() + WAIT_TIMEOUT;
                loop {
                    if !captured_requests.lock().await.is_empty() {
                        cancellation_trigger.cancel_now("Anthropic target client disconnected");
                        return;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "Anthropic target request should reach upstream before cancellation"
                    );
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            });

            let response = timeout(
                WAIT_TIMEOUT,
                router.send_with_cancellation(
                    &fixture,
                    false,
                    &fixture.request.downstream,
                    cancellation,
                ),
            )
            .await
            .expect("cancelled Anthropic target request should finish");
            cancel_task
                .await
                .expect("Anthropic target cancellation trigger should join");

            assert_eq!(response.status().as_u16(), 499, "{case_name}");
            let request_id = assert_downstream_request_identity(&response);
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("cancellation envelope should read");
            dropped.wait().await;
            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.request.upstream_path,
                None,
                &fixture.request.upstream,
                &router.requested_model(),
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Cancelled).await;
            assert_log_common(&router, &fixture, &log);
            assert_eq!(log.overall_status, RequestStatus::Cancelled, "{case_name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("client_cancelled_error"),
                "{case_name}"
            );
            assert_stream_failure_has_no_usage_or_cost(&log, &case_name);
            router.wait_for_api_key_lease_release().await;
            router.assert_no_api_key_usage_charge(&case_name).await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            assert_eq!(captured.len(), 1, "{case_name}: no retry");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn anthropic_same_wire_non_stream_preserves_unknown_response_bytes() {
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Anthropic)
        .expect("Anthropic same-wire target fixture");
    run_case(
        "anthropic-same-wire-non-stream-extension",
        move |context| async move {
            let original = br#"{
  "id": "msg_extension",
  "type": "message",
  "role": "assistant",
  "content": [{"type":"vendor_future_block","private":{"opaque":true}}],
  "model": "baseline-upstream-model",
  "stop_reason": "vendor_future_stop",
  "stop_sequence": null,
  "usage": {"input_tokens":11,"output_tokens":7,"cache_read_input_tokens":3,"cache_creation_input_tokens":2},
  "vendor_extension": {"spacing":"must remain exact"}
}
"#
            .to_vec();
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("application/json".to_string()),
                content_encoding: None,
                body: original.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), StatusCode::OK);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("same-wire extension response should read");
            assert_eq!(body.as_ref(), original.as_slice());
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_log_common(&router, &fixture, &log);
            assert_eq!(log.upstream_http_status, Some(200));
            assert_eq!(log.total_input_tokens, Some(16));
            assert_eq!(log.total_output_tokens, Some(7));
            assert_eq!(log.total_tokens, Some(23));
            assert_eq!(log.input_text_tokens, Some(11));
            assert_eq!(log.cache_read_tokens, Some(3));
            assert_eq!(log.cache_write_tokens, Some(2));
            assert_eq!(log.estimated_cost_nanos, Some(128));
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1);
            assert_eq!(upstream.requests().await.len(), 1);
            upstream.shutdown().await;
        },
    );
}

#[test]
fn anthropic_unknown_non_stream_terminal_fails_closed_cross_wire_without_cost() {
    const PRIVATE_REASON: &str = "vendor_private_stop_reason";
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Openai)
        .expect("OpenAI to Anthropic target fixture");
    let mut upstream_body = fixture.non_stream.upstream_response.clone();
    upstream_body["stop_reason"] = json!(PRIVATE_REASON);
    upstream_body["vendor_extension"] = json!({"private": PRIVATE_REASON});
    run_case(
        "openai-anthropic-unknown-non-stream-terminal",
        move |context| async move {
            let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_body).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
            let request_id = assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("unknown Anthropic terminal error should read");
            assert_payload_free_transform_bytes(&body, PRIVATE_REASON);
            let body: Value = serde_json::from_slice(&body).expect("error should be JSON");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("upstream_response_error")
            );
            let captured = upstream.requests().await;
            assert_upstream(
                "openai-anthropic-unknown-non-stream-terminal",
                &fixture,
                &captured,
                &fixture.request.upstream_path,
                None,
                &fixture.request.upstream,
                &router.requested_model(),
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error")
            );
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains(PRIVATE_REASON)
            );
            assert_eq!(log.total_tokens, None);
            assert_eq!(log.estimated_cost_nanos, None);
            assert_eq!(log.cost_snapshot_json, None);
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1);
            assert_eq!(upstream.requests().await.len(), 1);
            upstream.shutdown().await;
        },
    );
}

#[test]
fn anthropic_final_validation_rejects_before_credentials_and_network() {
    let (_, fixture) = anthropic_target_fixtures()
        .into_iter()
        .find(|(name, _)| *name == "anthropic")
        .expect("Anthropic same-wire target fixture");
    let cases = [
        (
            "max-tokens-zero",
            json!({
                "model":"$REQUESTED_MODEL","max_tokens":0,
                "messages":[{"role":"user","content":"private-payload-marker"}]
            }),
            "/max_tokens",
        ),
        (
            "invalid-role",
            json!({
                "model":"$REQUESTED_MODEL","max_tokens":64,
                "messages":[{"role":"system","content":"private-payload-marker"}]
            }),
            "/messages/*/role",
        ),
        (
            "invalid-tool-schema",
            json!({
                "model":"$REQUESTED_MODEL","max_tokens":64,
                "messages":[{"role":"user","content":"private-payload-marker"}],
                "tools":[{"name":"lookup","input_schema":"private-payload-marker"}]
            }),
            "/tools/*/input_schema",
        ),
        (
            "invalid-thinking-output-combination",
            json!({
                "model":"$REQUESTED_MODEL","max_tokens":64,
                "messages":[{"role":"user","content":"private-payload-marker"}],
                "thinking":{"type":"disabled"},"output_config":{"effort":"high"}
            }),
            "/output_config/effort",
        ),
    ];

    for (case, request, expected_path) in cases {
        let fixture = fixture.clone();
        run_case(case, move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                fixture.non_stream.upstream_response.clone(),
            )
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("invalid target response should read");
            let body: Value = serde_json::from_slice(&body).expect("error response should be JSON");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("invalid_request_error"),
                "{case}"
            );
            assert!(
                !body.to_string().contains("private-payload-marker"),
                "{case}"
            );
            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                0,
                "{case}: final validation must precede credentials"
            );
            assert!(upstream.requests().await.is_empty(), "{case}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("invalid_request_error")
            );
            let error = log.final_error_message.unwrap_or_default();
            assert!(error.contains(expected_path), "{case}: {error}");
            assert!(!error.contains("private-payload-marker"), "{case}");
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn malformed_request_is_rejected_before_request_record_or_upstream_call() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

        let response = router.send_malformed_generation_body(&fixture).await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_downstream_request_identity(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("parse error response body should read");
        let body: Value =
            serde_json::from_slice(&body).expect("parse error response should be JSON");
        assert_eq!(
            downstream_error_code(&body, fixture.protocol),
            Some("invalid_request_error")
        );
        assert!(router.request_logs().await.is_empty());
        assert!(upstream.requests().await.is_empty());
        upstream.shutdown().await;
    });
}

#[test]
fn cross_protocol_shape_failure_is_rejected_before_credential_or_upstream_use() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "responses")
        .expect("responses fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let persisted_sink = router.install_recording_persisted_sink();
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(
                &fixture,
                false,
                &json!({"model": "$REQUESTED_MODEL", "input": 42}),
            )
            .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("transform rejection response should read");
        let body: Value = serde_json::from_slice(&body).expect("response should be JSON");
        assert_eq!(
            downstream_error_code(&body, fixture.protocol),
            Some("invalid_request_error")
        );
        assert_eq!(
            router.app_state.secret_encryption.decrypt_call_count(),
            0,
            "request transform must precede provider credential decryption"
        );
        assert!(upstream.requests().await.is_empty());
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("invalid_request_error")
        );
        assert_single_persisted_terminal_fact(
            &persisted_sink,
            ExecutionStage::Parse,
            ResponseVisibility::NotVisible,
        )
        .await;
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn responses_stateful_controls_are_rejected_before_credential_or_upstream_use() {
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "responses")
        .expect("responses fixture");
    for (case, field, value) in [
        ("store", "store", json!(true)),
        (
            "previous-response",
            "previous_response_id",
            json!("response-state-private-marker"),
        ),
        (
            "conversation",
            "conversation",
            json!({"id": "conversation-state-private-marker"}),
        ),
        ("background", "background", json!(true)),
    ] {
        let fixture = fixture.clone();
        run_case(case, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::OK,
                body: json!({"upstream": "upstream-private-marker"}),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let source_id = router
                .replace_default_source_profile(&upstream.base_url, UpstreamProfileType::Responses)
                .await;
            let mut request = fixture.request.downstream.clone();
            request["input"] = json!([{
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "prompt-private-marker"},
                    {"type": "input_image", "image_url": "data:image/png;base64,media-private-marker"}
                ]
            }]);
            request["tools"] = json!([{
                "type": "function",
                "name": "private_tool",
                "parameters": {
                    "type": "object",
                    "properties": {"schema-private-marker": {"type": "string"}}
                }
            }]);
            request["metadata"] = json!({"arguments": "tool-arguments-private-marker"});
            request[field] = value;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("stateless rejection response should read");
            let error_body: Value =
                serde_json::from_slice(&body).expect("stateless rejection should be JSON");
            assert_eq!(
                downstream_error_code(&error_body, fixture.protocol),
                Some("invalid_request_error"),
                "{case}"
            );
            let public_body = String::from_utf8_lossy(&body);
            for marker in [
                "prompt-private-marker",
                "response-state-private-marker",
                "conversation-state-private-marker",
                "tool-arguments-private-marker",
                "schema-private-marker",
                "media-private-marker",
                "upstream-private-marker",
            ] {
                assert!(!public_body.contains(marker), "{case}: {marker}");
            }
            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                0,
                "{case}: stateless policy must precede credential decryption"
            );
            assert!(upstream.requests().await.is_empty(), "{case}");
            let log = router
                .wait_for_log_for_source(source_id, RequestStatus::Error)
                .await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("invalid_request_error")
            );
            let log_error = log.final_error_message.unwrap_or_default();
            for marker in [
                "prompt-private-marker",
                "response-state-private-marker",
                "conversation-state-private-marker",
                "tool-arguments-private-marker",
                "schema-private-marker",
                "media-private-marker",
                "upstream-private-marker",
            ] {
                assert!(!log_error.contains(marker), "{case}: logged {marker}");
            }
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn responses_target_materializes_native_requests_for_all_public_downstreams_and_modes() {
    for (name, fixture) in responses_target_fixtures() {
        for is_stream in [false, true] {
            let fixture = fixture.clone();
            let case_name = format!(
                "responses-materialize-{name}-{}",
                if is_stream { "stream" } else { "non-stream" }
            );
            let runtime_name = case_name.clone();
            run_case(&runtime_name, move |context| async move {
                let upstream = TestUpstream::spawn(ScriptedReply::Json {
                    status: StatusCode::TOO_MANY_REQUESTS,
                    body: fixture.error.upstream_response.clone(),
                })
                .await;
                let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
                let downstream_request = if is_stream {
                    &fixture.stream.downstream_request
                } else {
                    &fixture.request.downstream
                };

                let response = router.send(&fixture, is_stream, downstream_request).await;

                assert_eq!(
                    response.status(),
                    StatusCode::TOO_MANY_REQUESTS,
                    "{case_name}"
                );
                let request_id = assert_downstream_request_identity(&response);
                axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("upstream error response should be consumed");
                let captured = upstream.requests().await;
                let (expected_body, expected_path) = if is_stream {
                    (
                        &fixture.stream.upstream_request,
                        &fixture.stream.upstream_path,
                    )
                } else {
                    (&fixture.request.upstream, &fixture.request.upstream_path)
                };
                assert_upstream(
                    name,
                    &fixture,
                    &captured,
                    expected_path,
                    None,
                    expected_body,
                    &router.requested_model(),
                    &request_id,
                );
                assert_eq!(
                    captured[0]
                        .headers
                        .get("accept-encoding")
                        .and_then(|value| value.to_str().ok()),
                    Some(if is_stream {
                        "identity"
                    } else {
                        "gzip, identity"
                    }),
                    "{case_name}"
                );
                assert!(
                    !captured[0].headers.contains_key("x-api-key"),
                    "{case_name}"
                );
                assert!(
                    !captured[0].headers.contains_key("x-goog-api-key"),
                    "{case_name}"
                );
                assert_eq!(
                    router.app_state.secret_encryption.decrypt_call_count(),
                    1,
                    "{case_name}: one request resolves one Provider Key"
                );
                let log = router.wait_for_log(RequestStatus::Error).await;
                assert_log_common(&router, &fixture, &log);
                assert_eq!(log.upstream_http_status, Some(429), "{case_name}");
                assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
                router.wait_for_api_key_lease_release().await;
                upstream.shutdown().await;
            });
        }
    }
}

fn assert_responses_base_success_cell(test_name: &'static str, protocol: DownstreamProtocol) {
    let (_, fixture) = responses_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == protocol)
        .expect("Responses target fixture for protocol");

    let non_stream_fixture = fixture.clone();
    let non_stream_name = format!("{test_name}-non-stream");
    run_case(&non_stream_name, move |context| async move {
        let upstream = TestUpstream::spawn_json(
            StatusCode::OK,
            non_stream_fixture.non_stream.upstream_response.clone(),
        )
        .await;
        let router = RouterFixture::new(context, &non_stream_fixture, &upstream.base_url).await;
        let (catalog_id, catalog_version_id) = router.attach_cost_catalog(Some(100), Some(2)).await;

        let response = router
            .send(
                &non_stream_fixture,
                false,
                &non_stream_fixture.request.downstream,
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK, "{protocol:?}");
        assert_no_public_transform_diagnostics(&response);
        let request_id = assert_downstream_request_identity(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("non-stream response should complete");
        assert!(
            String::from_utf8_lossy(&body).contains("baseline pong"),
            "{protocol:?}"
        );
        let captured = upstream.requests().await;
        assert_upstream(
            test_name,
            &non_stream_fixture,
            &captured,
            &non_stream_fixture.request.upstream_path,
            None,
            &non_stream_fixture.request.upstream,
            &router.requested_model(),
            &request_id,
        );
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_eq!(log.request_id, request_id);
        assert_log_common(&router, &non_stream_fixture, &log);
        assert_log_timing_order(&log);
        assert!(!log.is_stream);
        assert_eq!(log.upstream_http_status, Some(200));
        assert_usage(&log, &non_stream_fixture.usage);
        assert_eq!(log.cache_read_tokens, Some(3));
        assert_eq!(log.reasoning_tokens, Some(2));
        assert_eq!(log.cost_catalog_id, Some(catalog_id));
        assert_eq!(log.cost_catalog_version_id, Some(catalog_version_id));
        assert_eq!(log.estimated_cost_nanos, Some(122));
        assert!(log.cost_snapshot_json.is_some());
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        assert_eq!(upstream.requests().await.len(), 1);
        upstream.shutdown().await;
    });

    let stream_fixture = fixture;
    let stream_name = format!("{test_name}-stream");
    run_case(&stream_name, move |context| async move {
        let mut upstream_events = stream_fixture.stream.upstream_events.clone();
        let terminal_sequence = upstream_events
            .last()
            .and_then(|event| event.data["sequence_number"].as_u64())
            .expect("terminal sequence");
        upstream_events.last_mut().expect("terminal event").data["sequence_number"] =
            json!(terminal_sequence + 1);
        upstream_events.insert(
            upstream_events.len() - 1,
            GoldenEvent {
                event: Some("response.usage".to_string()),
                data: json!({
                    "type":"response.usage","sequence_number":terminal_sequence,
                    "usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}
                }),
            },
        );
        let dropped = Arc::new(DropSignal::default());
        let upstream = TestUpstream::spawn(ScriptedReply::ChunkedSse {
            content_encoding: None,
            chunks: vec![events_to_sse_bytes(&upstream_events)],
            hang_after_chunks: true,
            dropped: Some(Arc::clone(&dropped)),
        })
        .await;
        let router = RouterFixture::new(context, &stream_fixture, &upstream.base_url).await;
        let (catalog_id, catalog_version_id) = router.attach_cost_catalog(Some(100), Some(2)).await;

        let response = router
            .send(
                &stream_fixture,
                true,
                &stream_fixture.stream.downstream_request,
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK, "{protocol:?}");
        assert_no_public_transform_diagnostics(&response);
        let request_id = assert_downstream_request_identity(&response);
        let body = timeout(
            WAIT_TIMEOUT,
            axum::body::to_bytes(response.into_body(), usize::MAX),
        )
        .await
        .expect("completed terminal should close the downstream body")
        .expect("stream body should be readable");
        let events = parse_downstream_events(protocol, &body);
        assert_eq!(
            stream_text(protocol, &events),
            "baseline pong",
            "{protocol:?}"
        );
        let terminal_count = events
            .iter()
            .filter(|event| match protocol {
                DownstreamProtocol::Openai => event.data == json!("[DONE]"),
                DownstreamProtocol::Responses => event.data["type"] == "response.completed",
                DownstreamProtocol::Anthropic => event.event.as_deref() == Some("message_stop"),
                DownstreamProtocol::Gemini => event.data["candidates"][0]["finishReason"]
                    .as_str()
                    .is_some(),
            })
            .count();
        assert_eq!(terminal_count, 1, "{protocol:?}: exactly one terminal");
        dropped.wait().await;
        let captured = upstream.requests().await;
        assert_upstream(
            test_name,
            &stream_fixture,
            &captured,
            &stream_fixture.stream.upstream_path,
            None,
            &stream_fixture.stream.upstream_request,
            &router.requested_model(),
            &request_id,
        );
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_eq!(log.request_id, request_id);
        assert_log_common(&router, &stream_fixture, &log);
        assert_log_timing_order(&log);
        assert!(log.is_stream);
        assert!(log.first_token_at.is_some());
        assert_eq!(log.upstream_http_status, Some(200));
        assert_usage(&log, &stream_fixture.usage);
        assert_eq!(log.cache_read_tokens, Some(3));
        assert_eq!(log.reasoning_tokens, Some(2));
        assert_eq!(log.cost_catalog_id, Some(catalog_id));
        assert_eq!(log.cost_catalog_version_id, Some(catalog_version_id));
        assert_eq!(log.estimated_cost_nanos, Some(122));
        assert!(log.cost_snapshot_json.is_some());
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        assert_eq!(upstream.requests().await.len(), 1);
        upstream.shutdown().await;
    });
}

macro_rules! responses_base_success_cell_test {
    ($name:ident, $protocol:expr) => {
        #[test]
        fn $name() {
            assert_responses_base_success_cell(stringify!($name), $protocol);
        }
    };
}

responses_base_success_cell_test!(
    openai_to_responses_base_cell_success_is_verified,
    DownstreamProtocol::Openai
);
responses_base_success_cell_test!(
    responses_to_responses_base_cell_success_is_verified,
    DownstreamProtocol::Responses
);
responses_base_success_cell_test!(
    anthropic_to_responses_base_cell_success_is_verified,
    DownstreamProtocol::Anthropic
);
responses_base_success_cell_test!(
    gemini_to_responses_base_cell_success_is_verified,
    DownstreamProtocol::Gemini
);

#[test]
fn responses_unversioned_create_alias_executes_the_same_native_contract() {
    let (_, mut fixture) = responses_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Responses)
        .expect("Responses target fixture");
    fixture.downstream_path = "/responses/responses".to_string();

    run_case("responses-unversioned-create", move |context| async move {
        let upstream =
            TestUpstream::spawn_json(StatusCode::OK, fixture.non_stream.upstream_response.clone())
                .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        let request_id = assert_downstream_request_identity(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("unversioned Create response should read");
        assert!(String::from_utf8_lossy(&body).contains("baseline pong"));
        let captured = upstream.requests().await;
        assert_upstream(
            "responses-unversioned-create",
            &fixture,
            &captured,
            &fixture.request.upstream_path,
            None,
            &fixture.request.upstream,
            &router.requested_model(),
            &request_id,
        );
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_log_common(&router, &fixture, &log);
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        upstream.shutdown().await;
    });
}

#[test]
fn responses_public_routes_enforce_methods_before_authentication_without_side_effects() {
    let (_, fixture) = responses_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Responses)
        .expect("Responses target fixture");

    run_case(
        "responses-route-method-boundary",
        move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                fixture.non_stream.upstream_response.clone(),
            )
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            for (method, path, expected_allow) in [
                (Method::GET, "/responses/responses", "POST"),
                (Method::GET, "/responses/v1/responses", "POST"),
                (Method::POST, "/responses/models", "GET,HEAD"),
                (Method::POST, "/responses/v1/models", "GET,HEAD"),
            ] {
                let response = router
                    .send_raw_method(
                        method,
                        path.to_string(),
                        Some(json!({"private": "route-method-private-marker"})),
                        None,
                    )
                    .await;
                assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{path}");
                assert_eq!(
                    response
                        .headers()
                        .get("allow")
                        .and_then(|value| value.to_str().ok()),
                    Some(expected_allow),
                    "{path}"
                );
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("method rejection should read");
                assert_payload_free_transform_bytes(&body, "route-method-private-marker");
                let body: Value = serde_json::from_slice(&body).expect("error should be JSON");
                assert_eq!(
                    downstream_error_code(&body, DownstreamProtocol::Responses),
                    Some("method_not_allowed_error"),
                    "{path}"
                );
            }

            router.app_state.flush_proxy_logs().await;
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty());
            assert!(router.request_logs().await.is_empty());
            upstream.shutdown().await;
        },
    );
}

#[test]
fn responses_stateful_resource_paths_are_unregistered_and_have_zero_side_effects() {
    let (_, fixture) = responses_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Responses)
        .expect("Responses target fixture");

    run_case(
        "responses-stateful-route-boundary",
        move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                fixture.non_stream.upstream_response.clone(),
            )
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            for prefix in ["/responses", "/responses/v1"] {
                for (method, suffix) in [
                    (Method::GET, "/responses/resp_route_private_marker"),
                    (Method::DELETE, "/responses/resp_route_private_marker"),
                    (
                        Method::GET,
                        "/responses/resp_route_private_marker/input_items",
                    ),
                    (Method::POST, "/responses/resp_route_private_marker/cancel"),
                    (Method::POST, "/conversations"),
                    (Method::GET, "/conversations/conv_route_private_marker"),
                    (Method::DELETE, "/conversations/conv_route_private_marker"),
                    (
                        Method::GET,
                        "/conversations/conv_route_private_marker/items",
                    ),
                    (
                        Method::POST,
                        "/conversations/conv_route_private_marker/items",
                    ),
                    (Method::GET, "/models/model_route_private_marker"),
                ] {
                    let path = format!("{prefix}{suffix}");
                    let body = (method != Method::GET).then(|| {
                        json!({
                            "input": "stateful-route-body-private-marker",
                            "metadata": {"private": true}
                        })
                    });
                    let response = router
                        .send_raw_method(method, path.clone(), body, Some(DownstreamAuth::Bearer))
                        .await;
                    assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
                    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                        .await
                        .expect("route-not-found response should read");
                    assert_payload_free_transform_bytes(&body, "route_private_marker");
                    assert_payload_free_transform_bytes(
                        &body,
                        "stateful-route-body-private-marker",
                    );
                    let body: Value = serde_json::from_slice(&body).expect("error should be JSON");
                    assert_eq!(
                        downstream_error_code(&body, DownstreamProtocol::Responses),
                        Some("route_not_found_error"),
                        "{path}"
                    );
                }
            }

            router.app_state.flush_proxy_logs().await;
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty());
            assert!(router.request_logs().await.is_empty());
            upstream.shutdown().await;
        },
    );
}

#[test]
fn responses_non_stream_completed_usage_and_missing_usage_drive_success_costs() {
    let (_, fixture) = responses_target_fixtures()
        .into_iter()
        .find(|(name, _)| *name == "responses")
        .expect("Responses target fixture");

    for missing_usage in [false, true] {
        let fixture = fixture.clone();
        let case_name = if missing_usage {
            "responses-non-stream-missing-usage"
        } else {
            "responses-non-stream-completed-usage"
        };
        run_case(case_name, move |context| async move {
            let mut upstream_body = fixture.non_stream.upstream_response.clone();
            if missing_usage {
                upstream_body
                    .as_object_mut()
                    .expect("response fixture object")
                    .remove("usage");
            } else {
                upstream_body["usage"]["total_tokens"] = json!(999);
            }
            upstream_body["vendor_extension"] = json!({"preserved": true});
            let raw_body = serde_json::to_vec_pretty(&upstream_body).expect("response serializes");
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("application/json; charset=utf-8".to_string()),
                content_encoding: None,
                body: raw_body.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let (catalog_id, catalog_version_id) =
                router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let downstream_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("Responses body should read");
            assert_eq!(downstream_body.as_ref(), raw_body.as_slice(), "{case_name}");
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(log.cost_catalog_id, Some(catalog_id));
            assert_eq!(log.cost_catalog_version_id, Some(catalog_version_id));
            let snapshot: CostSnapshot = serde_json::from_str(
                log.cost_snapshot_json
                    .as_deref()
                    .expect("successful response should persist cost"),
            )
            .expect("cost snapshot should parse");
            if missing_usage {
                assert_eq!(log.total_input_tokens, None);
                assert_eq!(log.total_output_tokens, None);
                assert_eq!(log.total_tokens, None);
                assert_eq!(log.estimated_cost_nanos, Some(100));
                assert_eq!(snapshot.total_cost_nanos, 100);
                assert_eq!(snapshot.detail_lines.len(), 1);
                assert_eq!(
                    snapshot.detail_lines[0].meter_key,
                    MeterKey::InvokeRequestCalls
                );
            } else {
                assert_eq!(log.total_input_tokens, Some(11));
                assert_eq!(log.total_output_tokens, Some(7));
                assert_eq!(log.total_tokens, Some(18));
                assert_eq!(log.cache_read_tokens, Some(3));
                assert_eq!(log.reasoning_tokens, Some(2));
                assert_eq!(log.estimated_cost_nanos, Some(122));
                assert_eq!(snapshot.total_cost_nanos, 122);
                assert!(
                    snapshot
                        .warnings
                        .iter()
                        .any(|warning| { warning.contains("999") && warning.contains("18") })
                );
            }
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn responses_non_stream_incomplete_is_billed_but_failed_and_illegal_finals_are_not() {
    let target = responses_target_golden();
    let (_, fixture) = responses_target_fixtures()
        .into_iter()
        .find(|(name, _)| *name == "responses")
        .expect("Responses target fixture");

    for (case_name, mut upstream_body, expected_http, expected_log_status, expected_cost) in [
        (
            "responses-non-stream-incomplete",
            {
                let mut body = fixture.non_stream.upstream_response.clone();
                body["status"] = json!("incomplete");
                body["completed_at"] = Value::Null;
                body["incomplete_details"] = json!({"reason": "max_tokens"});
                body["output"][0]["status"] = json!("incomplete");
                body
            },
            StatusCode::OK,
            RequestStatus::Success,
            Some(122),
        ),
        (
            "responses-non-stream-failed",
            target.error.failed_response,
            StatusCode::OK,
            RequestStatus::Error,
            None,
        ),
        (
            "responses-non-stream-illegal-in-progress",
            {
                let mut body = fixture.non_stream.upstream_response.clone();
                body["status"] = json!("in_progress");
                body
            },
            StatusCode::BAD_GATEWAY,
            RequestStatus::Error,
            None,
        ),
    ] {
        let fixture = fixture.clone();
        upstream_body["vendor_extension"] = json!({"private": "terminal-marker"});
        let raw_body = serde_json::to_vec_pretty(&upstream_body).expect("response serializes");
        run_case(case_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("application/json".to_string()),
                content_encoding: None,
                body: raw_body.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), expected_http, "{case_name}");
            let downstream_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("terminal response should read");
            if expected_http == StatusCode::OK {
                assert_eq!(downstream_body.as_ref(), raw_body.as_slice(), "{case_name}");
            } else {
                assert!(!String::from_utf8_lossy(&downstream_body).contains("terminal-marker"));
            }
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            let log = router.wait_for_log(expected_log_status.clone()).await;
            assert_eq!(log.estimated_cost_nanos, expected_cost, "{case_name}");
            if expected_log_status == RequestStatus::Success {
                assert_eq!(log.total_tokens, Some(18));
                assert!(log.cost_snapshot_json.is_some());
            } else {
                assert_eq!(log.total_input_tokens, None);
                assert_eq!(log.total_output_tokens, None);
                assert_eq!(log.total_tokens, None);
                assert!(log.cost_snapshot_json.is_none());
                assert_eq!(
                    log.final_error_code.as_deref(),
                    Some("upstream_response_error")
                );
            }
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn responses_stream_completed_and_incomplete_close_upstream_and_bill_once_for_all_downstreams() {
    let target = responses_target_golden();
    for (name, fixture) in responses_target_fixtures() {
        for incomplete in [false, true] {
            let fixture = fixture.clone();
            let mut upstream_events = target.stream_events.clone();
            if incomplete {
                let terminal = upstream_events
                    .last_mut()
                    .expect("Responses stream terminal fixture");
                terminal.event = Some("response.incomplete".to_string());
                terminal.data["type"] = json!("response.incomplete");
                terminal.data["response"]["status"] = json!("incomplete");
                terminal.data["response"]["incomplete_details"] =
                    json!({"reason": "max_output_tokens"});
                terminal.data["response"]["output"][0]["status"] = json!("incomplete");
            }
            let terminal_sequence = upstream_events
                .last()
                .and_then(|event| event.data["sequence_number"].as_u64())
                .expect("terminal sequence number");
            upstream_events
                .last_mut()
                .expect("Responses stream terminal fixture")
                .data["sequence_number"] = json!(terminal_sequence + 1);
            upstream_events.insert(
                upstream_events.len() - 1,
                GoldenEvent {
                    event: Some("response.usage".to_string()),
                    data: json!({
                        "type": "response.usage",
                        "sequence_number": terminal_sequence,
                        "usage": {
                            "input_tokens": 1,
                            "output_tokens": 1,
                            "total_tokens": 2
                        }
                    }),
                },
            );
            let mut preflight =
                StreamTransformer::new(UpstreamProtocol::Responses, fixture.protocol);
            for event in &upstream_events {
                preflight
                    .transform_event_with_observation(SseEvent {
                        event: event.event.clone(),
                        data: event_data_text(&event.data),
                        ..Default::default()
                    })
                    .unwrap_or_else(|failure| {
                        panic!(
                            "{name}: fixture event {:?} failed: {failure:?}",
                            event.event
                        )
                    });
            }
            let case_name = format!(
                "responses-stream-{name}-{}",
                if incomplete {
                    "incomplete"
                } else {
                    "completed"
                }
            );
            let runtime_name = case_name.clone();
            run_case(&runtime_name, move |context| async move {
                let dropped = Arc::new(DropSignal::default());
                let upstream = TestUpstream::spawn(ScriptedReply::ChunkedSse {
                    content_encoding: None,
                    chunks: vec![events_to_sse_bytes(&upstream_events)],
                    hang_after_chunks: true,
                    dropped: Some(Arc::clone(&dropped)),
                })
                .await;
                let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
                let (catalog_id, catalog_version_id) =
                    router.attach_cost_catalog(Some(100), Some(2)).await;

                let response = router
                    .send(&fixture, true, &fixture.stream.downstream_request)
                    .await;

                assert_eq!(response.status(), StatusCode::OK, "{case_name}");
                let body = timeout(
                    WAIT_TIMEOUT,
                    axum::body::to_bytes(response.into_body(), usize::MAX),
                )
                .await
                .expect("typed terminal should close the downstream body")
                .expect("successful Responses stream should remain readable");
                let downstream_events = parse_downstream_events(fixture.protocol, &body);
                let terminal_count = downstream_events
                    .iter()
                    .filter(|event| match fixture.protocol {
                        DownstreamProtocol::Openai => event.data == json!("[DONE]"),
                        DownstreamProtocol::Responses => {
                            event.data["type"]
                                == json!(if incomplete {
                                    "response.incomplete"
                                } else {
                                    "response.completed"
                                })
                        }
                        DownstreamProtocol::Anthropic => {
                            event.event.as_deref() == Some("message_stop")
                        }
                        DownstreamProtocol::Gemini => event.data["candidates"][0]["finishReason"]
                            .as_str()
                            .is_some(),
                    })
                    .count();
                assert_eq!(
                    terminal_count,
                    1,
                    "{case_name}: one body terminal; events={downstream_events:?}; body={}",
                    String::from_utf8_lossy(&body)
                );
                dropped.wait().await;

                let log = router.wait_for_log(RequestStatus::Success).await;
                assert_eq!(log.total_input_tokens, Some(11), "{case_name}");
                assert_eq!(log.total_output_tokens, Some(7), "{case_name}");
                assert_eq!(log.total_tokens, Some(18), "{case_name}");
                assert_eq!(log.cache_read_tokens, Some(3), "{case_name}");
                assert_eq!(log.reasoning_tokens, Some(2), "{case_name}");
                assert_eq!(log.cost_catalog_id, Some(catalog_id), "{case_name}");
                assert_eq!(
                    log.cost_catalog_version_id,
                    Some(catalog_version_id),
                    "{case_name}"
                );
                assert_eq!(log.estimated_cost_nanos, Some(122), "{case_name}");
                assert!(log.cost_snapshot_json.is_some(), "{case_name}");
                router.wait_for_api_key_lease_release().await;
                assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
                assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
                upstream.shutdown().await;
            });
        }
    }
}

#[test]
fn responses_stream_failed_and_error_events_emit_one_terminal_without_cost_for_all_downstreams() {
    let target = responses_target_golden();
    for (name, fixture) in responses_target_fixtures() {
        for independent_error in [false, true] {
            let fixture = fixture.clone();
            let usage_event = GoldenEvent {
                event: Some("response.usage".to_string()),
                data: json!({
                    "type": "response.usage",
                    "sequence_number": 1,
                    "usage": {
                        "input_tokens": 11,
                        "output_tokens": 7,
                        "total_tokens": 18,
                        "input_tokens_details": {"cached_tokens": 3},
                        "output_tokens_details": {"reasoning_tokens": 2}
                    }
                }),
            };
            let upstream_events = if independent_error {
                let mut error_event = target.error.error_event.clone();
                error_event.data["sequence_number"] = json!(2);
                vec![target.stream_events[0].clone(), usage_event, error_event]
            } else {
                let mut failed_response = target.error.failed_response.clone();
                failed_response["id"] = json!("resp_baseline");
                vec![
                    target.stream_events[0].clone(),
                    usage_event,
                    GoldenEvent {
                        event: Some("response.failed".to_string()),
                        data: json!({
                            "type": "response.failed",
                            "sequence_number": 2,
                            "response": failed_response,
                        }),
                    },
                ]
            };
            let case_name = format!(
                "responses-stream-{name}-{}",
                if independent_error { "error" } else { "failed" }
            );
            let runtime_name = case_name.clone();
            run_case(&runtime_name, move |context| async move {
                let dropped = Arc::new(DropSignal::default());
                let upstream = TestUpstream::spawn(ScriptedReply::ChunkedSse {
                    content_encoding: None,
                    chunks: vec![events_to_sse_bytes(&upstream_events)],
                    hang_after_chunks: true,
                    dropped: Some(Arc::clone(&dropped)),
                })
                .await;
                let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
                router.attach_cost_catalog(Some(100), Some(2)).await;

                let response = router
                    .send(&fixture, true, &fixture.stream.downstream_request)
                    .await;

                assert_eq!(response.status(), StatusCode::OK, "{case_name}");
                let request_id = assert_downstream_request_identity(&response);
                let body = timeout(
                    WAIT_TIMEOUT,
                    axum::body::to_bytes(response.into_body(), usize::MAX),
                )
                .await
                .expect("failed Responses terminal should close the downstream body")
                .expect("native failure terminal should close normally");
                let downstream_events = parse_downstream_events(fixture.protocol, &body);
                if fixture.protocol == DownstreamProtocol::Responses {
                    let expected_type = if independent_error {
                        "error"
                    } else {
                        "response.failed"
                    };
                    assert_eq!(
                        downstream_events
                            .iter()
                            .filter(|event| event.data["type"] == expected_type)
                            .count(),
                        1,
                        "{case_name}: same-wire source terminal must be preserved once"
                    );
                    assert!(
                        downstream_events
                            .iter()
                            .all(|event| event.data["type"] != "response.error"),
                        "{case_name}: no second gateway terminal"
                    );
                } else {
                    let terminal = downstream_events
                        .last()
                        .expect("cross-wire application failure terminal");
                    assert_native_fatal_stream_event(fixture.protocol, terminal, &request_id);
                    let terminal_count = downstream_events
                        .iter()
                        .filter(|event| match fixture.protocol {
                            DownstreamProtocol::Openai => event.data.get("error").is_some(),
                            DownstreamProtocol::Anthropic => {
                                event.event.as_deref() == Some("error")
                            }
                            DownstreamProtocol::Gemini => event.data.get("error").is_some(),
                            DownstreamProtocol::Responses => unreachable!(),
                        })
                        .count();
                    assert_eq!(terminal_count, 1, "{case_name}: one body terminal");
                }
                dropped.wait().await;

                let log = router.wait_for_log(RequestStatus::Error).await;
                assert_eq!(
                    log.final_error_code.as_deref(),
                    Some("upstream_response_error"),
                    "{case_name}"
                );
                assert_eq!(log.total_input_tokens, None, "{case_name}");
                assert_eq!(log.total_output_tokens, None, "{case_name}");
                assert_eq!(log.total_tokens, None, "{case_name}");
                assert_eq!(log.estimated_cost_nanos, None, "{case_name}");
                assert!(log.cost_snapshot_json.is_none(), "{case_name}");
                router.wait_for_api_key_lease_release().await;
                router.assert_no_api_key_usage_charge(&case_name).await;
                assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
                assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
                upstream.shutdown().await;
            });
        }
    }
}

#[test]
fn responses_stream_eof_without_terminal_fails_closed_for_all_downstreams() {
    let target = responses_target_golden();
    for (name, fixture) in responses_target_fixtures() {
        let upstream_events = vec![
            target.stream_events[0].clone(),
            GoldenEvent {
                event: Some("response.usage".to_string()),
                data: json!({
                    "type": "response.usage",
                    "sequence_number": 1,
                    "usage": {
                        "input_tokens": 11,
                        "output_tokens": 7,
                        "total_tokens": 18
                    }
                }),
            },
        ];
        let case_name = format!("responses-stream-{name}-missing-terminal");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Sse {
                events: upstream_events,
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let request_id = assert_downstream_request_identity(&response);
            let body = timeout(
                WAIT_TIMEOUT,
                axum::body::to_bytes(response.into_body(), usize::MAX),
            )
            .await
            .expect("missing terminal EOF should fail before deadline")
            .expect("typed EOF failure should use one native terminal event");
            let downstream_events = parse_downstream_events(fixture.protocol, &body);
            let terminal = downstream_events
                .last()
                .expect("EOF failure terminal event");
            assert_native_fatal_stream_event(fixture.protocol, terminal, &request_id);
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error"),
                "{case_name}"
            );
            assert_eq!(log.total_input_tokens, None, "{case_name}");
            assert_eq!(log.total_output_tokens, None, "{case_name}");
            assert_eq!(log.total_tokens, None, "{case_name}");
            assert_eq!(log.estimated_cost_nanos, None, "{case_name}");
            assert!(log.cost_snapshot_json.is_none(), "{case_name}");
            router.wait_for_api_key_lease_release().await;
            router.assert_no_api_key_usage_charge(&case_name).await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn responses_stream_client_cancellation_closes_upstream_once_for_all_downstreams() {
    let target = responses_target_golden();
    for (name, fixture) in responses_target_fixtures() {
        let mut first_events = target.stream_events[..3].to_vec();
        first_events[1].data["sequence_number"] = json!(2);
        first_events[2].data["sequence_number"] = json!(3);
        first_events.insert(
            1,
            GoldenEvent {
                event: Some("response.usage".to_string()),
                data: json!({
                    "type": "response.usage",
                    "sequence_number": 1,
                    "usage": {
                        "input_tokens": 11,
                        "output_tokens": 7,
                        "total_tokens": 18
                    }
                }),
            },
        );
        let case_name = format!("responses-stream-{name}-cancelled");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let dropped = Arc::new(DropSignal::default());
            let upstream = TestUpstream::spawn(ScriptedReply::ChunkedSse {
                content_encoding: None,
                chunks: vec![events_to_sse_bytes(&first_events)],
                hang_after_chunks: true,
                dropped: Some(Arc::clone(&dropped)),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let mut body = response.into_body().into_data_stream();
            loop {
                let frame = timeout(WAIT_TIMEOUT, body.next())
                    .await
                    .expect("transformed frame deadline")
                    .expect("transformed frame")
                    .expect("transformed frame should be readable");
                if String::from_utf8_lossy(&frame).contains("baseline ") {
                    break;
                }
            }
            drop(body);
            dropped.wait().await;
            let log = router.wait_for_log(RequestStatus::Cancelled).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("client_cancelled_error"),
                "{case_name}"
            );
            assert_eq!(log.total_input_tokens, None, "{case_name}");
            assert_eq!(log.total_output_tokens, None, "{case_name}");
            assert_eq!(log.total_tokens, None, "{case_name}");
            assert_eq!(log.estimated_cost_nanos, None, "{case_name}");
            assert!(log.cost_snapshot_json.is_none(), "{case_name}");
            router.wait_for_api_key_lease_release().await;
            router.assert_no_api_key_usage_charge(&case_name).await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn responses_target_http_429_is_authentic_bounded_and_never_retried_for_all_downstreams() {
    const DISCLOSURE_LIMIT: usize = 1_024;
    const RAW_LIMIT: usize = 1_048_576;

    for (name, fixture) in responses_target_fixtures() {
        let case_name = format!("responses-target-{name}-http-429");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let marker = format!("responses-provider-private-marker-{name}");
            let mut upstream_body = marker.as_bytes().to_vec();
            upstream_body.resize(RAW_LIMIT + 1, b'x');
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::TOO_MANY_REQUESTS,
                content_type: Some("text/plain; private=discarded".to_string()),
                content_encoding: None,
                body: upstream_body,
            })
            .await;
            let mut router =
                RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
            router
                .replace_proxy_request_config(
                    context,
                    one_mib_non_stream_proxy_config(DISCLOSURE_LIMIT),
                )
                .await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(
                response.status(),
                StatusCode::TOO_MANY_REQUESTS,
                "{case_name}"
            );
            let request_id = assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("bounded Responses target error should read");
            let body: Value = serde_json::from_slice(&body).expect("error should be JSON");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("upstream_rate_limit_error"),
                "{case_name}"
            );
            assert_eq!(body["upstream_error"]["status"], 429, "{case_name}");
            assert_eq!(body["upstream_error"]["truncated"], true, "{case_name}");
            assert_eq!(
                body["upstream_error"]["captured_bytes"], DISCLOSURE_LIMIT,
                "{case_name}"
            );
            assert_eq!(
                body["upstream_error"]["limit_bytes"], DISCLOSURE_LIMIT,
                "{case_name}"
            );
            assert!(
                body["upstream_error"]["body_text"]
                    .as_str()
                    .is_some_and(|value| value.starts_with(&marker)),
                "{case_name}: bounded public extension should retain only the allowed prefix"
            );
            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.request.upstream_path,
                fixture.request.upstream_query.as_deref(),
                &fixture.request.upstream,
                &router.requested_model(),
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(log.upstream_http_status, Some(429), "{case_name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_rate_limit_error"),
                "{case_name}"
            );
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains(&marker),
                "{case_name}: provider body must not enter persisted diagnostics"
            );
            assert_eq!(log.estimated_cost_nanos, None, "{case_name}");
            assert_eq!(log.cost_snapshot_json, None, "{case_name}");
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn responses_target_success_body_limits_fail_without_cost_for_all_downstreams() {
    const LIMIT: usize = 1_048_576;

    for (name, fixture) in responses_target_fixtures() {
        for compressed in [false, true] {
            let fixture = fixture.clone();
            let case_name = format!(
                "responses-target-{name}-{}-body-limit",
                if compressed { "decoded" } else { "raw" }
            );
            let runtime_name = case_name.clone();
            run_case(&runtime_name, move |context| async move {
                let (content_encoding, body) = if compressed {
                    let decoded =
                        serde_json::to_vec(&json_body_with_exact_serialized_size(LIMIT + 1))
                            .expect("decoded body-limit fixture should serialize");
                    let encoded = gzip_bytes(&decoded);
                    assert!(encoded.len() < LIMIT, "{case_name}: isolate decoded limit");
                    (Some("gzip".to_string()), encoded)
                } else {
                    (None, vec![b'x'; LIMIT + 1])
                };
                let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                    status: StatusCode::OK,
                    content_type: Some("application/json".to_string()),
                    content_encoding,
                    body,
                })
                .await;
                let mut router =
                    RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
                router
                    .replace_proxy_request_config(context, one_mib_non_stream_proxy_config(65_536))
                    .await;
                router.attach_cost_catalog(Some(100), Some(2)).await;

                let response = router
                    .send(&fixture, false, &fixture.request.downstream)
                    .await;

                assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{case_name}");
                let request_id = assert_downstream_request_identity(&response);
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("Responses target body-limit envelope should read");
                let body: Value = serde_json::from_slice(&body).expect("error should be JSON");
                assert_eq!(
                    downstream_error_code(&body, fixture.protocol),
                    Some("upstream_response_error"),
                    "{case_name}"
                );
                let captured = upstream.requests().await;
                assert_upstream(
                    name,
                    &fixture,
                    &captured,
                    &fixture.request.upstream_path,
                    fixture.request.upstream_query.as_deref(),
                    &fixture.request.upstream,
                    &router.requested_model(),
                    &request_id,
                );
                let log = router.wait_for_log(RequestStatus::Error).await;
                assert_eq!(log.upstream_http_status, Some(200), "{case_name}");
                assert_eq!(
                    log.final_error_code.as_deref(),
                    Some("upstream_response_error"),
                    "{case_name}"
                );
                assert_eq!(log.total_input_tokens, None, "{case_name}");
                assert_eq!(log.total_output_tokens, None, "{case_name}");
                assert_eq!(log.total_tokens, None, "{case_name}");
                assert_eq!(log.estimated_cost_nanos, None, "{case_name}");
                assert_eq!(log.cost_snapshot_json, None, "{case_name}");
                router.wait_for_api_key_lease_release().await;
                assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
                upstream.shutdown().await;
            });
        }
    }
}

#[test]
fn responses_unknown_incomplete_reason_fails_closed_cross_wire_without_cost() {
    const PRIVATE_REASON: &str = "future_private_incomplete_reason";

    for (name, fixture) in responses_target_fixtures()
        .into_iter()
        .filter(|(_, fixture)| fixture.protocol != DownstreamProtocol::Responses)
    {
        let mut upstream_body = fixture.non_stream.upstream_response.clone();
        upstream_body["status"] = json!("incomplete");
        upstream_body["completed_at"] = Value::Null;
        upstream_body["incomplete_details"] = json!({"reason": PRIVATE_REASON});
        upstream_body["output"][0]["status"] = json!("incomplete");
        upstream_body["vendor_extension"] = json!({"private": PRIVATE_REASON});
        let case_name = format!("responses-target-{name}-unknown-incomplete-reason");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::OK,
                body: upstream_body,
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{case_name}");
            let request_id = assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("unknown incomplete reason error should read");
            assert_payload_free_transform_bytes(&body, PRIVATE_REASON);
            let body: Value = serde_json::from_slice(&body).expect("error should be JSON");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("upstream_response_error"),
                "{case_name}"
            );
            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.request.upstream_path,
                fixture.request.upstream_query.as_deref(),
                &fixture.request.upstream,
                &router.requested_model(),
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error"),
                "{case_name}"
            );
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains(PRIVATE_REASON),
                "{case_name}: private incomplete reason must not enter logs"
            );
            assert_eq!(log.total_tokens, None, "{case_name}");
            assert_eq!(log.estimated_cost_nanos, None, "{case_name}");
            assert_eq!(log.cost_snapshot_json, None, "{case_name}");
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn responses_target_precommit_client_cancellation_returns_499_and_releases_once() {
    for (name, fixture) in responses_target_fixtures() {
        let case_name = format!("responses-target-{name}-precommit-cancel");
        let runtime_name = case_name.clone();
        run_case(&runtime_name, move |context| async move {
            let dropped = Arc::new(DropSignal::default());
            let upstream = TestUpstream::spawn(ScriptedReply::HangingBody {
                content_type: "application/json".to_string(),
                first_chunk: br#"{"#.to_vec(),
                dropped: Arc::clone(&dropped),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;
            let cancellation = ProxyCancellationContext::new();
            let cancellation_trigger = cancellation.clone();
            let captured_requests = Arc::clone(&upstream.captured);
            let cancel_task = tokio::spawn(async move {
                let deadline = Instant::now() + WAIT_TIMEOUT;
                loop {
                    if !captured_requests.lock().await.is_empty() {
                        cancellation_trigger.cancel_now("Responses client disconnected");
                        return;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "Responses request should reach upstream before cancellation"
                    );
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            });

            let response = timeout(
                WAIT_TIMEOUT,
                router.send_with_cancellation(
                    &fixture,
                    false,
                    &fixture.request.downstream,
                    cancellation,
                ),
            )
            .await
            .expect("cancelled Responses target request should finish");
            cancel_task
                .await
                .expect("Responses cancellation trigger should join");

            assert_eq!(response.status().as_u16(), 499, "{case_name}");
            let request_id = assert_downstream_request_identity(&response);
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("cancellation envelope should read");
            dropped.wait().await;
            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.request.upstream_path,
                fixture.request.upstream_query.as_deref(),
                &fixture.request.upstream,
                &router.requested_model(),
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Cancelled).await;
            assert_eq!(log.overall_status, RequestStatus::Cancelled, "{case_name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("client_cancelled_error"),
                "{case_name}"
            );
            assert!(
                matches!(log.upstream_http_status, None | Some(200)),
                "{case_name}: cancellation may win before or after authentic upstream headers"
            );
            assert_eq!(log.total_input_tokens, None, "{case_name}");
            assert_eq!(log.total_output_tokens, None, "{case_name}");
            assert_eq!(log.total_tokens, None, "{case_name}");
            assert_eq!(log.estimated_cost_nanos, None, "{case_name}");
            assert_eq!(log.cost_snapshot_json, None, "{case_name}");
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn responses_same_wire_non_stream_preserves_unknown_output_bytes_and_observes_usage() {
    let (_, fixture) = responses_target_fixtures()
        .into_iter()
        .find(|(name, _)| *name == "responses")
        .expect("Responses target fixture");
    run_case(
        "responses-same-wire-non-stream-extension",
        move |context| async move {
            let original = br#"{
  "id": "resp_extension",
  "object": "response",
  "status": "completed",
  "model": "baseline-upstream-model",
  "output": [{"type":"vendor_future_item","private":{"opaque":true}}],
  "usage": {"input_tokens":11,"output_tokens":7,"total_tokens":18},
  "vendor_extension": {"spacing":"must remain exact"}
}
"#
            .to_vec();
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("application/json".to_string()),
                content_encoding: None,
                body: original.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), StatusCode::OK);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("same-wire extension response should read");
            assert_eq!(body.as_ref(), original.as_slice());
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(log.total_input_tokens, Some(11));
            assert_eq!(log.total_output_tokens, Some(7));
            assert_eq!(log.total_tokens, Some(18));
            assert_eq!(log.estimated_cost_nanos, Some(122));
            assert!(log.cost_snapshot_json.is_some());
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1);
            assert_eq!(upstream.requests().await.len(), 1);
            upstream.shutdown().await;
        },
    );
}

#[test]
fn responses_same_wire_sse_preserves_event_name_data_and_order_while_replacing_usage() {
    let (_, fixture) = responses_target_fixtures()
        .into_iter()
        .find(|(name, _)| *name == "responses")
        .expect("Responses target fixture");
    run_case(
        "responses-same-wire-sse-extension",
        move |context| async move {
            let expected = vec![
            SseEvent {
                event: Some("response.created".to_string()),
                data: "{ \"type\" : \"response.created\", \"sequence_number\" : 0, \"response\" : {\"id\":\"resp_extension\",\"object\":\"response\",\"status\":\"in_progress\",\"model\":\"baseline-upstream-model\",\"output\":[],\"vendor\":true}, \"vendor_top\" : true }".to_string(),
                ..Default::default()
            },
            SseEvent {
                event: Some("response.vendor.extension".to_string()),
                data: "{ \"type\" : \"response.vendor.extension\", \"sequence_number\" : 1, \"private\" : {\"opaque\":true} }".to_string(),
                ..Default::default()
            },
            SseEvent {
                event: Some("response.usage".to_string()),
                data: "{ \"type\" : \"response.usage\", \"sequence_number\" : 2, \"usage\" : {\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2} }".to_string(),
                ..Default::default()
            },
            SseEvent {
                event: Some("response.completed".to_string()),
                data: "{ \"type\" : \"response.completed\", \"sequence_number\" : 3, \"response\" : {\"id\":\"resp_extension\",\"object\":\"response\",\"status\":\"completed\",\"model\":\"baseline-upstream-model\",\"output\":[],\"usage\":{\"input_tokens\":11,\"output_tokens\":7,\"total_tokens\":18,\"input_tokens_details\":{\"cached_tokens\":3},\"output_tokens_details\":{\"reasoning_tokens\":2}},\"metadata\":{\"future_vendor_field\":true},\"vendor\":true}, \"vendor_top\" : true }".to_string(),
                ..Default::default()
            },
        ];
            let upstream_events = expected
                .iter()
                .map(|event| GoldenEvent {
                    event: event.event.clone(),
                    data: Value::String(event.data.clone()),
                })
                .collect::<Vec<_>>();
            let dropped = Arc::new(DropSignal::default());
            let upstream = TestUpstream::spawn(ScriptedReply::ChunkedSse {
                content_encoding: None,
                chunks: vec![events_to_sse_bytes(&upstream_events)],
                hang_after_chunks: true,
                dropped: Some(Arc::clone(&dropped)),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;

            assert_eq!(response.status(), StatusCode::OK);
            let body = timeout(
                WAIT_TIMEOUT,
                axum::body::to_bytes(response.into_body(), usize::MAX),
            )
            .await
            .expect("typed same-wire terminal should close Body")
            .expect("same-wire SSE body should remain readable");
            assert_eq!(parse_downstream_sse_events(&body), expected);
            dropped.wait().await;
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(log.total_input_tokens, Some(11));
            assert_eq!(log.total_output_tokens, Some(7));
            assert_eq!(log.total_tokens, Some(18));
            assert_eq!(log.cache_read_tokens, Some(3));
            assert_eq!(log.reasoning_tokens, Some(2));
            assert_eq!(log.estimated_cost_nanos, Some(122));
            assert!(log.cost_snapshot_json.is_some());
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1);
            assert_eq!(upstream.requests().await.len(), 1);
            upstream.shutdown().await;
        },
    );
}

#[test]
fn responses_target_revalidates_patched_body_before_credentials() {
    let (_, fixture) = responses_target_fixtures()
        .into_iter()
        .find(|(name, _)| *name == "responses")
        .expect("Responses target fixture");
    run_case("responses-invalid-final-body", move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        router
            .app_state
            .admin
            .request_patch
            .create_source_variant(
                router.source_id,
                RequestPatchVariantInput {
                    source_id: router.source_id,
                    model_id: None,
                    suffix: None,
                    enabled: true,
                    expose_in_models: false,
                    rules: vec![RequestPatchRuleInput {
                        placement: RequestPatchPlacement::Body,
                        target: "/input".to_string(),
                        operation: RequestPatchOperation::Set,
                        value_json: Some(Some(json!(42))),
                        description: Some("Responses final target validation".to_string()),
                    }],
                },
            )
            .await
            .expect("invalid final-body Patch should be saved for runtime validation");
        router
            .app_state
            .catalog
            .invalidate_models_catalog()
            .await
            .expect("Patch catalog should invalidate");
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("final target rejection should read");
        let body: Value = serde_json::from_slice(&body).expect("rejection should be JSON");
        assert_eq!(
            downstream_error_code(&body, fixture.protocol),
            Some("provider_configuration_error")
        );
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        assert!(upstream.requests().await.is_empty());
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("provider_configuration_error")
        );
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn responses_target_credential_request_never_follows_redirects() {
    let (_, fixture) = responses_target_fixtures()
        .into_iter()
        .find(|(name, _)| *name == "responses")
        .expect("Responses target fixture");
    run_case("responses-no-redirect", move |context| async move {
        let redirect_target = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: json!({"captured": true}),
        })
        .await;
        let upstream = TestUpstream::spawn(ScriptedReply::Redirect {
            status: StatusCode::TEMPORARY_REDIRECT,
            location: format!("{}/credential-capture", redirect_target.base_url),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("redirect error response should read");
        let captured = upstream.requests().await;
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].path, "/v1/responses");
        assert_eq!(
            captured[0]
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer provider-baseline-secret")
        );
        assert!(redirect_target.requests().await.is_empty());
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(log.upstream_http_status, Some(307));
        assert_eq!(upstream.requests().await.len(), 1);
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
        redirect_target.shutdown().await;
    });
}

#[test]
fn responses_target_invalid_base_url_fails_before_credentials_and_network() {
    let (_, fixture) = responses_target_fixtures()
        .into_iter()
        .find(|(name, _)| *name == "responses")
        .expect("Responses target fixture");
    run_case("responses-invalid-base-url", move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        UpstreamSource::update(
            router.source_id,
            router.provider_id,
            &UpdateUpstreamSourceData {
                base_url: Some("http://user:base-url-private-marker@127.0.0.1:1/v1".to_string()),
                use_proxy: None,
                is_enabled: None,
                is_default: None,
                updated_at: chrono::Utc::now().timestamp_millis(),
                ..UpdateUpstreamSourceData::test_defaults()
            },
        )
        .expect("legacy invalid Responses base URL should be seeded directly");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("provider cache should invalidate");
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("invalid URL response should read");
        let body: Value = serde_json::from_slice(&body).expect("response should be JSON");
        assert_eq!(
            downstream_error_code(&body, fixture.protocol),
            Some("provider_configuration_error")
        );
        assert!(!body.to_string().contains("base-url-private-marker"));
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        assert!(upstream.requests().await.is_empty());
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("provider_configuration_error")
        );
        assert!(
            !log.final_error_message
                .unwrap_or_default()
                .contains("base-url-private-marker")
        );
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn responses_target_falls_back_to_model_name_when_real_model_name_is_absent() {
    let (_, fixture) = responses_target_fixtures()
        .into_iter()
        .find(|(name, _)| *name == "responses")
        .expect("Responses target fixture");
    run_case("responses-model-fallback", move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::TOO_MANY_REQUESTS,
            body: fixture.error.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        Model::update(
            router.model_id,
            &UpdateModelData {
                model_name: None,
                real_model_name: Some(None),
                is_enabled: None,
                cost_catalog_id: None,
            },
        )
        .expect("real model name should clear");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("model cache should invalidate");

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("upstream error response should read");
        let captured = upstream.requests().await;
        assert_eq!(captured.len(), 1);
        let body: Value =
            serde_json::from_slice(&captured[0].body).expect("request should be JSON");
        assert_eq!(body["model"], router.model_name);
        assert_eq!(body["store"], false);
        assert_eq!(captured[0].path, "/v1/responses");
        router.wait_for_log(RequestStatus::Error).await;
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn registered_request_conflicts_are_rejected_before_credential_or_upstream_use() {
    for (name, fixture) in fixtures().into_iter().filter(|(name, _)| *name == "gemini") {
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::OK,
                body: fixture.non_stream.upstream_response.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let source_id = router
                .replace_default_source_profile(&upstream.base_url, UpstreamProfileType::Openai)
                .await;
            let mut request = fixture.request.downstream.clone();
            request.as_object_mut().unwrap().insert(
                "generationConfig".to_string(),
                json!({"thinkingConfig": {"thinkingBudget": 1024}}),
            );
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{name}");
            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                0,
                "{name}: conflict registry must run before credential decryption"
            );
            assert!(upstream.requests().await.is_empty(), "{name}");
            router
                .wait_for_log_for_source(source_id, RequestStatus::Error)
                .await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn malformed_responses_reasoning_is_rejected_before_credential_or_upstream_use() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "responses")
        .expect("responses fixture");
    run_case(name, move |context| async move {
        const PRIVATE_MARKER: &str = "reasoning-private-marker";
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let mut request = fixture.request.downstream.clone();
        request["reasoning"] = json!({"effort": {"private": PRIVATE_MARKER}});
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router.send(&fixture, false, &request).await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("reasoning rejection response should read");
        assert!(!String::from_utf8_lossy(&body).contains(PRIVATE_MARKER));
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        assert!(upstream.requests().await.is_empty());
        router.wait_for_log(RequestStatus::Error).await;
        upstream.shutdown().await;
    });
}

#[test]
fn gemini_openai_profile_rejects_unknown_and_conflicting_chat_fields_before_credentials() {
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    for (case, extension) in [
        (
            "unknown",
            json!({"payload-secret-marker": {"enabled": true}}),
        ),
        (
            "reasoning-conflict",
            json!({
                "reasoning_effort": "low",
                "extra_body": {
                    "google": {"thinking_config": {"thinking_level": "low"}}
                }
            }),
        ),
    ] {
        let fixture = fixture.clone();
        run_case(case, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::OK,
                body: fixture.non_stream.upstream_response.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let source_id = router
                .replace_default_openai_profile_in_place(
                    &upstream.base_url,
                    UpstreamProfileType::GeminiOpenai,
                )
                .await;
            let mut request = fixture.request.downstream.clone();
            request
                .as_object_mut()
                .expect("request must be an object")
                .extend(
                    extension
                        .as_object()
                        .expect("extension fixture must be an object")
                        .clone(),
                );
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case}");
            let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("strict Profile rejection response should read");
            assert!(
                !String::from_utf8_lossy(&response_body).contains("payload-secret-marker"),
                "{case}: rejected request data must not leak"
            );
            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                0,
                "{case}: strict validation must precede credential decryption"
            );
            assert!(upstream.requests().await.is_empty(), "{case}");
            router
                .wait_for_log_for_source(source_id, RequestStatus::Error)
                .await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn gemini_openai_profile_revalidates_patch_output_before_credentials() {
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case("gemini-openai-patch", move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let source_id = router
            .replace_default_openai_profile_in_place(
                &upstream.base_url,
                UpstreamProfileType::GeminiOpenai,
            )
            .await;
        router
            .app_state
            .admin
            .request_patch
            .create_source_variant(
                source_id,
                RequestPatchVariantInput {
                    source_id,
                    model_id: None,
                    suffix: None,
                    enabled: true,
                    expose_in_models: false,
                    rules: vec![RequestPatchRuleInput {
                        placement: RequestPatchPlacement::Body,
                        target: "/payload-secret-marker".to_string(),
                        operation: RequestPatchOperation::Set,
                        value_json: Some(Some(json!({"enabled": true}))),
                        description: Some("strict post-Patch validation".to_string()),
                    }],
                },
            )
            .await
            .expect("unknown extension Patch should save before Profile validation");
        router
            .app_state
            .catalog
            .invalidate_models_catalog()
            .await
            .expect("Patch catalog should invalidate");
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("strict post-Patch rejection response should read");
        assert!(!String::from_utf8_lossy(&response_body).contains("payload-secret-marker"));
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        assert!(upstream.requests().await.is_empty());
        upstream.shutdown().await;
    });
}

#[test]
fn major_capability_rejection_is_zero_call_and_precedes_credential_use() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new_deepseek(context, &fixture, &upstream.base_url).await;
        let ollama_source = UpstreamSource::create(&NewUpstreamSource {
            id: ID_GENERATOR.generate_id(),
            provider_id: router.provider_id,
            profile_type: UpstreamProfileType::Ollama,
            base_url: upstream.base_url.clone(),
            use_proxy: false,
            is_enabled: true,
            is_default: false,
            created_at: 2,
            updated_at: 2,
            ..NewUpstreamSource::test_defaults(UpstreamProfileType::Ollama)
        })
        .expect("Ollama default Source should be created");
        let mutation_time = chrono::Utc::now().timestamp_millis();
        UpstreamSource::update(
            router.source_id,
            router.provider_id,
            &UpdateUpstreamSourceData {
                base_url: None,
                use_proxy: None,
                is_enabled: Some(false),
                is_default: Some(false),
                updated_at: mutation_time,
                ..UpdateUpstreamSourceData::test_defaults()
            },
        )
        .expect("exact OpenAI Source should be disabled");
        UpstreamSource::update(
            ollama_source.id,
            router.provider_id,
            &UpdateUpstreamSourceData {
                base_url: None,
                use_proxy: None,
                is_enabled: Some(true),
                is_default: Some(true),
                updated_at: mutation_time,
                ..UpdateUpstreamSourceData::test_defaults()
            },
        )
        .expect("Ollama Source should become default");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("Source mutation should invalidate catalog");
        let persisted_sink = router.install_recording_persisted_sink();
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();
        let mut body = fixture.request.downstream.clone();
        body.as_object_mut().expect("request object").insert(
            "tools".to_string(),
            json!([{
                "type": "function",
                "function": {"name": "lookup", "parameters": {"type": "object"}}
            }]),
        );

        let response = router.send(&fixture, false, &body).await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("capability rejection response should read");
        let response_body: Value =
            serde_json::from_slice(&response_body).expect("response should be JSON");
        assert_eq!(
            downstream_error_code(&response_body, fixture.protocol),
            Some("unsupported_capability_error")
        );
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        assert!(upstream.requests().await.is_empty());
        let log = router
            .wait_for_log_for_source(ollama_source.id, RequestStatus::Error)
            .await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("unsupported_capability_error")
        );
        assert_eq!(
            log.source_selection_reason.as_deref(),
            Some("provider_default_transform")
        );
        assert_single_persisted_terminal_fact(
            &persisted_sink,
            ExecutionStage::Capability,
            ResponseVisibility::NotVisible,
        )
        .await;
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn model_resolution_preserves_parse_and_capability_error_codes() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

        for (requested_model, expected_code) in [
            ("gpt-4o", "invalid_request_error"),
            ("missing-provider/gpt-4o", "unsupported_capability_error"),
        ] {
            let mut request_body = fixture.request.downstream.clone();
            request_body
                .as_object_mut()
                .expect("OpenAI request fixture should be an object")
                .insert("model".to_string(), json!(requested_model));

            let response = router.send(&fixture, false, &request_body).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("model resolution response body should read");
            let body: Value =
                serde_json::from_slice(&body).expect("model resolution response should be JSON");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some(expected_code),
                "{requested_model}"
            );
        }

        assert!(router.request_logs().await.is_empty());
        assert!(upstream.requests().await.is_empty());
        upstream.shutdown().await;
    });
}

#[test]
fn tool_request_is_not_rejected_by_model_capability_flags() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();
        let mut body = fixture.request.downstream.clone();
        body.as_object_mut().unwrap().insert(
            "tools".to_string(),
            json!([{"type":"function","function":{"name":"probe"}}]),
        );

        let response = router.send(&fixture, false, &body).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(router.app_state.secret_encryption.decrypt_call_count() > 0);
        assert_eq!(upstream.requests().await.len(), 1);
        upstream.shutdown().await;
    });
}

#[test]
fn same_wire_non_stream_observation_failure_preserves_upstream_bytes() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let original = b"same-wire-not-json\n".to_vec();
        let upstream = TestUpstream::spawn(ScriptedReply::Raw {
            status: StatusCode::OK,
            content_type: Some("application/json".to_string()),
            content_encoding: None,
            body: original.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("same-wire response body should read");
        assert_eq!(body.as_ref(), original.as_slice());
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert!(log.final_error_code.is_none());
        assert_eq!(log.upstream_http_status, Some(200));
        assert_eq!(upstream.requests().await.len(), 1);
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn cross_wire_non_stream_decode_failure_returns_502_without_provider_body() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "responses")
        .expect("responses fixture");
    run_case(name, move |context| async move {
        let marker = "cross-wire-provider-secret";
        let upstream = TestUpstream::spawn(ScriptedReply::Raw {
            status: StatusCode::OK,
            content_type: Some("application/json".to_string()),
            content_encoding: None,
            body: marker.as_bytes().to_vec(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let persisted_sink = router.install_recording_persisted_sink();

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_no_public_transform_diagnostics(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("cross-wire transform error response should read");
        assert_payload_free_transform_bytes(&body, marker);
        assert!(
            !body
                .windows(marker.len())
                .any(|window| window == marker.as_bytes())
        );
        let body: Value = serde_json::from_slice(&body).expect("response should be JSON");
        assert_eq!(
            downstream_error_code(&body, fixture.protocol),
            Some("upstream_response_error")
        );
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("upstream_response_error")
        );
        assert_eq!(log.upstream_http_status, Some(200));
        assert_eq!(upstream.requests().await.len(), 1, "no retry or fallback");
        assert_single_persisted_terminal_fact(
            &persisted_sink,
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::NotVisible,
        )
        .await;
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn cross_wire_minor_loss_succeeds_once_and_drops_only_audited_metadata() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        const PRIVATE_MARKER: &str = "minor-loss-private-operator-tag";
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: ollama_non_stream_response(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let source_id = router
            .replace_default_source_profile(&upstream.base_url, UpstreamProfileType::Ollama)
            .await;
        let persisted_sink = router.install_recording_persisted_sink();
        let mut request = fixture.request.downstream.clone();
        request
            .as_object_mut()
            .expect("OpenAI request fixture must be an object")
            .insert("user".to_string(), json!(PRIVATE_MARKER));

        let response = router.send(&fixture, false, &request).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_no_public_transform_diagnostics(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("minor-loss response should be readable");
        assert_payload_free_transform_bytes(&body, PRIVATE_MARKER);
        let captured = upstream.requests().await;
        assert_eq!(captured.len(), 1, "minor loss must not retry or fall back");
        assert_eq!(captured[0].path, "/api/chat");
        let upstream_body: Value = serde_json::from_slice(&captured[0].body)
            .expect("Ollama upstream request should be JSON");
        assert!(upstream_body.get("user").is_none());
        assert!(
            !String::from_utf8_lossy(&captured[0].body).contains(PRIVATE_MARKER),
            "the audited metadata field must be dropped before upstream send"
        );
        let log = router
            .wait_for_log_for_source(source_id, RequestStatus::Success)
            .await;
        assert_eq!(
            log.source_selection_reason.as_deref(),
            Some("provider_default_transform")
        );
        assert!(log.final_error_code.is_none());
        {
            let contexts = persisted_sink.contexts.lock().await;
            assert_eq!(contexts.len(), 1);
            assert_eq!(contexts[0].request_log_id, log.id);
            assert!(contexts[0].final_error_stage.is_none());
            assert_eq!(
                contexts[0].response_visibility,
                ResponseVisibility::NotVisible
            );
        }
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn acl_rejection_precedes_model_kind_and_invalid_provider_base_url_preflight() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new_with_default_action_identity_and_kind(
            context,
            &fixture,
            &upstream.base_url,
            Action::Deny,
            "acl-kind-provider",
            "ACL Kind Provider",
            "acl-kind-model",
            ModelKind::Embedding,
        )
        .await;
        UpstreamSource::update(
            router.source_id,
            router.provider_id,
            &UpdateUpstreamSourceData {
                base_url: Some("http://user:secret@127.0.0.1:1/v1".to_string()),
                use_proxy: None,
                is_enabled: None,
                is_default: None,
                updated_at: chrono::Utc::now().timestamp_millis(),
                ..UpdateUpstreamSourceData::test_defaults()
            },
        )
        .expect("legacy invalid base URL should be seeded directly");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("provider cache should invalidate");
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        assert!(upstream.requests().await.is_empty());
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(log.final_error_code.as_deref(), Some("permission_error"));
        assert!(
            log.final_error_message
                .as_deref()
                .is_some_and(|message| message.contains("Access denied"))
        );
        upstream.shutdown().await;
    });
}

#[test]
fn acl_rejection_precedes_masked_request_patch_suffix_validation() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new_with_default_action(
            context,
            &fixture,
            &upstream.base_url,
            Action::Deny,
        )
        .await;
        router
            .app_state
            .admin
            .request_patch
            .create_source_variant(
                router.source_id,
                RequestPatchVariantInput {
                    source_id: router.source_id,
                    model_id: None,
                    suffix: Some("fast".to_string()),
                    enabled: true,
                    expose_in_models: true,
                    rules: vec![RequestPatchRuleInput {
                        placement: RequestPatchPlacement::Body,
                        target: "/options/temperature".to_string(),
                        operation: RequestPatchOperation::Set,
                        value_json: Some(Some(json!(0.2))),
                        description: Some("ACL ordering regression".to_string()),
                    }],
                },
            )
            .await
            .expect("Source suffix should create");
        router
            .app_state
            .admin
            .request_patch
            .create_model_source_variant(
                router.model_id,
                router.source_id,
                RequestPatchVariantInput {
                    source_id: router.source_id,
                    model_id: Some(router.model_id),
                    suffix: Some("fast".to_string()),
                    enabled: false,
                    expose_in_models: false,
                    rules: Vec::new(),
                },
            )
            .await
            .expect("Model tombstone should create");
        let requested_model = format!("{}-fast", router.requested_model());
        let uri = fixture
            .downstream_path
            .replace("$REQUESTED_MODEL", &requested_model);

        let response = router
            .send_raw_post(
                uri,
                render_value(&fixture.request.downstream, &requested_model),
                fixture.downstream_auth,
            )
            .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(upstream.requests().await.is_empty());
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(log.final_error_code.as_deref(), Some("permission_error"));
        assert!(
            log.final_error_message
                .as_deref()
                .is_some_and(|message| message.contains("Access denied"))
        );
        upstream.shutdown().await;
    });
}

#[test]
fn acl_rejection_precedes_missing_proxy_preflight() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let infra_context = context.clone();
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let mut router = RouterFixture::new_with_default_action(
            context,
            &fixture,
            &upstream.base_url,
            Action::Deny,
        )
        .await;
        let mut app_state = (*router.app_state).clone();
        app_state.infra = Arc::new(
            AppInfra::new_with_config(
                OutboundHttpConfig::default(),
                ProxyRequestConfig::default(),
                None,
                Some(infra_context),
            )
            .await,
        );
        router.app_state = Arc::new(app_state);
        UpstreamSource::update(
            router.source_id,
            router.provider_id,
            &UpdateUpstreamSourceData {
                base_url: None,
                use_proxy: Some(true),
                is_enabled: None,
                is_default: None,
                updated_at: chrono::Utc::now().timestamp_millis(),
                ..UpdateUpstreamSourceData::test_defaults()
            },
        )
        .expect("proxy requirement should be seeded directly");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("provider cache should invalidate");
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        assert!(upstream.requests().await.is_empty());
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(log.final_error_code.as_deref(), Some("permission_error"));
        assert!(
            log.final_error_message
                .as_deref()
                .is_some_and(|message| message.contains("Access denied"))
        );
        upstream.shutdown().await;
    });
}

#[test]
fn request_patch_conflict_rejection_does_not_decrypt_provider_credential() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let patch = |model_id: Option<i64>, target: &str| RequestPatchVariantInput {
            source_id: router.source_id,
            model_id,
            suffix: None,
            enabled: true,
            expose_in_models: false,
            rules: vec![RequestPatchRuleInput {
                placement: RequestPatchPlacement::Body,
                target: target.to_string(),
                operation: RequestPatchOperation::Set,
                value_json: Some(Some(json!({"temperature": 0.2}))),
                description: Some("decrypt ordering regression".to_string()),
            }],
        };
        router
            .app_state
            .admin
            .request_patch
            .create_source_variant(router.source_id, patch(None, "/generation_config"))
            .await
            .expect("source patch should create");
        let model_variant = router
            .app_state
            .admin
            .request_patch
            .create_model_source_variant(
                router.model_id,
                router.source_id,
                patch(Some(router.model_id), "/safe"),
            )
            .await
            .expect("model source patch should create");
        let mut connection = get_connection().expect("test database connection");
        match &mut connection {
            DbConnection::Sqlite(connection) => diesel::sql_query(format!(
                "UPDATE request_patch_rule SET target = '/generation_config/temperature' WHERE id = {}",
                model_variant.rules[0].id
            ))
            .execute(connection)
            .expect("test corruption should update the model rule target"),
            DbConnection::Postgres(_) => {
                panic!("direct execution regression uses the isolated SQLite fixture")
            }
        };
        router
            .app_state
            .catalog
            .invalidate_models_catalog()
            .await
            .expect("corrupted catalog should be invalidated");
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        assert!(upstream.requests().await.is_empty());
        upstream.shutdown().await;
    });
}

#[test]
fn invalid_final_profile_patch_is_rejected_before_credential_for_all_downstreams() {
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            const INVALID_PATCH_VALUE: &str = "invalid-final-profile-secret-marker";
            let upstream = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::OK,
                body: fixture.non_stream.upstream_response.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let target = if matches!(&fixture.profile_type, UpstreamProfileType::Gemini) {
                "/contents"
            } else {
                "/messages"
            };
            router
                .app_state
                .admin
                .request_patch
                .create_source_variant(
                    router.source_id,
                    RequestPatchVariantInput {
                        source_id: router.source_id,
                        model_id: None,
                        suffix: None,
                        enabled: true,
                        expose_in_models: false,
                        rules: vec![RequestPatchRuleInput {
                            placement: RequestPatchPlacement::Body,
                            target: target.to_string(),
                            operation: RequestPatchOperation::Set,
                            value_json: Some(Some(json!(INVALID_PATCH_VALUE))),
                            description: Some("final Profile validation regression".to_string()),
                        }],
                    },
                )
                .await
                .expect("shape-changing Patch should be valid before Profile materialization");
            router
                .app_state
                .catalog
                .invalidate_models_catalog()
                .await
                .expect("Patch catalog should invalidate");
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(
                response.status(),
                StatusCode::INTERNAL_SERVER_ERROR,
                "{name}"
            );
            let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("final Profile rejection response should read");
            assert!(
                !String::from_utf8_lossy(&response_body).contains(INVALID_PATCH_VALUE),
                "{name}: rejected Patch values must not leak downstream"
            );
            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                0,
                "{name}: final Profile validation must precede credential decryption"
            );
            assert!(upstream.requests().await.is_empty(), "{name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            let final_message = log
                .final_error_message
                .as_deref()
                .expect("operator log should preserve the safe validation path");
            assert!(final_message.contains(target), "{name}: {final_message}");
            assert!(!final_message.contains(INVALID_PATCH_VALUE), "{name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn utility_execution_does_not_resolve_or_apply_request_patch() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: json!({
                "object": "list",
                "data": [],
                "model": UPSTREAM_MODEL,
                "usage": {"prompt_tokens": 1, "total_tokens": 1}
            }),
        })
        .await;
        let router = RouterFixture::new_with_model_kind(
            context,
            &fixture,
            &upstream.base_url,
            ModelKind::Embedding,
        )
        .await;
        router
            .app_state
            .admin
            .request_patch
            .create_source_variant(
                router.source_id,
                RequestPatchVariantInput {
                    source_id: router.source_id,
                    model_id: None,
                    suffix: None,
                    enabled: true,
                    expose_in_models: false,
                    rules: vec![RequestPatchRuleInput {
                        placement: RequestPatchPlacement::Body,
                        target: "/metadata/from_patch".to_string(),
                        operation: RequestPatchOperation::Set,
                        value_json: Some(Some(json!(true))),
                        description: Some("utility scope regression".to_string()),
                    }],
                },
            )
            .await
            .expect("generation Patch should persist independently of model kind");
        router
            .app_state
            .catalog
            .invalidate_models_catalog()
            .await
            .expect("Patch catalog should invalidate");

        let response = router
            .send_raw_post(
                "/openai/v1/embeddings".to_string(),
                json!({"model": router.requested_model(), "input": "hello"}),
                DownstreamAuth::Bearer,
            )
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("utility response should be consumed");
        let requests = upstream.requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/v1/embeddings");
        let upstream_body: Value = serde_json::from_slice(&requests[0].body)
            .expect("captured utility request should contain JSON");
        assert!(upstream_body.get("metadata").is_none());
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn request_patch_query_value_reaches_upstream_but_not_request_log() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        const PATCH_QUERY_SECRET: &str = "patch-query-secret-marker";
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        router
            .app_state
            .admin
            .request_patch
            .create_source_variant(
                router.source_id,
                RequestPatchVariantInput {
                    source_id: router.source_id,
                    model_id: None,
                    suffix: Some("tool-use-v2".to_string()),
                    enabled: true,
                    expose_in_models: false,
                    rules: vec![RequestPatchRuleInput {
                        placement: RequestPatchPlacement::Query,
                        target: "diagnostic".to_string(),
                        operation: RequestPatchOperation::Set,
                        value_json: Some(Some(json!(PATCH_QUERY_SECRET))),
                        description: Some("transient query regression".to_string()),
                    }],
                },
            )
            .await
            .expect("query patch should create");

        let requested_model = format!("{}-tool-use-v2", router.requested_model());
        let uri = fixture
            .downstream_path
            .replace("$REQUESTED_MODEL", &requested_model);
        let response = router
            .send_raw_post(
                uri,
                render_value(&fixture.request.downstream, &requested_model),
                fixture.downstream_auth,
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let _ = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("successful response body");
        let captured = upstream.requests().await;
        assert_eq!(captured.len(), 1);
        assert!(
            captured[0]
                .query
                .as_deref()
                .is_some_and(|query| query.contains(PATCH_QUERY_SECRET)),
            "fixture must prove the raw query patch reached the selected upstream"
        );
        assert_eq!(
            captured[0]
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer provider-baseline-secret"),
            "provider credential must remain platform-owned"
        );

        let log = router.wait_for_log(RequestStatus::Success).await;
        router.app_state.flush_proxy_logs().await;
        let persisted_log = RequestLog::get_by_id(log.id).expect("flushed request log exists");
        assert_eq!(
            persisted_log.resolved_patch_suffix.as_deref(),
            Some("tool-use-v2")
        );
        assert_eq!(
            persisted_log.base_requested_model_name.as_deref(),
            Some(router.requested_model().as_str())
        );
        let persisted = serde_json::to_string(&log).expect("request log should serialize");
        for secret in [
            PATCH_QUERY_SECRET,
            PROVIDER_SECRET,
            router.downstream_key.as_str(),
        ] {
            assert!(
                !persisted.contains(secret),
                "transient URL/query and credentials must not enter Request Log: {secret}"
            );
        }
        assert_eq!(router.request_logs().await.len(), 1);
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_regression_non_stream_request_response_usage_and_log_golden() {
    for (name, fixture) in generation_evidence_fixtures() {
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::OK,
                body: fixture.non_stream.upstream_response.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(
                response.status().as_u16(),
                fixture.non_stream.downstream_status,
                "{name}: downstream status"
            );
            assert!(
                response
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(
                        |value| value.starts_with(&fixture.non_stream.downstream_content_type)
                    ),
                "{name}: downstream content type"
            );
            let request_id = assert_downstream_request_identity(&response);
            assert!(
                response.headers().get("retry-after").is_none(),
                "{name}: provider Retry-After must not pass through"
            );
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let actual: Value = serde_json::from_slice(&body).unwrap_or_else(|error| {
                panic!(
                    "{name}: downstream JSON: {error}; body={}",
                    String::from_utf8_lossy(&body)
                )
            });
            assert_eq!(
                normalized(actual),
                normalized(fixture.non_stream.downstream_response.clone()),
                "{name}: downstream response"
            );
            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.request.upstream_path,
                fixture.request.upstream_query.as_deref(),
                &fixture.request.upstream,
                &router.requested_model(),
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(
                log.request_id, request_id,
                "{name}: response and persisted canonical request id"
            );
            assert_log_common(&router, &fixture, &log);
            assert_log_timing_order(&log);
            assert!(log.first_token_at.is_none());
            assert_eq!(log.upstream_http_status, Some(200));
            assert_usage(&log, &fixture.usage);
            upstream.shutdown().await;
        });
    }
}

#[test]
fn persisted_log_sink_receives_the_canonical_request_id() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let sink = Arc::new(RecordingPersistedSink::default());
        let persisted_sink: Arc<dyn RequestLogPersistedSink> = sink.clone();
        router
            .app_state
            .infra
            .log_manager()
            .set_request_log_persisted_sink(persisted_sink);

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let request_id = assert_downstream_request_identity(&response);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("successful response body should be consumed before persistence assertion");
        let log = router.wait_for_log(RequestStatus::Success).await;
        let contexts = sink.contexts.lock().await;

        assert_eq!(
            contexts.as_slice(),
            &[RequestLogPersistedContext {
                request_log_id: log.id,
                request_id,
                final_error_stage: None,
                response_visibility: ResponseVisibility::NotVisible,
            }]
        );
        upstream.shutdown().await;
    });
}

#[test]
fn four_public_downstream_generation_paths_call_upstream_at_most_once() {
    for (name, fixture) in openai_target_fixtures() {
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::OK,
                body: fixture.non_stream.upstream_response.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{name}");
            assert_eq!(
                upstream.requests().await.len(),
                1,
                "{name}: direct execution must issue exactly one upstream request"
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn deepseek_multi_source_provider_freezes_exact_or_default_source_once_for_public_downstreams() {
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let alternate_profile = match fixture.protocol {
                DownstreamProtocol::Gemini => UpstreamProfileType::Openai,
                DownstreamProtocol::Openai
                | DownstreamProtocol::Responses
                | DownstreamProtocol::Anthropic => UpstreamProfileType::Ollama,
            };
            let (alternate_path, alternate_response) = match alternate_profile {
                UpstreamProfileType::Openai => (
                    "/v1/chat/completions".to_string(),
                    fixtures()
                        .into_iter()
                        .find(|(fixture_name, _)| *fixture_name == "openai")
                        .map(|(_, fixture)| fixture.non_stream.upstream_response)
                        .expect("OpenAI response fixture"),
                ),
                UpstreamProfileType::Ollama => {
                    ("/api/chat".to_string(), ollama_non_stream_response())
                }
                _ => unreachable!("the DeepSeek regression uses OpenAI/Ollama alternates"),
            };
            let upstream = TestUpstream::spawn(ScriptedReply::JsonByPath {
                status: StatusCode::OK,
                default_body: fixture.non_stream.upstream_response.clone(),
                path_bodies: BTreeMap::from([(alternate_path, alternate_response)]),
            })
            .await;
            let router = RouterFixture::new_deepseek(context, &fixture, &upstream.base_url).await;
            let alternate_endpoint = match alternate_profile {
                UpstreamProfileType::Openai => format!("{}/v1", upstream.base_url),
                UpstreamProfileType::Ollama => upstream.base_url.clone(),
                _ => unreachable!("the DeepSeek regression uses OpenAI/Ollama alternates"),
            };
            let alternate_source = UpstreamSource::create(&NewUpstreamSource {
                id: ID_GENERATOR.generate_id(),
                provider_id: router.provider_id,
                profile_type: alternate_profile.clone(),
                base_url: alternate_endpoint,
                use_proxy: false,
                is_enabled: true,
                is_default: false,
                created_at: 1,
                updated_at: 1,
                ..NewUpstreamSource::test_defaults(alternate_profile)
            })
            .expect("DeepSeek alternate Source should be created");
            router
                .app_state
                .catalog
                .invalidate_provider(router.provider_id, Some(&router.provider_key))
                .await
                .expect("multi-Source fixture should invalidate its catalog");

            let exact_or_default = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(exact_or_default.status(), StatusCode::OK, "{name}");
            axum::body::to_bytes(exact_or_default.into_body(), usize::MAX)
                .await
                .expect("exact/default response should be consumed");
            let first_requests = upstream.requests().await;
            assert_eq!(
                first_requests.len(),
                1,
                "{name}: first selection must call once"
            );
            let first_log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(first_log.source_id, Some(router.source_id));
            let expected_first_reason = match fixture.protocol {
                DownstreamProtocol::Openai | DownstreamProtocol::Gemini => "protocol_match",
                DownstreamProtocol::Responses | DownstreamProtocol::Anthropic => {
                    "provider_default_transform"
                }
            };
            assert_eq!(
                first_log.source_selection_reason.as_deref(),
                Some(expected_first_reason),
                "{name}: the first selection must persist its shared selector reason"
            );

            let mutation_time = chrono::Utc::now().timestamp_millis();
            UpstreamSource::update(
                router.source_id,
                router.provider_id,
                &UpdateUpstreamSourceData {
                    base_url: None,
                    use_proxy: None,
                    is_enabled: Some(false),
                    is_default: Some(false),
                    updated_at: mutation_time,
                    ..UpdateUpstreamSourceData::test_defaults()
                },
            )
            .expect("the initial Source should be disabled");
            UpstreamSource::update(
                alternate_source.id,
                router.provider_id,
                &UpdateUpstreamSourceData {
                    base_url: None,
                    use_proxy: None,
                    is_enabled: Some(true),
                    is_default: Some(true),
                    updated_at: mutation_time,
                    ..UpdateUpstreamSourceData::test_defaults()
                },
            )
            .expect("the alternate Source should become default");
            router
                .app_state
                .catalog
                .invalidate_provider(router.provider_id, Some(&router.provider_key))
                .await
                .expect("default switch should invalidate its catalog");

            let default_transform = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(default_transform.status(), StatusCode::OK, "{name}");
            axum::body::to_bytes(default_transform.into_body(), usize::MAX)
                .await
                .expect("default transform response should be consumed");
            let requests = upstream.requests().await;
            assert_eq!(
                requests.len(),
                2,
                "{name}: default selection must call once"
            );
            match alternate_profile {
                UpstreamProfileType::Openai => {
                    assert_eq!(requests[1].path, "/v1/chat/completions", "{name}");
                }
                UpstreamProfileType::Ollama => {
                    assert_eq!(requests[1].path, "/api/chat", "{name}");
                }
                _ => unreachable!(),
            }
            let second_log = router
                .wait_for_log_for_source(alternate_source.id, RequestStatus::Success)
                .await;
            assert_eq!(second_log.source_id, Some(alternate_source.id));
            assert_eq!(
                second_log.source_profile_type_snapshot,
                Some(alternate_profile),
                "{name}: request log must retain the selected Source profile"
            );
            assert_eq!(
                second_log.source_selection_reason.as_deref(),
                Some("provider_default_transform"),
                "{name}: request log must persist the provider default selection reason"
            );
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn zen_fixture_uses_the_shared_selection_contract_for_all_public_downstreams() {
    for (name, fixture) in fixtures() {
        let case_name = format!("zen-{name}");
        run_case(&case_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::OK,
                body: fixture.non_stream.upstream_response.clone(),
            })
            .await;
            let router = RouterFixture::new_zen(context, &fixture, &upstream.base_url).await;
            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{name}");
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("Zen fixture response should be consumed");
            assert_eq!(
                upstream.requests().await.len(),
                1,
                "{name}: one upstream call"
            );
            assert!(router.provider_name.starts_with("Zen Provider"));
            assert_eq!(router.model_name, "zen-chat");
            let log = router.wait_for_log(RequestStatus::Success).await;
            let expected_reason = match fixture.protocol {
                DownstreamProtocol::Openai | DownstreamProtocol::Gemini => "protocol_match",
                DownstreamProtocol::Responses | DownstreamProtocol::Anthropic => {
                    "provider_default_transform"
                }
            };
            assert_eq!(
                log.source_selection_reason.as_deref(),
                Some(expected_reason),
                "{name}: Zen fixture should use the shared selector"
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_exact_default_and_zero_source_have_stable_call_counts() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("exact Source response should be consumed");
        assert_eq!(upstream.requests().await.len(), 1);
        router.wait_for_log(RequestStatus::Success).await;
        router.wait_for_api_key_lease_release().await;

        let mutation_time = chrono::Utc::now().timestamp_millis();
        UpstreamSource::update(
            router.source_id,
            router.provider_id,
            &UpdateUpstreamSourceData {
                base_url: None,
                use_proxy: None,
                is_enabled: Some(false),
                is_default: Some(false),
                updated_at: mutation_time,
                ..UpdateUpstreamSourceData::test_defaults()
            },
        )
        .expect("exact Source should be disabled");
        let default_source = UpstreamSource::create(&NewUpstreamSource {
            id: ID_GENERATOR.generate_id(),
            provider_id: router.provider_id,
            profile_type: UpstreamProfileType::Ollama,
            base_url: upstream.base_url.clone(),
            use_proxy: false,
            is_enabled: true,
            is_default: true,
            created_at: mutation_time,
            updated_at: mutation_time,
            ..NewUpstreamSource::test_defaults(UpstreamProfileType::Ollama)
        })
        .expect("default fallback Source should be created");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("Source mutation should invalidate provider cache");

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("default Source response should be consumed");
        let requests = upstream.requests().await;
        assert_eq!(
            requests.len(),
            2,
            "default fallback should still issue exactly one request"
        );
        assert_eq!(requests[1].path, "/api/chat");
        assert_eq!(
            requests[1]
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some(format!("Bearer {PROVIDER_SECRET}").as_str())
        );
        router.wait_for_api_key_lease_release().await;

        UpstreamSource::delete(router.source_id, router.provider_id)
            .expect("exact Source should be soft deleted");
        UpstreamSource::delete(default_source.id, router.provider_id)
            .expect("default Source should be soft deleted");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("zero-source mutation should invalidate provider cache");

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("zero-source response should be consumed");
        let body: Value = serde_json::from_slice(&body).expect("zero-source error should be JSON");
        assert_eq!(
            downstream_error_code(&body, fixture.protocol),
            Some("provider_configuration_error")
        );
        assert_eq!(upstream.requests().await.len(), 2);
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_explicit_scope_is_closed_and_fail_closed() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let mutation_time = chrono::Utc::now().timestamp_millis();
        let alternate_source = UpstreamSource::create(&NewUpstreamSource {
            id: ID_GENERATOR.generate_id(),
            provider_id: router.provider_id,
            profile_type: UpstreamProfileType::Ollama,
            base_url: upstream.base_url.clone(),
            use_proxy: false,
            is_enabled: true,
            is_default: false,
            created_at: mutation_time,
            updated_at: mutation_time,
            ..NewUpstreamSource::test_defaults(UpstreamProfileType::Ollama)
        })
        .expect("explicit-scope alternate Source should be created");

        let assert_configuration_failure = |response: Response<Body>| async {
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("configuration failure body should be readable");
            let body: Value =
                serde_json::from_slice(&body).expect("configuration failure should be JSON");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("provider_configuration_error")
            );
        };

        replace_for_model(
            router.model_id,
            &ModelSourceConfig::explicit(vec![ModelSourceBindingInput {
                source_id: alternate_source.id,
                is_default: false,
            }]),
        )
        .expect("singleton explicit Source should be saved");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("singleton explicit Source should invalidate the cache");
        assert_configuration_failure(
            router
                .send(&fixture, false, &fixture.request.downstream)
                .await,
        )
        .await;

        replace_for_model(router.model_id, &ModelSourceConfig::explicit(Vec::new()))
            .expect("explicit empty Source Config should be saved");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("explicit empty Source Config should invalidate the cache");
        assert_configuration_failure(
            router
                .send(&fixture, false, &fixture.request.downstream)
                .await,
        )
        .await;

        replace_for_model(
            router.model_id,
            &ModelSourceConfig::explicit(vec![ModelSourceBindingInput {
                source_id: alternate_source.id,
                is_default: true,
            }]),
        )
        .expect("disabled model default Source should be saved");
        UpstreamSource::update(
            alternate_source.id,
            router.provider_id,
            &UpdateUpstreamSourceData {
                base_url: None,
                use_proxy: None,
                is_enabled: Some(false),
                is_default: None,
                updated_at: chrono::Utc::now().timestamp_millis(),
                ..UpdateUpstreamSourceData::test_defaults()
            },
        )
        .expect("model default Source should be disabled");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("disabled model default should invalidate the cache");
        assert_configuration_failure(
            router
                .send(&fixture, false, &fixture.request.downstream)
                .await,
        )
        .await;

        UpstreamSource::delete(alternate_source.id, router.provider_id)
            .expect("model default Source should be soft deleted");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("deleted model default should invalidate the cache");
        assert_configuration_failure(
            router
                .send(&fixture, false, &fixture.request.downstream)
                .await,
        )
        .await;

        assert!(
            upstream.requests().await.is_empty(),
            "explicit scope failures must never penetrate the Provider default or retry"
        );
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_model_default_selection_reason_is_persisted_after_flush() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: ollama_non_stream_response(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let model_default_source = UpstreamSource::create(&NewUpstreamSource {
            id: ID_GENERATOR.generate_id(),
            provider_id: router.provider_id,
            profile_type: UpstreamProfileType::Ollama,
            base_url: upstream.base_url.clone(),
            use_proxy: false,
            is_enabled: true,
            is_default: false,
            created_at: 1,
            updated_at: 1,
            ..NewUpstreamSource::test_defaults(UpstreamProfileType::Ollama)
        })
        .expect("model default Source should be created");
        replace_for_model(
            router.model_id,
            &ModelSourceConfig::explicit(vec![ModelSourceBindingInput {
                source_id: model_default_source.id,
                is_default: true,
            }]),
        )
        .expect("model Source Config should be replaced atomically");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("model Source Config should invalidate the provider snapshot");

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("model default response should be consumed");
        let requests = upstream.requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/api/chat");
        let log = router
            .wait_for_log_for_source(model_default_source.id, RequestStatus::Success)
            .await;
        assert_eq!(
            log.source_selection_reason.as_deref(),
            Some("model_default_transform")
        );
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_openai_utility_exact_source_issues_one_call() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: json!({
                "object": "list",
                "data": [],
                "model": UPSTREAM_MODEL,
                "usage": {"prompt_tokens": 1, "total_tokens": 1}
            }),
        })
        .await;
        let router = RouterFixture::new_with_model_kind(
            context,
            &fixture,
            &upstream.base_url,
            ModelKind::Embedding,
        )
        .await;
        let response = router
            .send_raw_post(
                "/openai/v1/embeddings".to_string(),
                json!({"model": router.requested_model(), "input": "hello"}),
                DownstreamAuth::Bearer,
            )
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("utility response should be consumed");
        let requests = upstream.requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/v1/embeddings");
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn embeddings_execute_once_for_each_openai_wire_profile_and_preserve_the_response() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    for profile_type in [
        UpstreamProfileType::Openai,
        UpstreamProfileType::OpenaiCompatible,
        UpstreamProfileType::GeminiOpenai,
    ] {
        let case_name = format!("embeddings-profile-{profile_type:?}");
        let fixture = fixture.clone();
        run_case(&case_name, move |context| async move {
            let upstream_response =
                br#"{ "object":"list", "data":[], "usage":{"prompt_tokens":6,"total_tokens":6} }"#
                    .to_vec();
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("application/json; charset=utf-8".to_string()),
                content_encoding: None,
                body: upstream_response.clone(),
            })
            .await;
            let router = RouterFixture::new_with_model_kind(
                context,
                &fixture,
                &upstream.base_url,
                ModelKind::Embedding,
            )
            .await;
            if profile_type != UpstreamProfileType::Openai {
                router
                    .replace_default_openai_profile_in_place(&upstream.base_url, profile_type)
                    .await;
            }
            let (catalog_id, catalog_version_id) =
                router.attach_cost_catalog(Some(100), Some(2)).await;
            let request = match profile_type {
                UpstreamProfileType::Openai => json!({
                    "model": router.requested_model(),
                    "input": [[1, 2], [3, 4]],
                    "encoding_format": "base64",
                    "dimensions": 256,
                    "user": "direct-regression",
                    "future_extension": {"preserve": true}
                }),
                UpstreamProfileType::OpenaiCompatible => json!({
                    "model": router.requested_model(),
                    "input": ["hello", "world"],
                    "encoding_format": {"vendor": "private"},
                    "dimensions": "provider-default",
                    "vendor_extension": [1, 2, 3]
                }),
                UpstreamProfileType::GeminiOpenai => json!({
                    "model": router.requested_model(),
                    "input": "hello"
                }),
                _ => unreachable!("the test enumerates OpenAI-wire Profiles"),
            };

            let response = router
                .send_raw_post(
                    "/openai/v1/embeddings".to_string(),
                    request.clone(),
                    DownstreamAuth::Bearer,
                )
                .await;

            assert_eq!(response.status(), StatusCode::OK, "{profile_type:?}");
            assert_eq!(
                response
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok()),
                Some("application/json; charset=utf-8")
            );
            let downstream_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("embeddings response should be readable");
            assert_eq!(downstream_body.as_ref(), upstream_response.as_slice());

            let requests = upstream.requests().await;
            assert_eq!(requests.len(), 1, "{profile_type:?}");
            assert_eq!(requests[0].method, Method::POST);
            assert_eq!(requests[0].path, "/v1/embeddings");
            let upstream_body: Value = serde_json::from_slice(&requests[0].body)
                .expect("embeddings upstream request should be JSON");
            assert_eq!(upstream_body["model"], UPSTREAM_MODEL);
            for (field, value) in request
                .as_object()
                .expect("test request should be an object")
            {
                if field != "model" {
                    assert_eq!(upstream_body.get(field), Some(value), "{field}");
                }
            }

            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(log.model_kind_snapshot, Some(ModelKind::Embedding));
            assert_eq!(log.source_profile_type_snapshot, Some(profile_type));
            assert_eq!(log.total_input_tokens, Some(6));
            assert_eq!(log.total_output_tokens, None);
            assert_eq!(log.total_tokens, Some(6));
            assert_eq!(log.estimated_cost_nanos, Some(112));
            assert_eq!(log.cost_catalog_id, Some(catalog_id));
            assert_eq!(log.cost_catalog_version_id, Some(catalog_version_id));
            let snapshot: CostSnapshot = serde_json::from_str(
                log.cost_snapshot_json
                    .as_deref()
                    .expect("embedding cost snapshot should persist"),
            )
            .expect("embedding cost snapshot should parse");
            assert_eq!(snapshot.total_cost_nanos, 112);
            assert_eq!(snapshot.unmatched_items, Vec::<String>::new());
            assert_eq!(snapshot.detail_lines.len(), 2);
            assert_eq!(
                snapshot
                    .detail_lines
                    .iter()
                    .map(|line| (line.meter_key, line.quantity, line.amount_nanos))
                    .collect::<Vec<_>>(),
                vec![
                    (MeterKey::LlmInputTextTokens, 6, 12),
                    (MeterKey::InvokeRequestCalls, 1, 100),
                ]
            );
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn embeddings_without_usage_bill_only_the_successful_invocation_without_estimating_tokens() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    run_case("embeddings-missing-usage-cost", move |context| async move {
        let upstream = TestUpstream::spawn_json(
            StatusCode::OK,
            json!({"object": "list", "data": [], "model": UPSTREAM_MODEL}),
        )
        .await;
        let router = RouterFixture::new_with_model_kind(
            context,
            &fixture,
            &upstream.base_url,
            ModelKind::Embedding,
        )
        .await;
        let (catalog_id, catalog_version_id) = router.attach_cost_catalog(Some(100), Some(2)).await;

        let response = router
            .send_raw_post(
                "/openai/v1/embeddings".to_string(),
                json!({
                    "model": router.requested_model(),
                    "input": "a deliberately non-empty request body that must never be estimated"
                }),
                DownstreamAuth::Bearer,
            )
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("embedding response should be readable");
        assert_eq!(upstream.requests().await.len(), 1);
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_eq!(log.total_input_tokens, None);
        assert_eq!(log.total_output_tokens, None);
        assert_eq!(log.total_tokens, None);
        assert_eq!(log.estimated_cost_nanos, Some(100));
        assert_eq!(log.cost_catalog_id, Some(catalog_id));
        assert_eq!(log.cost_catalog_version_id, Some(catalog_version_id));
        let snapshot: CostSnapshot = serde_json::from_str(
            log.cost_snapshot_json
                .as_deref()
                .expect("request-fee-only snapshot should persist"),
        )
        .expect("request-fee-only snapshot should parse");
        assert_eq!(snapshot.total_cost_nanos, 100);
        assert_eq!(snapshot.detail_lines.len(), 1);
        assert_eq!(
            snapshot.detail_lines[0].meter_key,
            MeterKey::InvokeRequestCalls
        );
        assert_eq!(snapshot.detail_lines[0].quantity, 1);
        assert_eq!(snapshot.unmatched_items, Vec::<String>::new());
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn chat_without_usage_records_an_unmatched_invocation_when_the_catalog_has_no_component() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    run_case(
        "chat-missing-usage-unmatched-invocation",
        move |context| async move {
            let mut upstream_response = fixture.non_stream.upstream_response.clone();
            upstream_response
                .as_object_mut()
                .expect("OpenAI response fixture should be an object")
                .remove("usage");
            let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_response).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let (catalog_id, catalog_version_id) = router.attach_cost_catalog(None, None).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;

            assert_eq!(response.status(), StatusCode::OK);
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("chat response should be readable");
            assert_eq!(upstream.requests().await.len(), 1);
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(log.total_input_tokens, None);
            assert_eq!(log.total_output_tokens, None);
            assert_eq!(log.total_tokens, None);
            assert_eq!(log.estimated_cost_nanos, Some(0));
            assert_eq!(log.cost_catalog_id, Some(catalog_id));
            assert_eq!(log.cost_catalog_version_id, Some(catalog_version_id));
            let snapshot: CostSnapshot = serde_json::from_str(
                log.cost_snapshot_json
                    .as_deref()
                    .expect("unmatched invocation snapshot should persist"),
            )
            .expect("unmatched invocation snapshot should parse");
            assert!(snapshot.detail_lines.is_empty());
            assert_eq!(
                snapshot.unmatched_items,
                vec![MeterKey::InvokeRequestCalls.to_string()]
            );
            assert!(snapshot.warnings.is_empty());
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        },
    );
}

#[test]
fn invalid_embeddings_requests_are_rejected_before_credentials_and_network() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    for (profile_type, invalid_request) in [
        (
            UpstreamProfileType::Openai,
            json!({
                "input": "hello",
                "encoding_format": "private-payload-marker"
            }),
        ),
        (
            UpstreamProfileType::OpenaiCompatible,
            json!({"input": [1, "private-payload-marker"]}),
        ),
        (
            UpstreamProfileType::GeminiOpenai,
            json!({
                "input": "hello",
                "private-payload-marker": {"secret": true}
            }),
        ),
    ] {
        let case_name = format!("invalid-embeddings-{profile_type:?}");
        let fixture = fixture.clone();
        run_case(&case_name, move |context| async move {
            let upstream =
                TestUpstream::spawn_json(StatusCode::OK, json!({"unexpected": true})).await;
            let router = RouterFixture::new_with_model_kind(
                context,
                &fixture,
                &upstream.base_url,
                ModelKind::Embedding,
            )
            .await;
            if profile_type != UpstreamProfileType::Openai {
                router
                    .replace_default_openai_profile_in_place(&upstream.base_url, profile_type)
                    .await;
            }
            let mut invalid_request = invalid_request;
            invalid_request["model"] = json!(router.requested_model());
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router
                .send_raw_post(
                    "/openai/v1/embeddings".to_string(),
                    invalid_request,
                    DownstreamAuth::Bearer,
                )
                .await;

            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{profile_type:?}"
            );
            let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("embeddings rejection should be readable");
            assert!(!String::from_utf8_lossy(&response_body).contains("private-payload-marker"));
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty());
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("invalid_request_error")
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn embeddings_upstream_errors_are_single_call_and_release_the_lease() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    run_case("embeddings-upstream-error", move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Raw {
            status: StatusCode::TOO_MANY_REQUESTS,
            content_type: Some("application/json".to_string()),
            content_encoding: None,
            body: br#"{"error":{"message":"embedding rate limit"}}"#.to_vec(),
        })
        .await;
        let router = RouterFixture::new_with_model_kind(
            context,
            &fixture,
            &upstream.base_url,
            ModelKind::Embedding,
        )
        .await;

        let response = router
            .send_raw_post(
                "/openai/v1/embeddings".to_string(),
                json!({"model": router.requested_model(), "input": "hello"}),
                DownstreamAuth::Bearer,
            )
            .await;

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("embeddings error response should be readable");
        assert_eq!(upstream.requests().await.len(), 1);
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(log.upstream_http_status, Some(429));
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn embeddings_cancellation_closes_the_upstream_and_logs_one_terminal_request() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    run_case("embeddings-cancellation", move |context| async move {
        let dropped = Arc::new(DropSignal::default());
        let upstream = TestUpstream::spawn(ScriptedReply::HangingBody {
            content_type: "application/json".to_string(),
            first_chunk: br#"{"object":"list","data":["#.to_vec(),
            dropped: Arc::clone(&dropped),
        })
        .await;
        let router = RouterFixture::new_with_model_kind(
            context,
            &fixture,
            &upstream.base_url,
            ModelKind::Embedding,
        )
        .await;
        let cancellation = ProxyCancellationContext::new();
        let cancellation_trigger = cancellation.clone();
        let captured = Arc::clone(&upstream.captured);
        let cancel_task = tokio::spawn(async move {
            let deadline = Instant::now() + WAIT_TIMEOUT;
            loop {
                if !captured.lock().await.is_empty() {
                    cancellation_trigger.cancel_now("embedding client disconnected");
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "embeddings request should reach upstream before cancellation"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        let response = timeout(
            WAIT_TIMEOUT,
            router.send_raw_post_with_cancellation(
                "/openai/v1/embeddings".to_string(),
                json!({"model": router.requested_model(), "input": "hello"}),
                DownstreamAuth::Bearer,
                cancellation,
            ),
        )
        .await
        .expect("cancelled embeddings request should finish");
        cancel_task
            .await
            .expect("embeddings cancellation trigger should join");

        assert_eq!(response.status().as_u16(), 499);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("cancellation response should be readable");
        dropped.wait().await;
        assert_eq!(upstream.requests().await.len(), 1);
        let log = router.wait_for_log(RequestStatus::Cancelled).await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("client_cancelled_error")
        );
        assert_eq!(log.upstream_http_status, Some(200));
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        upstream.shutdown().await;
    });
}

#[test]
fn compatible_rerank_is_a_single_call_transparent_transport_without_private_usage_parsing() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    run_case("compatible-rerank-success", move |context| async move {
        let upstream_response = br#"{ "results":[{"index":0,"score":0.98}], "meta":{"tokens":{"input_tokens":777}}, "vendor":"opaque" }"#.to_vec();
        let upstream = TestUpstream::spawn(ScriptedReply::Raw {
            status: StatusCode::CREATED,
            content_type: Some("application/vnd.rerank+json; version=2".to_string()),
            content_encoding: None,
            body: upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new_with_model_kind(
            context,
            &fixture,
            &upstream.base_url,
            ModelKind::Rerank,
        )
        .await;
        router
            .replace_default_openai_profile_in_place(
                &upstream.base_url,
                UpstreamProfileType::OpenaiCompatible,
            )
            .await;
        router
            .update_source_operation(UpdateUpstreamSourceData {
                rerank_enabled: Some(true),
                rerank_path_override: Some(Some("vendor/v2/rank".to_string())),
                updated_at: chrono::Utc::now().timestamp_millis(),
                ..UpdateUpstreamSourceData::test_defaults()
            })
            .await;
        let (catalog_id, catalog_version_id) = router.attach_cost_catalog(Some(37), Some(2)).await;
        let request = json!({
            "model": router.requested_model(),
            "query": {"vendor_query": ["hello"]},
            "documents": "provider-defined-document-envelope",
            "top_n": "provider-default",
            "return_documents": {"opaque": true},
            "vendor_extension": [1, false, null]
        });

        let response = router
            .send_raw_post(
                "/openai/v1/rerank".to_string(),
                request.clone(),
                DownstreamAuth::Bearer,
            )
            .await;

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/vnd.rerank+json")
        );
        let downstream_body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("rerank response should be readable");
        assert_eq!(downstream_body.as_ref(), upstream_response.as_slice());

        let requests = upstream.requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, Method::POST);
        assert_eq!(requests[0].path, "/v1/vendor/v2/rank");
        let upstream_body: Value = serde_json::from_slice(&requests[0].body)
            .expect("rerank upstream request should be JSON");
        assert_eq!(upstream_body["model"], UPSTREAM_MODEL);
        for (field, value) in request
            .as_object()
            .expect("rerank test request should be an object")
        {
            if field != "model" {
                assert_eq!(upstream_body.get(field), Some(value), "{field}");
            }
        }

        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_eq!(log.model_kind_snapshot, Some(ModelKind::Rerank));
        assert_eq!(
            log.source_profile_type_snapshot,
            Some(UpstreamProfileType::OpenaiCompatible)
        );
        assert_eq!(log.total_input_tokens, None);
        assert_eq!(log.total_output_tokens, None);
        assert_eq!(log.total_tokens, None);
        assert_eq!(log.estimated_cost_nanos, Some(37));
        assert_eq!(log.cost_catalog_id, Some(catalog_id));
        assert_eq!(log.cost_catalog_version_id, Some(catalog_version_id));
        let snapshot: CostSnapshot = serde_json::from_str(
            log.cost_snapshot_json
                .as_deref()
                .expect("rerank invocation snapshot should persist"),
        )
        .expect("rerank invocation snapshot should parse");
        assert_eq!(snapshot.total_cost_nanos, 37);
        assert_eq!(snapshot.detail_lines.len(), 1);
        assert_eq!(
            snapshot.detail_lines[0].meter_key,
            MeterKey::InvokeRequestCalls
        );
        assert_eq!(snapshot.detail_lines[0].quantity, 1);
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

#[test]
fn rerank_requires_an_enabled_compatible_source_before_credential_decryption() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    for profile_type in [
        UpstreamProfileType::Openai,
        UpstreamProfileType::GeminiOpenai,
        UpstreamProfileType::OpenaiCompatible,
    ] {
        let case_name = format!("rerank-profile-guard-{profile_type:?}");
        let fixture = fixture.clone();
        run_case(&case_name, move |context| async move {
            let upstream =
                TestUpstream::spawn_json(StatusCode::OK, json!({"unexpected": true})).await;
            let router = RouterFixture::new_with_model_kind(
                context,
                &fixture,
                &upstream.base_url,
                ModelKind::Rerank,
            )
            .await;
            if profile_type != UpstreamProfileType::Openai {
                router
                    .replace_default_openai_profile_in_place(&upstream.base_url, profile_type)
                    .await;
            }
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router
                .send_raw_post(
                    "/openai/v1/rerank".to_string(),
                    json!({
                        "model": router.requested_model(),
                        "query": "hello",
                        "documents": ["world"]
                    }),
                    DownstreamAuth::Bearer,
                )
                .await;

            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{profile_type:?}"
            );
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty());
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("unsupported_capability_error")
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn compatible_rerank_errors_and_success_body_limits_are_bounded_single_calls() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    for (case_name, upstream_status, upstream_body, expected_status, expected_code) in [
        (
            "rerank-upstream-error",
            StatusCode::SERVICE_UNAVAILABLE,
            br#"{"error":{"message":"rerank unavailable"}}"#.to_vec(),
            StatusCode::SERVICE_UNAVAILABLE,
            "upstream_service_error",
        ),
        (
            "rerank-success-body-limit",
            StatusCode::OK,
            vec![b'x'; 1_048_577],
            StatusCode::BAD_GATEWAY,
            "upstream_response_error",
        ),
    ] {
        let fixture = fixture.clone();
        run_case(case_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: upstream_status,
                content_type: Some("application/vnd.rerank+json".to_string()),
                content_encoding: None,
                body: upstream_body,
            })
            .await;
            let mut router = RouterFixture::new_with_model_kind(
                context.clone(),
                &fixture,
                &upstream.base_url,
                ModelKind::Rerank,
            )
            .await;
            router
                .replace_default_openai_profile_in_place(
                    &upstream.base_url,
                    UpstreamProfileType::OpenaiCompatible,
                )
                .await;
            router
                .update_source_operation(UpdateUpstreamSourceData {
                    rerank_enabled: Some(true),
                    updated_at: chrono::Utc::now().timestamp_millis(),
                    ..UpdateUpstreamSourceData::test_defaults()
                })
                .await;
            let (catalog_id, _catalog_version_id) =
                router.attach_cost_catalog(Some(37), Some(2)).await;
            if upstream_status.is_success() {
                router
                    .replace_proxy_request_config(context, one_mib_non_stream_proxy_config(65_536))
                    .await;
            }

            let response = router
                .send_raw_post(
                    "/openai/v1/rerank".to_string(),
                    json!({
                        "model": router.requested_model(),
                        "query": "hello",
                        "documents": ["world"]
                    }),
                    DownstreamAuth::Bearer,
                )
                .await;

            assert_eq!(response.status(), expected_status, "{case_name}");
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("rerank failure response should be readable");
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(log.final_error_code.as_deref(), Some(expected_code));
            assert_eq!(
                log.upstream_http_status,
                Some(i32::from(upstream_status.as_u16()))
            );
            assert_eq!(log.cost_catalog_id, Some(catalog_id));
            assert_eq!(log.estimated_cost_nanos, None);
            assert_eq!(log.cost_catalog_version_id, None);
            assert_eq!(log.cost_snapshot_json, None);
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn compatible_rerank_cancellation_closes_the_upstream_and_releases_resources() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    run_case("rerank-cancellation", move |context| async move {
        let dropped = Arc::new(DropSignal::default());
        let upstream = TestUpstream::spawn(ScriptedReply::HangingBody {
            content_type: "application/vnd.rerank+json".to_string(),
            first_chunk: br#"{"results":["#.to_vec(),
            dropped: Arc::clone(&dropped),
        })
        .await;
        let router = RouterFixture::new_with_model_kind(
            context,
            &fixture,
            &upstream.base_url,
            ModelKind::Rerank,
        )
        .await;
        router
            .replace_default_openai_profile_in_place(
                &upstream.base_url,
                UpstreamProfileType::OpenaiCompatible,
            )
            .await;
        router
            .update_source_operation(UpdateUpstreamSourceData {
                rerank_enabled: Some(true),
                updated_at: chrono::Utc::now().timestamp_millis(),
                ..UpdateUpstreamSourceData::test_defaults()
            })
            .await;
        let (catalog_id, _catalog_version_id) = router.attach_cost_catalog(Some(37), Some(2)).await;
        let cancellation = ProxyCancellationContext::new();
        let cancellation_trigger = cancellation.clone();
        let captured = Arc::clone(&upstream.captured);
        let cancel_task = tokio::spawn(async move {
            let deadline = Instant::now() + WAIT_TIMEOUT;
            loop {
                if !captured.lock().await.is_empty() {
                    cancellation_trigger.cancel_now("rerank client disconnected");
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "rerank request should reach upstream before cancellation"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        let response = timeout(
            WAIT_TIMEOUT,
            router.send_raw_post_with_cancellation(
                "/openai/v1/rerank".to_string(),
                json!({
                    "model": router.requested_model(),
                    "query": "hello",
                    "documents": ["world"]
                }),
                DownstreamAuth::Bearer,
                cancellation,
            ),
        )
        .await
        .expect("cancelled rerank request should finish");
        cancel_task
            .await
            .expect("rerank cancellation trigger should join");

        assert_eq!(response.status().as_u16(), 499);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("rerank cancellation response should be readable");
        dropped.wait().await;
        assert_eq!(upstream.requests().await.len(), 1);
        let log = router.wait_for_log(RequestStatus::Cancelled).await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("client_cancelled_error")
        );
        assert_eq!(log.cost_catalog_id, Some(catalog_id));
        assert_eq!(log.estimated_cost_nanos, None);
        assert_eq!(log.cost_catalog_version_id, None);
        assert_eq!(log.cost_snapshot_json, None);
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_client_identity_http_persists_normalized_forwarded_ip() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let resolver = Arc::new(ClientIdentityResolver::new(&ClientIdentityConfig {
            trusted_proxy_cidrs: vec!["10.0.0.0/8".parse().expect("test CIDR should parse")],
            max_forwarded_hops: 8,
        }));

        let response = router
            .send_with_client_identity(
                &fixture,
                false,
                &fixture.request.downstream,
                SocketAddr::from(([10, 0, 0, 9], 3000)),
                Some("for=\"[::ffff:198.51.100.42]:8443\""),
                resolver,
                None,
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("successful response body should be consumed before log assertion");
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_eq!(log.client_ip.as_deref(), Some("198.51.100.42"));
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_regression_stream_events_usage_and_single_call_golden() {
    for (name, fixture) in generation_evidence_fixtures() {
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Sse {
                events: fixture.stream.upstream_events.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{name}: stream status");
            assert!(response.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|value| value.starts_with(&fixture.stream.downstream_content_type)), "{name}: stream content type");
            let request_id = assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let actual_events = parse_downstream_events(fixture.protocol, &body);
            assert_eq!(
                normalized_events(actual_events.clone()),
                normalized_events(fixture.stream.downstream_events.clone()),
                "{name}: ordered stream events; body={}",
                String::from_utf8_lossy(&body)
            );
            assert_eq!(
                stream_text(fixture.protocol, &actual_events),
                fixture.stream.expected_text,
                "{name}: stream text"
            );
            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.stream.upstream_path,
                fixture.stream.upstream_query.as_deref(),
                &fixture.stream.upstream_request,
                &router.requested_model(),
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(
                log.request_id, request_id,
                "{name}: stream response and persisted canonical request id"
            );
            assert_log_common(&router, &fixture, &log);
            assert_log_timing_order(&log);
            assert!(log.first_token_at.is_some(), "{name}: stream TTFT sample");
            assert!(log.is_stream, "{name}: log should be streaming");
            assert_usage(&log, &fixture.usage);
            upstream.shutdown().await;
        });
    }
}

#[test]
fn all_public_downstream_reasoning_controls_reach_the_openai_target() {
    let openai_response = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture.non_stream.upstream_response)
        .expect("openai response fixture");

    for (name, fixture) in fixtures() {
        let upstream_response = openai_response.clone();
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::OK,
                body: upstream_response,
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            if fixture.profile_type != UpstreamProfileType::Openai {
                router
                    .replace_default_source_profile(&upstream.base_url, UpstreamProfileType::Openai)
                    .await;
            }

            let mut request = fixture.request.downstream.clone();
            let expected_effort = match fixture.protocol {
                DownstreamProtocol::Openai => {
                    request["reasoning_effort"] = json!("medium");
                    "medium"
                }
                DownstreamProtocol::Responses => {
                    request["reasoning"] = json!({
                        "effort": "medium",
                        "summary": "detailed"
                    });
                    "medium"
                }
                DownstreamProtocol::Anthropic => {
                    request["thinking"] = json!({
                        "type": "enabled",
                        "budget_tokens": 1024
                    });
                    "high"
                }
                DownstreamProtocol::Gemini => {
                    request["generationConfig"]["thinkingConfig"] = json!({
                        "thinkingLevel": "medium",
                        "includeThoughts": true
                    });
                    "medium"
                }
            };

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::OK, "{name}");
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("reasoning response should complete");
            let captured = upstream.requests().await;
            assert_eq!(captured.len(), 1, "{name}");
            let body: Value = serde_json::from_slice(&captured[0].body)
                .expect("OpenAI target request should be JSON");
            assert_eq!(
                body.get("reasoning_effort"),
                Some(&json!(expected_effort)),
                "{name}"
            );
            let serialized = serde_json::to_string(&body).expect("JSON should serialize");
            assert!(!serialized.contains("summary"), "{name}");
            assert!(!serialized.contains("display"), "{name}");
            assert!(!serialized.contains("includeThoughts"), "{name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn all_public_downstream_reasoning_controls_reach_the_responses_target() {
    let mut upstream_response = responses_target_golden().non_stream_response;
    upstream_response["output"] = json!([
        {"type":"reasoning","id":"rs_visible","content":[],
         "summary":[{"type":"summary_text","text":"visible summary"}],
         "encrypted_content":null},
        {"type":"message","id":"msg_answer","status":"completed","role":"assistant",
         "content":[{"type":"output_text","text":"reasoned answer","annotations":[],"logprobs":[]}]}
    ]);

    for (name, fixture) in responses_target_fixtures() {
        let upstream_response = upstream_response.clone();
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_response).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let mut request = fixture.request.downstream.clone();
            let expected_effort = match fixture.protocol {
                DownstreamProtocol::Openai => {
                    request["reasoning_effort"] = json!("medium");
                    "medium"
                }
                DownstreamProtocol::Responses => {
                    request["reasoning"] = json!({"effort":"xhigh","summary":"detailed"});
                    "xhigh"
                }
                DownstreamProtocol::Anthropic => {
                    request["thinking"] = json!({"type":"adaptive","display":"summarized"});
                    request["output_config"] = json!({"effort":"max"});
                    "xhigh"
                }
                DownstreamProtocol::Gemini => {
                    request["generationConfig"]["thinkingConfig"] = json!({
                        "thinkingLevel":"low","includeThoughts":true
                    });
                    "low"
                }
            };

            let response = router.send(&fixture, false, &request).await;
            assert_eq!(response.status(), StatusCode::OK, "{name}");
            assert_no_public_transform_diagnostics(&response);
            let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("reasoning response should complete");
            let public = String::from_utf8_lossy(&response_body);
            assert!(public.contains("reasoned answer"), "{name}: {public}");
            if fixture.protocol == DownstreamProtocol::Openai {
                let public: Value = serde_json::from_slice(&response_body)
                    .expect("OpenAI reasoning response should be JSON");
                assert_eq!(
                    public.pointer("/choices/0/message/content"),
                    Some(&json!("reasoned answer")),
                    "{name}: answer text remains distinct"
                );
                assert_eq!(
                    public.pointer("/choices/0/message/reasoning_content"),
                    Some(&json!("visible summary")),
                    "{name}: reasoning uses the dedicated field"
                );
            } else {
                assert!(
                    public.contains("visible summary"),
                    "{name}: caller-visible reasoning"
                );
            }

            let captured = upstream.requests().await;
            assert_eq!(captured.len(), 1, "{name}: exactly one Responses call");
            let body: Value = serde_json::from_slice(&captured[0].body)
                .expect("Responses target request should be JSON");
            assert_eq!(body["reasoning"]["effort"], expected_effort, "{name}");
            assert_eq!(body["store"], false, "{name}");
            if fixture.protocol == DownstreamProtocol::Responses {
                assert_eq!(body["reasoning"]["summary"], "detailed", "{name}");
            } else {
                assert!(
                    body["reasoning"].get("summary").is_none_or(Value::is_null),
                    "{name}"
                );
            }
            let serialized = serde_json::to_string(&body).expect("JSON");
            assert!(!serialized.contains("display"), "{name}");
            assert!(!serialized.contains("includeThoughts"), "{name}");

            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(log.reasoning_tokens, Some(2), "{name}");
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn responses_target_rejects_reasoning_conflicts_before_credentials() {
    const PRIVATE_MARKER: &str = "responses-reasoning-private-marker";
    for (name, fixture) in responses_target_fixtures() {
        run_case(name, move |context| async move {
            let upstream =
                TestUpstream::spawn_json(StatusCode::OK, json!({"should_not":"be called"})).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let mut request = fixture.request.downstream.clone();
            match fixture.protocol {
                DownstreamProtocol::Openai => {
                    request["reasoning_effort"] = json!(PRIVATE_MARKER);
                }
                DownstreamProtocol::Responses => {
                    request["reasoning"] = json!({"effort":PRIVATE_MARKER});
                }
                DownstreamProtocol::Anthropic => {
                    request["thinking"] = json!({"type":"disabled","display":PRIVATE_MARKER});
                    request["output_config"] = json!({"effort":"high"});
                }
                DownstreamProtocol::Gemini => {
                    request["generationConfig"]["thinkingConfig"] = json!({
                        "thinkingLevel":"low","thinkingBudget":128,
                        "private":PRIVATE_MARKER
                    });
                }
            }
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router.send(&fixture, false, &request).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("reasoning rejection should be readable");
            assert!(
                !String::from_utf8_lossy(&body).contains(PRIVATE_MARKER),
                "{name}"
            );
            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                0,
                "{name}"
            );
            assert!(upstream.requests().await.is_empty(), "{name}");
            let log = router
                .wait_for_log_for_source(router.source_id, RequestStatus::Error)
                .await;
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains(PRIVATE_MARKER),
                "{name}: private reasoning control must not enter Request Log"
            );
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn gemini_stream_preserves_openai_reasoning_as_native_thought_parts() {
    let mut upstream_events = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture.stream.upstream_events)
        .expect("OpenAI stream fixture");
    upstream_events.insert(
        0,
        GoldenEvent {
            event: None,
            data: json!({
                "id": "chatcmpl-reasoning",
                "object": "chat.completion.chunk",
                "created": 1700000000,
                "model": "baseline-upstream-model",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "role": "assistant",
                        "reasoning_content": "considering the answer"
                    },
                    "finish_reason": null
                }]
            }),
        },
    );
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "gemini")
        .expect("Gemini fixture");

    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Sse {
            events: upstream_events,
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let source_id = router
            .replace_default_source_profile(&upstream.base_url, UpstreamProfileType::Openai)
            .await;

        let response = router
            .send(&fixture, true, &fixture.stream.downstream_request)
            .await;

        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Gemini reasoning stream should complete");
        assert_eq!(
            status,
            StatusCode::OK,
            "Gemini reasoning stream body={}",
            String::from_utf8_lossy(&body)
        );
        let events = parse_downstream_events(DownstreamProtocol::Gemini, &body);
        assert!(events.iter().any(|event| {
            event.data.pointer("/candidates/0/content/parts/0/text")
                == Some(&json!("considering the answer"))
                && event.data.pointer("/candidates/0/content/parts/0/thought") == Some(&json!(true))
        }));
        assert!(events.iter().any(|event| {
            event.data.pointer("/candidates/0/content/parts/0/text") == Some(&json!("baseline "))
        }));
        router
            .wait_for_log_for_source(source_id, RequestStatus::Success)
            .await;
        upstream.shutdown().await;
    });
}

#[test]
fn all_public_downstream_multimodal_inputs_reach_the_openai_target() {
    let openai_response = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture.non_stream.upstream_response)
        .expect("openai response fixture");

    for (name, fixture) in fixtures() {
        let upstream_response = openai_response.clone();
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::OK,
                body: upstream_response,
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            if fixture.profile_type != UpstreamProfileType::Openai {
                router
                    .replace_default_source_profile(&upstream.base_url, UpstreamProfileType::Openai)
                    .await;
            }

            let mut request = fixture.request.downstream.clone();
            match fixture.protocol {
                DownstreamProtocol::Openai => {
                    request["messages"] = json!([{
                        "role": "user",
                        "content": [
                            {"type":"image_url","image_url":{"url":"data:image/png;base64,ZmFrZQ==","detail":"high"}},
                            {"type":"input_audio","input_audio":{"data":"UklGRg==","format":"wav"}},
                            {"type":"file","file":{"filename":"report.pdf","file_data":"JVBERi0="}}
                        ]
                    }]);
                }
                DownstreamProtocol::Responses => {
                    request["input"] = json!([{
                        "type": "message",
                        "role": "user",
                        "content": [
                            {"type":"input_image","image_url":"https://example.com/image.png","detail":"high"},
                            {"type":"input_audio","input_audio":{"data":"SUQz","format":"mp3"}},
                            {"type":"input_file","filename":"report.pdf","file_data":"JVBERi0="},
                            {"type":"input_image","file_id":"file_same_target"}
                        ]
                    }]);
                }
                DownstreamProtocol::Anthropic => {
                    request["messages"] = json!([{
                        "role": "user",
                        "content": [
                            {"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"ZmFrZQ=="}},
                            {"type":"image","source":{"type":"url","url":"https://example.com/image.webp"}},
                            {"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"JVBERi0="},"title":"report.pdf"}
                        ]
                    }]);
                }
                DownstreamProtocol::Gemini => {
                    request["contents"] = json!([{
                        "role": "user",
                        "parts": [
                            {"inlineData":{"mimeType":"image/png","data":"ZmFrZQ=="}},
                            {"inlineData":{"mimeType":"audio/wav","data":"UklGRg=="}},
                            {"inlineData":{"mimeType":"application/pdf","data":"JVBERi0=","displayName":"report.pdf"}}
                        ]
                    }]);
                }
            }

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::OK, "{name}");
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("multimodal response should complete");
            let captured = upstream.requests().await;
            assert_eq!(captured.len(), 1, "{name}: no retry or upload call");
            let body: Value = serde_json::from_slice(&captured[0].body)
                .expect("OpenAI target request should be JSON");
            let parts = body["messages"][0]["content"]
                .as_array()
                .expect("OpenAI target should receive typed content parts");
            assert!(
                parts.iter().any(|part| part["type"] == "image_url"),
                "{name}: image input"
            );
            if name != "anthropic" {
                assert!(
                    parts.iter().any(|part| part["type"] == "input_audio"),
                    "{name}: audio input"
                );
            }
            assert!(
                parts.iter().any(|part| part["type"] == "file"),
                "{name}: file input"
            );
            let serialized = serde_json::to_string(&body).expect("JSON should serialize");
            assert!(!serialized.contains("file_data: "), "{name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn all_public_downstream_multimodal_inputs_reach_the_responses_target() {
    let upstream_response = responses_target_golden().non_stream_response;

    for (name, fixture) in responses_target_fixtures() {
        let upstream_response = upstream_response.clone();
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_response).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let mut request = fixture.request.downstream.clone();
            match fixture.protocol {
                DownstreamProtocol::Openai => {
                    request["messages"] = json!([{
                        "role":"user","content":[
                            {"type":"image_url","image_url":{"url":"https://example.com/image.png","detail":"high"}},
                            {"type":"image_url","image_url":{"url":"data:image/png;base64,ZmFrZQ=="}},
                            {"type":"input_audio","input_audio":{"data":"UklGRg==","format":"wav"}},
                            {"type":"file","file":{"filename":"report.pdf","file_data":"JVBERi0="}},
                            {"type":"file","file":{"file_id":"file_same_target"}}
                        ]
                    }]);
                }
                DownstreamProtocol::Responses => {
                    request["input"] = json!([{
                        "type":"message","role":"user","content":[
                            {"type":"input_image","image_url":"data:image/webp;base64,ZmFrZQ==","detail":"low"},
                            {"type":"input_audio","input_audio":{"data":"SUQz","format":"mp3"}},
                            {"type":"input_file","filename":"notes.md","file_data":"IyBub3Rlcw=="},
                            {"type":"input_file","file_id":"file_same_target"}
                        ]
                    }]);
                }
                DownstreamProtocol::Anthropic => {
                    request["messages"] = json!([{
                        "role":"user","content":[
                            {"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"ZmFrZQ=="}},
                            {"type":"image","source":{"type":"url","url":"https://example.com/image.webp"}},
                            {"type":"document","source":{"type":"base64","media_type":"text/csv","data":"YSxi"},"title":"table.csv"}
                        ]
                    }]);
                }
                DownstreamProtocol::Gemini => {
                    request["contents"] = json!([{
                        "role":"user","parts":[
                            {"inlineData":{"mimeType":"image/png","data":"ZmFrZQ==","displayName":"preview.png"}},
                            {"inlineData":{"mimeType":"audio/mpeg","data":"SUQz","displayName":"voice.mp3"}},
                            {"inlineData":{"mimeType":"application/json","data":"e30=","displayName":"data.json"}},
                            {"fileData":{"mimeType":"image/jpeg","fileUri":"https://example.com/image.jpg","displayName":"preview.jpg"}}
                        ]
                    }]);
                }
            }

            let response = router.send(&fixture, false, &request).await;
            assert_eq!(response.status(), StatusCode::OK, "{name}");
            assert_no_public_transform_diagnostics(&response);
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("multimodal response should complete");

            let captured = upstream.requests().await;
            assert_eq!(captured.len(), 1, "{name}: no upload, probe, or retry call");
            let body: Value = serde_json::from_slice(&captured[0].body)
                .expect("Responses target request should be JSON");
            assert_eq!(body["store"], false, "{name}");
            let parts = body["input"]
                .as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .find_map(|item| item.get("content").and_then(Value::as_array))
                })
                .expect("Responses target should receive typed message content");
            assert!(
                parts.iter().any(|part| part["type"] == "input_image"),
                "{name}"
            );
            assert!(
                parts.iter().any(|part| part["type"] == "input_file"),
                "{name}"
            );
            if fixture.protocol != DownstreamProtocol::Anthropic {
                assert!(
                    parts.iter().any(|part| part["type"] == "input_audio"),
                    "{name}"
                );
            }
            if fixture.protocol == DownstreamProtocol::Openai {
                assert!(
                    parts
                        .iter()
                        .any(|part| { part["type"] == "input_image" && part["detail"] == "high" })
                );
            }
            let serialized = serde_json::to_string(&body).expect("JSON");
            assert!(!serialized.contains("file_data: "), "{name}");
            assert!(!serialized.contains("displayName"), "{name}");

            router.wait_for_log(RequestStatus::Success).await;
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn all_public_downstreams_reject_unportable_media_before_credentials() {
    const PRIVATE_MARKER: &str = "multimodal-private-marker";
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let upstream =
                TestUpstream::spawn_json(StatusCode::OK, json!({"should_not":"be called"})).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            if fixture.profile_type != UpstreamProfileType::Openai {
                router
                    .replace_default_source_profile(&upstream.base_url, UpstreamProfileType::Openai)
                    .await;
            }
            let mut request = fixture.request.downstream.clone();
            match fixture.protocol {
                DownstreamProtocol::Openai => {
                    request["messages"] = json!([{
                        "role":"user",
                        "content":[{"type":"image_url","image_url":{"url":format!("data:image/png;base64,{PRIVATE_MARKER}")}}]
                    }]);
                }
                DownstreamProtocol::Responses => {
                    request["input"] = json!([{
                        "type":"message",
                        "role":"user",
                        "content":[{"type":"input_file","filename":"remote.pdf","file_url":format!("https://files.example.com/{PRIVATE_MARKER}.pdf")}]
                    }]);
                }
                DownstreamProtocol::Anthropic => {
                    request["messages"] = json!([{
                        "role":"user",
                        "content":[{"type":"document","source":{"type":"file","file_id":PRIVATE_MARKER}}]
                    }]);
                }
                DownstreamProtocol::Gemini => {
                    request["contents"] = json!([{
                        "role":"user",
                        "parts":[{"fileData":{"mimeType":"application/pdf","fileUri":format!("https://generativelanguage.googleapis.com/v1beta/files/{PRIVATE_MARKER}")}}]
                    }]);
                }
            }
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{name}");
            let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("multimodal rejection should be readable");
            assert!(
                !String::from_utf8_lossy(&response_body).contains(PRIVATE_MARKER),
                "{name}: payload must not be disclosed"
            );
            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                0,
                "{name}: reject before credentials"
            );
            assert!(
                upstream.requests().await.is_empty(),
                "{name}: reject before network"
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn responses_target_rejects_all_unportable_media_before_credentials() {
    const PRIVATE_MARKER: &str = "responses-media-private-marker";

    for (wire_name, fixture) in responses_target_fixtures() {
        let cases: Vec<(&str, Value)> = match fixture.protocol {
            DownstreamProtocol::Openai => vec![
                (
                    "invalid-base64",
                    json!({"model":"gpt-5","messages":[{"role":"user","content":[
                        {"type":"image_url","image_url":{"url":format!("data:image/png;base64,{PRIVATE_MARKER}")}}
                    ]}]}),
                ),
                (
                    "empty-filename",
                    json!({"model":"gpt-5","messages":[{"role":"user","content":[
                        {"type":"file","file":{"filename":"","file_data":"JVBERi0="}}
                    ]}]}),
                ),
                (
                    "executable-mime",
                    json!({"model":"gpt-5","messages":[{"role":"user","content":[
                        {"type":"file","file":{"filename":"payload.exe","file_data":"AA=="}}
                    ]}]}),
                ),
                (
                    "illegal-role",
                    json!({"model":"gpt-5","messages":[{"role":"assistant","content":[
                        {"type":"image_url","image_url":{"url":"https://example.com/a.png"}}
                    ]}]}),
                ),
            ],
            DownstreamProtocol::Responses => vec![
                (
                    "conflicting-source",
                    json!({"model":"gpt-5","input":[{"role":"user","content":[
                        {"type":"input_image","image_url":"https://example.com/a.png","file_id":"file_same_target"}
                    ]}]}),
                ),
                (
                    "external-file-url",
                    json!({"model":"gpt-5","input":[{"role":"user","content":[
                        {"type":"input_file","filename":"remote.pdf","file_url":format!("https://example.com/{PRIVATE_MARKER}.pdf")}
                    ]}]}),
                ),
                (
                    "empty-filename",
                    json!({"model":"gpt-5","input":[{"role":"user","content":[
                        {"type":"input_file","filename":"","file_data":"JVBERi0="}
                    ]}]}),
                ),
                (
                    "unknown-mime",
                    json!({"model":"gpt-5","input":[{"role":"user","content":[
                        {"type":"input_file","filename":"payload.bin","file_data":"data:application/octet-stream;base64,AA=="}
                    ]}]}),
                ),
                (
                    "illegal-role",
                    json!({"model":"gpt-5","input":[{"role":"assistant","content":[
                        {"type":"input_audio","input_audio":{"data":"AA==","format":"wav"}}
                    ]}]}),
                ),
            ],
            DownstreamProtocol::Anthropic => vec![
                (
                    "external-file-id",
                    json!({"model":"claude","max_tokens":64,"messages":[{"role":"user","content":[
                        {"type":"document","source":{"type":"file","file_id":PRIVATE_MARKER},"title":"remote.pdf"}
                    ]}]}),
                ),
                (
                    "missing-filename",
                    json!({"model":"claude","max_tokens":64,"messages":[{"role":"user","content":[
                        {"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"JVBERi0="}}
                    ]}]}),
                ),
                (
                    "invalid-base64",
                    json!({"model":"claude","max_tokens":64,"messages":[{"role":"user","content":[
                        {"type":"image","source":{"type":"base64","media_type":"image/png","data":PRIVATE_MARKER}}
                    ]}]}),
                ),
                (
                    "illegal-role",
                    json!({"model":"claude","max_tokens":64,"messages":[{"role":"assistant","content":[
                        {"type":"image","source":{"type":"base64","media_type":"image/png","data":"AA=="}}
                    ]}]}),
                ),
            ],
            DownstreamProtocol::Gemini => vec![
                (
                    "hosted-uri",
                    json!({"contents":[{"role":"user","parts":[
                        {"fileData":{"mimeType":"application/pdf","fileUri":format!("https://generativelanguage.googleapis.com/v1beta/files/{PRIVATE_MARKER}")}}
                    ]}]}),
                ),
                (
                    "video",
                    json!({"contents":[{"role":"user","parts":[
                        {"inlineData":{"mimeType":"video/mp4","data":"AA==","displayName":"clip.mp4"}}
                    ]}]}),
                ),
                (
                    "unknown-mime",
                    json!({"contents":[{"role":"user","parts":[
                        {"inlineData":{"mimeType":"application/octet-stream","data":"AA==","displayName":"blob.bin"}}
                    ]}]}),
                ),
                (
                    "missing-filename",
                    json!({"contents":[{"role":"user","parts":[
                        {"inlineData":{"mimeType":"application/pdf","data":"JVBERi0="}}
                    ]}]}),
                ),
                (
                    "illegal-role",
                    json!({"contents":[{"role":"model","parts":[
                        {"inlineData":{"mimeType":"image/png","data":"AA=="}}
                    ]}]}),
                ),
                (
                    "executable",
                    json!({"contents":[{"role":"user","parts":[
                        {"executableCode":{"language":"python","code":PRIVATE_MARKER}}
                    ]}]}),
                ),
            ],
        };

        for (case_name, request) in cases {
            let fixture = fixture.clone();
            let name = format!("{wire_name}-{case_name}");
            run_case(&name, move |context| async move {
                let upstream =
                    TestUpstream::spawn_json(StatusCode::OK, json!({"should_not":"be called"}))
                        .await;
                let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
                let mut request = request;
                if fixture.protocol != DownstreamProtocol::Gemini {
                    request["model"] = json!(router.requested_model());
                }
                router
                    .app_state
                    .secret_encryption
                    .reset_decrypt_call_count();

                let response = router.send(&fixture, false, &request).await;
                assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case_name}");
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("media rejection should be readable");
                assert!(
                    !String::from_utf8_lossy(&body).contains(PRIVATE_MARKER),
                    "{case_name}"
                );
                assert_eq!(
                    router.app_state.secret_encryption.decrypt_call_count(),
                    0,
                    "{case_name}"
                );
                assert!(upstream.requests().await.is_empty(), "{case_name}");
                let log = router
                    .wait_for_log_for_source(router.source_id, RequestStatus::Error)
                    .await;
                assert!(
                    !log.final_error_message
                        .as_deref()
                        .unwrap_or_default()
                        .contains(PRIVATE_MARKER),
                    "{case_name}: private media must not enter Request Log"
                );
                router.wait_for_api_key_lease_release().await;
                assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
                upstream.shutdown().await;
            });
        }
    }
}

#[test]
fn all_public_downstream_structured_outputs_reach_the_openai_target() {
    let openai_response = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture.non_stream.upstream_response)
        .expect("openai response fixture");

    for (name, fixture) in fixtures() {
        let upstream_response = openai_response.clone();
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_response).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            if fixture.profile_type != UpstreamProfileType::Openai {
                router
                    .replace_default_source_profile(&upstream.base_url, UpstreamProfileType::Openai)
                    .await;
            }

            let schema = json!({
                "type":"object",
                "properties":{"answer":{"type":"string","minLength":2}},
                "required":["answer"],
                "additionalProperties":false
            });
            let mut request = fixture.request.downstream.clone();
            match fixture.protocol {
                DownstreamProtocol::Openai => {
                    request["response_format"] = json!({
                        "type":"json_schema",
                        "json_schema":{
                            "name":"answer_contract",
                            "description":"An answer",
                            "schema":schema,
                            "strict":true
                        }
                    });
                }
                DownstreamProtocol::Responses => {
                    request["text"] = json!({"format":{
                        "type":"json_schema",
                        "name":"answer_contract",
                        "description":"An answer",
                        "schema":schema,
                        "strict":true
                    }});
                }
                DownstreamProtocol::Anthropic => {
                    request["output_config"] = json!({"format":{
                        "type":"json_schema",
                        "schema":schema
                    }});
                }
                DownstreamProtocol::Gemini => {
                    request["generationConfig"] = json!({
                        "responseMimeType":"application/json",
                        "responseJsonSchema":schema
                    });
                }
            }

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::OK, "{name}");
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("structured response should complete");
            let captured = upstream.requests().await;
            assert_eq!(captured.len(), 1, "{name}: exactly one generation call");
            let body: Value = serde_json::from_slice(&captured[0].body)
                .expect("OpenAI target request should be JSON");
            let definition = &body["response_format"]["json_schema"];
            assert_eq!(
                body["response_format"]["type"],
                json!("json_schema"),
                "{name}"
            );
            assert!(
                definition["name"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty()),
                "{name}: explicit or stable synthesized name"
            );
            assert_eq!(
                definition["schema"]["properties"]["answer"]["minLength"],
                json!(2),
                "{name}"
            );
            assert_eq!(
                definition["schema"]["additionalProperties"],
                json!(false),
                "{name}"
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn all_public_downstream_structured_outputs_reach_the_responses_target() {
    let upstream_response = responses_target_golden().non_stream_response;

    for (name, fixture) in responses_target_fixtures() {
        let upstream_response = upstream_response.clone();
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_response).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let schema = json!({
                "type":"object",
                "properties":{"answer":{"type":"string","minLength":2}},
                "required":["answer"],
                "additionalProperties":false
            });
            let mut request = fixture.request.downstream.clone();
            match fixture.protocol {
                DownstreamProtocol::Openai => {
                    request["response_format"] = json!({
                        "type":"json_schema","json_schema":{
                            "name":"answer_contract","description":"An answer",
                            "schema":schema,"strict":true
                        }
                    });
                }
                DownstreamProtocol::Responses => {
                    request["text"] = json!({"format":{
                        "type":"json_schema","name":"answer_contract",
                        "description":"An answer","schema":schema,"strict":true
                    }});
                }
                DownstreamProtocol::Anthropic => {
                    request["output_config"] = json!({"format":{
                        "type":"json_schema","schema":schema
                    }});
                }
                DownstreamProtocol::Gemini => {
                    request["generationConfig"] = json!({"responseFormat":{"text":{
                        "mimeType":"application/json","schema":{
                            "type":"object","propertyOrdering":["answer"],
                            "properties":{"answer":{
                                "type":"string","minLength":2,"propertyOrdering":[]
                            }},
                            "required":["answer"],"additionalProperties":false
                        }
                    }}});
                }
            }

            let response = router.send(&fixture, false, &request).await;
            assert_eq!(response.status(), StatusCode::OK, "{name}");
            assert_no_public_transform_diagnostics(&response);
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("structured response should complete without gateway schema validation");

            let captured = upstream.requests().await;
            assert_eq!(captured.len(), 1, "{name}: exactly one Responses call");
            let body: Value = serde_json::from_slice(&captured[0].body)
                .expect("Responses target request should be JSON");
            assert_eq!(body["store"], false, "{name}");
            let format = &body["text"]["format"];
            assert_eq!(format["type"], "json_schema", "{name}");
            assert_eq!(format["strict"], true, "{name}");
            assert_eq!(
                format["schema"]["properties"]["answer"]["minLength"], 2,
                "{name}"
            );
            assert!(
                format["name"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty()),
                "{name}: explicit or stable synthesized name"
            );
            if matches!(
                fixture.protocol,
                DownstreamProtocol::Openai | DownstreamProtocol::Responses
            ) {
                assert_eq!(format["name"], "answer_contract", "{name}");
                assert_eq!(format["description"], "An answer", "{name}");
            }
            if fixture.protocol == DownstreamProtocol::Gemini {
                assert!(!format["schema"].to_string().contains("propertyOrdering"));
            }

            router.wait_for_log(RequestStatus::Success).await;
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[derive(Clone, Copy, Debug)]
enum ResponsesAdvancedCellCapability {
    Tools,
    Reasoning,
    Multimodal,
    StructuredOutput,
}

fn advanced_responses_cell_request(
    fixture: &DirectExecutionFixture,
    capability: ResponsesAdvancedCellCapability,
) -> Value {
    let mut request = fixture.request.downstream.clone();
    match (fixture.protocol, capability) {
        (DownstreamProtocol::Openai, ResponsesAdvancedCellCapability::Tools) => {
            request["messages"] = json!([
                {"role":"assistant","content":null,"tool_calls":[{
                    "id":"call_lookup","type":"function",
                    "function":{"name":"lookup","arguments":"{\"city\":\"Paris\"}"}
                }]},
                {"role":"tool","tool_call_id":"call_lookup","content":"{\"ok\":true}"}
            ]);
            request["tools"] = json!([{"type":"function","function":{
                "name":"lookup","parameters":{"type":"object"},"strict":true
            }}]);
            request["tool_choice"] = json!({"type":"function","function":{"name":"lookup"}});
            request["parallel_tool_calls"] = json!(false);
        }
        (DownstreamProtocol::Responses, ResponsesAdvancedCellCapability::Tools) => {
            request["input"] = json!([
                {"type":"function_call","call_id":"call_lookup","name":"lookup","arguments":"{\"city\":\"Paris\"}"},
                {"type":"function_call_output","call_id":"call_lookup","output":{"ok":true}}
            ]);
            request["tools"] = json!([{
                "type":"function","name":"lookup","parameters":{"type":"object"},"strict":true
            }]);
            request["tool_choice"] = json!({"type":"function","name":"lookup"});
            request["parallel_tool_calls"] = json!(false);
        }
        (DownstreamProtocol::Anthropic, ResponsesAdvancedCellCapability::Tools) => {
            request["messages"] = json!([
                {"role":"assistant","content":[{"type":"tool_use","id":"call_lookup","name":"lookup","input":{"city":"Paris"}}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"call_lookup","content":{"ok":true}}]}
            ]);
            request["tools"] = json!([{
                "name":"lookup","input_schema":{"type":"object"}
            }]);
            request["tool_choice"] =
                json!({"type":"tool","name":"lookup","disable_parallel_tool_use":true});
        }
        (DownstreamProtocol::Gemini, ResponsesAdvancedCellCapability::Tools) => {
            request["contents"] = json!([
                {"role":"model","parts":[{"functionCall":{"name":"lookup","args":{"city":"Paris"}}}]},
                {"role":"user","parts":[{"functionResponse":{"name":"lookup","response":{"ok":true}}}]}
            ]);
            request["tools"] = json!([{"functionDeclarations":[{
                "name":"lookup","parameters":{"type":"object"}
            }]}]);
            request["toolConfig"] = json!({"functionCallingConfig":{
                "mode":"ANY","allowedFunctionNames":["lookup"]
            }});
        }
        (DownstreamProtocol::Openai, ResponsesAdvancedCellCapability::Reasoning) => {
            request["reasoning_effort"] = json!("medium");
        }
        (DownstreamProtocol::Responses, ResponsesAdvancedCellCapability::Reasoning) => {
            request["reasoning"] = json!({"effort":"xhigh","summary":"detailed"});
        }
        (DownstreamProtocol::Anthropic, ResponsesAdvancedCellCapability::Reasoning) => {
            request["thinking"] = json!({"type":"adaptive"});
            request["output_config"] = json!({"effort":"max"});
        }
        (DownstreamProtocol::Gemini, ResponsesAdvancedCellCapability::Reasoning) => {
            request["generationConfig"]["thinkingConfig"] =
                json!({"thinkingLevel":"low","includeThoughts":true});
        }
        (DownstreamProtocol::Openai, ResponsesAdvancedCellCapability::Multimodal) => {
            request["messages"] = json!([{"role":"user","content":[
                {"type":"image_url","image_url":{"url":"https://example.com/image.png","detail":"high"}},
                {"type":"file","file":{"filename":"report.pdf","file_data":"JVBERi0="}}
            ]}]);
        }
        (DownstreamProtocol::Responses, ResponsesAdvancedCellCapability::Multimodal) => {
            request["input"] = json!([{"role":"user","content":[
                {"type":"input_image","image_url":"data:image/png;base64,ZmFrZQ==","detail":"high"},
                {"type":"input_file","filename":"report.pdf","file_data":"JVBERi0="}
            ]}]);
        }
        (DownstreamProtocol::Anthropic, ResponsesAdvancedCellCapability::Multimodal) => {
            request["messages"] = json!([{"role":"user","content":[
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"ZmFrZQ=="}},
                {"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"JVBERi0="},"title":"report.pdf"}
            ]}]);
        }
        (DownstreamProtocol::Gemini, ResponsesAdvancedCellCapability::Multimodal) => {
            request["contents"] = json!([{"role":"user","parts":[
                {"inlineData":{"mimeType":"image/png","data":"ZmFrZQ==","displayName":"preview.png"}},
                {"inlineData":{"mimeType":"application/pdf","data":"JVBERi0=","displayName":"report.pdf"}}
            ]}]);
        }
        (DownstreamProtocol::Openai, ResponsesAdvancedCellCapability::StructuredOutput) => {
            request["response_format"] = json!({"type":"json_schema","json_schema":{
                "name":"answer_contract","description":"An answer","strict":true,
                "schema":{"type":"object","properties":{"answer":{"type":"string","minLength":2}}}
            }});
        }
        (DownstreamProtocol::Responses, ResponsesAdvancedCellCapability::StructuredOutput) => {
            request["text"] = json!({"format":{
                "type":"json_schema","name":"answer_contract","description":"An answer","strict":true,
                "schema":{"type":"object","properties":{"answer":{"type":"string","minLength":2}}}
            }});
        }
        (DownstreamProtocol::Anthropic, ResponsesAdvancedCellCapability::StructuredOutput) => {
            request["output_config"] = json!({"format":{
                "type":"json_schema","schema":{"type":"object","properties":{"answer":{"type":"string","minLength":2}}}
            }});
        }
        (DownstreamProtocol::Gemini, ResponsesAdvancedCellCapability::StructuredOutput) => {
            request["generationConfig"] = json!({"responseFormat":{"text":{
                "mimeType":"application/json","schema":{"type":"object","propertyOrdering":["answer"],
                    "properties":{"answer":{"type":"string","minLength":2}}}
            }}});
        }
    }
    request
}

fn assert_responses_advanced_cell(
    test_name: &'static str,
    protocol: DownstreamProtocol,
    capability: ResponsesAdvancedCellCapability,
    expects_controlled_loss: bool,
) {
    let (_, fixture) = responses_target_fixtures()
        .into_iter()
        .find(|(_, fixture)| fixture.protocol == protocol)
        .expect("Responses target fixture for protocol");

    run_case(test_name, move |context| async move {
        let upstream = TestUpstream::spawn_json(
            StatusCode::OK,
            responses_target_golden().non_stream_response,
        )
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let request = advanced_responses_cell_request(&fixture, capability);

        let transformed = crate::service::transform::transform_request_data(
            request.clone(),
            protocol,
            UpstreamProtocol::Responses,
            false,
        )
        .expect("advanced cell transform must be sendable");
        let controlled_facts = transformed
            .summary
            .facts
            .iter()
            .filter(|fact| {
                matches!(
                    fact.outcome,
                    TransformOutcomeKind::ControlledLossMinor
                        | TransformOutcomeKind::ControlledLossMajor
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            !controlled_facts.is_empty(),
            expects_controlled_loss,
            "{protocol:?}/{capability:?}"
        );
        assert!(
            controlled_facts
                .iter()
                .all(|fact| fact.safe_summary.is_none()),
            "{protocol:?}/{capability:?}: diagnostics must be payload-free"
        );

        let response = router.send(&fixture, false, &request).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{protocol:?}/{capability:?}"
        );
        assert_no_public_transform_diagnostics(&response);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("advanced response should complete");
        let captured = upstream.requests().await;
        assert_eq!(captured.len(), 1, "one direct Responses call");
        assert_eq!(captured[0].path, "/v1/responses");
        let body: Value = serde_json::from_slice(&captured[0].body).expect("Responses JSON");
        assert_eq!(body["store"], false);
        match capability {
            ResponsesAdvancedCellCapability::Tools => {
                assert_eq!(body["tools"][0]["type"], "function");
                assert!(
                    body["input"]
                        .as_array()
                        .is_some_and(|items| items.iter().any(|item| {
                            item["type"] == "function_call"
                                || item["type"] == "function_call_output"
                        }))
                );
            }
            ResponsesAdvancedCellCapability::Reasoning => {
                assert!(body["reasoning"]["effort"].is_string());
            }
            ResponsesAdvancedCellCapability::Multimodal => {
                let serialized = body["input"].to_string();
                assert!(serialized.contains("input_image"));
                assert!(serialized.contains("input_file"));
            }
            ResponsesAdvancedCellCapability::StructuredOutput => {
                assert_eq!(body["text"]["format"]["type"], "json_schema");
                assert!(body["text"]["format"]["name"].is_string());
            }
        }
        router.wait_for_log(RequestStatus::Success).await;
        router.wait_for_api_key_lease_release().await;
        upstream.shutdown().await;
    });
}

macro_rules! responses_advanced_cell_test {
    ($name:ident, $protocol:expr, $capability:expr, $loss:expr) => {
        #[test]
        fn $name() {
            assert_responses_advanced_cell(stringify!($name), $protocol, $capability, $loss);
        }
    };
}

responses_advanced_cell_test!(
    openai_to_responses_tools_cell_is_full,
    DownstreamProtocol::Openai,
    ResponsesAdvancedCellCapability::Tools,
    false
);
responses_advanced_cell_test!(
    responses_to_responses_tools_cell_is_full,
    DownstreamProtocol::Responses,
    ResponsesAdvancedCellCapability::Tools,
    false
);
responses_advanced_cell_test!(
    anthropic_to_responses_tools_cell_is_full,
    DownstreamProtocol::Anthropic,
    ResponsesAdvancedCellCapability::Tools,
    false
);
responses_advanced_cell_test!(
    gemini_to_responses_tools_cell_has_typed_controlled_loss,
    DownstreamProtocol::Gemini,
    ResponsesAdvancedCellCapability::Tools,
    true
);
responses_advanced_cell_test!(
    openai_to_responses_reasoning_cell_has_typed_controlled_loss,
    DownstreamProtocol::Openai,
    ResponsesAdvancedCellCapability::Reasoning,
    true
);
responses_advanced_cell_test!(
    responses_to_responses_reasoning_cell_is_full,
    DownstreamProtocol::Responses,
    ResponsesAdvancedCellCapability::Reasoning,
    false
);
responses_advanced_cell_test!(
    anthropic_to_responses_reasoning_cell_has_typed_controlled_loss,
    DownstreamProtocol::Anthropic,
    ResponsesAdvancedCellCapability::Reasoning,
    true
);
responses_advanced_cell_test!(
    gemini_to_responses_reasoning_cell_has_typed_controlled_loss,
    DownstreamProtocol::Gemini,
    ResponsesAdvancedCellCapability::Reasoning,
    true
);
responses_advanced_cell_test!(
    openai_to_responses_multimodal_cell_is_full,
    DownstreamProtocol::Openai,
    ResponsesAdvancedCellCapability::Multimodal,
    false
);
responses_advanced_cell_test!(
    responses_to_responses_multimodal_cell_is_full,
    DownstreamProtocol::Responses,
    ResponsesAdvancedCellCapability::Multimodal,
    false
);
responses_advanced_cell_test!(
    anthropic_to_responses_multimodal_cell_is_full,
    DownstreamProtocol::Anthropic,
    ResponsesAdvancedCellCapability::Multimodal,
    false
);
responses_advanced_cell_test!(
    gemini_to_responses_multimodal_cell_has_typed_controlled_loss,
    DownstreamProtocol::Gemini,
    ResponsesAdvancedCellCapability::Multimodal,
    true
);
responses_advanced_cell_test!(
    openai_to_responses_structured_output_cell_is_full,
    DownstreamProtocol::Openai,
    ResponsesAdvancedCellCapability::StructuredOutput,
    false
);
responses_advanced_cell_test!(
    responses_to_responses_structured_output_cell_is_full,
    DownstreamProtocol::Responses,
    ResponsesAdvancedCellCapability::StructuredOutput,
    false
);
responses_advanced_cell_test!(
    anthropic_to_responses_structured_output_cell_has_typed_controlled_loss,
    DownstreamProtocol::Anthropic,
    ResponsesAdvancedCellCapability::StructuredOutput,
    true
);
responses_advanced_cell_test!(
    gemini_to_responses_structured_output_cell_has_typed_controlled_loss,
    DownstreamProtocol::Gemini,
    ResponsesAdvancedCellCapability::StructuredOutput,
    true
);

#[test]
fn all_public_downstream_portable_tool_lifecycles_reach_the_openai_target() {
    let openai_response = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture.non_stream.upstream_response)
        .expect("openai response fixture");

    for (name, fixture) in fixtures() {
        let upstream_response = openai_response.clone();
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_response).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            if fixture.profile_type != UpstreamProfileType::Openai {
                router
                    .replace_default_source_profile(&upstream.base_url, UpstreamProfileType::Openai)
                    .await;
            }

            let mut request = fixture.request.downstream.clone();
            match fixture.protocol {
                DownstreamProtocol::Openai => {
                    request["messages"] = json!([
                        {"role":"assistant","content":null,"tool_calls":[{
                            "id":"call_lookup","type":"function",
                            "function":{"name":"lookup","arguments":"{\"city\":\"Paris\"}"}
                        }]},
                        {"role":"tool","tool_call_id":"call_lookup","content":"{\"ok\":true}"}
                    ]);
                    request["tools"] = json!([{"type":"function","function":{
                        "name":"lookup","parameters":{"type":"object"},"strict":true
                    }}]);
                    request["tool_choice"] =
                        json!({"type":"function","function":{"name":"lookup"}});
                    request["parallel_tool_calls"] = json!(false);
                }
                DownstreamProtocol::Responses => {
                    request["input"] = json!([
                        {"type":"function_call","call_id":"call_lookup","name":"lookup","arguments":"{\"city\":\"Paris\"}"},
                        {"type":"function_call_output","call_id":"call_lookup","output":{"ok":true}}
                    ]);
                    request["tools"] = json!([{
                        "type":"function","name":"lookup","parameters":{"type":"object"},"strict":true
                    }]);
                    request["tool_choice"] = json!({"type":"function","name":"lookup"});
                    request["parallel_tool_calls"] = json!(false);
                }
                DownstreamProtocol::Anthropic => {
                    request["messages"] = json!([
                        {"role":"assistant","content":[{"type":"tool_use","id":"call_lookup","name":"lookup","input":{"city":"Paris"}}]},
                        {"role":"user","content":[{"type":"tool_result","tool_use_id":"call_lookup","content":{"ok":true}}]}
                    ]);
                    request["tools"] = json!([{
                        "name":"lookup","input_schema":{"type":"object"},"strict":true
                    }]);
                    request["tool_choice"] = json!({
                        "type":"tool","name":"lookup","disable_parallel_tool_use":true
                    });
                }
                DownstreamProtocol::Gemini => {
                    request["contents"] = json!([
                        {"role":"model","parts":[{"functionCall":{"name":"lookup","args":{"city":"Paris"}}}]},
                        {"role":"user","parts":[{"functionResponse":{"name":"lookup","response":{"ok":true}}}]}
                    ]);
                    request["tools"] = json!([{"functionDeclarations":[{
                        "name":"lookup","parameters":{"type":"object"}
                    }]}]);
                    request["toolConfig"] = json!({"functionCallingConfig":{
                        "mode":"ANY","allowedFunctionNames":["lookup"]
                    }});
                }
            }

            let response = router.send(&fixture, false, &request).await;
            assert_eq!(response.status(), StatusCode::OK, "{name}");
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("tool lifecycle response should complete");
            let captured = upstream.requests().await;
            assert_eq!(captured.len(), 1, "{name}: exactly one generation call");
            let body: Value = serde_json::from_slice(&captured[0].body)
                .expect("OpenAI target request should be JSON");
            assert_eq!(body["tools"][0]["function"]["strict"], true, "{name}");
            let assistant = body["messages"]
                .as_array()
                .and_then(|messages| {
                    messages
                        .iter()
                        .find(|message| message["role"] == "assistant")
                })
                .expect("assistant tool call message");
            let tool = body["messages"]
                .as_array()
                .and_then(|messages| messages.iter().find(|message| message["role"] == "tool"))
                .expect("tool result message");
            assert_eq!(
                assistant["tool_calls"][0]["id"], tool["tool_call_id"],
                "{name}: call/result correlation"
            );
            assert_eq!(
                assistant["tool_calls"][0]["function"]["name"], "lookup",
                "{name}"
            );
            if fixture.protocol != DownstreamProtocol::Gemini {
                assert_eq!(body["parallel_tool_calls"], false, "{name}");
            }
            upstream.shutdown().await;
        });
    }
}

#[test]
fn all_public_downstream_portable_tool_lifecycles_reach_the_responses_target() {
    let mut upstream_response = responses_target_golden().non_stream_response;
    upstream_response["output"] = json!([{
        "type":"function_call",
        "id":"fc_model_lookup",
        "call_id":"model-call-lookup",
        "name":"lookup",
        "arguments":"{\"city\":\"London\"}",
        "status":"completed"
    }]);

    for (name, fixture) in responses_target_fixtures() {
        let upstream_response = upstream_response.clone();
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn_json(StatusCode::OK, upstream_response).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let mut request = fixture.request.downstream.clone();
            match fixture.protocol {
                DownstreamProtocol::Openai => {
                    request["messages"] = json!([
                        {"role":"assistant","content":null,"tool_calls":[
                            {"id":"openai-weather","type":"function","function":{"name":"weather","arguments":"{\"city\":\"Paris\"}"}},
                            {"id":"openai-time","type":"function","function":{"name":"time","arguments":"{\"zone\":\"UTC\"}"}}
                        ]},
                        {"role":"tool","tool_call_id":"openai-weather","content":"{\"temp\":21}"},
                        {"role":"tool","tool_call_id":"openai-time","content":"12:00"}
                    ]);
                    request["tools"] = json!([
                        {"type":"function","function":{"name":"weather","description":"lookup weather","parameters":{"type":"object"},"strict":true}},
                        {"type":"function","function":{"name":"time","parameters":{"type":"object"},"strict":false}}
                    ]);
                    request["tool_choice"] = json!({"type":"allowed_tools","allowed_tools":{
                        "mode":"required","tools":[
                            {"type":"function","function":{"name":"weather"}},
                            {"type":"function","function":{"name":"time"}}
                        ]
                    }});
                    request["parallel_tool_calls"] = json!(false);
                }
                DownstreamProtocol::Responses => {
                    request["input"] = json!([
                        {"type":"function_call","id":"fc-weather","call_id":"responses-weather","name":"weather","arguments":"{\"city\":\"Paris\"}"},
                        {"type":"function_call","id":"fc-time","call_id":"responses-time","name":"time","arguments":"{\"zone\":\"UTC\"}"},
                        {"type":"function_call_output","id":"fco-weather","call_id":"responses-weather","output":{"temp":21}},
                        {"type":"function_call_output","id":"fco-time","call_id":"responses-time","output":"12:00"}
                    ]);
                    request["tools"] = json!([
                        {"type":"function","name":"weather","description":"lookup weather","parameters":{"type":"object"},"strict":true},
                        {"type":"function","name":"time","parameters":{"type":"object"},"strict":false}
                    ]);
                    request["tool_choice"] = json!({"type":"allowed_tools","mode":"required","tools":[
                        {"type":"function","name":"weather"},
                        {"type":"function","name":"time"}
                    ]});
                    request["parallel_tool_calls"] = json!(false);
                }
                DownstreamProtocol::Anthropic => {
                    request["messages"] = json!([
                        {"role":"assistant","content":[
                            {"type":"tool_use","id":"anthropic-weather","name":"weather","input":{"city":"Paris"}},
                            {"type":"tool_use","id":"anthropic-time","name":"time","input":{"zone":"UTC"}}
                        ]},
                        {"role":"user","content":[
                            {"type":"tool_result","tool_use_id":"anthropic-weather","content":"{\"temp\":21}"},
                            {"type":"tool_result","tool_use_id":"anthropic-time","content":"12:00"}
                        ]}
                    ]);
                    request["tools"] = json!([
                        {"name":"weather","description":"lookup weather","input_schema":{"type":"object"},"strict":true},
                        {"name":"time","input_schema":{"type":"object"},"strict":false}
                    ]);
                    request["tool_choice"] = json!({
                        "type":"tool","name":"weather","disable_parallel_tool_use":true
                    });
                }
                DownstreamProtocol::Gemini => {
                    request["contents"] = json!([
                        {"role":"model","parts":[
                            {"functionCall":{"name":"weather","args":{"city":"Paris"}}},
                            {"functionCall":{"name":"time","args":{"zone":"UTC"}}}
                        ]},
                        {"role":"user","parts":[
                            {"functionResponse":{"name":"weather","response":{"temp":21}}},
                            {"functionResponse":{"name":"time","response":{"result":"12:00"}}}
                        ]}
                    ]);
                    request["tools"] = json!([{"functionDeclarations":[
                        {"name":"weather","description":"lookup weather","parameters":{"type":"object"}},
                        {"name":"time","parameters":{"type":"object"}}
                    ]}]);
                    request["toolConfig"] = json!({"functionCallingConfig":{
                        "mode":"ANY","allowedFunctionNames":["weather","time"]
                    }});
                }
            }

            let response = router.send(&fixture, false, &request).await;
            assert_eq!(response.status(), StatusCode::OK, "{name}");
            let response_body: Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("tool response should complete"),
            )
            .expect("downstream tool response should be JSON");

            let captured = upstream.requests().await;
            assert_eq!(captured.len(), 1, "{name}: exactly one Responses call");
            assert_eq!(captured[0].path, "/v1/responses", "{name}");
            let body: Value = serde_json::from_slice(&captured[0].body)
                .expect("Responses target request should be JSON");
            assert_eq!(body["store"], false, "{name}");
            assert_eq!(body["tools"][0]["name"], "weather", "{name}");
            assert_eq!(body["tools"][0]["description"], "lookup weather", "{name}");
            assert_eq!(
                body["tools"][0]["parameters"],
                json!({"type":"object"}),
                "{name}"
            );
            assert_eq!(body["tools"][0]["strict"], true, "{name}");
            if fixture.protocol != DownstreamProtocol::Gemini {
                assert_eq!(body["parallel_tool_calls"], false, "{name}");
            }
            assert_eq!(
                body["input"][0]["call_id"], body["input"][2]["call_id"],
                "{name}"
            );
            assert_eq!(
                body["input"][1]["call_id"], body["input"][3]["call_id"],
                "{name}"
            );
            assert_ne!(
                body["input"][0]["call_id"], body["input"][1]["call_id"],
                "{name}"
            );

            match fixture.protocol {
                DownstreamProtocol::Openai => {
                    assert_eq!(response_body["choices"][0]["finish_reason"], "tool_calls");
                    assert_eq!(
                        response_body["choices"][0]["message"]["tool_calls"][0]["id"],
                        "model-call-lookup"
                    );
                    assert_eq!(
                        response_body["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
                        "lookup"
                    );
                }
                DownstreamProtocol::Responses => {
                    assert_eq!(response_body["status"], "completed");
                    assert_eq!(response_body["output"][0]["call_id"], "model-call-lookup");
                }
                DownstreamProtocol::Anthropic => {
                    assert_eq!(response_body["stop_reason"], "tool_use");
                    assert_eq!(response_body["content"][0]["id"], "model-call-lookup");
                    assert_eq!(response_body["content"][0]["name"], "lookup");
                }
                DownstreamProtocol::Gemini => {
                    assert_eq!(
                        response_body["candidates"][0]["content"]["parts"][0]["functionCall"]["name"],
                        "lookup"
                    );
                    assert_eq!(
                        response_body["candidates"][0]["content"]["parts"][0]["functionCall"]["args"],
                        json!({"city":"London"})
                    );
                }
            }
            upstream.shutdown().await;
        });
    }
}

#[test]
fn responses_target_rejects_invalid_or_forced_nonportable_tools_before_credentials() {
    const PRIVATE_MARKER: &str = "responses-tool-private-marker";
    for (name, fixture) in responses_target_fixtures() {
        run_case(name, move |context| async move {
            let upstream =
                TestUpstream::spawn_json(StatusCode::OK, json!({"should_not":"be called"})).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let mut request = fixture.request.downstream.clone();
            match fixture.protocol {
                DownstreamProtocol::Openai => {
                    request["tools"] = json!([{"type":"function","function":{
                        "name":"lookup","parameters":[PRIVATE_MARKER]
                    }}])
                }
                DownstreamProtocol::Responses => {
                    request["tools"] = json!([{
                        "type":"function","name":"lookup","parameters":[PRIVATE_MARKER]
                    }])
                }
                DownstreamProtocol::Anthropic => {
                    request["tools"] = json!([{
                        "name":"lookup","input_schema":[PRIVATE_MARKER]
                    }])
                }
                DownstreamProtocol::Gemini => {
                    request["tools"] = json!([{"functionDeclarations":[{
                        "name":"lookup","parameters":[PRIVATE_MARKER]
                    }]}])
                }
            }
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router.send(&fixture, false, &request).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("invalid function rejection should be readable");
            assert!(
                !String::from_utf8_lossy(&body).contains(PRIVATE_MARKER),
                "{name}"
            );
            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                0,
                "{name}"
            );
            assert!(upstream.requests().await.is_empty(), "{name}");
            let log = router
                .wait_for_log_for_source(router.source_id, RequestStatus::Error)
                .await;
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains(PRIVATE_MARKER),
                "{name}: private tool payload must not enter Request Log"
            );
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }

    for (name, fixture) in responses_target_fixtures()
        .into_iter()
        .filter(|(_, fixture)| fixture.protocol != DownstreamProtocol::Responses)
    {
        run_case(name, move |context| async move {
            let upstream =
                TestUpstream::spawn_json(StatusCode::OK, json!({"should_not":"be called"})).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let mut request = fixture.request.downstream.clone();
            match fixture.protocol {
                DownstreamProtocol::Openai => {
                    request["tools"] = json!([{"type":"custom","marker":PRIVATE_MARKER}]);
                    request["tool_choice"] = json!("required");
                }
                DownstreamProtocol::Anthropic => {
                    request["tools"] =
                        json!([{"type":"web_search_20250305","marker":PRIVATE_MARKER}]);
                    request["tool_choice"] = json!({"type":"any"});
                }
                DownstreamProtocol::Gemini => {
                    request["tools"] = json!([{"googleSearch":{"marker":PRIVATE_MARKER}}]);
                    request["toolConfig"] = json!({"functionCallingConfig":{"mode":"ANY"}});
                }
                DownstreamProtocol::Responses => unreachable!(),
            }
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router.send(&fixture, false, &request).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("forced nonportable rejection should be readable");
            assert!(
                !String::from_utf8_lossy(&body).contains(PRIVATE_MARKER),
                "{name}"
            );
            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                0,
                "{name}"
            );
            assert!(upstream.requests().await.is_empty(), "{name}");
            let log = router
                .wait_for_log_for_source(router.source_id, RequestStatus::Error)
                .await;
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains(PRIVATE_MARKER),
                "{name}: private nonportable payload must not enter Request Log"
            );
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn forced_nonportable_cross_wire_tools_reject_before_credentials() {
    const PRIVATE_MARKER: &str = "tool-private-marker";
    for (name, fixture) in fixtures()
        .into_iter()
        .filter(|(_, fixture)| fixture.protocol != DownstreamProtocol::Openai)
    {
        run_case(name, move |context| async move {
            let upstream =
                TestUpstream::spawn_json(StatusCode::OK, json!({"should_not":"be called"})).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            if fixture.profile_type != UpstreamProfileType::Openai {
                router
                    .replace_default_source_profile(&upstream.base_url, UpstreamProfileType::Openai)
                    .await;
            }
            let mut request = fixture.request.downstream.clone();
            match fixture.protocol {
                DownstreamProtocol::Responses => {
                    request["tools"] =
                        json!([{"type":"web_search_preview","marker":PRIVATE_MARKER}]);
                    request["tool_choice"] = json!("required");
                }
                DownstreamProtocol::Anthropic => {
                    request["tools"] = json!([{
                        "type":"web_search_20250305","name":"search","marker":PRIVATE_MARKER
                    }]);
                    request["tool_choice"] = json!({"type":"any"});
                }
                DownstreamProtocol::Gemini => {
                    request["tools"] = json!([{"googleSearch":{"marker":PRIVATE_MARKER}}]);
                    request["toolConfig"] = json!({"functionCallingConfig":{"mode":"ANY"}});
                }
                DownstreamProtocol::Openai => unreachable!(),
            }
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router.send(&fixture, false, &request).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("tool rejection should be readable");
            assert!(
                !String::from_utf8_lossy(&body).contains(PRIVATE_MARKER),
                "{name}"
            );
            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                0,
                "{name}"
            );
            assert!(upstream.requests().await.is_empty(), "{name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn all_public_downstreams_reject_unrepresentable_structured_outputs_before_credentials() {
    const PRIVATE_MARKER: &str = "structured-private-marker";
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let upstream =
                TestUpstream::spawn_json(StatusCode::OK, json!({"should_not":"be called"})).await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            if fixture.profile_type != UpstreamProfileType::Openai {
                router
                    .replace_default_source_profile(&upstream.base_url, UpstreamProfileType::Openai)
                    .await;
            }
            let mut request = fixture.request.downstream.clone();
            match fixture.protocol {
                DownstreamProtocol::Openai => {
                    request["response_format"] = json!({"type":"grammar","grammar":PRIVATE_MARKER});
                }
                DownstreamProtocol::Responses => {
                    request["text"] = json!({"format":{
                        "type":"grammar",
                        "grammar":PRIVATE_MARKER
                    }});
                }
                DownstreamProtocol::Anthropic => {
                    request["output_config"] = json!({"format":{
                        "type":"grammar",
                        "grammar":PRIVATE_MARKER
                    }});
                }
                DownstreamProtocol::Gemini => {
                    request["generationConfig"] = json!({
                        "responseMimeType":"text/x.enum",
                        "responseSchema":{"description":PRIVATE_MARKER}
                    });
                }
            }
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router.send(&fixture, false, &request).await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{name}");
            let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("structured rejection should be readable");
            assert!(
                !String::from_utf8_lossy(&response_body).contains(PRIVATE_MARKER),
                "{name}: structured payload must not be disclosed"
            );
            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                0,
                "{name}: reject before credentials"
            );
            assert!(
                upstream.requests().await.is_empty(),
                "{name}: reject before network"
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn responses_target_rejects_invalid_structured_outputs_before_credentials() {
    const PRIVATE_MARKER: &str = "responses-structured-private-marker";

    for (wire_name, fixture) in responses_target_fixtures() {
        let mut cases = Vec::new();
        match fixture.protocol {
            DownstreamProtocol::Openai => {
                let mut grammar = fixture.request.downstream.clone();
                grammar["response_format"] = json!({"type":"grammar","grammar":PRIVATE_MARKER});
                cases.push(("grammar", grammar));

                let mut invalid_name = fixture.request.downstream.clone();
                invalid_name["response_format"] = json!({"type":"json_schema","json_schema":{
                    "name":"","schema":{"private":PRIVATE_MARKER},"strict":true
                }});
                cases.push(("invalid-name", invalid_name));

                let mut invalid_schema = fixture.request.downstream.clone();
                invalid_schema["response_format"] = json!({"type":"json_schema","json_schema":{
                    "name":"valid_name","schema":[PRIVATE_MARKER],"strict":true
                }});
                cases.push(("invalid-schema", invalid_schema));
            }
            DownstreamProtocol::Responses => {
                let formats = [
                    (
                        "grammar",
                        json!({"type":"grammar","grammar":PRIVATE_MARKER}),
                    ),
                    (
                        "invalid-name",
                        json!({"type":"json_schema","name":"","schema":{"private":PRIVATE_MARKER}}),
                    ),
                    (
                        "invalid-schema",
                        json!({"type":"json_schema","name":"valid_name","schema":[PRIVATE_MARKER]}),
                    ),
                    (
                        "conflicting-json-object",
                        json!({"type":"json_object","schema":{"private":PRIVATE_MARKER}}),
                    ),
                ];
                for (name, format) in formats {
                    let mut request = fixture.request.downstream.clone();
                    request["text"] = json!({"format":format});
                    cases.push((name, request));
                }
                let mut wrong_envelope = fixture.request.downstream.clone();
                wrong_envelope["response_format"] =
                    json!({"type":"json_schema","private":PRIVATE_MARKER});
                cases.push(("wrong-envelope", wrong_envelope));
            }
            DownstreamProtocol::Anthropic => {
                let mut grammar = fixture.request.downstream.clone();
                grammar["output_config"] = json!({"format":{
                    "type":"grammar","grammar":PRIVATE_MARKER
                }});
                cases.push(("grammar", grammar));

                let mut invalid_schema = fixture.request.downstream.clone();
                invalid_schema["output_config"] = json!({"format":{
                    "type":"json_schema","schema":[PRIVATE_MARKER]
                }});
                cases.push(("invalid-schema", invalid_schema));
            }
            DownstreamProtocol::Gemini => {
                let mut non_json = fixture.request.downstream.clone();
                non_json["generationConfig"] = json!({
                    "responseMimeType":"text/x.enum",
                    "responseSchema":{"private":PRIVATE_MARKER}
                });
                cases.push(("non-json", non_json));

                let mut double_schema = fixture.request.downstream.clone();
                double_schema["generationConfig"] = json!({
                    "responseMimeType":"application/json",
                    "responseSchema":{"private":PRIVATE_MARKER},
                    "responseJsonSchema":{"private":PRIVATE_MARKER}
                });
                cases.push(("double-schema", double_schema));

                let mut conflicting_envelopes = fixture.request.downstream.clone();
                conflicting_envelopes["generationConfig"] = json!({
                    "responseFormat":{"text":{"mimeType":"application/json",
                        "schema":{"private":PRIVATE_MARKER}}},
                    "responseMimeType":"application/json"
                });
                cases.push(("conflicting-envelopes", conflicting_envelopes));

                let mut invalid_schema = fixture.request.downstream.clone();
                invalid_schema["generationConfig"] = json!({
                    "responseFormat":{"text":{"mimeType":"application/json",
                        "schema":[PRIVATE_MARKER]}}
                });
                cases.push(("invalid-schema", invalid_schema));
            }
        }

        for (case_name, request) in cases {
            let fixture = fixture.clone();
            let name = format!("{wire_name}-{case_name}");
            run_case(&name, move |context| async move {
                let upstream =
                    TestUpstream::spawn_json(StatusCode::OK, json!({"should_not":"be called"}))
                        .await;
                let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
                router
                    .app_state
                    .secret_encryption
                    .reset_decrypt_call_count();

                let response = router.send(&fixture, false, &request).await;
                assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case_name}");
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("structured rejection should be readable");
                assert!(
                    !String::from_utf8_lossy(&body).contains(PRIVATE_MARKER),
                    "{case_name}"
                );
                assert_eq!(
                    router.app_state.secret_encryption.decrypt_call_count(),
                    0,
                    "{case_name}"
                );
                assert!(upstream.requests().await.is_empty(), "{case_name}");
                let log = router
                    .wait_for_log_for_source(router.source_id, RequestStatus::Error)
                    .await;
                assert!(
                    !log.final_error_message
                        .as_deref()
                        .unwrap_or_default()
                        .contains(PRIVATE_MARKER),
                    "{case_name}: schema must not enter Request Log"
                );
                router.wait_for_api_key_lease_release().await;
                assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
                upstream.shutdown().await;
            });
        }
    }
}

#[test]
fn all_public_downstreams_insert_openai_stream_usage_on_the_final_target() {
    let openai_events = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture.stream.upstream_events)
        .expect("openai stream fixture");

    for (name, fixture) in fixtures() {
        let upstream_events = openai_events.clone();
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Sse {
                events: upstream_events,
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let source_id = if fixture.profile_type != UpstreamProfileType::Openai {
                router
                    .replace_default_source_profile(&upstream.base_url, UpstreamProfileType::Openai)
                    .await
            } else {
                router.source_id
            };

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;

            let status = response.status();
            let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("stream response should complete");
            if status != StatusCode::OK {
                let log = router
                    .wait_for_log_for_source(source_id, RequestStatus::Error)
                    .await;
                panic!(
                    "{name}: status={status}, response={}, operator={:?}",
                    String::from_utf8_lossy(&response_body),
                    log.final_error_message
                );
            }
            assert_eq!(
                status,
                StatusCode::OK,
                "{name}: {}",
                String::from_utf8_lossy(&response_body)
            );
            let captured = upstream.requests().await;
            assert_eq!(captured.len(), 1, "{name}");
            let body: Value = serde_json::from_slice(&captured[0].body)
                .expect("OpenAI target request should be JSON");
            assert_eq!(
                body.pointer("/stream_options/include_usage"),
                Some(&Value::Bool(true)),
                "{name}: final OpenAI target must request usage"
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn explicit_false_openai_stream_usage_is_overridden_before_send() {
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case("openai-usage-override", move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Sse {
            events: fixture.stream.upstream_events.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let mut request = fixture.stream.downstream_request.clone();
        request["stream_options"] = json!({"include_usage": false});

        let response = router.send(&fixture, true, &request).await;

        assert_eq!(response.status(), StatusCode::OK);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("stream response should complete");
        let captured = upstream.requests().await;
        assert_eq!(captured.len(), 1);
        let body: Value = serde_json::from_slice(&captured[0].body)
            .expect("OpenAI target request should be JSON");
        assert_eq!(
            body.pointer("/stream_options/include_usage"),
            Some(&Value::Bool(true))
        );
        upstream.shutdown().await;
    });
}

#[test]
fn malformed_openai_stream_usage_is_rejected_before_credentials() {
    const INVALID_USAGE_VALUE: &str = "payload-secret-marker";
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case("openai-usage-type", move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Sse {
            events: fixture.stream.upstream_events.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let mut request = fixture.stream.downstream_request.clone();
        request["stream_options"] = json!({"include_usage": INVALID_USAGE_VALUE});
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router.send(&fixture, true, &request).await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("usage rejection response should read");
        assert!(!String::from_utf8_lossy(&response_body).contains(INVALID_USAGE_VALUE));
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        assert!(upstream.requests().await.is_empty());
        upstream.shutdown().await;
    });
}

#[test]
fn successful_stream_without_upstream_usage_keeps_tokens_unknown() {
    let (_, mut fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    fixture.stream.upstream_events.retain(|event| {
        event.data == Value::String("[DONE]".to_string()) || event.data.get("usage").is_none()
    });
    run_case("openai-missing-usage", move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Sse {
            events: fixture.stream.upstream_events.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

        let response = router
            .send(&fixture, true, &fixture.stream.downstream_request)
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("stream response should complete");
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_eq!(log.total_input_tokens, None);
        assert_eq!(log.total_output_tokens, None);
        assert_eq!(log.total_tokens, None);
        upstream.shutdown().await;
    });
}

#[test]
fn four_public_protocols_reject_sse_encoding_before_headers() {
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("text/event-stream; charset=utf-8".to_string()),
                content_encoding: Some("gzip".to_string()),
                body: gzip_bytes(b"data: provider-secret\n\n"),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let persisted_sink = router.install_recording_persisted_sink();

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{name}");
            assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("pre-commit SSE encoding error should be a readable envelope");
            let body: Value = serde_json::from_slice(&body)
                .expect("pre-commit SSE encoding error should use the protocol envelope");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("upstream_response_error"),
                "{name}"
            );
            assert_eq!(
                downstream_error_message(&body),
                Some("The gateway could not read a valid response from the upstream provider."),
                "{name}"
            );
            assert!(body.get("upstream_error").is_none(), "{name}");

            let captured = upstream.requests().await;
            assert_eq!(captured.len(), 1, "{name}: no retry");
            assert_eq!(
                captured[0].headers.get("accept-encoding").unwrap(),
                "identity"
            );
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error")
            );
            assert_eq!(log.upstream_http_status, Some(200));
            assert_single_persisted_terminal_fact(
                &persisted_sink,
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::NotVisible,
            )
            .await;
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn four_public_protocols_emit_one_native_terminal_on_cross_wire_stream_decode_failure() {
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let dropped = Arc::new(DropSignal::default());
            let upstream = TestUpstream::spawn(ScriptedReply::HangingSse {
                first_event: GoldenEvent {
                    event: None,
                    data: Value::String("malformed-upstream-private-marker".to_string()),
                },
                dropped: Arc::clone(&dropped),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let source_id = match fixture.protocol {
                DownstreamProtocol::Openai => {
                    router
                        .replace_default_source_profile(
                            &upstream.base_url,
                            UpstreamProfileType::Gemini,
                        )
                        .await
                }
                DownstreamProtocol::Gemini => {
                    router
                        .replace_default_source_profile(
                            &upstream.base_url,
                            UpstreamProfileType::Openai,
                        )
                        .await
                }
                DownstreamProtocol::Responses | DownstreamProtocol::Anthropic => router.source_id,
            };
            let persisted_sink = router.install_recording_persisted_sink();

            let response = router
                .send(&fixture, true, &fixture.cancellation.downstream_request)
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{name}");
            assert_no_public_transform_diagnostics(&response);
            let request_id = assert_downstream_request_identity(&response);
            let mut body = response.into_body().into_data_stream();
            let terminal_chunk = timeout(WAIT_TIMEOUT, body.next())
                .await
                .expect("fatal terminal should arrive before deadline")
                .expect("fatal stream should contain one terminal chunk")
                .expect("native fatal terminal should close the Body normally");
            assert!(
                timeout(WAIT_TIMEOUT, body.next()).await.unwrap().is_none(),
                "{name}: native fatal must be the only Body terminal"
            );
            let terminal_events = parse_downstream_events(fixture.protocol, &terminal_chunk);
            assert_eq!(terminal_events.len(), 1, "{name}");
            assert_native_fatal_stream_event(fixture.protocol, &terminal_events[0], &request_id);
            dropped.wait().await;

            let log = router
                .wait_for_log_for_source(source_id, RequestStatus::Error)
                .await;
            assert_eq!(log.request_id, request_id, "{name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error"),
                "{name}"
            );
            assert_single_persisted_terminal_fact(
                &persisted_sink,
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::HeadersCommitted,
            )
            .await;
            router.wait_for_api_key_lease_release().await;
            assert_eq!(
                upstream.requests().await.len(),
                1,
                "{name}: no retry or fallback"
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn cross_wire_target_stream_rejection_emits_one_native_terminal_and_releases_resources() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        const PRIVATE_MARKER: &str = "target-image-private-marker";
        const CODE: &str = "unsupported_capability_error";
        const MESSAGE: &str = "The selected target does not support the requested capability.";
        let dropped = Arc::new(DropSignal::default());
        let upstream = TestUpstream::spawn(ScriptedReply::HangingSse {
            first_event: GoldenEvent {
                event: None,
                data: json!({
                    "candidates":[{
                        "index":0,
                        "content":{
                            "role":"model",
                            "parts":[{
                                "inlineData":{
                                    "mimeType":"image/png",
                                    "data":PRIVATE_MARKER
                                }
                            }]
                        }
                    }]
                }),
            },
            dropped: Arc::clone(&dropped),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let source_id = router
            .replace_default_source_profile(&upstream.base_url, UpstreamProfileType::Gemini)
            .await;
        let persisted_sink = router.install_recording_persisted_sink();

        let response = router
            .send(&fixture, true, &fixture.cancellation.downstream_request)
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_no_public_transform_diagnostics(&response);
        let request_id = assert_downstream_request_identity(&response);
        let mut body = response.into_body().into_data_stream();
        let terminal_chunk = timeout(WAIT_TIMEOUT, body.next())
            .await
            .expect("target rejection terminal should arrive before deadline")
            .expect("target rejection should emit one terminal chunk")
            .expect("native target rejection terminal should close normally");
        assert!(
            timeout(WAIT_TIMEOUT, body.next()).await.unwrap().is_none(),
            "target rejection terminal must be the only downstream chunk"
        );
        assert_payload_free_transform_bytes(&terminal_chunk, PRIVATE_MARKER);
        let terminal_events = parse_downstream_events(fixture.protocol, &terminal_chunk);
        assert_eq!(terminal_events.len(), 1);
        let event = &terminal_events[0];
        assert_eq!(event.event, None);
        assert_eq!(event.data["error"]["code"], CODE);
        assert_eq!(event.data["error"]["type"], "invalid_request_error");
        assert_eq!(event.data["error"]["message"], MESSAGE);
        assert_eq!(event.data["request_id"], request_id);
        dropped.wait().await;

        let log = router
            .wait_for_log_for_source(source_id, RequestStatus::Error)
            .await;
        assert_eq!(log.request_id, request_id);
        assert_eq!(log.final_error_code.as_deref(), Some(CODE));
        assert_eq!(
            log.source_selection_reason.as_deref(),
            Some("provider_default_transform")
        );
        assert_single_persisted_terminal_fact(
            &persisted_sink,
            ExecutionStage::Capability,
            ResponseVisibility::HeadersCommitted,
        )
        .await;
        router.wait_for_api_key_lease_release().await;
        assert_eq!(
            upstream.requests().await.len(),
            1,
            "target rejection must not switch Source, retry, or fall back"
        );
        upstream.shutdown().await;
    });
}

#[test]
fn four_public_protocols_terminate_body_on_sse_parser_failure() {
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let dropped = Arc::new(DropSignal::default());
            let upstream = TestUpstream::spawn(ScriptedReply::ChunkedSse {
                content_encoding: None,
                chunks: vec![
                    events_to_sse_bytes(&[fixture.cancellation.first_upstream_event.clone()]),
                    b"data: \xff\n\n".to_vec(),
                ],
                hang_after_chunks: true,
                dropped: Some(Arc::clone(&dropped)),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let persisted_sink = router.install_recording_persisted_sink();

            let response = router
                .send(&fixture, true, &fixture.cancellation.downstream_request)
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{name}");
            let request_id = assert_downstream_request_identity(&response);
            let mut body = response.into_body().into_data_stream();
            let mut successful_body = Vec::new();
            let mut saw_error = false;
            loop {
                let item = timeout(WAIT_TIMEOUT, body.next())
                    .await
                    .expect("post-commit parser failure should terminate before deadline");
                match item {
                    Some(Ok(chunk)) => successful_body.extend_from_slice(&chunk),
                    Some(Err(_)) => {
                        saw_error = true;
                        break;
                    }
                    None => break,
                }
            }
            assert!(
                saw_error,
                "{name}: parser failure must surface as a Body error"
            );
            assert!(
                !successful_body.is_empty(),
                "{name}: a valid event must precede failure"
            );
            assert!(
                !successful_body
                    .windows("upstream_response_error".len())
                    .any(|window| window == b"upstream_response_error"),
                "{name}: no second protocol envelope may be written after Body start"
            );
            assert!(
                timeout(WAIT_TIMEOUT, body.next()).await.unwrap().is_none(),
                "{name}: Body must end immediately after its single error"
            );
            dropped.wait().await;

            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(log.request_id, request_id, "{name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error")
            );
            assert_eq!(log.upstream_http_status, Some(200));
            assert_single_persisted_terminal_fact(
                &persisted_sink,
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::BodyStarted,
            )
            .await;
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{name}");
            assert_eq!(upstream.requests().await.len(), 1, "{name}: no retry");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_sse_resource_limits_finalize_once() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    let first_event = events_to_sse_bytes(&[fixture.cancellation.first_upstream_event.clone()]);
    let exact_data_line = format!("data: {}\n", "x".repeat(1_017)).into_bytes();
    assert_eq!(exact_data_line.len(), 1_024);
    let partial_line = format!("data: {}", "x".repeat(1_019)).into_bytes();
    assert_eq!(partial_line.len(), 1_025);

    let cases = vec![
        (
            "sse-line-plus-one",
            sse_proxy_config(1_024, 2_048, 4_096, 100),
            vec![
                first_event.clone(),
                format!("data: {}\n\n", "x".repeat(1_019)).into_bytes(),
            ],
            ResponseVisibility::BodyStarted,
        ),
        (
            "sse-event-plus-one",
            sse_proxy_config(1_024, 2_048, 4_096, 100),
            vec![
                first_event.clone(),
                format!(
                    "data: {}\ndata: {}\ndata: {}\n\n",
                    "x".repeat(700),
                    "x".repeat(700),
                    "x".repeat(700)
                )
                .into_bytes(),
            ],
            ResponseVisibility::BodyStarted,
        ),
        (
            "sse-buffer-plus-one",
            sse_proxy_config(1_024, 2_048, 2_048, 100),
            vec![first_event.clone(), exact_data_line, partial_line],
            ResponseVisibility::BodyStarted,
        ),
        (
            "sse-frame-plus-one",
            sse_proxy_config(1_024, 2_048, 4_096, 1),
            vec![first_event.clone(), b":\n\n".to_vec()],
            ResponseVisibility::BodyStarted,
        ),
        (
            "sse-invalid-utf8",
            sse_proxy_config(1_024, 2_048, 4_096, 100),
            vec![first_event, b"data: \xff\n\n".to_vec()],
            ResponseVisibility::BodyStarted,
        ),
        (
            "sse-single-chunk-buffer-plus-one",
            sse_proxy_config(1_024, 2_048, 2_048, 100),
            vec![vec![b'x'; 2_049]],
            ResponseVisibility::HeadersCommitted,
        ),
    ];

    for (case_name, proxy_config, chunks, expected_visibility) in cases {
        let fixture = fixture.clone();
        run_case(case_name, move |context| async move {
            let dropped = Arc::new(DropSignal::default());
            let upstream = TestUpstream::spawn(ScriptedReply::ChunkedSse {
                content_encoding: None,
                chunks,
                hang_after_chunks: true,
                dropped: Some(Arc::clone(&dropped)),
            })
            .await;
            let mut router =
                RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
            router
                .replace_proxy_request_config(context, proxy_config)
                .await;
            let persisted_sink = router.install_recording_persisted_sink();

            let response = router
                .send(&fixture, true, &fixture.cancellation.downstream_request)
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let mut body = response.into_body().into_data_stream();
            let mut successful_chunks = 0usize;
            let mut saw_error = false;
            while let Some(item) = timeout(WAIT_TIMEOUT, body.next()).await.unwrap() {
                match item {
                    Ok(_) => successful_chunks += 1,
                    Err(_) => {
                        saw_error = true;
                        break;
                    }
                }
            }
            assert!(saw_error, "{case_name}");
            assert_eq!(
                successful_chunks > 0,
                expected_visibility == ResponseVisibility::BodyStarted,
                "{case_name}"
            );
            dropped.wait().await;
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error")
            );
            assert_single_persisted_terminal_fact(
                &persisted_sink,
                ExecutionStage::UpstreamResponse,
                expected_visibility,
            )
            .await;
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_sse_eof_residual_is_discarded() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let first_event = events_to_sse_bytes(&[fixture.cancellation.first_upstream_event.clone()]);
        let split = first_event.len() / 2;
        let upstream = TestUpstream::spawn(ScriptedReply::ChunkedSse {
            content_encoding: Some("identity".to_string()),
            chunks: vec![
                first_event[..split].to_vec(),
                first_event[split..].to_vec(),
                b"data: unterminated residual".to_vec(),
            ],
            hang_after_chunks: false,
            dropped: None,
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let response = router
            .send(&fixture, true, &fixture.cancellation.downstream_request)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("EOF residual should be discarded without failing the stream");
        assert_eq!(parse_downstream_events(fixture.protocol, &body).len(), 1);
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert!(log.final_error_code.is_none());
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        assert_eq!(upstream.requests().await.len(), 1);
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_openai_done_closes_upstream_and_finalizes_once() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let dropped = Arc::new(DropSignal::default());
        let upstream = TestUpstream::spawn(ScriptedReply::HangingSse {
            first_event: GoldenEvent {
                event: None,
                data: Value::String("[DONE]".to_string()),
            },
            dropped: Arc::clone(&dropped),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let persisted_sink = router.install_recording_persisted_sink();
        let response = router
            .send(&fixture, true, &fixture.cancellation.downstream_request)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("[DONE] should terminate the downstream stream cleanly");
        assert_eq!(body, Bytes::from_static(b"data: [DONE]\n\n"));
        dropped.wait().await;
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert!(log.final_error_code.is_none());
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        let contexts = persisted_sink.contexts.lock().await;
        assert_eq!(contexts.len(), 1);
        drop(contexts);
        assert_eq!(upstream.requests().await.len(), 1);
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_stream_emits_one_event_per_downstream_body_chunk() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Sse {
            events: fixture.stream.upstream_events.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let response = router
            .send(&fixture, true, &fixture.stream.downstream_request)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body().into_data_stream();
        let mut body_chunks = 0usize;
        while let Some(chunk) = body.next().await {
            let chunk = chunk.expect("valid stream chunk");
            assert_eq!(
                parse_downstream_events(fixture.protocol, &chunk).len(),
                1,
                "each downstream Body chunk must contain one transformed SSE event"
            );
            body_chunks += 1;
        }
        assert_eq!(body_chunks, fixture.stream.downstream_events.len());
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert!(log.final_error_code.is_none());
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_regression_upstream_429_is_authentic_logged_and_never_retried() {
    for (name, fixture) in generation_evidence_fixtures() {
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::from_u16(fixture.error.upstream_status).unwrap(),
                body: fixture.error.upstream_response.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;
            let response = router
                .send(&fixture, false, &fixture.error.downstream_request)
                .await;
            assert_eq!(
                response.status().as_u16(),
                fixture.error.downstream_status,
                "{name}: error status"
            );
            let request_id = assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                body,
                render_request_id(&fixture.error.downstream_response, &request_id),
                "{name}: complete protocol error envelope and authentic upstream extension"
            );
            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.request.upstream_path,
                fixture.request.upstream_query.as_deref(),
                &fixture.request.upstream,
                &router.requested_model(),
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.request_id, request_id,
                "{name}: error response and persisted canonical request id"
            );
            assert_log_common(&router, &fixture, &log);
            assert_eq!(log.upstream_http_status, Some(429));
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_rate_limit_error")
            );
            assert!(
                log.final_error_message
                    .as_deref()
                    .is_some_and(|message| message.contains("JSON error body with a message field"))
            );
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("baseline throttled")
            );
            assert_stream_failure_has_no_usage_or_cost(&log, name);
            router.wait_for_api_key_lease_release().await;
            router.assert_no_api_key_usage_charge(name).await;
            assert_eq!(captured.len(), 1, "{name}: HTTP failure must not retry");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_repeated_upstream_failures_never_create_cross_request_gating() {
    const RETIRED_FAILURE_THRESHOLD: usize = 5;
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::from_u16(fixture.error.upstream_status).unwrap(),
            body: fixture.error.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

        for attempt in 0..=RETIRED_FAILURE_THRESHOLD {
            let response = router
                .send(&fixture, false, &fixture.error.downstream_request)
                .await;
            assert_eq!(
                response.status().as_u16(),
                fixture.error.downstream_status,
                "attempt {attempt} should preserve the upstream error status"
            );
            assert!(
                response.headers().get("retry-after").is_none(),
                "attempt {attempt} must not expose a cross-request Retry-After"
            );
            let request_id = assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("repeated error response should be readable");
            let body: Value =
                serde_json::from_slice(&body).expect("repeated error response should be JSON");
            assert_eq!(
                body,
                render_request_id(&fixture.error.downstream_response, &request_id),
                "attempt {attempt} should preserve the authentic error envelope"
            );
            router.wait_for_api_key_lease_release().await;

            let logs = router.request_logs().await;
            assert_eq!(
                logs.len(),
                attempt + 1,
                "attempt {attempt} should persist exactly one request-level log"
            );
            assert!(logs.iter().all(|log| {
                log.overall_status == RequestStatus::Error
                    && log.final_error_code.as_deref() == Some("upstream_rate_limit_error")
                    && log.upstream_http_status == Some(429)
            }));
        }

        assert_eq!(
            upstream.requests().await.len(),
            RETIRED_FAILURE_THRESHOLD + 1,
            "the request after the retired threshold must still reach upstream"
        );
        upstream.shutdown().await;
    });
}

#[test]
fn explicit_upstream_statuses_preserve_json_text_binary_empty_and_truncated_bodies() {
    #[derive(Clone)]
    enum ExpectedBody {
        Json(Value),
        Text(String),
        Base64(Vec<u8>),
    }

    #[derive(Clone)]
    struct ErrorCase {
        name: &'static str,
        upstream_status: StatusCode,
        content_type: Option<&'static str>,
        upstream_body: Vec<u8>,
        downstream_status: StatusCode,
        downstream_code: &'static str,
        downstream_message: &'static str,
        expected_body: ExpectedBody,
        truncated: bool,
    }

    let long_body = "x".repeat(65_537);
    let cases = [
        ErrorCase {
            name: "upstream-401-json",
            upstream_status: StatusCode::UNAUTHORIZED,
            content_type: Some("application/json"),
            upstream_body: br#"{"detail":"invalid provider credential"}"#.to_vec(),
            downstream_status: StatusCode::BAD_GATEWAY,
            downstream_code: "upstream_authentication_error",
            downstream_message: "Upstream provider rejected its credentials.",
            expected_body: ExpectedBody::Json(json!({"detail":"invalid provider credential"})),
            truncated: false,
        },
        ErrorCase {
            name: "upstream-403-text",
            upstream_status: StatusCode::FORBIDDEN,
            content_type: Some("text/plain; charset=utf-8"),
            upstream_body: b"provider credential lacks scope".to_vec(),
            downstream_status: StatusCode::BAD_GATEWAY,
            downstream_code: "upstream_authentication_error",
            downstream_message: "Upstream provider rejected its credentials.",
            expected_body: ExpectedBody::Text("provider credential lacks scope".to_string()),
            truncated: false,
        },
        ErrorCase {
            name: "upstream-413-text",
            upstream_status: StatusCode::PAYLOAD_TOO_LARGE,
            content_type: Some("text/plain"),
            upstream_body: b"provider payload limit".to_vec(),
            downstream_status: StatusCode::PAYLOAD_TOO_LARGE,
            downstream_code: "upstream_payload_too_large_error",
            downstream_message: "Upstream provider rejected the request payload as too large.",
            expected_body: ExpectedBody::Text("provider payload limit".to_string()),
            truncated: false,
        },
        ErrorCase {
            name: "upstream-500-binary",
            upstream_status: StatusCode::INTERNAL_SERVER_ERROR,
            content_type: Some("application/octet-stream"),
            upstream_body: vec![0xff, 0x00, 0x01, 0xfe],
            downstream_status: StatusCode::SERVICE_UNAVAILABLE,
            downstream_code: "upstream_service_error",
            downstream_message: "Upstream provider service failed.",
            expected_body: ExpectedBody::Base64(vec![0xff, 0x00, 0x01, 0xfe]),
            truncated: false,
        },
        ErrorCase {
            name: "upstream-302-empty",
            upstream_status: StatusCode::FOUND,
            content_type: None,
            upstream_body: Vec::new(),
            downstream_status: StatusCode::BAD_GATEWAY,
            downstream_code: "upstream_unexpected_status_error",
            downstream_message: "Upstream provider returned an unexpected HTTP status.",
            expected_body: ExpectedBody::Text(String::new()),
            truncated: false,
        },
        ErrorCase {
            name: "upstream-400-truncated",
            upstream_status: StatusCode::BAD_REQUEST,
            content_type: Some("application/json"),
            upstream_body: long_body.as_bytes().to_vec(),
            downstream_status: StatusCode::BAD_REQUEST,
            downstream_code: "upstream_invalid_request_error",
            downstream_message: "Upstream provider rejected the request.",
            expected_body: ExpectedBody::Text("x".repeat(65_536)),
            truncated: true,
        },
    ];

    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");

    for case in cases {
        let fixture = fixture.clone();
        run_case(case.name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: case.upstream_status,
                content_type: case.content_type.map(str::to_string),
                content_encoding: None,
                body: case.upstream_body.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(response.status(), case.downstream_status, "{}", case.name);
            assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("error response should read");
            let body: Value = serde_json::from_slice(&body).expect("error response should be json");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some(case.downstream_code),
                "{}",
                case.name
            );
            assert_eq!(
                downstream_error_message(&body),
                Some(case.downstream_message),
                "{}",
                case.name
            );
            let upstream_error = &body["upstream_error"];
            assert_eq!(
                upstream_error["status"],
                case.upstream_status.as_u16(),
                "{}",
                case.name
            );
            assert_eq!(
                upstream_error["content_type"],
                case.content_type
                    .map_or(Value::Null, |value| Value::String(value.to_string())),
                "{}",
                case.name
            );
            assert_eq!(upstream_error["truncated"], case.truncated, "{}", case.name);
            assert_eq!(
                upstream_error["captured_bytes"],
                case.upstream_body.len().min(65_536),
                "{}",
                case.name
            );
            assert_eq!(upstream_error["limit_bytes"], 65_536, "{}", case.name);

            match &case.expected_body {
                ExpectedBody::Json(expected) => {
                    assert_eq!(&upstream_error["body"], expected, "{}", case.name);
                }
                ExpectedBody::Text(expected) => {
                    assert_eq!(&upstream_error["body_text"], expected, "{}", case.name);
                }
                ExpectedBody::Base64(expected) => {
                    use base64::{Engine as _, engine::general_purpose::STANDARD};
                    let encoded = upstream_error["body_base64"]
                        .as_str()
                        .expect("binary error body should be base64");
                    assert_eq!(
                        STANDARD.decode(encoded).expect("base64 should decode"),
                        *expected,
                        "{}",
                        case.name
                    );
                    assert_eq!(upstream_error["body_encoding"], "base64", "{}", case.name);
                }
            }
            if case.truncated {
                assert_eq!(
                    upstream_error["notice"], "Upstream error body was truncated by the gateway.",
                    "{}",
                    case.name
                );
                assert!(upstream_error.get("body").is_none(), "{}", case.name);
            } else {
                assert!(upstream_error.get("notice").is_none(), "{}", case.name);
            }

            assert_eq!(upstream.requests().await.len(), 1, "{}", case.name);
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some(case.downstream_code),
                "{}",
                case.name
            );
            assert_eq!(
                log.upstream_http_status,
                Some(i32::from(case.upstream_status.as_u16())),
                "{}",
                case.name
            );
            if let Ok(upstream_text) = std::str::from_utf8(&case.upstream_body)
                && !upstream_text.is_empty()
            {
                assert!(
                    !log.final_error_message
                        .as_deref()
                        .unwrap_or_default()
                        .contains(upstream_text),
                    "{}: Request Log must not persist the complete upstream body",
                    case.name
                );
            }
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_connect_failure_has_fixed_gateway_payload_without_upstream_extension() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let unused_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("unused local endpoint should bind");
        let unused_addr = unused_listener
            .local_addr()
            .expect("unused local endpoint address");
        drop(unused_listener);
        let endpoint = format!("http://{unused_addr}");
        let router = RouterFixture::new(context, &fixture, &endpoint).await;

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_downstream_request_identity(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("connect error response should read");
        let body: Value =
            serde_json::from_slice(&body).expect("connect error response should be json");
        assert_eq!(
            downstream_error_code(&body, fixture.protocol),
            Some("upstream_connect_error")
        );
        assert_eq!(
            downstream_error_message(&body),
            Some("The gateway could not connect to the upstream provider.")
        );
        assert!(body.get("upstream_error").is_none());

        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("upstream_connect_error")
        );
        assert!(log.upstream_http_status.is_none());
    });
}

#[test]
fn direct_execution_non_stream_body_interruption_is_an_upstream_response_error() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::InterruptedBody {
            content_type: "application/json".to_string(),
            first_chunk: br#"{"partial":"#.to_vec(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_downstream_request_identity(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("interrupted response error envelope should read");
        let body: Value = serde_json::from_slice(&body).expect("response should be JSON");
        assert_eq!(
            downstream_error_code(&body, fixture.protocol),
            Some("upstream_response_error")
        );
        assert_eq!(upstream.requests().await.len(), 1);

        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("upstream_response_error")
        );
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_non_stream_identity_and_gzip_enforce_exact_and_plus_one_limits() {
    const LIMIT: usize = 1_048_576;
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");

    for (case_name, use_gzip, body_size, succeeds) in [
        ("identity-exact-limit", false, LIMIT, true),
        ("identity-raw-plus-one", false, LIMIT + 1, false),
        ("gzip-decoded-exact-limit", true, LIMIT, true),
        ("gzip-decoded-plus-one", true, LIMIT + 1, false),
    ] {
        let fixture = fixture.clone();
        run_case(case_name, move |context| async move {
            let response_value = json_body_with_exact_serialized_size(body_size);
            let decoded = serde_json::to_vec(&response_value).unwrap();
            let (content_encoding, wire_body) = if use_gzip {
                (Some("gzip".to_string()), gzip_bytes(&decoded))
            } else {
                (None, decoded)
            };
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("application/json".to_string()),
                content_encoding,
                body: wire_body,
            })
            .await;
            let mut router =
                RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
            router
                .replace_proxy_request_config(context, one_mib_non_stream_proxy_config(65_536))
                .await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(
                response.status(),
                if succeeds {
                    StatusCode::OK
                } else {
                    StatusCode::BAD_GATEWAY
                },
                "{case_name}"
            );
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("non-stream response should be a complete envelope");
            if succeeds {
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body, response_value, "{case_name}");
            } else {
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(
                    downstream_error_code(&body, fixture.protocol),
                    Some("upstream_response_error"),
                    "{case_name}"
                );
            }

            let captured = upstream.requests().await;
            assert_eq!(captured.len(), 1, "{case_name}: no retry");
            assert_eq!(
                captured[0].headers.get("accept-encoding").unwrap(),
                "gzip, identity",
                "{case_name}"
            );
            let expected_status = if succeeds {
                RequestStatus::Success
            } else {
                RequestStatus::Error
            };
            let log = router.wait_for_log(expected_status).await;
            assert_eq!(log.upstream_http_status, Some(200), "{case_name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                (!succeeds).then_some("upstream_response_error"),
                "{case_name}"
            );
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn four_public_protocols_use_existing_envelopes_for_non_stream_response_limit() {
    const LIMIT: usize = 1_048_576;
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("application/json".to_string()),
                content_encoding: None,
                body: vec![b'x'; LIMIT + 1],
            })
            .await;
            let mut router =
                RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
            router
                .replace_proxy_request_config(context, one_mib_non_stream_proxy_config(65_536))
                .await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{name}");
            assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("{name}: response limit envelope should read");
            let body: Value = serde_json::from_slice(&body).expect("response should be JSON");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("upstream_response_error"),
                "{name}"
            );
            assert_eq!(upstream.requests().await.len(), 1, "{name}: no retry");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error"),
                "{name}"
            );
            assert_eq!(log.upstream_http_status, Some(200), "{name}");
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn four_public_protocols_use_existing_envelopes_for_decoded_response_limit() {
    const LIMIT: usize = 1_048_576;
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let decoded = serde_json::to_vec(&json_body_with_exact_serialized_size(LIMIT + 1))
                .expect("decoded-limit fixture should serialize");
            let wire_body = gzip_bytes(&decoded);
            assert!(
                wire_body.len() < LIMIT,
                "{name}: fixture must isolate decoded limit"
            );
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("application/json".to_string()),
                content_encoding: Some("gzip".to_string()),
                body: wire_body,
            })
            .await;
            let mut router =
                RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
            router
                .replace_proxy_request_config(context, one_mib_non_stream_proxy_config(65_536))
                .await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{name}");
            assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("{name}: decoded response limit envelope should read");
            let body: Value = serde_json::from_slice(&body).expect("response should be JSON");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("upstream_response_error"),
                "{name}"
            );
            assert_eq!(upstream.requests().await.len(), 1, "{name}: no retry");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error"),
                "{name}"
            );
            assert_eq!(log.upstream_http_status, Some(200), "{name}");
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn four_public_protocols_preserve_bounded_provider_error_when_body_reaches_hard_limit() {
    const DISCLOSURE_LIMIT: usize = 1_024;
    const RAW_LIMIT: usize = 1_048_576;
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let marker = format!("provider-error-prefix-{name}");
            let mut upstream_body = marker.as_bytes().to_vec();
            upstream_body.resize(RAW_LIMIT + 1, b'x');
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::TOO_MANY_REQUESTS,
                content_type: Some("text/plain; private=discarded".to_string()),
                content_encoding: None,
                body: upstream_body,
            })
            .await;
            let mut router =
                RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
            router
                .replace_proxy_request_config(
                    context,
                    one_mib_non_stream_proxy_config(DISCLOSURE_LIMIT),
                )
                .await;

            let response = router
                .send(&fixture, false, &fixture.error.downstream_request)
                .await;
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS, "{name}");
            assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("upstream_rate_limit_error"),
                "{name}"
            );
            assert_eq!(body["upstream_error"]["status"], 429, "{name}");
            assert_eq!(body["upstream_error"]["truncated"], true, "{name}");
            assert_eq!(
                body["upstream_error"]["captured_bytes"], DISCLOSURE_LIMIT,
                "{name}"
            );
            assert_eq!(
                body["upstream_error"]["limit_bytes"], DISCLOSURE_LIMIT,
                "{name}"
            );
            assert!(
                body["upstream_error"]["body_text"]
                    .as_str()
                    .is_some_and(|body| body.starts_with(&marker)),
                "{name}"
            );
            assert_eq!(upstream.requests().await.len(), 1, "{name}: no retry");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_rate_limit_error")
            );
            assert_eq!(log.upstream_http_status, Some(429));
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains(&marker),
                "{name}: Provider body must not enter persisted operator diagnostics"
            );
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_provider_error_hard_limit_preserves_status_and_disclosure_prefix() {
    const DISCLOSURE_LIMIT: usize = 1_024;
    const RAW_LIMIT: usize = 1_048_576;
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    for (case_name, upstream_status, downstream_status, error_code) in [
        (
            "hard-limit-400",
            StatusCode::BAD_REQUEST,
            StatusCode::BAD_REQUEST,
            "upstream_invalid_request_error",
        ),
        (
            "hard-limit-401",
            StatusCode::UNAUTHORIZED,
            StatusCode::BAD_GATEWAY,
            "upstream_authentication_error",
        ),
        (
            "hard-limit-413",
            StatusCode::PAYLOAD_TOO_LARGE,
            StatusCode::PAYLOAD_TOO_LARGE,
            "upstream_payload_too_large_error",
        ),
        (
            "hard-limit-429",
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::TOO_MANY_REQUESTS,
            "upstream_rate_limit_error",
        ),
        (
            "hard-limit-503",
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::SERVICE_UNAVAILABLE,
            "upstream_service_error",
        ),
    ] {
        let fixture = fixture.clone();
        run_case(case_name, move |context| async move {
            let marker = format!("provider-body-secret-marker-{case_name}");
            let mut upstream_body = marker.as_bytes().to_vec();
            upstream_body.resize(RAW_LIMIT + 1, b'x');
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: upstream_status,
                content_type: Some("text/plain; private=discarded".to_string()),
                content_encoding: None,
                body: upstream_body,
            })
            .await;
            let mut router =
                RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
            router
                .replace_proxy_request_config(
                    context,
                    one_mib_non_stream_proxy_config(DISCLOSURE_LIMIT),
                )
                .await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(response.status(), downstream_status, "{case_name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some(error_code),
                "{case_name}"
            );
            assert_eq!(
                body["upstream_error"]["status"],
                upstream_status.as_u16(),
                "{case_name}"
            );
            assert_eq!(body["upstream_error"]["truncated"], true, "{case_name}");
            assert_eq!(
                body["upstream_error"]["captured_bytes"], DISCLOSURE_LIMIT,
                "{case_name}"
            );
            assert_eq!(
                body["upstream_error"]["limit_bytes"], DISCLOSURE_LIMIT,
                "{case_name}"
            );
            assert!(
                body["upstream_error"]["body_text"]
                    .as_str()
                    .unwrap()
                    .starts_with(&marker),
                "{case_name}"
            );
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");

            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.upstream_http_status,
                Some(i32::from(upstream_status.as_u16())),
                "{case_name}"
            );
            assert_eq!(
                log.final_error_code.as_deref(),
                Some(error_code),
                "{case_name}"
            );
            assert!(
                !log.final_error_message
                    .unwrap_or_default()
                    .contains(&marker),
                "{case_name}"
            );
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_invalid_gzip_never_falls_back_to_compressed_bytes() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    for upstream_status in [StatusCode::OK, StatusCode::TOO_MANY_REQUESTS] {
        let fixture = fixture.clone();
        let case_name = if upstream_status.is_success() {
            "invalid-gzip-success"
        } else {
            "invalid-gzip-provider-error"
        };
        run_case(case_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: upstream_status,
                content_type: Some("application/json".to_string()),
                content_encoding: Some("gzip".to_string()),
                body: b"not-a-gzip-provider-body-secret".to_vec(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{case_name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("upstream_response_error"),
                "{case_name}"
            );
            assert!(body.get("upstream_error").is_none(), "{case_name}");
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.upstream_http_status,
                Some(i32::from(upstream_status.as_u16())),
                "{case_name}"
            );
            assert!(
                !log.final_error_message
                    .unwrap_or_default()
                    .contains("provider-body-secret"),
                "{case_name}"
            );
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_regression_credential_requests_never_follow_redirects() {
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let redirect_target = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::OK,
                body: json!({"captured": true}),
            })
            .await;
            let location = format!(
                "{}/credential-capture?evidence=raw",
                redirect_target.base_url
            );
            let upstream = TestUpstream::spawn(ScriptedReply::Redirect {
                status: StatusCode::TEMPORARY_REDIRECT,
                location: location.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("redirect error response should read");
            let body: Value =
                serde_json::from_slice(&body).expect("redirect error response should be json");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("upstream_unexpected_status_error")
            );
            assert_eq!(body["upstream_error"]["status"], 307);
            assert_eq!(body["upstream_error"]["body_text"], "redirect");
            assert_eq!(body["upstream_error"]["content_type"], Value::Null);
            assert_eq!(upstream.requests().await.len(), 1, "{name}");
            assert!(
                redirect_target.requests().await.is_empty(),
                "{name}: redirect target must never receive credentials or request body"
            );

            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(log.upstream_http_status, Some(307), "{name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_unexpected_status_error"),
                "{name}"
            );

            upstream.shutdown().await;
            redirect_target.shutdown().await;
        });
    }
}

#[test]
fn incompatible_utility_targets_are_rejected_before_any_upstream_call() {
    let openai_fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    let gemini_fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "gemini")
        .map(|(_, fixture)| fixture)
        .expect("gemini fixture");

    run_case("utility-zero-upstream", move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: json!({"unexpected": true}),
        })
        .await;

        let gemini_target =
            RouterFixture::new(context.clone(), &gemini_fixture, &upstream.base_url).await;
        for (path, body) in [
            (
                "/openai/v1/embeddings",
                json!({"model": gemini_target.requested_model(), "input": "hello"}),
            ),
            (
                "/openai/v1/rerank",
                json!({
                    "model": gemini_target.requested_model(),
                    "query": "hello",
                    "documents": ["world"]
                }),
            ),
        ] {
            let response = gemini_target
                .send_raw_post(path.to_string(), body, DownstreamAuth::Bearer)
                .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        }

        let openai_target = RouterFixture::new(context, &openai_fixture, &upstream.base_url).await;
        let count_tokens_uri = format!(
            "/gemini/v1/models/{}:countTokens?key={}",
            openai_target.requested_model(),
            openai_target.downstream_key
        );
        let response = openai_target
            .send_raw_post(
                count_tokens_uri,
                json!({"contents": [{"parts": [{"text": "hello"}]}]}),
                DownstreamAuth::Bearer,
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        assert!(
            upstream.requests().await.is_empty(),
            "incompatible utilities must fail before HTTP send"
        );
        upstream.shutdown().await;
    });
}

#[test]
fn openai_wire_chat_uses_the_frozen_source_operation_path_and_bearer_auth() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream =
            TestUpstream::spawn_json(StatusCode::OK, fixture.non_stream.upstream_response.clone())
                .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        router
            .update_source_operation(UpdateUpstreamSourceData {
                chat_completions_path_override: Some(Some("custom/chat/".to_string())),
                updated_at: chrono::Utc::now().timestamp_millis(),
                ..UpdateUpstreamSourceData::test_defaults()
            })
            .await;

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let requests = upstream.requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/v1/custom/chat");
        assert_eq!(
            requests[0]
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer provider-baseline-secret")
        );
        assert!(!requests[0].headers.contains_key("x-api-key"));
        assert!(!requests[0].headers.contains_key("x-goog-api-key"));
        upstream.shutdown().await;
    });
}

#[test]
fn disabled_or_invalid_source_operation_fails_before_decryption_and_upstream_access() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream =
            TestUpstream::spawn_json(StatusCode::OK, fixture.non_stream.upstream_response.clone())
                .await;

        let disabled = RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
        disabled
            .update_source_operation(UpdateUpstreamSourceData {
                chat_completions_enabled: Some(false),
                updated_at: chrono::Utc::now().timestamp_millis(),
                ..UpdateUpstreamSourceData::test_defaults()
            })
            .await;
        disabled
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();
        let response = disabled
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(disabled.app_state.secret_encryption.decrypt_call_count(), 0);

        let invalid = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        invalid
            .update_source_operation(UpdateUpstreamSourceData {
                chat_completions_path_override: Some(Some("../chat".to_string())),
                updated_at: chrono::Utc::now().timestamp_millis(),
                ..UpdateUpstreamSourceData::test_defaults()
            })
            .await;
        invalid
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();
        let response = invalid
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(invalid.app_state.secret_encryption.decrypt_call_count(), 0);

        assert!(
            upstream.requests().await.is_empty(),
            "invalid Source operation state must fail before HTTP send"
        );
        upstream.shutdown().await;
    });
}

#[test]
fn generation_model_kind_mismatch_is_rejected_for_all_downstream_protocols_before_decryption() {
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                fixture.non_stream.upstream_response.clone(),
            )
            .await;
            let router = RouterFixture::new_with_model_kind(
                context,
                &fixture,
                &upstream.base_url,
                ModelKind::Embedding,
            )
            .await;
            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{name}");
            assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
            assert!(upstream.requests().await.is_empty(), "{name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(log.model_kind_snapshot, Some(ModelKind::Embedding));
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("unsupported_capability_error")
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn utility_model_kind_mismatch_is_rejected_before_decryption_and_upstream_access() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    run_case("utility-model-kind", move |context| async move {
        let upstream = TestUpstream::spawn_json(StatusCode::OK, json!({"unexpected": true})).await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        for (path, body) in [
            (
                "/openai/v1/embeddings",
                json!({"model": router.requested_model(), "input": "hello"}),
            ),
            (
                "/openai/v1/rerank",
                json!({
                    "model": router.requested_model(),
                    "query": "hello",
                    "documents": ["world"]
                }),
            ),
        ] {
            let response = router
                .send_raw_post(path.to_string(), body, DownstreamAuth::Bearer)
                .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        }

        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        assert!(upstream.requests().await.is_empty());
        upstream.shutdown().await;
    });
}

#[test]
fn utility_operation_and_profile_guards_run_after_kind_but_before_decryption() {
    let openai_fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    let gemini_fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "gemini")
        .map(|(_, fixture)| fixture)
        .expect("gemini fixture");
    run_case("utility-static-guards", move |context| async move {
        let upstream = TestUpstream::spawn_json(StatusCode::OK, json!({"unexpected": true})).await;

        let disabled_embedding = RouterFixture::new_with_model_kind(
            context.clone(),
            &openai_fixture,
            &upstream.base_url,
            ModelKind::Embedding,
        )
        .await;
        disabled_embedding
            .update_source_operation(UpdateUpstreamSourceData {
                embeddings_enabled: Some(false),
                updated_at: chrono::Utc::now().timestamp_millis(),
                ..UpdateUpstreamSourceData::test_defaults()
            })
            .await;
        disabled_embedding
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();
        let response = disabled_embedding
            .send_raw_post(
                "/openai/v1/embeddings".to_string(),
                json!({"model": disabled_embedding.requested_model(), "input": "hello"}),
                DownstreamAuth::Bearer,
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            disabled_embedding
                .app_state
                .secret_encryption
                .decrypt_call_count(),
            0
        );

        let unsupported_rerank = RouterFixture::new_with_model_kind(
            context.clone(),
            &openai_fixture,
            &upstream.base_url,
            ModelKind::Rerank,
        )
        .await;
        unsupported_rerank
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();
        let response = unsupported_rerank
            .send_raw_post(
                "/openai/v1/rerank".to_string(),
                json!({
                    "model": unsupported_rerank.requested_model(),
                    "query": "hello",
                    "documents": ["world"]
                }),
                DownstreamAuth::Bearer,
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            unsupported_rerank
                .app_state
                .secret_encryption
                .decrypt_call_count(),
            0
        );

        let wrong_profile = RouterFixture::new_with_model_kind(
            context,
            &gemini_fixture,
            &upstream.base_url,
            ModelKind::Embedding,
        )
        .await;
        wrong_profile
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();
        let response = wrong_profile
            .send_raw_post(
                "/openai/v1/embeddings".to_string(),
                json!({"model": wrong_profile.requested_model(), "input": "hello"}),
                DownstreamAuth::Bearer,
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            wrong_profile
                .app_state
                .secret_encryption
                .decrypt_call_count(),
            0
        );

        assert!(upstream.requests().await.is_empty());
        upstream.shutdown().await;
    });
}

#[test]
fn models_routes_apply_static_kind_and_source_operation_filters_without_exposing_kind() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    run_case("models-kind-filter", move |context| async move {
        let upstream = TestUpstream::spawn_json(StatusCode::OK, json!({"unexpected": true})).await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        Model::create(
            router.provider_id,
            "embedding-model",
            None,
            ModelKind::Embedding,
            true,
        )
        .expect("embedding model should create");
        Model::create(
            router.provider_id,
            "rerank-model",
            None,
            ModelKind::Rerank,
            true,
        )
        .expect("rerank model should create");
        router
            .app_state
            .admin
            .request_patch
            .create_source_variant(
                router.source_id,
                RequestPatchVariantInput {
                    source_id: router.source_id,
                    model_id: None,
                    suffix: Some("fast".to_string()),
                    enabled: true,
                    expose_in_models: true,
                    rules: vec![RequestPatchRuleInput {
                        placement: RequestPatchPlacement::Body,
                        target: "/temperature".to_string(),
                        operation: RequestPatchOperation::Set,
                        value_json: Some(Some(json!(0.2))),
                        description: Some("models kind filter".to_string()),
                    }],
                },
            )
            .await
            .expect("chat suffix should create");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("model catalog should invalidate");

        for (path, auth, allowed_embedding) in [
            ("/openai/v1/models", DownstreamAuth::Bearer, true),
            ("/responses/models", DownstreamAuth::Bearer, false),
            ("/responses/v1/models", DownstreamAuth::Bearer, false),
            ("/anthropic/v1/models", DownstreamAuth::XApiKey, false),
            ("/gemini/v1/models", DownstreamAuth::GeminiQuery, false),
        ] {
            let response = router.send_models(path, auth).await;
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("models response should read");
            let body: Value = serde_json::from_slice(&bytes).expect("models response should parse");
            let serialized = serde_json::to_string(&body).unwrap();
            assert!(serialized.contains(&router.model_name), "{path}");
            assert!(
                serialized.contains(&format!("{}-fast", router.model_name)),
                "{path}"
            );
            assert!(!serialized.contains("kind-guard"));
            assert_eq!(
                serialized.contains("embedding-model"),
                allowed_embedding,
                "{path}"
            );
            assert!(!serialized.contains("rerank-model"), "{path}");
            assert!(!serialized.contains("embedding-model-fast"), "{path}");
            assert!(!serialized.contains("model_kind"), "{path}");
        }

        assert!(upstream.requests().await.is_empty());
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_legacy_invalid_base_url_fails_before_upstream_access() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        UpstreamSource::update(
            router.source_id,
            router.provider_id,
            &UpdateUpstreamSourceData {
                base_url: Some("http://user:secret@127.0.0.1:1/v1".to_string()),
                use_proxy: None,
                is_enabled: None,
                is_default: None,
                updated_at: chrono::Utc::now().timestamp_millis(),
                ..UpdateUpstreamSourceData::test_defaults()
            },
        )
        .expect("legacy invalid base URL should be seeded directly");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("provider cache should invalidate");
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("provider configuration response should read");
        let body: Value =
            serde_json::from_slice(&body).expect("provider configuration response should be json");
        assert_eq!(
            downstream_error_code(&body, fixture.protocol),
            Some("provider_configuration_error")
        );
        assert_eq!(
            downstream_error_message(&body),
            Some("The gateway provider configuration is invalid.")
        );
        assert!(body.get("upstream_error").is_none());
        assert!(upstream.requests().await.is_empty());
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("provider_configuration_error")
        );
        assert!(
            log.final_error_message
                .as_deref()
                .is_some_and(|message| message.contains("must not contain embedded credentials"))
        );
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_missing_source_fails_before_upstream_access() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let conn = &mut get_connection().expect("test connection should load");
        let DbConnection::Sqlite(conn) = conn else {
            panic!("direct execution test must use SQLite");
        };
        diesel::delete(
            crate::database::_sqlite_schema::upstream_source::table.find(router.source_id),
        )
        .execute(conn)
        .expect("source should be removed for corruption fixture");
        let invalidation = router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await;
        assert!(
            invalidation.is_ok(),
            "zero-source provider aggregate should remain cacheable"
        );

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(upstream.requests().await.is_empty());
        assert!(router.request_logs().await.is_empty());
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_legacy_valid_endpoint_is_normalized_per_request_without_database_write() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let legacy_endpoint = format!(
            "  {}/v1///  ",
            upstream.base_url.replacen("http://", "HTTP://", 1)
        );
        UpstreamSource::update(
            router.source_id,
            router.provider_id,
            &UpdateUpstreamSourceData {
                base_url: Some(legacy_endpoint.clone()),
                use_proxy: None,
                is_enabled: None,
                is_default: None,
                updated_at: chrono::Utc::now().timestamp_millis(),
                ..UpdateUpstreamSourceData::test_defaults()
            },
        )
        .expect("legacy noncanonical endpoint should be seeded directly");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("provider cache should invalidate");

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(upstream.requests().await.len(), 1);
        assert_eq!(
            Provider::get_by_id(router.provider_id)
                .expect("provider should remain persisted")
                .upstream_sources[0]
                .base_url,
            legacy_endpoint
        );
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("successful response body should be consumed before log assertion");
        router.wait_for_log(RequestStatus::Success).await;
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_proxy_requirement_without_configuration_fails_closed() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let infra_context = context.clone();
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let mut router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let mut app_state = (*router.app_state).clone();
        app_state.infra = Arc::new(
            AppInfra::new_with_config(
                OutboundHttpConfig::default(),
                ProxyRequestConfig::default(),
                None,
                Some(infra_context),
            )
            .await,
        );
        router.app_state = Arc::new(app_state);
        UpstreamSource::update(
            router.source_id,
            router.provider_id,
            &UpdateUpstreamSourceData {
                base_url: None,
                use_proxy: Some(true),
                is_enabled: None,
                is_default: None,
                updated_at: chrono::Utc::now().timestamp_millis(),
                ..UpdateUpstreamSourceData::test_defaults()
            },
        )
        .expect("proxy requirement should be seeded directly");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("provider cache should invalidate");
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("proxy configuration response should read");
        let body: Value =
            serde_json::from_slice(&body).expect("proxy configuration response should be json");
        assert_eq!(
            downstream_error_code(&body, fixture.protocol),
            Some("provider_configuration_error")
        );
        assert_eq!(
            downstream_error_message(&body),
            Some("The gateway provider configuration is invalid.")
        );
        assert!(body.get("upstream_error").is_none());
        assert!(upstream.requests().await.is_empty());
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("provider_configuration_error")
        );
        assert_eq!(
            log.final_error_message.as_deref(),
            Some("provider requires the global proxy, but no proxy is configured")
        );
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_regression_client_cancellation_closes_upstream_and_logs_cancelled() {
    for (name, fixture) in generation_evidence_fixtures() {
        run_case(name, move |context| async move {
            let dropped = Arc::new(DropSignal::default());
            let upstream = TestUpstream::spawn(ScriptedReply::HangingSse {
                first_event: fixture.cancellation.first_upstream_event.clone(),
                dropped: Arc::clone(&dropped),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            router.attach_cost_catalog(Some(100), Some(2)).await;
            let persisted_sink = router.install_recording_persisted_sink();
            let response = router
                .send(&fixture, true, &fixture.cancellation.downstream_request)
                .await;
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{name}: cancellation stream status"
            );
            let request_id = assert_downstream_request_identity(&response);
            let mut body = response.into_body().into_data_stream();
            let first = timeout(WAIT_TIMEOUT, body.next())
                .await
                .expect("first downstream frame deadline")
                .expect("first downstream frame")
                .expect("first downstream frame should be readable");
            assert!(!first.is_empty(), "{name}: first downstream frame");
            drop(body);
            dropped.wait().await;
            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.cancellation.upstream_path,
                fixture.cancellation.upstream_query.as_deref(),
                &fixture.cancellation.upstream_request,
                &router.requested_model(),
                &request_id,
            );
            let log = router
                .wait_for_log(fixture.cancellation.expected_status.clone())
                .await;
            assert_eq!(
                log.request_id, request_id,
                "{name}: cancelled stream response and persisted canonical request id"
            );
            assert_log_common(&router, &fixture, &log);
            assert_log_timing_order(&log);
            assert_eq!(log.overall_status, RequestStatus::Cancelled);
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("client_cancelled_error")
            );
            assert!(
                log.final_error_message
                    .as_deref()
                    .is_some_and(|message| message.contains("Client disconnected")),
                "{name}: cancelled stream must persist a bounded operator diagnostic"
            );
            assert_stream_failure_has_no_usage_or_cost(&log, name);
            router.wait_for_api_key_lease_release().await;
            router.assert_no_api_key_usage_charge(name).await;
            assert_eq!(
                router.request_logs().await.len(),
                1,
                "{name}: cancelled stream must persist exactly one terminal request log"
            );
            assert_single_persisted_terminal_fact(
                &persisted_sink,
                ExecutionStage::DownstreamSend,
                ResponseVisibility::BodyStarted,
            )
            .await;
            assert_eq!(captured.len(), 1, "{name}: cancellation must not retry");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_regression_interrupted_stream_logs_same_request_id() {
    for (name, fixture) in openai_target_fixtures() {
        let case_name = format!("interrupted-stream-{name}");
        run_case(&case_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::InterruptedSse {
                first_event: fixture.cancellation.first_upstream_event.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let persisted_sink = router.install_recording_persisted_sink();
            let response = router
                .send(&fixture, true, &fixture.cancellation.downstream_request)
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{name}");
            let request_id = assert_downstream_request_identity(&response);
            let mut body = response.into_body().into_data_stream();
            let first = timeout(WAIT_TIMEOUT, body.next())
                .await
                .expect("first downstream frame deadline")
                .expect("first downstream frame")
                .expect("first downstream frame should be readable");
            assert!(!first.is_empty(), "{name}");
            loop {
                match timeout(WAIT_TIMEOUT, body.next())
                    .await
                    .expect("stream interruption deadline")
                {
                    Some(Ok(chunk)) => {
                        assert!(
                            !chunk.is_empty(),
                            "{name}: native terminal frames must not be empty"
                        );
                    }
                    Some(Err(_)) | None => break,
                }
            }

            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.request_id, request_id,
                "{name}: interrupted stream response and persisted canonical request id"
            );
            assert_log_common(&router, &fixture, &log);
            assert_eq!(log.overall_status, RequestStatus::Error, "{name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error"),
                "{name}"
            );
            assert_single_persisted_terminal_fact(
                &persisted_sink,
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::BodyStarted,
            )
            .await;
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{name}");
            assert_eq!(
                upstream.requests().await.len(),
                1,
                "{name}: partial stream must not retry or select another Source"
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_stream_interruption_before_first_chunk_is_headers_committed() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::InterruptedSseBeforeFirstEvent).await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let persisted_sink = router.install_recording_persisted_sink();

        let response = router
            .send(&fixture, true, &fixture.cancellation.downstream_request)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let request_id = assert_downstream_request_identity(&response);
        let first = timeout(WAIT_TIMEOUT, response.into_body().into_data_stream().next())
            .await
            .expect("pre-first-chunk interruption deadline")
            .expect("stream should yield an interruption");
        assert!(
            first.is_err(),
            "downstream body should fail before its first non-empty chunk"
        );

        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(log.request_id, request_id);
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("upstream_response_error")
        );
        assert_log_timing_order(&log);
        assert!(log.upstream_first_body_chunk_at.is_none());
        assert!(log.first_token_at.is_none());
        assert!(log.first_response_body_at.is_none());
        assert_single_persisted_terminal_fact(
            &persisted_sink,
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::HeadersCommitted,
        )
        .await;
        assert_eq!(upstream.requests().await.len(), 1);
        upstream.shutdown().await;
    });
}
