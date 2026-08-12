use std::{
    collections::{HashMap, HashSet},
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use cyder_api::{
    schema::enum_def::{DownstreamProtocol, UpstreamProfileType, UpstreamProtocol},
    service::upstream_profile::upstream_runtime_profile,
};
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 5;
const SOURCE_RELATIVE_PATH: &str = "docs/protocol-compatibility.yaml";
const GENERATED_RELATIVE_PATH: &str = "docs/protocol-compatibility.md";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityMatrix {
    schema_version: u32,
    downstream_protocols: Vec<DownstreamProtocol>,
    upstream_protocols: Vec<UpstreamProtocol>,
    downstream_error_contracts: DownstreamErrorContracts,
    evidence: Vec<Evidence>,
    transform_runtime_contract: TransformRuntimeContract,
    upstream_source_contract: UpstreamSourceContract,
    upstream_source_profiles: Vec<UpstreamSourceProfileContract>,
    openai_wire_profiles: Vec<OpenAiWireProfileContract>,
    routes: Vec<RouteContract>,
    generation_cells: Vec<GenerationCell>,
    utilities: Vec<UtilityContract>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DownstreamErrorContracts {
    scope: ErrorContractScope,
    router_rejections: Vec<RouterRejectionContract>,
    protocols: Vec<ProtocolErrorContract>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ErrorContractScope {
    before_headers_committed: bool,
    after_headers_committed_owners: Vec<String>,
    ollama_downstream_contract: ContractPresence,
    upstream_error_location: ExtensionLocation,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ContractPresence {
    Absent,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ExtensionLocation {
    TopLevel,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RouterRejectionContract {
    code: String,
    http_status: u16,
    required_header: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ProtocolErrorContract {
    downstream_protocol: DownstreamProtocol,
    envelope: ErrorEnvelope,
    stable_code_path: String,
    request_id_body_path: String,
    request_id_headers: Vec<String>,
    upstream_error_path: String,
    evidence: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ErrorEnvelope {
    OpenaiError,
    AnthropicError,
    GoogleRpcError,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    id: String,
    kind: EvidenceKind,
    reference: String,
    summary: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct TransformRuntimeContract {
    owner: String,
    same_wire_behavior: String,
    same_wire_observation_failure: String,
    cross_wire_pipeline: Vec<String>,
    cross_wire_failure_behavior: String,
    unknown_field_policy: UnknownFieldPolicy,
    loss_policy: TransformLossPolicy,
    header_boundary: TransformHeaderBoundary,
    diagnostics: TransformDiagnosticsContract,
    persistence: TransformPersistenceContract,
    advanced_cell_owners: Vec<TransformAdvancedCellOwner>,
    final_matrix_owner: String,
    evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct TransformLossPolicy {
    minor: String,
    major: String,
    deterministic_text_downgrade: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct UnknownFieldPolicy {
    ordinary_object_fields: String,
    unknown_tagged_semantics: String,
    registered_conflicts: String,
    full_second_schema_audit: bool,
    strict_target_profile_exception: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct TransformHeaderBoundary {
    before_commit: String,
    after_commit: String,
    normal_terminal_after_failure: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct TransformDiagnosticsContract {
    visibility: String,
    max_retained_facts: usize,
    overflow_accounted: bool,
    payload_free: bool,
    public_extensions: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct TransformPersistenceContract {
    enabled: bool,
    owner: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct TransformAdvancedCellOwner {
    upstream_protocol: UpstreamProtocol,
    owner: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum EvidenceKind {
    Test,
    Code,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpstreamSourceContract {
    owner: String,
    source_cardinality: String,
    source_identity: String,
    aggregate_field: String,
    source_state: String,
    default_semantics: String,
    profile_mutability: String,
    family_uniqueness: String,
    disabled_source_reserves_family: bool,
    deleted_source_releases_family: bool,
    selection_order: Vec<String>,
    no_fallback_after_selection: bool,
    credential_scope: String,
    credential_representation: String,
    model_scope: String,
    model_owner: String,
    request_patch_scope: String,
    reasoning_scope: String,
    config_owner: String,
    evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpstreamSourceProfileContract {
    profile_type: UpstreamProfileType,
    upstream_protocol: UpstreamProtocol,
    dialect: String,
    auth: String,
    endpoint: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct OpenAiWireProfileContract {
    profile_type: UpstreamProfileType,
    base_url_requirement: BaseUrlRequirement,
    default_base_url: Option<String>,
    base_url_customizable: bool,
    auth: String,
    chat: OpenAiWireOperationContract,
    embeddings: OpenAiWireOperationContract,
    rerank: OpenAiWireOperationContract,
    evidence: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BaseUrlRequirement {
    OptionalWithDefault,
    Required,
}

impl BaseUrlRequirement {
    const fn label(self) -> &'static str {
        match self {
            Self::OptionalWithDefault => "optional_with_default",
            Self::Required => "required",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct OpenAiWireOperationContract {
    availability: OperationAvailability,
    default_enabled: bool,
    field_policy: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum OperationAvailability {
    Configurable,
    Unsupported,
}

impl OperationAvailability {
    const fn label(self) -> &'static str {
        match self {
            Self::Configurable => "configurable",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RouteContract {
    downstream_protocol: DownstreamProtocol,
    public_prefix: String,
    versions: Vec<String>,
    unversioned_alias_target: String,
    endpoints: Vec<RouteEndpoint>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RouteEndpoint {
    path: String,
    method: String,
    kind: EndpointKind,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum EndpointKind {
    Generation,
    Utility,
    Local,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenerationCell {
    downstream: DownstreamProtocol,
    upstream: UpstreamProtocol,
    base: BaseDimensions,
    advanced: AdvancedDimensions,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BaseDimensions {
    non_stream_text: BaseAssessment,
    stream_text: BaseAssessment,
    usage: BaseAssessment,
    normal_termination: BaseAssessment,
    upstream_error: BaseAssessment,
    cancellation: BaseAssessment,
}

impl BaseDimensions {
    fn entries(&self) -> [(&'static str, &BaseAssessment); 6] {
        [
            ("non_stream_text", &self.non_stream_text),
            ("stream_text", &self.stream_text),
            ("usage", &self.usage),
            ("normal_termination", &self.normal_termination),
            ("upstream_error", &self.upstream_error),
            ("cancellation", &self.cancellation),
        ]
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdvancedDimensions {
    tools: AdvancedAssessment,
    reasoning: AdvancedAssessment,
    multimodal: AdvancedAssessment,
    structured_output: AdvancedAssessment,
}

impl AdvancedDimensions {
    fn entries(&self) -> [(&'static str, &AdvancedAssessment); 4] {
        [
            ("tools", &self.tools),
            ("reasoning", &self.reasoning),
            ("multimodal", &self.multimodal),
            ("structured_output", &self.structured_output),
        ]
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BaseAssessment {
    status: BaseStatus,
    evidence: Vec<String>,
    owner: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BaseStatus {
    Verified,
    Partial,
    Unavailable,
    NotVerified,
}

impl BaseStatus {
    const fn label(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Partial => "partial",
            Self::Unavailable => "unavailable",
            Self::NotVerified => "not_verified",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdvancedAssessment {
    status: AdvancedStatus,
    evidence: Vec<String>,
    owner: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AdvancedStatus {
    Full,
    ControlledLoss,
    ExplicitReject,
    NotVerified,
}

impl AdvancedStatus {
    const fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::ControlledLoss => "controlled_loss",
            Self::ExplicitReject => "explicit_reject",
            Self::NotVerified => "not_verified",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UtilityContract {
    name: String,
    route_suffix: String,
    method: String,
    exposed_on: Vec<DownstreamProtocol>,
    not_exposed_on: Vec<DownstreamProtocol>,
    execution: UtilityExecution,
    allowed_upstreams: Vec<UpstreamProtocol>,
    incompatible_upstream_behavior: IncompatibleUpstreamBehavior,
    verification: UtilityVerification,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum UtilityExecution {
    Local,
    Upstream,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum IncompatibleUpstreamBehavior {
    NotApplicable,
    PreSendReject,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UtilityVerification {
    status: UtilityVerificationStatus,
    evidence: Vec<String>,
    owner: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum UtilityVerificationStatus {
    Verified,
    Partial,
    NotVerified,
}

impl UtilityVerificationStatus {
    const fn label(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Partial => "partial",
            Self::NotVerified => "not_verified",
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Compatibility matrix error: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<String, String> {
    let mode = parse_mode(env::args().skip(1))?;
    let root = workspace_root()?;
    let source_path = root.join(SOURCE_RELATIVE_PATH);
    let generated_path = root.join(GENERATED_RELATIVE_PATH);
    let source = fs::read_to_string(&source_path)
        .map_err(|error| format!("read {}: {error}", source_path.display()))?;
    let matrix = parse_and_validate(&source)?;
    let rendered = render_markdown(&matrix);

    match mode {
        Mode::Write => {
            fs::write(&generated_path, rendered)
                .map_err(|error| format!("write {}: {error}", generated_path.display()))?;
            Ok(format!(
                "Wrote {} from {}.",
                GENERATED_RELATIVE_PATH, SOURCE_RELATIVE_PATH
            ))
        }
        Mode::Check => {
            let actual = fs::read_to_string(&generated_path)
                .map_err(|error| format!("read {}: {error}", generated_path.display()))?;
            verify_rendered(&rendered, &actual)?;
            Ok(format!(
                "{} is valid and {} is current.",
                SOURCE_RELATIVE_PATH, GENERATED_RELATIVE_PATH
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Write,
    Check,
}

fn parse_mode(args: impl Iterator<Item = String>) -> Result<Mode, String> {
    let args = args.collect::<Vec<_>>();
    match args.as_slice() {
        [arg] if arg == "--write" => Ok(Mode::Write),
        [arg] if arg == "--check" => Ok(Mode::Check),
        _ => Err("specify exactly one of --write or --check".to_string()),
    }
}

fn workspace_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "server manifest directory has no workspace parent".to_string())
}

fn parse_and_validate(source: &str) -> Result<CompatibilityMatrix, String> {
    let matrix = serde_yaml::from_str::<CompatibilityMatrix>(source)
        .map_err(|error| format!("parse {SOURCE_RELATIVE_PATH}: {error}"))?;
    validate_matrix(&matrix)?;
    Ok(matrix)
}

fn validate_matrix(matrix: &CompatibilityMatrix) -> Result<(), String> {
    if matrix.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "schema_version must be {SCHEMA_VERSION}, found {}",
            matrix.schema_version
        ));
    }
    if matrix.downstream_protocols != DownstreamProtocol::ALL {
        return Err(
            "downstream_protocols must declare the exact four runtime protocols".to_string(),
        );
    }
    if matrix.upstream_protocols != UpstreamProtocol::ALL {
        return Err("upstream_protocols must declare the exact five runtime protocols".to_string());
    }

    let evidence = validate_evidence(&matrix.evidence)?;
    validate_r316_executable_evidence(&evidence)?;
    validate_r317_executable_evidence(&evidence)?;
    validate_downstream_error_contracts(&matrix.downstream_error_contracts, &evidence)?;
    validate_transform_runtime_contract(&matrix.transform_runtime_contract, &evidence)?;
    validate_upstream_source_contract(&matrix.upstream_source_contract, &evidence)?;
    validate_upstream_source_profiles(&matrix.upstream_source_profiles)?;
    validate_openai_wire_profiles(&matrix.openai_wire_profiles, &evidence)?;
    validate_routes(&matrix.routes)?;
    validate_generation_cells(&matrix.generation_cells, &evidence)?;
    validate_utilities(&matrix.utilities, &evidence)?;
    Ok(())
}

const REQUIRED_TRANSFORM_RUNTIME_EVIDENCE: [(&str, &str); 6] = [
    (
        "r3-15-transform-quality-contract",
        "service::transform::quality::tests::test_transform_contract_summary_covers_failures_outcomes_and_accounting",
    ),
    (
        "r3-15-transform-payload-free-contract",
        "service::transform::quality::tests::test_transform_contract_report_omits_payload_and_safe_summary",
    ),
    (
        "r3-15-same-wire-passthrough",
        "proxy::direct_execution_regression::same_wire_non_stream_observation_failure_preserves_upstream_bytes",
    ),
    (
        "r3-15-minor-loss-runtime",
        "proxy::direct_execution_regression::cross_wire_minor_loss_succeeds_once_and_drops_only_audited_metadata",
    ),
    (
        "r3-15-four-protocol-stream-failure",
        "proxy::direct_execution_regression::four_public_protocols_emit_one_native_terminal_on_cross_wire_stream_decode_failure",
    ),
    (
        "r3-15-target-stream-failure",
        "proxy::direct_execution_regression::cross_wire_target_stream_rejection_emits_one_native_terminal_and_releases_resources",
    ),
];

const REQUIRED_R316_EXECUTABLE_EVIDENCE: [(&str, &str); 27] = [
    (
        "r3-16-reasoning-direct",
        "proxy::direct_execution_regression::all_public_downstream_reasoning_controls_reach_the_openai_target",
    ),
    (
        "r3-16-reasoning-responses-diagnostic",
        "service::transform::facade::tests::responses_reasoning_effort_maps_to_openai_and_summary_is_safely_ignored",
    ),
    (
        "r3-16-reasoning-anthropic-diagnostic",
        "service::transform::facade::tests::anthropic_qualitative_reasoning_maps_to_openai_without_injecting_cot",
    ),
    (
        "r3-16-reasoning-gemini-diagnostic",
        "service::transform::facade::tests::gemini_reasoning_controls_map_to_openai_with_budget_sentinels",
    ),
    (
        "r3-16-reasoning-conflict-reject",
        "proxy::direct_execution_regression::registered_request_conflicts_are_rejected_before_credential_or_upstream_use",
    ),
    (
        "r3-16-reasoning-responses-reject",
        "proxy::direct_execution_regression::malformed_responses_reasoning_is_rejected_before_credential_or_upstream_use",
    ),
    (
        "r3-16-multimodal-direct",
        "proxy::direct_execution_regression::all_public_downstream_multimodal_inputs_reach_the_openai_target",
    ),
    (
        "r3-16-multimodal-responses-diagnostic",
        "service::transform::facade::tests::responses_multimodal_input_maps_to_openai_image_audio_and_file_parts",
    ),
    (
        "r3-16-multimodal-anthropic-diagnostic",
        "service::transform::facade::tests::anthropic_multimodal_input_maps_to_openai_without_textualizing_payloads",
    ),
    (
        "r3-16-multimodal-gemini-diagnostic",
        "service::transform::facade::tests::gemini_multimodal_input_classifies_inline_media_for_openai",
    ),
    (
        "r3-16-multimodal-reject",
        "proxy::direct_execution_regression::all_public_downstreams_reject_unportable_media_before_credentials",
    ),
    (
        "r3-16-structured-direct",
        "proxy::direct_execution_regression::all_public_downstream_structured_outputs_reach_the_openai_target",
    ),
    (
        "r3-16-structured-responses-transform",
        "service::transform::facade::tests::responses_structured_outputs_preserve_json_object_and_schema_contracts",
    ),
    (
        "r3-16-structured-anthropic-diagnostic",
        "service::transform::facade::tests::anthropic_structured_output_synthesizes_a_stable_openai_name",
    ),
    (
        "r3-16-structured-gemini-diagnostic",
        "service::transform::facade::tests::gemini_structured_output_preserves_constraints_and_drops_only_property_ordering",
    ),
    (
        "r3-16-structured-reject",
        "proxy::direct_execution_regression::all_public_downstreams_reject_unrepresentable_structured_outputs_before_credentials",
    ),
    (
        "r3-16-tools-direct",
        "proxy::direct_execution_regression::all_public_downstream_portable_tool_lifecycles_reach_the_openai_target",
    ),
    (
        "r3-16-tools-cross-wire-transform",
        "service::transform::facade::tests::portable_tool_controls_from_each_cross_wire_protocol_reach_openai",
    ),
    (
        "r3-16-tools-stable-results",
        "service::transform::facade::tests::missing_tool_call_ids_are_stable_and_structured_results_are_canonical_text",
    ),
    (
        "r3-16-tools-reject",
        "proxy::direct_execution_regression::forced_nonportable_cross_wire_tools_reject_before_credentials",
    ),
    (
        "r3-16-profile-field-policy",
        "service::transform::providers::openai::target::tests::profiles_apply_distinct_unknown_field_policies",
    ),
    (
        "r3-16-profile-source-contract",
        "database::migration_smoke_tests::sqlite_r316_destructive_upgrade_enforces_openai_upstream_contract",
    ),
    (
        "r3-16-gemini-openai-closed-policy",
        "proxy::direct_execution_regression::gemini_openai_profile_rejects_unknown_and_conflicting_chat_fields_before_credentials",
    ),
    (
        "r3-16-embeddings-direct",
        "proxy::direct_execution_regression::embeddings_execute_once_for_each_openai_wire_profile_and_preserve_the_response",
    ),
    (
        "r3-16-embeddings-reject",
        "proxy::direct_execution_regression::invalid_embeddings_requests_are_rejected_before_credentials_and_network",
    ),
    (
        "r3-16-rerank-direct",
        "proxy::direct_execution_regression::compatible_rerank_is_a_single_call_transparent_transport_without_private_usage_parsing",
    ),
    (
        "r3-16-rerank-profile-guard",
        "proxy::direct_execution_regression::rerank_requires_an_enabled_compatible_source_before_credential_decryption",
    ),
];

fn validate_r316_executable_evidence(evidence: &HashMap<&str, &Evidence>) -> Result<(), String> {
    for (id, reference) in REQUIRED_R316_EXECUTABLE_EVIDENCE {
        let item = evidence
            .get(id)
            .ok_or_else(|| format!("R3.16 requires executable evidence '{id}'"))?;
        if item.kind != EvidenceKind::Test || item.reference != reference {
            return Err(format!(
                "R3.16 evidence '{id}' must reference stable automated test '{reference}'"
            ));
        }
    }
    Ok(())
}

const REQUIRED_R317_EXECUTABLE_EVIDENCE: [(&str, &str); 36] = [
    (
        "r3-17-fixture-scope",
        "proxy::direct_execution_regression::responses_target_fixtures_define_four_complete_protocols_and_native_evidence",
    ),
    (
        "r3-17-stateless-policy",
        "proxy::direct_execution_regression::responses_stateful_controls_are_rejected_before_credential_or_upstream_use",
    ),
    (
        "r3-17-materializer",
        "proxy::direct_execution_regression::responses_target_materializes_native_requests_for_all_public_downstreams_and_modes",
    ),
    (
        "r3-17-source-check",
        "controller::provider::tests::responses_source_check_saved_and_draft_keys_share_native_contract_without_proxy_logs",
    ),
    (
        "r3-17-openai-base",
        "proxy::direct_execution_regression::openai_to_responses_base_cell_success_is_verified",
    ),
    (
        "r3-17-responses-base",
        "proxy::direct_execution_regression::responses_to_responses_base_cell_success_is_verified",
    ),
    (
        "r3-17-anthropic-base",
        "proxy::direct_execution_regression::anthropic_to_responses_base_cell_success_is_verified",
    ),
    (
        "r3-17-gemini-base",
        "proxy::direct_execution_regression::gemini_to_responses_base_cell_success_is_verified",
    ),
    (
        "r3-17-http-error",
        "proxy::direct_execution_regression::responses_target_http_429_is_authentic_bounded_and_never_retried_for_all_downstreams",
    ),
    (
        "r3-17-cancellation",
        "proxy::direct_execution_regression::responses_target_precommit_client_cancellation_returns_499_and_releases_once",
    ),
    (
        "r3-17-terminal-cost",
        "proxy::direct_execution_regression::responses_non_stream_incomplete_is_billed_but_failed_and_illegal_finals_are_not",
    ),
    (
        "r3-17-stream-terminal-error",
        "proxy::direct_execution_regression::responses_stream_failed_and_error_events_emit_one_terminal_without_cost_for_all_downstreams",
    ),
    (
        "r3-17-stream-eof",
        "proxy::direct_execution_regression::responses_stream_eof_without_terminal_fails_closed_for_all_downstreams",
    ),
    (
        "r3-17-body-limit",
        "proxy::direct_execution_regression::responses_target_success_body_limits_fail_without_cost_for_all_downstreams",
    ),
    (
        "r3-17-route-boundary",
        "proxy::direct_execution_regression::responses_stateful_resource_paths_are_unregistered_and_have_zero_side_effects",
    ),
    (
        "r3-17-models-boundary",
        "proxy::direct_execution_regression::models_routes_apply_static_kind_and_source_operation_filters_without_exposing_kind",
    ),
    (
        "r3-17-openai-tools-cell",
        "proxy::direct_execution_regression::openai_to_responses_tools_cell_is_full",
    ),
    (
        "r3-17-responses-tools-cell",
        "proxy::direct_execution_regression::responses_to_responses_tools_cell_is_full",
    ),
    (
        "r3-17-anthropic-tools-cell",
        "proxy::direct_execution_regression::anthropic_to_responses_tools_cell_is_full",
    ),
    (
        "r3-17-gemini-tools-cell",
        "proxy::direct_execution_regression::gemini_to_responses_tools_cell_has_typed_controlled_loss",
    ),
    (
        "r3-17-openai-reasoning-cell",
        "proxy::direct_execution_regression::openai_to_responses_reasoning_cell_has_typed_controlled_loss",
    ),
    (
        "r3-17-responses-reasoning-cell",
        "proxy::direct_execution_regression::responses_to_responses_reasoning_cell_is_full",
    ),
    (
        "r3-17-anthropic-reasoning-cell",
        "proxy::direct_execution_regression::anthropic_to_responses_reasoning_cell_has_typed_controlled_loss",
    ),
    (
        "r3-17-gemini-reasoning-cell",
        "proxy::direct_execution_regression::gemini_to_responses_reasoning_cell_has_typed_controlled_loss",
    ),
    (
        "r3-17-openai-multimodal-cell",
        "proxy::direct_execution_regression::openai_to_responses_multimodal_cell_is_full",
    ),
    (
        "r3-17-responses-multimodal-cell",
        "proxy::direct_execution_regression::responses_to_responses_multimodal_cell_is_full",
    ),
    (
        "r3-17-anthropic-multimodal-cell",
        "proxy::direct_execution_regression::anthropic_to_responses_multimodal_cell_is_full",
    ),
    (
        "r3-17-gemini-multimodal-cell",
        "proxy::direct_execution_regression::gemini_to_responses_multimodal_cell_has_typed_controlled_loss",
    ),
    (
        "r3-17-openai-structured-cell",
        "proxy::direct_execution_regression::openai_to_responses_structured_output_cell_is_full",
    ),
    (
        "r3-17-responses-structured-cell",
        "proxy::direct_execution_regression::responses_to_responses_structured_output_cell_is_full",
    ),
    (
        "r3-17-anthropic-structured-cell",
        "proxy::direct_execution_regression::anthropic_to_responses_structured_output_cell_has_typed_controlled_loss",
    ),
    (
        "r3-17-gemini-structured-cell",
        "proxy::direct_execution_regression::gemini_to_responses_structured_output_cell_has_typed_controlled_loss",
    ),
    (
        "r3-17-tools-reject",
        "proxy::direct_execution_regression::responses_target_rejects_invalid_or_forced_nonportable_tools_before_credentials",
    ),
    (
        "r3-17-reasoning-reject",
        "proxy::direct_execution_regression::responses_target_rejects_reasoning_conflicts_before_credentials",
    ),
    (
        "r3-17-multimodal-reject",
        "proxy::direct_execution_regression::responses_target_rejects_all_unportable_media_before_credentials",
    ),
    (
        "r3-17-structured-reject",
        "proxy::direct_execution_regression::responses_target_rejects_invalid_structured_outputs_before_credentials",
    ),
];

fn validate_r317_executable_evidence(evidence: &HashMap<&str, &Evidence>) -> Result<(), String> {
    for (id, reference) in REQUIRED_R317_EXECUTABLE_EVIDENCE {
        let item = evidence
            .get(id)
            .ok_or_else(|| format!("R3.17 requires executable evidence '{id}'"))?;
        if item.kind != EvidenceKind::Test || item.reference != reference {
            return Err(format!(
                "R3.17 evidence '{id}' must reference stable automated test '{reference}'"
            ));
        }
    }
    Ok(())
}

fn validate_transform_runtime_contract(
    contract: &TransformRuntimeContract,
    evidence: &HashMap<&str, &Evidence>,
) -> Result<(), String> {
    if contract != &expected_transform_runtime_contract() {
        return Err(
            "transform_runtime_contract must pin the R3.15 passthrough, fail-closed, loss, header-boundary, internal-only diagnostic, R4.6 persistence, and R3.16-R3.21 owner contract"
                .to_string(),
        );
    }
    for (id, reference) in REQUIRED_TRANSFORM_RUNTIME_EVIDENCE {
        let item = evidence
            .get(id)
            .ok_or_else(|| format!("transform_runtime_contract requires evidence '{id}'"))?;
        if item.kind != EvidenceKind::Test || item.reference != reference {
            return Err(format!(
                "transform runtime evidence '{id}' must reference stable automated test '{reference}'"
            ));
        }
    }
    Ok(())
}

fn expected_transform_runtime_contract() -> TransformRuntimeContract {
    TransformRuntimeContract {
        owner: "R3.15".to_string(),
        same_wire_behavior: "byte_preserving_passthrough".to_string(),
        same_wire_observation_failure: "observation_degraded".to_string(),
        cross_wire_pipeline: vec![
            "source_decode".to_string(),
            "unified_ir".to_string(),
            "target_encode".to_string(),
        ],
        cross_wire_failure_behavior: "fail_closed".to_string(),
        unknown_field_policy: UnknownFieldPolicy {
            ordinary_object_fields: "serde_default_silent_ignore".to_string(),
            unknown_tagged_semantics: "pre_send_explicit_reject".to_string(),
            registered_conflicts: "targeted_borrowed_value_check".to_string(),
            full_second_schema_audit: false,
            strict_target_profile_exception: "gemini_openai_recursive_closed_allowlist".to_string(),
        },
        loss_policy: TransformLossPolicy {
            minor: "controlled_loss_with_internal_fact".to_string(),
            major: "explicit_reject".to_string(),
            deterministic_text_downgrade: "fixture_backed_controlled_loss".to_string(),
        },
        header_boundary: TransformHeaderBoundary {
            before_commit: "downstream_native_error_envelope".to_string(),
            after_commit: "single_downstream_native_error_terminal_or_body_error".to_string(),
            normal_terminal_after_failure: false,
        },
        diagnostics: TransformDiagnosticsContract {
            visibility: "internal_only".to_string(),
            max_retained_facts: 32,
            overflow_accounted: true,
            payload_free: true,
            public_extensions: false,
        },
        persistence: TransformPersistenceContract {
            enabled: false,
            owner: "R4.6".to_string(),
        },
        advanced_cell_owners: UpstreamProtocol::ALL
            .into_iter()
            .filter(|upstream_protocol| {
                !matches!(
                    upstream_protocol,
                    UpstreamProtocol::Openai | UpstreamProtocol::Responses
                )
            })
            .map(|upstream_protocol| TransformAdvancedCellOwner {
                upstream_protocol,
                owner: generation_owner(upstream_protocol).to_string(),
            })
            .collect(),
        final_matrix_owner: "R3.21".to_string(),
        evidence: REQUIRED_TRANSFORM_RUNTIME_EVIDENCE
            .iter()
            .map(|(id, _)| (*id).to_string())
            .collect(),
    }
}

const REQUIRED_ERROR_EVIDENCE: [(&str, &str); 8] = [
    (
        "error-contract-unit",
        "proxy::error::response::tests::protocol_error_contracts_cover_all_116_proxy_and_8_router_combinations",
    ),
    (
        "error-contract-golden",
        "proxy::error_contract_regression::four_downstream_error_contracts_match_golden_fixtures",
    ),
    (
        "error-contract-router",
        "proxy::error_contract_regression::router_and_ingress_rejections_use_protocol_contracts",
    ),
    (
        "response-limit-raw-four-protocol",
        "proxy::direct_execution_regression::four_public_protocols_use_existing_envelopes_for_non_stream_response_limit",
    ),
    (
        "response-limit-decoded-four-protocol",
        "proxy::direct_execution_regression::four_public_protocols_use_existing_envelopes_for_decoded_response_limit",
    ),
    (
        "response-encoding-four-protocol",
        "proxy::direct_execution_regression::four_public_protocols_reject_sse_encoding_before_headers",
    ),
    (
        "stream-resource-postcommit-four-protocol",
        "proxy::direct_execution_regression::four_public_protocols_terminate_body_on_sse_parser_failure",
    ),
    (
        "provider-error-hard-limit-four-protocol",
        "proxy::direct_execution_regression::four_public_protocols_preserve_bounded_provider_error_when_body_reaches_hard_limit",
    ),
];

fn validate_downstream_error_contracts(
    contracts: &DownstreamErrorContracts,
    evidence: &HashMap<&str, &Evidence>,
) -> Result<(), String> {
    let expected = expected_downstream_error_contracts();
    if contracts.scope != expected.scope {
        return Err(
            "downstream error scope must pin pre-commit handling, stream owners, absent Ollama downstream, and top-level upstream_error"
                .to_string(),
        );
    }
    if contracts.router_rejections != expected.router_rejections {
        return Err(
            "downstream error router rejections must exactly pin 404, 405, and Allow".to_string(),
        );
    }
    if contracts.protocols.len() != DownstreamProtocol::ALL.len() {
        return Err(format!(
            "downstream error contracts must contain exactly {} protocols, found {}",
            DownstreamProtocol::ALL.len(),
            contracts.protocols.len()
        ));
    }
    for (index, (contract, expected_contract)) in contracts
        .protocols
        .iter()
        .zip(expected.protocols.iter())
        .enumerate()
    {
        if contract.downstream_protocol != DownstreamProtocol::ALL[index] {
            return Err(
                "downstream error protocols must follow DownstreamProtocol::ALL with no duplicates"
                    .to_string(),
            );
        }
        if contract != expected_contract {
            return Err(format!(
                "downstream error contract for {:?} has an invalid envelope, path, header, or evidence list",
                contract.downstream_protocol
            ));
        }
    }

    for (id, reference) in REQUIRED_ERROR_EVIDENCE {
        let item = evidence
            .get(id)
            .ok_or_else(|| format!("downstream error contracts require evidence '{id}'"))?;
        if item.kind != EvidenceKind::Test || item.reference != reference {
            return Err(format!(
                "downstream error evidence '{id}' must reference stable automated test '{reference}'"
            ));
        }
    }
    Ok(())
}

fn expected_downstream_error_contracts() -> DownstreamErrorContracts {
    let evidence = vec![
        "error-contract-unit".to_string(),
        "error-contract-golden".to_string(),
        "error-contract-router".to_string(),
        "response-limit-raw-four-protocol".to_string(),
        "response-limit-decoded-four-protocol".to_string(),
        "response-encoding-four-protocol".to_string(),
        "stream-resource-postcommit-four-protocol".to_string(),
        "provider-error-hard-limit-four-protocol".to_string(),
    ];
    DownstreamErrorContracts {
        scope: ErrorContractScope {
            before_headers_committed: true,
            after_headers_committed_owners: vec![
                "R3.7".to_string(),
                "R3.8".to_string(),
                "R3.15-R3.21".to_string(),
            ],
            ollama_downstream_contract: ContractPresence::Absent,
            upstream_error_location: ExtensionLocation::TopLevel,
        },
        router_rejections: vec![
            RouterRejectionContract {
                code: "route_not_found_error".to_string(),
                http_status: 404,
                required_header: None,
            },
            RouterRejectionContract {
                code: "method_not_allowed_error".to_string(),
                http_status: 405,
                required_header: Some("Allow".to_string()),
            },
        ],
        protocols: vec![
            ProtocolErrorContract {
                downstream_protocol: DownstreamProtocol::Openai,
                envelope: ErrorEnvelope::OpenaiError,
                stable_code_path: "error.code".to_string(),
                request_id_body_path: "absent".to_string(),
                request_id_headers: vec!["x-request-id".to_string()],
                upstream_error_path: "upstream_error".to_string(),
                evidence: evidence.clone(),
            },
            ProtocolErrorContract {
                downstream_protocol: DownstreamProtocol::Responses,
                envelope: ErrorEnvelope::OpenaiError,
                stable_code_path: "error.code".to_string(),
                request_id_body_path: "absent".to_string(),
                request_id_headers: vec!["x-request-id".to_string()],
                upstream_error_path: "upstream_error".to_string(),
                evidence: evidence.clone(),
            },
            ProtocolErrorContract {
                downstream_protocol: DownstreamProtocol::Anthropic,
                envelope: ErrorEnvelope::AnthropicError,
                stable_code_path: "error.code".to_string(),
                request_id_body_path: "request_id".to_string(),
                request_id_headers: vec!["x-request-id".to_string(), "request-id".to_string()],
                upstream_error_path: "upstream_error".to_string(),
                evidence: evidence.clone(),
            },
            ProtocolErrorContract {
                downstream_protocol: DownstreamProtocol::Gemini,
                envelope: ErrorEnvelope::GoogleRpcError,
                stable_code_path: "error.details.google_rpc_error_info.metadata.cyder_code"
                    .to_string(),
                request_id_body_path: "error.details.google_rpc_error_info.metadata.request_id"
                    .to_string(),
                request_id_headers: vec!["x-request-id".to_string()],
                upstream_error_path: "upstream_error".to_string(),
                evidence,
            },
        ],
    }
}

fn validate_evidence(items: &[Evidence]) -> Result<HashMap<&str, &Evidence>, String> {
    let mut indexed = HashMap::new();
    for item in items {
        if item.id.trim().is_empty()
            || item.reference.trim().is_empty()
            || item.summary.trim().is_empty()
        {
            return Err("evidence id, reference, and summary must be non-empty".to_string());
        }
        if indexed.insert(item.id.as_str(), item).is_some() {
            return Err(format!("duplicate evidence id '{}'", item.id));
        }
    }
    if indexed.is_empty() {
        return Err("at least one evidence record is required".to_string());
    }
    Ok(indexed)
}

fn validate_upstream_source_contract(
    contract: &UpstreamSourceContract,
    evidence: &HashMap<&str, &Evidence>,
) -> Result<(), String> {
    const REQUIRED_EVIDENCE: [&str; 10] = [
        "r3-10-source-aggregate",
        "r3-10-source-repository",
        "r3-10-source-selector",
        "r3-10-manager-source-contract",
        "r3-10-runtime-source-contract",
        "r3-10-migration-contract",
        "r3-11-source-selector",
        "r3-11-model-source-config",
        "r3-11-source-impact-and-check",
        "r3-11-request-log-reason",
    ];
    const REQUIRED_R312_EVIDENCE: [&str; 3] = [
        "r3-12-request-patch-contract",
        "r3-12-request-patch-execution",
        "r3-12-request-patch-migration",
    ];
    if contract.owner != "R3.12"
        || contract.source_cardinality != "zero_to_many"
        || contract.source_identity != "source_id_only"
        || contract.aggregate_field != "upstream_sources"
        || contract.source_state != "explicit_enabled_and_default"
        || contract.default_semantics != "optional"
        || contract.profile_mutability != "create_only"
        || contract.family_uniqueness != "one_active_source_per_wire_family"
        || !contract.disabled_source_reserves_family
        || !contract.deleted_source_releases_family
        || contract.selection_order
            != [
                "protocol_match".to_string(),
                "inherit_all_provider_default_transform".to_string(),
                "explicit_model_default_transform".to_string(),
            ]
        || !contract.no_fallback_after_selection
        || contract.credential_scope != "provider"
        || contract.credential_representation != "opaque"
        || contract.model_scope != "model_source_selection_mode_and_visible_bindings"
        || contract.model_owner != "R3.11"
        || contract.request_patch_scope != "source_bound_variant"
        || contract.reasoning_scope != "protocol_transform_only"
        || contract.config_owner != "R3.12"
        || contract.evidence.len() != REQUIRED_EVIDENCE.len() + REQUIRED_R312_EVIDENCE.len()
        || contract.evidence[..REQUIRED_EVIDENCE.len()] != REQUIRED_EVIDENCE
        || contract.evidence[REQUIRED_EVIDENCE.len()..] != REQUIRED_R312_EVIDENCE
    {
        return Err(
            "upstream_source_contract must pin the R3.12 Source-bound Variant scope while preserving the R3.10 zero-to-many Source and R3.11 model-scoped selector boundary"
                .to_string(),
        );
    }
    for id in REQUIRED_EVIDENCE {
        if !evidence.contains_key(id) {
            return Err(format!("upstream_source_contract requires evidence '{id}'"));
        }
    }
    Ok(())
}

fn validate_upstream_source_profiles(
    profiles: &[UpstreamSourceProfileContract],
) -> Result<(), String> {
    if profiles.len() != UpstreamProfileType::ALL.len() {
        return Err(format!(
            "upstream_source_profiles must contain {} entries, found {}",
            UpstreamProfileType::ALL.len(),
            profiles.len()
        ));
    }
    let mut seen = HashSet::new();
    for profile in profiles {
        if !seen.insert(profile.profile_type) {
            return Err(format!(
                "duplicate upstream Source profile for {:?}",
                profile.profile_type
            ));
        }
        let runtime = upstream_runtime_profile(&profile.profile_type);
        if profile.upstream_protocol != runtime.upstream_protocol
            || profile.dialect != runtime.dialect.as_key()
            || profile.auth != runtime.auth.as_key()
            || profile.endpoint != runtime.endpoint.as_key()
        {
            return Err(format!(
                "upstream Source profile for {:?} differs from upstream_runtime_profile",
                profile.profile_type
            ));
        }
    }
    if seen != UpstreamProfileType::ALL.into_iter().collect() {
        return Err("upstream_source_profiles do not exhaust UpstreamProfileType::ALL".to_string());
    }
    Ok(())
}

fn validate_openai_wire_profiles(
    profiles: &[OpenAiWireProfileContract],
    evidence: &HashMap<&str, &Evidence>,
) -> Result<(), String> {
    let expected = expected_openai_wire_profiles();
    if profiles != expected {
        return Err(
            "openai_wire_profiles must exactly pin OPENAI, OPENAI_COMPATIBLE, and GEMINI_OPENAI Base URL, auth, operation availability/defaults, and field policies"
                .to_string(),
        );
    }
    for profile in profiles {
        for id in &profile.evidence {
            let item = evidence.get(id.as_str()).ok_or_else(|| {
                format!(
                    "OpenAI-wire Profile {:?} references unknown evidence '{id}'",
                    profile.profile_type
                )
            })?;
            if item.kind != EvidenceKind::Test {
                return Err(format!(
                    "OpenAI-wire Profile {:?} evidence '{id}' must be an executable test",
                    profile.profile_type
                ));
            }
        }
    }
    Ok(())
}

fn expected_openai_wire_profiles() -> Vec<OpenAiWireProfileContract> {
    vec![
        OpenAiWireProfileContract {
            profile_type: UpstreamProfileType::Openai,
            base_url_requirement: BaseUrlRequirement::OptionalWithDefault,
            default_base_url: Some("https://api.openai.com/v1".to_string()),
            base_url_customizable: true,
            auth: "bearer_api_key".to_string(),
            chat: openai_wire_operation(
                OperationAvailability::Configurable,
                true,
                "official_fields_validated_unknown_extensions_passthrough",
            ),
            embeddings: openai_wire_operation(
                OperationAvailability::Configurable,
                true,
                "official_fields_validated_unknown_extensions_passthrough",
            ),
            rerank: openai_wire_operation(OperationAvailability::Unsupported, false, "unsupported"),
            evidence: vec![
                "r3-16-profile-field-policy".to_string(),
                "r3-16-profile-source-contract".to_string(),
                "r3-16-embeddings-direct".to_string(),
                "r3-16-rerank-profile-guard".to_string(),
            ],
        },
        OpenAiWireProfileContract {
            profile_type: UpstreamProfileType::OpenaiCompatible,
            base_url_requirement: BaseUrlRequirement::Required,
            default_base_url: None,
            base_url_customizable: true,
            auth: "bearer_api_key".to_string(),
            chat: openai_wire_operation(
                OperationAvailability::Configurable,
                true,
                "core_fields_validated_vendor_extensions_passthrough",
            ),
            embeddings: openai_wire_operation(
                OperationAvailability::Configurable,
                false,
                "core_fields_validated_vendor_extensions_passthrough",
            ),
            rerank: openai_wire_operation(
                OperationAvailability::Configurable,
                false,
                "opaque_envelope_passthrough",
            ),
            evidence: vec![
                "r3-16-profile-field-policy".to_string(),
                "r3-16-profile-source-contract".to_string(),
                "r3-16-embeddings-direct".to_string(),
                "r3-16-rerank-direct".to_string(),
            ],
        },
        OpenAiWireProfileContract {
            profile_type: UpstreamProfileType::GeminiOpenai,
            base_url_requirement: BaseUrlRequirement::OptionalWithDefault,
            default_base_url: Some(
                "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
            ),
            base_url_customizable: true,
            auth: "bearer_api_key".to_string(),
            chat: openai_wire_operation(
                OperationAvailability::Configurable,
                true,
                "recursive_closed_allowlist",
            ),
            embeddings: openai_wire_operation(
                OperationAvailability::Configurable,
                true,
                "model_input_closed_contract",
            ),
            rerank: openai_wire_operation(OperationAvailability::Unsupported, false, "unsupported"),
            evidence: vec![
                "r3-16-profile-field-policy".to_string(),
                "r3-16-profile-source-contract".to_string(),
                "r3-16-gemini-openai-closed-policy".to_string(),
                "r3-16-embeddings-direct".to_string(),
                "r3-16-rerank-profile-guard".to_string(),
            ],
        },
    ]
}

fn openai_wire_operation(
    availability: OperationAvailability,
    default_enabled: bool,
    field_policy: &str,
) -> OpenAiWireOperationContract {
    OpenAiWireOperationContract {
        availability,
        default_enabled,
        field_policy: field_policy.to_string(),
    }
}

fn validate_routes(routes: &[RouteContract]) -> Result<(), String> {
    if routes != expected_routes() {
        return Err(
            "routes must exactly match the four public prefixes, aliases, methods, and endpoints"
                .to_string(),
        );
    }
    Ok(())
}

fn expected_routes() -> Vec<RouteContract> {
    vec![
        RouteContract {
            downstream_protocol: DownstreamProtocol::Openai,
            public_prefix: "/ai/openai".to_string(),
            versions: vec!["unversioned".to_string(), "v1".to_string()],
            unversioned_alias_target: "current_v1_semantics".to_string(),
            endpoints: vec![
                endpoint("/chat/completions", "POST", EndpointKind::Generation),
                endpoint("/embeddings", "POST", EndpointKind::Utility),
                endpoint("/rerank", "POST", EndpointKind::Utility),
                endpoint("/models", "GET", EndpointKind::Local),
            ],
        },
        RouteContract {
            downstream_protocol: DownstreamProtocol::Responses,
            public_prefix: "/ai/responses".to_string(),
            versions: vec!["unversioned".to_string(), "v1".to_string()],
            unversioned_alias_target: "current_v1_semantics".to_string(),
            endpoints: vec![
                endpoint("/responses", "POST", EndpointKind::Generation),
                endpoint("/models", "GET", EndpointKind::Local),
            ],
        },
        RouteContract {
            downstream_protocol: DownstreamProtocol::Anthropic,
            public_prefix: "/ai/anthropic".to_string(),
            versions: vec!["unversioned".to_string(), "v1".to_string()],
            unversioned_alias_target: "current_v1_semantics".to_string(),
            endpoints: vec![
                endpoint("/messages", "POST", EndpointKind::Generation),
                endpoint("/models", "GET", EndpointKind::Local),
            ],
        },
        RouteContract {
            downstream_protocol: DownstreamProtocol::Gemini,
            public_prefix: "/ai/gemini".to_string(),
            versions: vec![
                "unversioned".to_string(),
                "v1".to_string(),
                "v1beta".to_string(),
            ],
            unversioned_alias_target: "current_v1_semantics".to_string(),
            endpoints: vec![
                endpoint(
                    "/models/{model}:generateContent",
                    "POST",
                    EndpointKind::Generation,
                ),
                endpoint(
                    "/models/{model}:streamGenerateContent",
                    "POST",
                    EndpointKind::Generation,
                ),
                endpoint("/models/{model}:countTokens", "POST", EndpointKind::Utility),
                endpoint("/models", "GET", EndpointKind::Local),
            ],
        },
    ]
}

fn endpoint(path: &str, method: &str, kind: EndpointKind) -> RouteEndpoint {
    RouteEndpoint {
        path: path.to_string(),
        method: method.to_string(),
        kind,
    }
}

fn validate_generation_cells(
    cells: &[GenerationCell],
    evidence: &HashMap<&str, &Evidence>,
) -> Result<(), String> {
    let expected_count = DownstreamProtocol::ALL.len() * UpstreamProtocol::ALL.len();
    if cells.len() != expected_count {
        return Err(format!(
            "generation_cells must contain exactly 4x5={expected_count} entries, found {}",
            cells.len()
        ));
    }

    let mut seen = HashSet::new();
    for cell in cells {
        if !seen.insert((cell.downstream, cell.upstream)) {
            return Err(format!(
                "duplicate generation cell {:?}->{:?}",
                cell.downstream, cell.upstream
            ));
        }
        let owner = generation_owner(cell.upstream);
        for (dimension, assessment) in cell.base.entries() {
            validate_assessment_evidence(
                &format!("{:?}->{:?} {dimension}", cell.downstream, cell.upstream),
                assessment.status == BaseStatus::Verified,
                &assessment.evidence,
                assessment.owner.as_deref(),
                Some(owner),
                evidence,
            )?;
        }
        for (dimension, assessment) in cell.advanced.entries() {
            validate_assessment_evidence(
                &format!("{:?}->{:?} {dimension}", cell.downstream, cell.upstream),
                assessment.status != AdvancedStatus::NotVerified,
                &assessment.evidence,
                assessment.owner.as_deref(),
                Some(owner),
                evidence,
            )?;
            if assessment.status == AdvancedStatus::ExplicitReject
                && !assessment.evidence.iter().any(|id| {
                    evidence
                        .get(id.as_str())
                        .is_some_and(|item| item.kind == EvidenceKind::Test)
                })
            {
                return Err(format!(
                    "{:?}->{:?} {dimension} explicit_reject requires automated test evidence",
                    cell.downstream, cell.upstream
                ));
            }
        }
        validate_initial_generation_truth(cell)?;
    }

    for downstream in DownstreamProtocol::ALL {
        for upstream in UpstreamProtocol::ALL {
            if !seen.contains(&(downstream, upstream)) {
                return Err(format!(
                    "missing generation cell {:?}->{:?}",
                    downstream, upstream
                ));
            }
        }
    }
    Ok(())
}

fn validate_initial_generation_truth(cell: &GenerationCell) -> Result<(), String> {
    let verified_cell = cell.upstream == UpstreamProtocol::Responses
        || matches!(
            (cell.downstream, cell.upstream),
            (DownstreamProtocol::Openai, UpstreamProtocol::Openai)
                | (DownstreamProtocol::Responses, UpstreamProtocol::Openai)
                | (DownstreamProtocol::Anthropic, UpstreamProtocol::Openai)
                | (DownstreamProtocol::Gemini, UpstreamProtocol::Openai)
                | (DownstreamProtocol::Gemini, UpstreamProtocol::Gemini)
        );
    let base_statuses = cell.base.entries().map(|(_, assessment)| assessment.status);
    if verified_cell {
        if !base_statuses
            .into_iter()
            .all(|status| status == BaseStatus::Verified)
        {
            return Err(format!(
                "representative cell {:?}->{:?} must have all six base dimensions verified",
                cell.downstream, cell.upstream
            ));
        }
    } else if base_statuses
        .into_iter()
        .any(|status| status == BaseStatus::Verified)
    {
        return Err(format!(
            "cell {:?}->{:?} lacks direct-execution evidence and cannot be verified",
            cell.downstream, cell.upstream
        ));
    }

    if cell.upstream == UpstreamProtocol::Anthropic
        && !cell
            .base
            .entries()
            .into_iter()
            .all(|(_, assessment)| assessment.status == BaseStatus::Unavailable)
    {
        return Err(format!(
            "native {:?} materialization is unavailable in every base dimension",
            cell.upstream
        ));
    }
    if cell.upstream == UpstreamProtocol::Openai {
        for (dimension, assessment) in cell.advanced.entries() {
            let expected_status = expected_openai_advanced_status(cell.downstream, dimension);
            let expected_evidence = expected_openai_advanced_evidence(cell.downstream, dimension);
            if assessment.status != expected_status
                || assessment
                    .evidence
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    != expected_evidence
            {
                return Err(format!(
                    "R3.16 advanced cell {:?}->OpenAI {dimension} must be {:?} with its exact executable main/diagnostic/rejection evidence",
                    cell.downstream, expected_status
                ));
            }
        }
    } else if cell.upstream == UpstreamProtocol::Responses {
        for (dimension, assessment) in cell.base.entries() {
            let expected_evidence = expected_responses_base_evidence(cell.downstream, dimension);
            if assessment.status != BaseStatus::Verified
                || assessment
                    .evidence
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    != expected_evidence
            {
                return Err(format!(
                    "R3.17 base cell {:?}->Responses {dimension} must be verified with its exact direct-execution evidence",
                    cell.downstream
                ));
            }
        }
        for (dimension, assessment) in cell.advanced.entries() {
            let expected_status = expected_responses_advanced_status(cell.downstream, dimension);
            let expected_evidence =
                expected_responses_advanced_evidence(cell.downstream, dimension);
            if assessment.status != expected_status
                || assessment
                    .evidence
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    != expected_evidence
            {
                return Err(format!(
                    "R3.17 advanced cell {:?}->Responses {dimension} must be {:?} with its exact executable main/diagnostic/rejection evidence",
                    cell.downstream, expected_status
                ));
            }
        }
    } else if !cell
        .advanced
        .entries()
        .into_iter()
        .all(|(_, assessment)| assessment.status == AdvancedStatus::NotVerified)
    {
        return Err(format!(
            "advanced dimensions outside the completed OpenAI and Responses upstream columns for {:?}->{:?} must remain not_verified",
            cell.downstream, cell.upstream
        ));
    }
    Ok(())
}

fn expected_responses_base_evidence(
    downstream: DownstreamProtocol,
    dimension: &str,
) -> Vec<&'static str> {
    let success = match downstream {
        DownstreamProtocol::Openai => "r3-17-openai-base",
        DownstreamProtocol::Responses => "r3-17-responses-base",
        DownstreamProtocol::Anthropic => "r3-17-anthropic-base",
        DownstreamProtocol::Gemini => "r3-17-gemini-base",
    };
    match dimension {
        "non_stream_text" | "stream_text" => vec![success, "r3-17-materializer"],
        "usage" | "normal_termination" => vec![success, "r3-17-terminal-cost"],
        "upstream_error" => vec![
            "r3-17-http-error",
            "r3-17-stream-terminal-error",
            "r3-17-stream-eof",
            "r3-17-body-limit",
        ],
        "cancellation" => vec!["r3-17-cancellation"],
        _ => unreachable!("base dimensions are closed"),
    }
}

fn expected_responses_advanced_status(
    downstream: DownstreamProtocol,
    dimension: &str,
) -> AdvancedStatus {
    match (downstream, dimension) {
        (DownstreamProtocol::Openai, "tools" | "multimodal" | "structured_output")
        | (DownstreamProtocol::Responses, _)
        | (DownstreamProtocol::Anthropic, "tools" | "multimodal") => AdvancedStatus::Full,
        _ => AdvancedStatus::ControlledLoss,
    }
}

fn expected_responses_advanced_evidence(
    downstream: DownstreamProtocol,
    dimension: &str,
) -> Vec<&'static str> {
    let cell = match (downstream, dimension) {
        (DownstreamProtocol::Openai, "tools") => "r3-17-openai-tools-cell",
        (DownstreamProtocol::Responses, "tools") => "r3-17-responses-tools-cell",
        (DownstreamProtocol::Anthropic, "tools") => "r3-17-anthropic-tools-cell",
        (DownstreamProtocol::Gemini, "tools") => "r3-17-gemini-tools-cell",
        (DownstreamProtocol::Openai, "reasoning") => "r3-17-openai-reasoning-cell",
        (DownstreamProtocol::Responses, "reasoning") => "r3-17-responses-reasoning-cell",
        (DownstreamProtocol::Anthropic, "reasoning") => "r3-17-anthropic-reasoning-cell",
        (DownstreamProtocol::Gemini, "reasoning") => "r3-17-gemini-reasoning-cell",
        (DownstreamProtocol::Openai, "multimodal") => "r3-17-openai-multimodal-cell",
        (DownstreamProtocol::Responses, "multimodal") => "r3-17-responses-multimodal-cell",
        (DownstreamProtocol::Anthropic, "multimodal") => "r3-17-anthropic-multimodal-cell",
        (DownstreamProtocol::Gemini, "multimodal") => "r3-17-gemini-multimodal-cell",
        (DownstreamProtocol::Openai, "structured_output") => "r3-17-openai-structured-cell",
        (DownstreamProtocol::Responses, "structured_output") => "r3-17-responses-structured-cell",
        (DownstreamProtocol::Anthropic, "structured_output") => "r3-17-anthropic-structured-cell",
        (DownstreamProtocol::Gemini, "structured_output") => "r3-17-gemini-structured-cell",
        _ => unreachable!("advanced dimensions are a closed four-by-four matrix"),
    };
    if expected_responses_advanced_status(downstream, dimension) == AdvancedStatus::ControlledLoss {
        let rejection = match dimension {
            "tools" => "r3-17-tools-reject",
            "reasoning" => "r3-17-reasoning-reject",
            "multimodal" => "r3-17-multimodal-reject",
            "structured_output" => "r3-17-structured-reject",
            _ => unreachable!("advanced dimensions are closed"),
        };
        vec![cell, rejection]
    } else {
        vec![cell]
    }
}

fn expected_openai_advanced_status(
    downstream: DownstreamProtocol,
    dimension: &str,
) -> AdvancedStatus {
    match (downstream, dimension) {
        (DownstreamProtocol::Openai, _) => AdvancedStatus::Full,
        (DownstreamProtocol::Responses, "structured_output") => AdvancedStatus::Full,
        _ => AdvancedStatus::ControlledLoss,
    }
}

fn expected_openai_advanced_evidence(
    downstream: DownstreamProtocol,
    dimension: &str,
) -> Vec<&'static str> {
    match (downstream, dimension) {
        (DownstreamProtocol::Openai, "tools") => vec!["r3-16-tools-direct"],
        (DownstreamProtocol::Openai, "reasoning") => vec!["r3-16-reasoning-direct"],
        (DownstreamProtocol::Openai, "multimodal") => vec!["r3-16-multimodal-direct"],
        (DownstreamProtocol::Openai, "structured_output") => {
            vec!["r3-16-structured-direct"]
        }
        (DownstreamProtocol::Responses, "tools")
        | (DownstreamProtocol::Anthropic, "tools")
        | (DownstreamProtocol::Gemini, "tools") => vec![
            "r3-16-tools-direct",
            "r3-16-tools-cross-wire-transform",
            "r3-16-tools-stable-results",
            "r3-16-tools-reject",
        ],
        (DownstreamProtocol::Responses, "reasoning") => vec![
            "r3-16-reasoning-direct",
            "r3-16-reasoning-responses-diagnostic",
            "r3-16-reasoning-responses-reject",
        ],
        (DownstreamProtocol::Anthropic, "reasoning") => vec![
            "r3-16-reasoning-direct",
            "r3-16-reasoning-anthropic-diagnostic",
        ],
        (DownstreamProtocol::Gemini, "reasoning") => vec![
            "r3-16-reasoning-direct",
            "r3-16-reasoning-gemini-diagnostic",
            "r3-16-reasoning-conflict-reject",
        ],
        (DownstreamProtocol::Responses, "multimodal") => vec![
            "r3-16-multimodal-direct",
            "r3-16-multimodal-responses-diagnostic",
            "r3-16-multimodal-reject",
        ],
        (DownstreamProtocol::Anthropic, "multimodal") => vec![
            "r3-16-multimodal-direct",
            "r3-16-multimodal-anthropic-diagnostic",
            "r3-16-multimodal-reject",
        ],
        (DownstreamProtocol::Gemini, "multimodal") => vec![
            "r3-16-multimodal-direct",
            "r3-16-multimodal-gemini-diagnostic",
            "r3-16-multimodal-reject",
        ],
        (DownstreamProtocol::Responses, "structured_output") => vec![
            "r3-16-structured-direct",
            "r3-16-structured-responses-transform",
        ],
        (DownstreamProtocol::Anthropic, "structured_output") => vec![
            "r3-16-structured-direct",
            "r3-16-structured-anthropic-diagnostic",
            "r3-16-structured-reject",
        ],
        (DownstreamProtocol::Gemini, "structured_output") => vec![
            "r3-16-structured-direct",
            "r3-16-structured-gemini-diagnostic",
            "r3-16-structured-reject",
        ],
        _ => unreachable!("advanced dimensions are a closed four-by-four matrix"),
    }
}

fn generation_owner(upstream: UpstreamProtocol) -> &'static str {
    match upstream {
        UpstreamProtocol::Openai => "R3.16",
        UpstreamProtocol::Responses => "R3.17",
        UpstreamProtocol::Anthropic => "R3.18",
        UpstreamProtocol::Gemini => "R3.19",
        UpstreamProtocol::Ollama => "R3.20",
    }
}

fn utility_owner(utility: &UtilityContract) -> &'static str {
    match utility.allowed_upstreams.first().copied() {
        Some(UpstreamProtocol::Openai) => "R3.16",
        Some(UpstreamProtocol::Responses) => "R3.17",
        Some(UpstreamProtocol::Anthropic) => "R3.18",
        Some(UpstreamProtocol::Gemini) => "R3.19",
        Some(UpstreamProtocol::Ollama) => "R3.20",
        None => "R3.21",
    }
}

fn validate_assessment_evidence(
    label: &str,
    is_complete: bool,
    ids: &[String],
    owner: Option<&str>,
    expected_owner: Option<&str>,
    evidence: &HashMap<&str, &Evidence>,
) -> Result<(), String> {
    if ids.is_empty() {
        return Err(format!("{label} must reference evidence"));
    }
    for id in ids {
        if !evidence.contains_key(id.as_str()) {
            return Err(format!("{label} references unknown evidence '{id}'"));
        }
    }
    if is_complete {
        if owner.is_some() {
            return Err(format!("{label} is complete and must not have an owner"));
        }
        if !ids.iter().any(|id| {
            evidence
                .get(id.as_str())
                .is_some_and(|item| item.kind == EvidenceKind::Test)
        }) {
            return Err(format!("{label} complete status requires test evidence"));
        }
    } else if owner != expected_owner {
        return Err(format!(
            "{label} must be owned by {}, found {:?}",
            expected_owner.unwrap_or("an explicit owner"),
            owner
        ));
    }
    Ok(())
}

fn validate_utilities(
    utilities: &[UtilityContract],
    evidence: &HashMap<&str, &Evidence>,
) -> Result<(), String> {
    let expected = expected_utility_shapes();
    if utilities.len() != expected.len() {
        return Err(format!(
            "utilities must contain {} entries, found {}",
            expected.len(),
            utilities.len()
        ));
    }
    let mut names = HashSet::new();
    for utility in utilities {
        if !names.insert(utility.name.as_str()) {
            return Err(format!("duplicate utility '{}'", utility.name));
        }
        let exposed = utility.exposed_on.iter().copied().collect::<HashSet<_>>();
        let not_exposed = utility
            .not_exposed_on
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        if exposed.len() != utility.exposed_on.len()
            || not_exposed.len() != utility.not_exposed_on.len()
            || !exposed.is_disjoint(&not_exposed)
            || exposed.union(&not_exposed).copied().collect::<HashSet<_>>()
                != DownstreamProtocol::ALL.into_iter().collect()
        {
            return Err(format!(
                "utility '{}' exposed_on and not_exposed_on must partition all downstream protocols",
                utility.name
            ));
        }
        let shape = UtilityShape::from(utility);
        if !expected.contains(&shape) {
            return Err(format!(
                "utility '{}' differs from the runtime utility contract",
                utility.name
            ));
        }
        let is_complete = utility.verification.status == UtilityVerificationStatus::Verified;
        validate_assessment_evidence(
            &format!("utility {} verification", utility.name),
            is_complete,
            &utility.verification.evidence,
            utility.verification.owner.as_deref(),
            if is_complete {
                None
            } else {
                Some(utility_owner(utility))
            },
            evidence,
        )?;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct UtilityShape {
    name: String,
    route_suffix: String,
    method: String,
    exposed_on: Vec<DownstreamProtocol>,
    not_exposed_on: Vec<DownstreamProtocol>,
    execution: UtilityExecution,
    allowed_upstreams: Vec<UpstreamProtocol>,
    incompatible_upstream_behavior: IncompatibleUpstreamBehavior,
    verification_status: UtilityVerificationStatus,
}

impl From<&UtilityContract> for UtilityShape {
    fn from(value: &UtilityContract) -> Self {
        Self {
            name: value.name.clone(),
            route_suffix: value.route_suffix.clone(),
            method: value.method.clone(),
            exposed_on: value.exposed_on.clone(),
            not_exposed_on: value.not_exposed_on.clone(),
            execution: value.execution,
            allowed_upstreams: value.allowed_upstreams.clone(),
            incompatible_upstream_behavior: value.incompatible_upstream_behavior,
            verification_status: value.verification.status,
        }
    }
}

fn expected_utility_shapes() -> Vec<UtilityShape> {
    vec![
        UtilityShape {
            name: "models".to_string(),
            route_suffix: "/models".to_string(),
            method: "GET".to_string(),
            exposed_on: DownstreamProtocol::ALL.to_vec(),
            not_exposed_on: vec![],
            execution: UtilityExecution::Local,
            allowed_upstreams: vec![],
            incompatible_upstream_behavior: IncompatibleUpstreamBehavior::NotApplicable,
            verification_status: UtilityVerificationStatus::Verified,
        },
        UtilityShape {
            name: "embeddings".to_string(),
            route_suffix: "/embeddings".to_string(),
            method: "POST".to_string(),
            exposed_on: vec![DownstreamProtocol::Openai],
            not_exposed_on: vec![
                DownstreamProtocol::Responses,
                DownstreamProtocol::Anthropic,
                DownstreamProtocol::Gemini,
            ],
            execution: UtilityExecution::Upstream,
            allowed_upstreams: vec![UpstreamProtocol::Openai],
            incompatible_upstream_behavior: IncompatibleUpstreamBehavior::PreSendReject,
            verification_status: UtilityVerificationStatus::Verified,
        },
        UtilityShape {
            name: "rerank".to_string(),
            route_suffix: "/rerank".to_string(),
            method: "POST".to_string(),
            exposed_on: vec![DownstreamProtocol::Openai],
            not_exposed_on: vec![
                DownstreamProtocol::Responses,
                DownstreamProtocol::Anthropic,
                DownstreamProtocol::Gemini,
            ],
            execution: UtilityExecution::Upstream,
            allowed_upstreams: vec![UpstreamProtocol::Openai],
            incompatible_upstream_behavior: IncompatibleUpstreamBehavior::PreSendReject,
            verification_status: UtilityVerificationStatus::Verified,
        },
        UtilityShape {
            name: "countTokens".to_string(),
            route_suffix: "/models/{model}:countTokens".to_string(),
            method: "POST".to_string(),
            exposed_on: vec![DownstreamProtocol::Gemini],
            not_exposed_on: vec![
                DownstreamProtocol::Openai,
                DownstreamProtocol::Responses,
                DownstreamProtocol::Anthropic,
            ],
            execution: UtilityExecution::Upstream,
            allowed_upstreams: vec![UpstreamProtocol::Gemini],
            incompatible_upstream_behavior: IncompatibleUpstreamBehavior::PreSendReject,
            verification_status: UtilityVerificationStatus::Partial,
        },
    ]
}

fn render_markdown(matrix: &CompatibilityMatrix) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "<!-- GENERATED FILE. DO NOT EDIT. Canonical source: `{SOURCE_RELATIVE_PATH}`. -->"
    )
    .unwrap();
    writeln!(output, "# Protocol Compatibility\n").unwrap();
    writeln!(
        output,
        "This document reports current, evidence-backed runtime behavior. It is generated deterministically by `cargo run -p cyder-api --bin compatibility_matrix -- --write` from `{SOURCE_RELATIVE_PATH}`."
    )
    .unwrap();
    writeln!(
        output,
        "\nUnversioned downstream routes are compatibility aliases for the current `/v1` semantics. They are not a protocol-version claim; a future `/v2` may become the alias target without removing the unversioned route."
    )
    .unwrap();

    writeln!(output, "\n## Protocol boundaries\n").unwrap();
    writeln!(output, "- Matrix schema: v{}", matrix.schema_version).unwrap();
    writeln!(
        output,
        "- Downstream: {}",
        join_downstream(&matrix.downstream_protocols)
    )
    .unwrap();
    writeln!(
        output,
        "- Upstream: {}",
        join_upstream(&matrix.upstream_protocols)
    )
    .unwrap();
    writeln!(
        output,
        "- Ollama is an upstream-only protocol and has no public downstream router."
    )
    .unwrap();

    let error_contracts = &matrix.downstream_error_contracts;
    writeln!(output, "\n## Downstream error contracts\n").unwrap();
    writeln!(
        output,
        "- Scope: HTTP error envelopes apply before response headers are committed: `{}`.",
        error_contracts.scope.before_headers_committed
    )
    .unwrap();
    writeln!(
        output,
        "- After headers are committed, stream/error ownership remains with {}.",
        error_contracts
            .scope
            .after_headers_committed_owners
            .join(", ")
    )
    .unwrap();
    writeln!(
        output,
        "- Ollama downstream contract: `{}`; Provider error extension location: `{}`.",
        contract_presence_label(error_contracts.scope.ollama_downstream_contract),
        extension_location_label(error_contracts.scope.upstream_error_location)
    )
    .unwrap();
    writeln!(
        output,
        "- Every pre-commit error is JSON with `X-Request-ID`, `Cache-Control: no-store`, and `X-Content-Type-Options: nosniff`; Anthropic also returns `request-id`."
    )
    .unwrap();
    writeln!(
        output,
        "- OpenAI, Responses, and Anthropic 401 responses use `WWW-Authenticate: Bearer`; Gemini does not. `Retry-After` appears only when an exact producer fact exists."
    )
    .unwrap();

    writeln!(output, "\n### Protocol envelopes\n").unwrap();
    writeln!(
        output,
        "| Protocol | Envelope | Stable code path | Request ID body path | Request ID headers | Upstream error path | Evidence |"
    )
    .unwrap();
    writeln!(output, "| --- | --- | --- | --- | --- | --- | --- |").unwrap();
    for contract in &error_contracts.protocols {
        writeln!(
            output,
            "| {} | `{}` | `{}` | `{}` | {} | `{}` | {} |",
            downstream_label(contract.downstream_protocol),
            error_envelope_label(contract.envelope),
            contract.stable_code_path,
            contract.request_id_body_path,
            contract
                .request_id_headers
                .iter()
                .map(|header| format!("`{header}`"))
                .collect::<Vec<_>>()
                .join(", "),
            contract.upstream_error_path,
            contract.evidence.join(", ")
        )
        .unwrap();
    }

    writeln!(output, "\n### Router rejections\n").unwrap();
    writeln!(output, "| Stable code | HTTP status | Required header |").unwrap();
    writeln!(output, "| --- | --- | --- |").unwrap();
    for rejection in &error_contracts.router_rejections {
        writeln!(
            output,
            "| `{}` | {} | {} |",
            rejection.code,
            rejection.http_status,
            rejection.required_header.as_deref().unwrap_or("—")
        )
        .unwrap();
    }

    let transform_contract = &matrix.transform_runtime_contract;
    writeln!(output, "\n## Transform runtime contract\n").unwrap();
    writeln!(
        output,
        "- Owner: `{}`. Same-wire behavior is `{}`; observation failure is `{}`.",
        transform_contract.owner,
        transform_contract.same_wire_behavior,
        transform_contract.same_wire_observation_failure
    )
    .unwrap();
    writeln!(
        output,
        "- Cross-wire pipeline: `{}`; any pipeline failure is `{}`.",
        transform_contract.cross_wire_pipeline.join(" -> "),
        transform_contract.cross_wire_failure_behavior
    )
    .unwrap();
    writeln!(
        output,
        "- Unknown-field policy: ordinary object fields `{}`; unknown tagged semantics `{}`; registered conflicts `{}`; full second schema audit `{}`; strict target exception `{}`.",
        transform_contract
            .unknown_field_policy
            .ordinary_object_fields,
        transform_contract
            .unknown_field_policy
            .unknown_tagged_semantics,
        transform_contract.unknown_field_policy.registered_conflicts,
        transform_contract
            .unknown_field_policy
            .full_second_schema_audit,
        transform_contract
            .unknown_field_policy
            .strict_target_profile_exception
    )
    .unwrap();
    writeln!(
        output,
        "- Loss policy: minor `{}`, major `{}`, deterministic text downgrade `{}`.",
        transform_contract.loss_policy.minor,
        transform_contract.loss_policy.major,
        transform_contract.loss_policy.deterministic_text_downgrade
    )
    .unwrap();
    writeln!(
        output,
        "- Header boundary: before commit `{}`; after commit `{}`; normal terminal after failure `{}`.",
        transform_contract.header_boundary.before_commit,
        transform_contract.header_boundary.after_commit,
        transform_contract
            .header_boundary
            .normal_terminal_after_failure
    )
    .unwrap();
    writeln!(
        output,
        "- Diagnostics: visibility `{}`, retained fact cap `{}`, overflow accounted `{}`, payload-free `{}`, public extensions `{}`.",
        transform_contract.diagnostics.visibility,
        transform_contract.diagnostics.max_retained_facts,
        transform_contract.diagnostics.overflow_accounted,
        transform_contract.diagnostics.payload_free,
        transform_contract.diagnostics.public_extensions
    )
    .unwrap();
    writeln!(
        output,
        "- Persistence active in R3.15: `{}`; persistence owner `{}`; final matrix owner `{}`.",
        transform_contract.persistence.enabled,
        transform_contract.persistence.owner,
        transform_contract.final_matrix_owner
    )
    .unwrap();
    writeln!(output, "\n### Advanced cell owners\n").unwrap();
    writeln!(output, "| Upstream protocol | Owner |").unwrap();
    writeln!(output, "| --- | --- |").unwrap();
    for owner in &transform_contract.advanced_cell_owners {
        writeln!(
            output,
            "| {} | `{}` |",
            upstream_label(owner.upstream_protocol),
            owner.owner
        )
        .unwrap();
    }
    writeln!(
        output,
        "\nEvidence: {}.",
        transform_contract.evidence.join(", ")
    )
    .unwrap();

    let source_contract = &matrix.upstream_source_contract;
    writeln!(output, "\n## Upstream Source contract\n").unwrap();
    writeln!(
        output,
        "- Owner: `{}`; aggregate field `{}` has `{}` cardinality and `{}` identity. Source state is `{}`, default semantics are `{}`, and Profile mutability is `{}`.",
        source_contract.owner,
        source_contract.aggregate_field,
        source_contract.source_cardinality,
        source_contract.source_identity,
        source_contract.source_state,
        source_contract.default_semantics,
        source_contract.profile_mutability
    )
    .unwrap();
    writeln!(
        output,
        "- Family uniqueness is `{}`; disabled Sources reserve family capacity: `{}`; deleted Sources release it: `{}`.",
        source_contract.family_uniqueness,
        source_contract.disabled_source_reserves_family,
        source_contract.deleted_source_releases_family
    )
    .unwrap();
    writeln!(
        output,
        "- Selection order: `{}`; no fallback after selection: `{}`. Credentials remain `{}` scoped and `{}` representation; models use `{}`.",
        source_contract.selection_order.join(" -> "),
        source_contract.no_fallback_after_selection,
        source_contract.credential_scope,
        source_contract.credential_representation,
        source_contract.model_scope
    )
    .unwrap();
    writeln!(
        output,
        "- Model owner: `{}`; Request Patch scope: `{}`; Reasoning scope: `{}`; configuration owner: `{}`.",
        source_contract.model_owner,
        source_contract.request_patch_scope,
        source_contract.reasoning_scope,
        source_contract.config_owner
    )
    .unwrap();
    writeln!(
        output,
        "- Evidence: {}.",
        source_contract.evidence.join(", ")
    )
    .unwrap();

    writeln!(output, "\n## Upstream Source profiles\n").unwrap();
    writeln!(
        output,
        "| Profile type | Upstream protocol | Dialect | Auth | Endpoint |"
    )
    .unwrap();
    writeln!(output, "| --- | --- | --- | --- | --- |").unwrap();
    for profile in &matrix.upstream_source_profiles {
        writeln!(
            output,
            "| {} | {} | `{}` | `{}` | `{}` |",
            profile_label(profile.profile_type),
            upstream_label(profile.upstream_protocol),
            profile.dialect,
            profile.auth,
            profile.endpoint
        )
        .unwrap();
    }

    writeln!(output, "\n## OpenAI-wire Profile contracts\n").unwrap();
    writeln!(
        output,
        "| Profile | Base URL requirement | Default Base URL | Customizable | Auth | Chat | Chat field policy | Embeddings | Embeddings field policy | Rerank | Rerank field policy | Evidence |"
    )
    .unwrap();
    writeln!(
        output,
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    )
    .unwrap();
    for profile in &matrix.openai_wire_profiles {
        writeln!(
            output,
            "| {} | `{}` | {} | `{}` | `{}` | {} | `{}` | {} | `{}` | {} | `{}` | {} |",
            profile_label(profile.profile_type),
            profile.base_url_requirement.label(),
            profile.default_base_url.as_deref().unwrap_or("—"),
            profile.base_url_customizable,
            profile.auth,
            operation_contract_label(&profile.chat),
            profile.chat.field_policy,
            operation_contract_label(&profile.embeddings),
            profile.embeddings.field_policy,
            operation_contract_label(&profile.rerank),
            profile.rerank.field_policy,
            profile.evidence.join(", ")
        )
        .unwrap();
    }

    writeln!(output, "\n## Public downstream routes\n").unwrap();
    writeln!(
        output,
        "| Protocol | Prefix | Versions | Endpoint | Method | Kind |"
    )
    .unwrap();
    writeln!(output, "| --- | --- | --- | --- | --- | --- |").unwrap();
    for route in &matrix.routes {
        for endpoint in &route.endpoints {
            writeln!(
                output,
                "| {} | `{}` | {} | `{}` | {} | {} |",
                downstream_label(route.downstream_protocol),
                route.public_prefix,
                route.versions.join(", "),
                endpoint.path,
                endpoint.method,
                endpoint_kind_label(endpoint.kind)
            )
            .unwrap();
        }
    }

    writeln!(output, "\n## Generation base dimensions\n").unwrap();
    writeln!(
        output,
        "| Downstream | Upstream | Non-stream text | Stream text | Usage | Normal termination | Upstream error | Cancellation | Owner | Evidence |"
    )
    .unwrap();
    writeln!(
        output,
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    )
    .unwrap();
    for cell in &matrix.generation_cells {
        let owners = cell
            .base
            .entries()
            .into_iter()
            .filter_map(|(_, assessment)| assessment.owner.as_deref())
            .collect::<HashSet<_>>();
        let evidence_ids = cell
            .base
            .entries()
            .into_iter()
            .flat_map(|(_, assessment)| assessment.evidence.iter().map(String::as_str))
            .collect::<HashSet<_>>();
        writeln!(
            output,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            downstream_label(cell.downstream),
            upstream_label(cell.upstream),
            cell.base.non_stream_text.status.label(),
            cell.base.stream_text.status.label(),
            cell.base.usage.status.label(),
            cell.base.normal_termination.status.label(),
            cell.base.upstream_error.status.label(),
            cell.base.cancellation.status.label(),
            sorted_join(owners),
            sorted_join(evidence_ids)
        )
        .unwrap();
    }

    writeln!(output, "\n## Generation advanced dimensions\n").unwrap();
    writeln!(
        output,
        "| Downstream | Upstream | Tools | Reasoning | Multimodal | Structured output | Owner | Evidence |"
    )
    .unwrap();
    writeln!(output, "| --- | --- | --- | --- | --- | --- | --- | --- |").unwrap();
    for cell in &matrix.generation_cells {
        let owners = cell
            .advanced
            .entries()
            .into_iter()
            .filter_map(|(_, assessment)| assessment.owner.as_deref())
            .collect::<HashSet<_>>();
        let evidence_ids = cell
            .advanced
            .entries()
            .into_iter()
            .flat_map(|(_, assessment)| assessment.evidence.iter().map(String::as_str))
            .collect::<HashSet<_>>();
        writeln!(
            output,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            downstream_label(cell.downstream),
            upstream_label(cell.upstream),
            cell.advanced.tools.status.label(),
            cell.advanced.reasoning.status.label(),
            cell.advanced.multimodal.status.label(),
            cell.advanced.structured_output.status.label(),
            sorted_join(owners),
            sorted_join(evidence_ids)
        )
        .unwrap();
    }

    writeln!(output, "\n## Utility contracts\n").unwrap();
    writeln!(
        output,
        "| Utility | Route suffix | Method | Exposed on | Not exposed on | Execution | Allowed upstreams | Incompatible upstream | Verification | Owner | Evidence |"
    )
    .unwrap();
    writeln!(
        output,
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    )
    .unwrap();
    for utility in &matrix.utilities {
        writeln!(
            output,
            "| {} | `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            utility.name,
            utility.route_suffix,
            utility.method,
            join_downstream(&utility.exposed_on),
            join_downstream(&utility.not_exposed_on),
            utility_execution_label(utility.execution),
            join_upstream(&utility.allowed_upstreams),
            incompatible_behavior_label(utility.incompatible_upstream_behavior),
            utility.verification.status.label(),
            utility.verification.owner.as_deref().unwrap_or("—"),
            utility.verification.evidence.join(", ")
        )
        .unwrap();
    }

    writeln!(output, "\n## Evidence registry\n").unwrap();
    writeln!(output, "| ID | Kind | Reference | Summary |").unwrap();
    writeln!(output, "| --- | --- | --- | --- |").unwrap();
    for item in &matrix.evidence {
        writeln!(
            output,
            "| `{}` | {} | `{}` | {} |",
            item.id,
            evidence_kind_label(item.kind),
            item.reference.replace('|', "\\|"),
            item.summary.replace('|', "\\|")
        )
        .unwrap();
    }

    writeln!(output, "\n## Status semantics\n").unwrap();
    writeln!(
        output,
        "- Base: `verified`, `partial`, `unavailable`, `not_verified`."
    )
    .unwrap();
    writeln!(
        output,
        "- Advanced: `full`, `controlled_loss`, `explicit_reject`, `not_verified`."
    )
    .unwrap();
    writeln!(
        output,
        "- `verified` requires direct execution through routing, materialization, transport, downstream output, and call-count assertions. Transform-only evidence is insufficient."
    )
    .unwrap();
    output
}

fn sorted_join(values: HashSet<&str>) -> String {
    if values.is_empty() {
        return "—".to_string();
    }
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort_unstable();
    values.join(", ")
}

fn join_downstream(values: &[DownstreamProtocol]) -> String {
    if values.is_empty() {
        return "—".to_string();
    }
    values
        .iter()
        .map(|value| downstream_label(*value))
        .collect::<Vec<_>>()
        .join(", ")
}

fn join_upstream(values: &[UpstreamProtocol]) -> String {
    if values.is_empty() {
        return "—".to_string();
    }
    values
        .iter()
        .map(|value| upstream_label(*value))
        .collect::<Vec<_>>()
        .join(", ")
}

const fn downstream_label(value: DownstreamProtocol) -> &'static str {
    match value {
        DownstreamProtocol::Openai => "OpenAI",
        DownstreamProtocol::Responses => "Responses",
        DownstreamProtocol::Anthropic => "Anthropic",
        DownstreamProtocol::Gemini => "Gemini",
    }
}

const fn upstream_label(value: UpstreamProtocol) -> &'static str {
    match value {
        UpstreamProtocol::Openai => "OpenAI",
        UpstreamProtocol::Responses => "Responses",
        UpstreamProtocol::Anthropic => "Anthropic",
        UpstreamProtocol::Gemini => "Gemini",
        UpstreamProtocol::Ollama => "Ollama",
    }
}

const fn profile_label(value: UpstreamProfileType) -> &'static str {
    match value {
        UpstreamProfileType::Openai => "OpenAI",
        UpstreamProfileType::Gemini => "Gemini",
        UpstreamProfileType::Vertex => "Vertex",
        UpstreamProfileType::OpenaiCompatible => "OpenAICompatible",
        UpstreamProfileType::Ollama => "Ollama",
        UpstreamProfileType::Anthropic => "Anthropic",
        UpstreamProfileType::Responses => "Responses",
        UpstreamProfileType::GeminiOpenai => "GeminiOpenAI",
    }
}

const fn endpoint_kind_label(value: EndpointKind) -> &'static str {
    match value {
        EndpointKind::Generation => "generation",
        EndpointKind::Utility => "utility",
        EndpointKind::Local => "local",
    }
}

const fn utility_execution_label(value: UtilityExecution) -> &'static str {
    match value {
        UtilityExecution::Local => "local",
        UtilityExecution::Upstream => "upstream",
    }
}

const fn incompatible_behavior_label(value: IncompatibleUpstreamBehavior) -> &'static str {
    match value {
        IncompatibleUpstreamBehavior::NotApplicable => "not_applicable",
        IncompatibleUpstreamBehavior::PreSendReject => "pre_send_reject",
    }
}

fn operation_contract_label(operation: &OpenAiWireOperationContract) -> String {
    format!(
        "{} (default {})",
        operation.availability.label(),
        if operation.default_enabled {
            "enabled"
        } else {
            "disabled"
        }
    )
}

const fn evidence_kind_label(value: EvidenceKind) -> &'static str {
    match value {
        EvidenceKind::Test => "test",
        EvidenceKind::Code => "code",
    }
}

const fn contract_presence_label(value: ContractPresence) -> &'static str {
    match value {
        ContractPresence::Absent => "absent",
    }
}

const fn extension_location_label(value: ExtensionLocation) -> &'static str {
    match value {
        ExtensionLocation::TopLevel => "top_level",
    }
}

const fn error_envelope_label(value: ErrorEnvelope) -> &'static str {
    match value {
        ErrorEnvelope::OpenaiError => "openai_error",
        ErrorEnvelope::AnthropicError => "anthropic_error",
        ErrorEnvelope::GoogleRpcError => "google_rpc_error",
    }
}

fn verify_rendered(expected: &str, actual: &str) -> Result<(), String> {
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "{GENERATED_RELATIVE_PATH} is stale; run `cargo run -p cyder-api --bin compatibility_matrix -- --write`"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANONICAL_SOURCE: &str = include_str!("../../../docs/protocol-compatibility.yaml");
    const CI_WORKFLOW: &str = include_str!("../../../.github/workflows/ci.yml");

    fn canonical_matrix() -> CompatibilityMatrix {
        serde_yaml::from_str(CANONICAL_SOURCE).expect("canonical matrix should parse")
    }

    #[test]
    fn canonical_matrix_is_valid_and_deterministic() {
        let matrix = canonical_matrix();
        validate_matrix(&matrix).expect("canonical matrix should validate");
        assert_eq!(render_markdown(&matrix), render_markdown(&matrix));
    }

    #[test]
    fn missing_downstream_error_protocol_is_rejected() {
        let mut matrix = canonical_matrix();
        matrix.downstream_error_contracts.protocols.pop();
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("exactly 4 protocols")
        );
    }

    #[test]
    fn duplicate_downstream_error_protocol_is_rejected() {
        let mut matrix = canonical_matrix();
        matrix.downstream_error_contracts.protocols[1].downstream_protocol =
            DownstreamProtocol::Openai;
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("DownstreamProtocol::ALL with no duplicates")
        );
    }

    #[test]
    fn ollama_downstream_error_protocol_is_rejected_during_yaml_parse() {
        let invalid = CANONICAL_SOURCE.replacen(
            "    - downstream_protocol: GEMINI\n      envelope: google_rpc_error",
            "    - downstream_protocol: OLLAMA\n      envelope: google_rpc_error",
            1,
        );
        assert!(serde_yaml::from_str::<CompatibilityMatrix>(&invalid).is_err());
    }

    #[test]
    fn downstream_error_code_path_drift_is_rejected() {
        let mut matrix = canonical_matrix();
        matrix.downstream_error_contracts.protocols[0].stable_code_path = "code".to_string();
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("invalid envelope, path, header, or evidence list")
        );
    }

    #[test]
    fn downstream_error_contract_missing_test_evidence_is_rejected() {
        let mut matrix = canonical_matrix();
        matrix.downstream_error_contracts.protocols[0]
            .evidence
            .pop();
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("invalid envelope, path, header, or evidence list")
        );

        let mut matrix = canonical_matrix();
        matrix
            .evidence
            .retain(|item| item.id != "error-contract-router");
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("require evidence 'error-contract-router'")
        );
    }

    #[test]
    fn downstream_error_scope_and_router_drift_are_rejected() {
        let mut matrix = canonical_matrix();
        matrix
            .downstream_error_contracts
            .scope
            .before_headers_committed = false;
        assert!(validate_matrix(&matrix).unwrap_err().contains("scope"));

        let mut matrix = canonical_matrix();
        matrix.downstream_error_contracts.router_rejections[1].required_header = None;
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("404, 405, and Allow")
        );
    }

    #[test]
    fn r3_7_stream_owner_and_four_protocol_resource_evidence_are_required() {
        let mut matrix = canonical_matrix();
        matrix
            .downstream_error_contracts
            .scope
            .after_headers_committed_owners
            .retain(|owner| owner != "R3.7");
        assert!(validate_matrix(&matrix).unwrap_err().contains("scope"));

        let mut matrix = canonical_matrix();
        matrix
            .evidence
            .retain(|item| item.id != "stream-resource-postcommit-four-protocol");
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("require evidence 'stream-resource-postcommit-four-protocol'")
        );

        let mut matrix = canonical_matrix();
        let item = matrix
            .evidence
            .iter_mut()
            .find(|item| item.id == "response-limit-decoded-four-protocol")
            .expect("decoded response evidence");
        item.reference = "wrong-test".to_string();
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("must reference stable automated test")
        );
    }

    #[test]
    fn missing_generation_cell_is_rejected() {
        let mut matrix = canonical_matrix();
        matrix.generation_cells.pop();
        assert!(validate_matrix(&matrix).unwrap_err().contains("4x5=20"));
    }

    #[test]
    fn duplicate_generation_cell_is_rejected() {
        let mut matrix = canonical_matrix();
        matrix.generation_cells[1] = matrix.generation_cells[0].clone();
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("duplicate generation cell")
        );
    }

    #[test]
    fn illegal_status_is_rejected_during_yaml_parse() {
        let invalid = CANONICAL_SOURCE.replacen("status: verified", "status: imaginary", 1);
        assert!(serde_yaml::from_str::<CompatibilityMatrix>(&invalid).is_err());
    }

    #[test]
    fn missing_evidence_is_rejected() {
        let mut matrix = canonical_matrix();
        matrix.generation_cells[0]
            .base
            .non_stream_text
            .evidence
            .clear();
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("must reference evidence")
        );
    }

    #[test]
    fn missing_owner_is_rejected() {
        let mut matrix = canonical_matrix();
        matrix.generation_cells[2].advanced.tools.owner = None;
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("must be owned by R3.18")
        );
    }

    #[test]
    fn transform_runtime_contract_drift_and_missing_evidence_are_rejected() {
        let mut matrix = canonical_matrix();
        matrix.transform_runtime_contract.diagnostics.payload_free = false;
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("transform_runtime_contract must pin")
        );

        let mut matrix = canonical_matrix();
        matrix
            .evidence
            .retain(|item| item.id != "r3-15-target-stream-failure");
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("requires evidence 'r3-15-target-stream-failure'")
        );
    }

    #[test]
    fn advanced_final_statuses_remove_owner_and_require_test_evidence() {
        let matrix = canonical_matrix();
        let evidence = validate_evidence(&matrix.evidence).expect("evidence should validate");
        let ids = vec!["r3-15-minor-loss-runtime".to_string()];
        for status in [
            AdvancedStatus::Full,
            AdvancedStatus::ControlledLoss,
            AdvancedStatus::ExplicitReject,
        ] {
            assert!(
                validate_assessment_evidence(
                    status.label(),
                    true,
                    &ids,
                    None,
                    Some("R3.16"),
                    &evidence,
                )
                .is_ok()
            );
        }
        assert!(
            validate_assessment_evidence(
                "advanced controlled loss",
                true,
                &ids,
                Some("R3.16"),
                Some("R3.16"),
                &evidence,
            )
            .unwrap_err()
            .contains("must not have an owner")
        );

        let code_only = vec!["reachable-materializers-without-cell-regression".to_string()];
        assert!(
            validate_assessment_evidence(
                "advanced explicit reject",
                true,
                &code_only,
                None,
                Some("R3.16"),
                &evidence,
            )
            .unwrap_err()
            .contains("requires test evidence")
        );
    }

    #[test]
    fn upstream_source_profile_drift_is_rejected() {
        let mut matrix = canonical_matrix();
        matrix.upstream_source_profiles[0].auth = "wrong".to_string();
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("differs from upstream_runtime_profile")
        );
    }

    #[test]
    fn openai_wire_profile_contract_and_count_drift_are_rejected() {
        let mut matrix = canonical_matrix();
        matrix.openai_wire_profiles[0].base_url_customizable = false;
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("openai_wire_profiles must exactly pin")
        );

        let mut matrix = canonical_matrix();
        matrix.openai_wire_profiles.pop();
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("openai_wire_profiles must exactly pin")
        );
    }

    #[test]
    fn r316_advanced_status_and_executable_reference_drift_are_rejected() {
        let mut matrix = canonical_matrix();
        matrix.generation_cells[0].advanced.tools.status = AdvancedStatus::ControlledLoss;
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("R3.16 advanced cell")
        );

        let mut matrix = canonical_matrix();
        matrix
            .evidence
            .iter_mut()
            .find(|item| item.id == "r3-16-tools-direct")
            .expect("R3.16 tools evidence")
            .reference = "wrong-test".to_string();
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("must reference stable automated test")
        );
    }

    #[test]
    fn r317_base_advanced_owner_and_executable_reference_drift_are_rejected() {
        let mut matrix = canonical_matrix();
        matrix.generation_cells[1].base.non_stream_text.status = BaseStatus::Partial;
        matrix.generation_cells[1].base.non_stream_text.owner = Some("R3.17".to_string());
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("representative cell")
        );

        let mut matrix = canonical_matrix();
        matrix.generation_cells[1].base.non_stream_text.owner = Some("R3.17".to_string());
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("complete and must not have an owner")
        );

        let mut matrix = canonical_matrix();
        matrix.generation_cells[1].advanced.reasoning.status = AdvancedStatus::Full;
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("R3.17 advanced cell")
        );

        let mut matrix = canonical_matrix();
        matrix.generation_cells[16].advanced.tools.evidence.pop();
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("R3.17 advanced cell")
        );

        let mut matrix = canonical_matrix();
        matrix
            .evidence
            .iter_mut()
            .find(|item| item.id == "r3-17-openai-base")
            .expect("R3.17 base evidence")
            .reference = "wrong-test".to_string();
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("must reference stable automated test")
        );
    }

    #[test]
    fn r3_10_source_contract_drift_is_rejected() {
        let mut matrix = canonical_matrix();
        matrix.upstream_source_contract.source_cardinality = "exactly_one".to_string();
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("zero-to-many Source")
        );
    }

    #[test]
    fn r3_10_source_contract_requires_new_evidence_and_owner_chain() {
        let mut matrix = canonical_matrix();
        matrix
            .evidence
            .retain(|item| item.id != "r3-10-source-selector");
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("r3-10-source-selector")
        );

        let mut matrix = canonical_matrix();
        matrix.upstream_source_contract.config_owner = "R3.13".to_string();
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("zero-to-many Source")
        );
    }

    #[test]
    fn protocol_and_advanced_owners_follow_r3_16_to_r3_20() {
        let mut matrix = canonical_matrix();
        matrix.generation_cells[3].base.non_stream_text.owner = Some("R3.19".to_string());
        assert!(validate_matrix(&matrix).is_ok());

        matrix.generation_cells[3].advanced.tools.owner = Some("R3.17".to_string());
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("must be owned by R3.19")
        );

        let mut matrix = canonical_matrix();
        matrix.utilities[3].verification.owner = Some("R3.17".to_string());
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("utility countTokens verification must be owned by R3.19")
        );
    }

    #[test]
    fn schema_v4_and_provider_profile_fields_are_not_accepted() {
        let v4 = CANONICAL_SOURCE.replacen("schema_version: 5", "schema_version: 4", 1);
        let matrix = serde_yaml::from_str::<CompatibilityMatrix>(&v4)
            .expect("schema number should parse before validation");
        assert!(validate_matrix(&matrix).unwrap_err().contains("must be 5"));

        let legacy = CANONICAL_SOURCE
            .replacen("upstream_source_profiles:", "provider_profiles:", 1)
            .replacen("profile_type:", "provider_type:", 1);
        assert!(serde_yaml::from_str::<CompatibilityMatrix>(&legacy).is_err());
    }

    #[test]
    fn generated_markdown_drift_is_rejected() {
        let expected = render_markdown(&canonical_matrix());
        let drifted = format!("{expected}\nmanual edit\n");
        assert!(verify_rendered(&expected, &drifted).is_err());
    }

    #[test]
    fn write_and_check_modes_are_mutually_exclusive_and_required() {
        assert_eq!(
            parse_mode(["--write".to_string()].into_iter()).unwrap(),
            Mode::Write
        );
        assert_eq!(
            parse_mode(["--check".to_string()].into_iter()).unwrap(),
            Mode::Check
        );
        assert!(parse_mode(std::iter::empty()).is_err());
        assert!(parse_mode(["--write".to_string(), "--check".to_string()].into_iter()).is_err());
    }

    #[test]
    fn ci_workflow_parses_and_matrix_changes_trigger_locked_check() {
        serde_yaml::from_str::<serde_yaml::Value>(CI_WORKFLOW)
            .expect("CI workflow should be valid YAML");
        for required in [
            "Cargo.toml|Cargo.lock|server/*)",
            "docs/protocol-compatibility.yaml|docs/protocol-compatibility.md)",
            ".github/workflows/ci.yml)",
            "cargo run --locked -p cyder-api --bin compatibility_matrix -- --check",
        ] {
            assert!(
                CI_WORKFLOW.contains(required),
                "CI workflow is missing matrix trigger/check evidence: {required}"
            );
        }
    }
}
