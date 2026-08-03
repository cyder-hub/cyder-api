use std::{
    collections::{HashMap, HashSet},
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use cyder_api::{
    schema::enum_def::{DownstreamProtocol, ProviderType, UpstreamProtocol},
    service::provider_profile::provider_runtime_profile,
};
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;
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
    provider_profiles: Vec<ProviderProfileContract>,
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

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum EvidenceKind {
    Test,
    Code,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderProfileContract {
    provider_type: ProviderType,
    upstream_protocol: UpstreamProtocol,
    dialect: String,
    auth: String,
    endpoint: String,
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
    validate_downstream_error_contracts(&matrix.downstream_error_contracts, &evidence)?;
    validate_provider_profiles(&matrix.provider_profiles)?;
    validate_routes(&matrix.routes)?;
    validate_generation_cells(&matrix.generation_cells, &evidence)?;
    validate_utilities(&matrix.utilities, &evidence)?;
    Ok(())
}

const REQUIRED_ERROR_EVIDENCE: [(&str, &str); 3] = [
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
    ];
    DownstreamErrorContracts {
        scope: ErrorContractScope {
            before_headers_committed: true,
            after_headers_committed_owners: vec!["R3.11".to_string(), "R3.12-R3.17".to_string()],
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

fn validate_provider_profiles(profiles: &[ProviderProfileContract]) -> Result<(), String> {
    if profiles.len() != ProviderType::ALL.len() {
        return Err(format!(
            "provider_profiles must contain {} entries, found {}",
            ProviderType::ALL.len(),
            profiles.len()
        ));
    }
    let mut seen = HashSet::new();
    for profile in profiles {
        if !seen.insert(profile.provider_type) {
            return Err(format!(
                "duplicate provider profile for {:?}",
                profile.provider_type
            ));
        }
        let runtime = provider_runtime_profile(&profile.provider_type);
        if profile.upstream_protocol != runtime.upstream_protocol
            || profile.dialect != runtime.dialect.as_key()
            || profile.auth != runtime.auth.as_key()
            || profile.endpoint != runtime.endpoint.as_key()
        {
            return Err(format!(
                "provider profile for {:?} differs from provider_runtime_profile",
                profile.provider_type
            ));
        }
    }
    if seen != ProviderType::ALL.into_iter().collect() {
        return Err("provider_profiles do not exhaust ProviderType::ALL".to_string());
    }
    Ok(())
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
                assessment.status == AdvancedStatus::Full,
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
    let verified_cell = matches!(
        (cell.downstream, cell.upstream),
        (DownstreamProtocol::Openai, UpstreamProtocol::Openai)
            | (DownstreamProtocol::Responses, UpstreamProtocol::Openai)
            | (DownstreamProtocol::Anthropic, UpstreamProtocol::Openai)
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

    if matches!(
        cell.upstream,
        UpstreamProtocol::Responses | UpstreamProtocol::Anthropic
    ) && !cell
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
    if !cell
        .advanced
        .entries()
        .into_iter()
        .all(|(_, assessment)| assessment.status == AdvancedStatus::NotVerified)
    {
        return Err(format!(
            "initial advanced dimensions for {:?}->{:?} must remain not_verified",
            cell.downstream, cell.upstream
        ));
    }
    Ok(())
}

fn generation_owner(upstream: UpstreamProtocol) -> &'static str {
    match upstream {
        UpstreamProtocol::Openai => "R3.12",
        UpstreamProtocol::Responses => "R3.13",
        UpstreamProtocol::Anthropic => "R3.14",
        UpstreamProtocol::Gemini => "R3.15",
        UpstreamProtocol::Ollama => "R3.16",
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
            if is_complete { None } else { Some("R3.17") },
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
            verification_status: UtilityVerificationStatus::Partial,
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
            verification_status: UtilityVerificationStatus::Partial,
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

    writeln!(output, "\n## Provider runtime profiles\n").unwrap();
    writeln!(
        output,
        "| Provider type | Upstream protocol | Dialect | Auth | Endpoint |"
    )
    .unwrap();
    writeln!(output, "| --- | --- | --- | --- | --- |").unwrap();
    for profile in &matrix.provider_profiles {
        writeln!(
            output,
            "| {} | {} | `{}` | `{}` | `{}` |",
            provider_label(profile.provider_type),
            upstream_label(profile.upstream_protocol),
            profile.dialect,
            profile.auth,
            profile.endpoint
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

const fn provider_label(value: ProviderType) -> &'static str {
    match value {
        ProviderType::Openai => "OpenAI",
        ProviderType::Gemini => "Gemini",
        ProviderType::Vertex => "Vertex",
        ProviderType::VertexOpenai => "VertexOpenAI",
        ProviderType::Ollama => "Ollama",
        ProviderType::Anthropic => "Anthropic",
        ProviderType::Responses => "Responses",
        ProviderType::GeminiOpenai => "GeminiOpenAI",
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
        matrix.generation_cells[0].advanced.tools.owner = None;
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("must be owned by R3.12")
        );
    }

    #[test]
    fn provider_profile_drift_is_rejected() {
        let mut matrix = canonical_matrix();
        matrix.provider_profiles[0].auth = "wrong".to_string();
        assert!(
            validate_matrix(&matrix)
                .unwrap_err()
                .contains("differs from provider_runtime_profile")
        );
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
