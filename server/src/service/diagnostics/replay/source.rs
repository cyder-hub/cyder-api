use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use cyder_tools::log::debug;
use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::{
    controller::BaseError,
    database::{
        api_key::ApiKey,
        request_attempt::{RequestAttempt, RequestAttemptDetail},
        request_log::{RequestLog, RequestLogRecord},
    },
    proxy::{UtilityOperation, UtilityProtocol},
    schema::enum_def::{LlmApiType, ProviderType},
    service::{
        app_state::AppState,
        cache::types::{CacheApiKey, CacheCostCatalogVersion, CacheProvider},
        diagnostics::{
            body::{
                build_header_map_from_name_values, header_map_from_snapshot,
                log_capture_state_to_string, parse_name_values_json_map,
            },
            bundle::load_request_log_bundle,
            replay::types::RequestReplayNameValue,
        },
    },
    utils::storage::{
        RequestLogBundleAttemptSection, RequestLogBundleRequestSnapshot, RequestLogBundleV2,
    },
};

#[derive(Debug, Clone)]
pub(crate) struct AttemptReplaySource {
    pub(crate) request_log_id: i64,
    pub(crate) attempt: RequestAttemptDetail,
    pub(crate) requested_model_name: Option<String>,
    pub(crate) base_requested_model_name: Option<String>,
    pub(crate) resolved_reasoning_suffix: Option<String>,
    pub(crate) resolved_reasoning_preset: Option<String>,
    pub(crate) resolved_route_id: Option<i64>,
    pub(crate) resolved_route_name: Option<String>,
    pub(crate) provider: Arc<CacheProvider>,
    pub(crate) llm_api_type: LlmApiType,
    pub(crate) request_uri: String,
    pub(crate) sanitized_request_headers: Vec<RequestReplayNameValue>,
    pub(crate) request_headers: HeaderMap,
    pub(crate) llm_request_body: DecodedBundleBody,
    pub(crate) baseline_response_headers: Vec<RequestReplayNameValue>,
    pub(crate) baseline_response_body: Option<DecodedBundleBody>,
    pub(crate) cost_catalog_version: Option<CacheCostCatalogVersion>,
}

#[derive(Debug, Clone)]
pub(crate) struct GatewayReplaySource {
    pub(crate) request_log: RequestLogRecord,
    pub(crate) request_snapshot: RequestLogBundleRequestSnapshot,
    pub(crate) original_headers: HeaderMap,
    pub(crate) user_request_body: DecodedBundleBody,
    pub(crate) baseline_user_response_body: Option<DecodedBundleBody>,
    pub(crate) baseline_final_attempt: Option<RequestAttemptDetail>,
    pub(crate) api_key: Arc<CacheApiKey>,
    pub(crate) requested_model_name: String,
    pub(crate) kind: GatewayReplaySourceKind,
}

#[derive(Debug, Clone)]
pub(crate) struct DecodedBundleBody {
    pub(crate) bytes: Bytes,
    pub(crate) media_type: Option<String>,
    pub(crate) capture_state: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum GatewayReplaySourceKind {
    Generation {
        api_type: LlmApiType,
        is_stream: bool,
        data: Value,
        original_request_value: Value,
    },
    Utility {
        operation: UtilityOperation,
        data: Value,
    },
}

pub(crate) async fn load_attempt_replay_source(
    app_state: &Arc<AppState>,
    request_log_id: i64,
    attempt_id: i64,
) -> Result<AttemptReplaySource, BaseError> {
    let request_log = RequestLog::get_by_id(request_log_id)?;
    let attempt = RequestAttempt::get_by_id(attempt_id)?;

    if attempt.request_log_id != request_log_id {
        return Err(BaseError::NotFound(Some(format!(
            "Request attempt {} does not belong to request_log {}",
            attempt_id, request_log_id
        ))));
    }

    let Some(provider_id) = attempt.provider_id.or(request_log.final_provider_id) else {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Attempt {} does not have a provider snapshot",
            attempt_id
        ))));
    };
    let Some(model_id) = attempt.model_id.or(request_log.final_model_id) else {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Attempt {} does not have a model snapshot",
            attempt_id
        ))));
    };

    let provider = app_state
        .catalog
        .get_provider_by_id(provider_id)
        .await?
        .ok_or_else(|| BaseError::NotFound(Some(format!("Provider {} not found", provider_id))))?;
    let model = app_state
        .catalog
        .get_model_by_id(model_id)
        .await?
        .ok_or_else(|| BaseError::NotFound(Some(format!("Model {} not found", model_id))))?;

    let bundle = load_request_log_bundle(&request_log)
        .await?
        .ok_or_else(|| {
            BaseError::ParamInvalid(Some(format!(
                "Request log {} does not have a persisted bundle",
                request_log_id
            )))
        })?;

    let request_uri = require_non_empty(
        attempt.request_uri.as_deref(),
        format!("Attempt {} is missing downstream request URI", attempt_id),
    )?;
    let raw_request_headers = require_non_empty(
        attempt.request_headers_json.as_deref(),
        format!(
            "Attempt {} is missing downstream request headers",
            attempt_id
        ),
    )?;
    let sanitized_request_headers =
        parse_name_values_json_map(&raw_request_headers, "request headers")?;
    let request_headers = build_header_map_from_name_values(&sanitized_request_headers)?;
    let llm_request_body = extract_attempt_request_body(&bundle, &attempt)?;
    let baseline_response_headers = match attempt.response_headers_json.as_deref() {
        Some(raw) if !raw.trim().is_empty() => parse_name_values_json_map(raw, "response headers")?,
        _ => Vec::new(),
    };
    let baseline_response_body = extract_attempt_response_body(&bundle, &attempt)?;

    let cost_catalog_version = match attempt.cost_catalog_version_id {
        Some(cost_catalog_version_id) => app_state
            .catalog
            .get_cost_catalog_version_by_id(cost_catalog_version_id)
            .await?
            .map(|version| (*version).clone()),
        None => app_state
            .catalog
            .get_cost_catalog_version_by_model(model.id, Utc::now().timestamp_millis())
            .await?
            .map(|version| (*version).clone()),
    };
    let llm_api_type = attempt
        .llm_api_type
        .unwrap_or_else(|| infer_llm_api_type(request_log.user_api_type, &provider));

    Ok(AttemptReplaySource {
        request_log_id,
        attempt,
        requested_model_name: request_log.requested_model_name,
        base_requested_model_name: request_log.base_requested_model_name,
        resolved_reasoning_suffix: request_log.resolved_reasoning_suffix,
        resolved_reasoning_preset: request_log.resolved_reasoning_preset,
        resolved_route_id: request_log.resolved_route_id,
        resolved_route_name: request_log.resolved_route_name,
        provider,
        llm_api_type,
        request_uri,
        sanitized_request_headers,
        request_headers,
        llm_request_body,
        baseline_response_headers,
        baseline_response_body,
        cost_catalog_version,
    })
}

pub(crate) async fn load_gateway_replay_source(
    request_log_id: i64,
) -> Result<GatewayReplaySource, BaseError> {
    let request_log = RequestLog::get_by_id(request_log_id)?;
    let bundle = load_request_log_bundle(&request_log)
        .await?
        .ok_or_else(|| {
            BaseError::ParamInvalid(Some(format!(
                "Request log {} does not have a persisted bundle",
                request_log_id
            )))
        })?;
    let request_snapshot = bundle.request_snapshot.clone().ok_or_else(|| {
        BaseError::ParamInvalid(Some(format!(
            "Request log {} is missing request snapshot",
            request_log_id
        )))
    })?;
    if request_snapshot.request_path.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Request log {} has an empty request snapshot path",
            request_log_id
        ))));
    }

    let user_request_body = extract_gateway_user_request_body(&bundle)?;
    let baseline_user_response_body = extract_gateway_user_response_body(&bundle);
    let baseline_final_attempt = load_gateway_baseline_final_attempt(&request_log);
    let request_value =
        serde_json::from_slice::<Value>(&user_request_body.bytes).map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "Gateway replay user request body is not valid JSON: {}",
                err
            )))
        })?;
    let (requested_model_name, kind) =
        gateway_replay_kind_from_snapshot(&request_log, &request_snapshot, &request_value)?;
    let api_key = Arc::new(load_cache_api_key_by_id(request_log.api_key_id)?);
    let original_headers = header_map_from_snapshot(&request_snapshot)?;

    Ok(GatewayReplaySource {
        request_log,
        request_snapshot,
        original_headers,
        user_request_body,
        baseline_user_response_body,
        baseline_final_attempt,
        api_key,
        requested_model_name,
        kind,
    })
}

fn load_gateway_baseline_final_attempt(
    request_log: &RequestLogRecord,
) -> Option<RequestAttemptDetail> {
    let final_attempt_id = request_log.final_attempt_id?;
    match RequestAttempt::get_by_id(final_attempt_id) {
        Ok(attempt) if attempt.request_log_id == request_log.id => Some(attempt),
        Ok(attempt) => {
            debug!(
                "Gateway replay baseline final attempt {} belongs to request_log {}, expected {}",
                final_attempt_id, attempt.request_log_id, request_log.id
            );
            None
        }
        Err(err) => {
            debug!(
                "Gateway replay could not load baseline final attempt {} for request_log {}: {:?}",
                final_attempt_id, request_log.id, err
            );
            None
        }
    }
}

fn load_cache_api_key_by_id(api_key_id: i64) -> Result<CacheApiKey, BaseError> {
    let row = ApiKey::get_by_id(api_key_id)?;
    let acl_rules = ApiKey::load_acl_rules(row.id)?;
    let cache_key = CacheApiKey::from_db(row, acl_rules);
    if !cache_key.is_active_at(Utc::now().timestamp_millis()) {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Source api key {} is not active under current configuration",
            api_key_id
        ))));
    }
    Ok(cache_key)
}

fn extract_gateway_user_request_body(
    bundle: &RequestLogBundleV2,
) -> Result<DecodedBundleBody, BaseError> {
    let blob_id = bundle.request_section.user_request_blob_id.ok_or_else(|| {
        BaseError::ParamInvalid(Some(
            "Gateway replay requires a captured user request body".to_string(),
        ))
    })?;
    let blob = bundle
        .blob_pool
        .iter()
        .find(|blob| blob.blob_id == blob_id)
        .ok_or_else(|| {
            BaseError::ParamInvalid(Some(format!(
                "Gateway replay user request blob {} is missing",
                blob_id
            )))
        })?;
    Ok(DecodedBundleBody {
        bytes: blob.body.clone(),
        media_type: Some(blob.media_type.clone()),
        capture_state: Some("complete".to_string()),
    })
}

fn extract_gateway_user_response_body(bundle: &RequestLogBundleV2) -> Option<DecodedBundleBody> {
    let blob_id = bundle.request_section.user_response_blob_id?;
    bundle
        .blob_pool
        .iter()
        .find(|blob| blob.blob_id == blob_id)
        .map(|blob| DecodedBundleBody {
            bytes: blob.body.clone(),
            media_type: Some(blob.media_type.clone()),
            capture_state: bundle
                .request_section
                .user_response_capture_state
                .as_ref()
                .map(log_capture_state_to_string),
        })
}

pub(crate) fn gateway_replay_kind_from_snapshot(
    request_log: &RequestLogRecord,
    snapshot: &RequestLogBundleRequestSnapshot,
    request_value: &Value,
) -> Result<(String, GatewayReplaySourceKind), BaseError> {
    let operation_kind = snapshot.operation_kind.as_str();
    if request_log.user_api_type == LlmApiType::Openai
        && matches!(operation_kind, "embeddings" | "rerank")
    {
        let requested_model = require_model_from_request_value(request_value)?;
        let operation = UtilityOperation {
            name: operation_kind.to_string(),
            api_type: LlmApiType::Openai,
            protocol: UtilityProtocol::OpenaiCompatible,
            downstream_path: operation_kind.to_string(),
        };
        return Ok((
            requested_model,
            GatewayReplaySourceKind::Utility {
                operation,
                data: request_value.clone(),
            },
        ));
    }

    if request_log.user_api_type == LlmApiType::Gemini {
        let (model_name, action) = parse_gemini_model_action_from_path(&snapshot.request_path)?;
        if matches!(
            action.as_str(),
            "countMessageTokens" | "countTextTokens" | "countTokens"
        ) {
            let operation = UtilityOperation {
                name: action.clone(),
                api_type: LlmApiType::Gemini,
                protocol: UtilityProtocol::GeminiCompatible,
                downstream_path: action,
            };
            return Ok((
                model_name,
                GatewayReplaySourceKind::Utility {
                    operation,
                    data: request_value.clone(),
                },
            ));
        }

        let is_stream = action == "streamGenerateContent";
        return Ok((
            model_name,
            GatewayReplaySourceKind::Generation {
                api_type: LlmApiType::Gemini,
                is_stream,
                data: request_value.clone(),
                original_request_value: request_value.clone(),
            },
        ));
    }

    let requested_model = require_model_from_request_value(request_value)?;
    let is_stream = request_value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok((
        requested_model,
        GatewayReplaySourceKind::Generation {
            api_type: request_log.user_api_type,
            is_stream,
            data: request_value.clone(),
            original_request_value: request_value.clone(),
        },
    ))
}

fn require_model_from_request_value(value: &Value) -> Result<String, BaseError> {
    value
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            BaseError::ParamInvalid(Some(
                "Gateway replay request body is missing string field 'model'".to_string(),
            ))
        })
}

fn parse_gemini_model_action_from_path(path: &str) -> Result<(String, String), BaseError> {
    let action_segment = path
        .find("/models/")
        .map(|index| &path[index + "/models/".len()..])
        .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path));
    let (model_name, action) = action_segment.rsplit_once(':').ok_or_else(|| {
        BaseError::ParamInvalid(Some(format!(
            "Gateway replay Gemini request path '{}' does not contain a model action",
            path
        )))
    })?;
    Ok((model_name.to_string(), action.to_string()))
}

fn extract_attempt_request_body(
    bundle: &RequestLogBundleV2,
    attempt: &RequestAttemptDetail,
) -> Result<DecodedBundleBody, BaseError> {
    let section = bundle_attempt_section(bundle, attempt).ok_or_else(|| {
        BaseError::ParamInvalid(Some(format!(
            "Attempt {} is missing bundle section",
            attempt.id
        )))
    })?;
    reconstruct_request_body_from_v2_bundle(
        bundle,
        section.llm_request_blob_id,
        section.llm_request_patch_id,
    )
    .ok_or_else(|| {
        BaseError::ParamInvalid(Some(format!(
            "Attempt {} is missing historical downstream request body",
            attempt.id
        )))
    })
}

fn extract_attempt_response_body(
    bundle: &RequestLogBundleV2,
    attempt: &RequestAttemptDetail,
) -> Result<Option<DecodedBundleBody>, BaseError> {
    let Some(section) = bundle_attempt_section(bundle, attempt) else {
        return Ok(None);
    };
    Ok(section
        .llm_response_blob_id
        .and_then(|blob_id| bundle.blob_pool.iter().find(|blob| blob.blob_id == blob_id))
        .map(|blob| DecodedBundleBody {
            bytes: blob.body.clone(),
            media_type: Some(blob.media_type.clone()),
            capture_state: section
                .llm_response_capture_state
                .as_ref()
                .map(log_capture_state_to_string),
        }))
}

fn bundle_attempt_section<'a>(
    bundle: &'a RequestLogBundleV2,
    attempt: &RequestAttemptDetail,
) -> Option<&'a RequestLogBundleAttemptSection> {
    bundle.attempt_sections.iter().find(|section| {
        section
            .attempt_id
            .is_some_and(|attempt_id| attempt_id == attempt.id)
            || section.attempt_index == attempt.attempt_index
    })
}

fn reconstruct_request_body_from_v2_bundle(
    bundle: &RequestLogBundleV2,
    blob_id: Option<i32>,
    patch_id: Option<i32>,
) -> Option<DecodedBundleBody> {
    let blob_id = blob_id?;
    let blob = bundle
        .blob_pool
        .iter()
        .find(|blob| blob.blob_id == blob_id)?;
    let bytes = if let Some(patch_id) = patch_id {
        let patch = bundle
            .patch_pool
            .iter()
            .find(|patch| patch.patch_id == patch_id)?;
        apply_json_patch_bytes(&blob.body, &patch.patch_body).ok()?
    } else {
        blob.body.clone()
    };

    Some(DecodedBundleBody {
        bytes,
        media_type: Some(blob.media_type.clone()),
        capture_state: Some("complete".to_string()),
    })
}

fn apply_json_patch_bytes(base: &[u8], patch: &[u8]) -> Result<Bytes, BaseError> {
    let mut value = serde_json::from_slice::<Value>(base).map_err(|err| {
        BaseError::ParamInvalid(Some(format!(
            "Replay request patch base JSON decode failed: {}",
            err
        )))
    })?;
    let patch: json_patch::Patch = serde_json::from_slice(patch).map_err(|err| {
        BaseError::ParamInvalid(Some(format!(
            "Replay request patch JSON decode failed: {}",
            err
        )))
    })?;
    json_patch::patch(&mut value, &patch).map_err(|err| {
        BaseError::ParamInvalid(Some(format!(
            "Replay request patch application failed: {}",
            err
        )))
    })?;
    serde_json::to_vec(&value).map(Bytes::from).map_err(|err| {
        BaseError::ParamInvalid(Some(format!(
            "Replay request patch serialization failed: {}",
            err
        )))
    })
}

fn infer_llm_api_type(user_api_type: LlmApiType, provider: &CacheProvider) -> LlmApiType {
    match provider.provider_type {
        ProviderType::Vertex | ProviderType::Gemini => LlmApiType::Gemini,
        ProviderType::Ollama => LlmApiType::Ollama,
        ProviderType::Anthropic => LlmApiType::Anthropic,
        ProviderType::Responses => LlmApiType::Responses,
        ProviderType::GeminiOpenai => LlmApiType::GeminiOpenai,
        ProviderType::Openai | ProviderType::VertexOpenai => user_api_type,
    }
}

fn require_non_empty(value: Option<&str>, message: String) -> Result<String, BaseError> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| BaseError::ParamInvalid(Some(message)))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use diesel::connection::SimpleConnection;
    use flate2::{Compression, write::GzEncoder};
    use serde_json::json;

    use super::*;
    use crate::{
        database::{DbConnection, TestDbContext, get_connection},
        schema::enum_def::{RequestAttemptStatus, RequestStatus, SchedulerAction},
        service::app_state::create_test_app_state,
        service::storage::{Storage, get_local_storage},
        utils::storage::{
            RequestLogBundleBlob, RequestLogBundlePatch, RequestLogBundleRequestSection,
        },
    };

    fn request_log(user_api_type: LlmApiType) -> RequestLogRecord {
        RequestLogRecord {
            id: 42,
            api_key_id: 7,
            requested_model_name: Some("gpt-test".to_string()),
            base_requested_model_name: Some("gpt-test".to_string()),
            resolved_reasoning_suffix: None,
            resolved_reasoning_preset: None,
            resolved_name_scope: None,
            resolved_route_id: None,
            resolved_route_name: None,
            user_api_type,
            overall_status: RequestStatus::Success,
            final_error_code: None,
            final_error_message: None,
            attempt_count: 1,
            retry_count: 0,
            fallback_count: 0,
            request_received_at: 100,
            first_attempt_started_at: None,
            response_started_to_client_at: None,
            completed_at: None,
            is_stream: false,
            client_ip: None,
            final_attempt_id: None,
            final_provider_id: None,
            final_provider_api_key_id: None,
            final_model_id: None,
            final_provider_key_snapshot: None,
            final_provider_name_snapshot: None,
            final_model_name_snapshot: None,
            final_real_model_name_snapshot: None,
            final_llm_api_type: None,
            estimated_cost_nanos: None,
            estimated_cost_currency: None,
            cost_catalog_id: None,
            cost_catalog_version_id: None,
            cost_snapshot_json: None,
            total_input_tokens: None,
            total_output_tokens: None,
            input_text_tokens: None,
            output_text_tokens: None,
            input_image_tokens: None,
            output_image_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
            has_transform_diagnostics: false,
            transform_diagnostic_count: 0,
            transform_diagnostic_max_loss_level: None,
            bundle_version: None,
            bundle_storage_type: None,
            bundle_storage_key: None,
            created_at: 100,
            updated_at: 100,
        }
    }

    fn attempt(id: i64, request_log_id: i64, attempt_index: i32) -> RequestAttemptDetail {
        RequestAttemptDetail {
            id,
            request_log_id,
            attempt_index,
            candidate_position: attempt_index,
            provider_id: None,
            provider_api_key_id: None,
            model_id: None,
            provider_key_snapshot: None,
            provider_name_snapshot: None,
            model_name_snapshot: None,
            real_model_name_snapshot: None,
            llm_api_type: None,
            attempt_status: RequestAttemptStatus::Success,
            scheduler_action: SchedulerAction::ReturnSuccess,
            error_code: None,
            error_message: None,
            request_uri: Some("https://upstream.example/v1/chat/completions".to_string()),
            request_headers_json: Some(r#"{"content-type":"application/json"}"#.to_string()),
            response_headers_json: None,
            http_status: Some(200),
            started_at: None,
            first_byte_at: None,
            completed_at: None,
            response_started_to_client: false,
            backoff_ms: None,
            applied_request_patch_ids_json: None,
            request_patch_summary_json: None,
            estimated_cost_nanos: None,
            estimated_cost_currency: None,
            cost_catalog_version_id: None,
            total_input_tokens: None,
            total_output_tokens: None,
            input_text_tokens: None,
            output_text_tokens: None,
            input_image_tokens: None,
            output_image_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
            llm_request_blob_id: None,
            llm_request_patch_id: None,
            llm_response_blob_id: None,
            llm_response_capture_state: None,
            created_at: 100,
            updated_at: 100,
        }
    }

    fn bundle_with_attempt_section(section: RequestLogBundleAttemptSection) -> RequestLogBundleV2 {
        RequestLogBundleV2 {
            version: 2,
            log_id: 42,
            created_at: 100,
            request_section: RequestLogBundleRequestSection::default(),
            attempt_sections: vec![section],
            request_snapshot: None,
            candidate_manifest: None,
            transform_diagnostics: None,
            blob_pool: vec![
                RequestLogBundleBlob {
                    blob_id: 1,
                    media_type: "application/json".to_string(),
                    sha256: "base".to_string(),
                    size_bytes: 28,
                    body: Bytes::from_static(br#"{"model":"gpt","input":"old"}"#),
                },
                RequestLogBundleBlob {
                    blob_id: 2,
                    media_type: "application/json".to_string(),
                    sha256: "response".to_string(),
                    size_bytes: 11,
                    body: Bytes::from_static(br#"{"ok":true}"#),
                },
            ],
            patch_pool: vec![RequestLogBundlePatch {
                patch_id: 1,
                format: "application/json-patch+json".to_string(),
                target_sha256: "base".to_string(),
                target_size_bytes: 28,
                patch_body: Bytes::from_static(
                    br#"[{"op":"replace","path":"/input","value":"new"}]"#,
                ),
            }],
        }
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn seed_api_key(conn: &mut diesel::SqliteConnection) {
        conn.batch_execute(
            "INSERT INTO api_key (
                id, api_key, api_key_hash, key_prefix, key_last4, name, description,
                default_action, is_enabled, expires_at, rate_limit_rpm, max_concurrent_requests,
                quota_daily_requests, quota_daily_tokens, quota_monthly_tokens,
                budget_daily_nanos, budget_daily_currency, budget_monthly_nanos,
                budget_monthly_currency, deleted_at, created_at, updated_at
            ) VALUES (
                7, 'ck-test', 'hash', 'ck-test', 'test', 'Test key', NULL,
                'ALLOW', 1, NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, 1, 1
            );",
        )
        .expect("api key seed should insert");
    }

    #[test]
    fn attempt_request_body_reconstructs_historical_patch_from_bundle() {
        let attempt = attempt(101, 42, 1);
        let bundle = bundle_with_attempt_section(RequestLogBundleAttemptSection {
            attempt_id: Some(101),
            attempt_index: 1,
            llm_request_blob_id: Some(1),
            llm_request_patch_id: Some(1),
            llm_response_blob_id: Some(2),
            llm_response_capture_state: None,
        });

        let body = extract_attempt_request_body(&bundle, &attempt)
            .expect("attempt request body should reconstruct");

        assert_eq!(
            serde_json::from_slice::<Value>(&body.bytes).unwrap(),
            json!({"model": "gpt", "input": "new"})
        );
        assert_eq!(body.media_type.as_deref(), Some("application/json"));
        assert_eq!(body.capture_state.as_deref(), Some("complete"));
    }

    #[test]
    fn attempt_request_body_requires_historical_bundle_body() {
        let attempt = attempt(101, 42, 1);
        let bundle = bundle_with_attempt_section(RequestLogBundleAttemptSection {
            attempt_id: Some(101),
            attempt_index: 1,
            llm_request_blob_id: None,
            llm_request_patch_id: None,
            llm_response_blob_id: None,
            llm_response_capture_state: None,
        });

        let err = extract_attempt_request_body(&bundle, &attempt).unwrap_err();

        assert!(
            matches!(err, BaseError::ParamInvalid(Some(message)) if message.contains("missing historical downstream request body"))
        );
    }

    #[test]
    fn gateway_request_body_requires_historical_bundle_blob() {
        let bundle = RequestLogBundleV2 {
            version: 2,
            log_id: 42,
            created_at: 100,
            request_section: RequestLogBundleRequestSection::default(),
            attempt_sections: Vec::new(),
            request_snapshot: None,
            candidate_manifest: None,
            transform_diagnostics: None,
            blob_pool: Vec::new(),
            patch_pool: Vec::new(),
        };

        let err = extract_gateway_user_request_body(&bundle).unwrap_err();

        assert!(
            matches!(err, BaseError::ParamInvalid(Some(message)) if message.contains("requires a captured user request body"))
        );
    }

    #[test]
    fn gateway_kind_infers_openai_utility_and_gemini_stream() {
        let openai_snapshot = RequestLogBundleRequestSnapshot {
            request_path: "/ai/openai/v1/embeddings".to_string(),
            operation_kind: "embeddings".to_string(),
            ..Default::default()
        };
        let (openai_model, openai_kind) = gateway_replay_kind_from_snapshot(
            &request_log(LlmApiType::Openai),
            &openai_snapshot,
            &json!({"model": "text-embedding-3-small", "input": "hello"}),
        )
        .expect("openai utility kind should infer");

        assert_eq!(openai_model, "text-embedding-3-small");
        match openai_kind {
            GatewayReplaySourceKind::Utility { operation, data } => {
                assert_eq!(operation.name, "embeddings");
                assert_eq!(operation.api_type, LlmApiType::Openai);
                assert_eq!(data["model"], "text-embedding-3-small");
            }
            other => panic!("expected utility kind, got {other:?}"),
        }

        let gemini_snapshot = RequestLogBundleRequestSnapshot {
            request_path: "/ai/gemini/v1beta/models/gemini-2.5-pro:streamGenerateContent"
                .to_string(),
            operation_kind: "stream_generate_content".to_string(),
            ..Default::default()
        };
        let (gemini_model, gemini_kind) = gateway_replay_kind_from_snapshot(
            &request_log(LlmApiType::Gemini),
            &gemini_snapshot,
            &json!({"contents": []}),
        )
        .expect("gemini generation kind should infer");

        assert_eq!(gemini_model, "gemini-2.5-pro");
        match gemini_kind {
            GatewayReplaySourceKind::Generation {
                api_type,
                is_stream,
                ..
            } => {
                assert_eq!(api_type, LlmApiType::Gemini);
                assert!(is_stream);
            }
            other => panic!("expected generation kind, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn gateway_source_requires_persisted_bundle() {
        let test_db_context = TestDbContext::new_sqlite("diagnostics-source-missing-bundle.sqlite");

        test_db_context
            .run_async(async {
                let DbConnection::Sqlite(mut conn) =
                    get_connection().expect("scoped sqlite connection should be available")
                else {
                    panic!("expected sqlite connection");
                };
                seed_api_key(&mut conn);
                conn.batch_execute(
                    "INSERT INTO request_log (
                        id, api_key_id, user_api_type, overall_status, attempt_count,
                        retry_count, fallback_count, request_received_at, created_at, updated_at
                    ) VALUES (
                        12, 7, 'OPENAI', 'SUCCESS', 0, 0, 0, 100, 100, 100
                    );",
                )
                .expect("request log seed should insert");

                let err = load_gateway_replay_source(12).await.unwrap_err();

                assert!(matches!(err, BaseError::ParamInvalid(Some(message)) if message.contains("does not have a persisted bundle")));
            })
            .await;
    }

    #[tokio::test]
    async fn gateway_source_requires_request_snapshot_from_bundle() {
        let test_db_context =
            TestDbContext::new_sqlite("diagnostics-source-missing-snapshot.sqlite");
        let key = "diagnostics/source/missing-snapshot.msgpack.gz";
        let bundle = RequestLogBundleV2 {
            version: 2,
            log_id: 13,
            created_at: 100,
            request_section: RequestLogBundleRequestSection {
                user_request_blob_id: Some(1),
                user_response_blob_id: None,
                user_response_capture_state: None,
            },
            attempt_sections: Vec::new(),
            request_snapshot: None,
            candidate_manifest: None,
            transform_diagnostics: None,
            blob_pool: vec![RequestLogBundleBlob {
                blob_id: 1,
                media_type: "application/json".to_string(),
                sha256: "request".to_string(),
                size_bytes: 20,
                body: Bytes::from_static(br#"{"model":"gpt-test"}"#),
            }],
            patch_pool: Vec::new(),
        };
        let encoded = rmp_serde::to_vec_named(&bundle).unwrap();
        get_local_storage()
            .await
            .put_object(key, Bytes::from(gzip(&encoded)), None)
            .await
            .expect("bundle should be stored");

        test_db_context
            .run_async(async {
                let DbConnection::Sqlite(mut conn) =
                    get_connection().expect("scoped sqlite connection should be available")
                else {
                    panic!("expected sqlite connection");
                };
                seed_api_key(&mut conn);
                conn.batch_execute(
                    "INSERT INTO request_log (
                        id, api_key_id, user_api_type, overall_status, attempt_count,
                        retry_count, fallback_count, request_received_at,
                        bundle_version, bundle_storage_type, bundle_storage_key,
                        created_at, updated_at
                    ) VALUES (
                        13, 7, 'OPENAI', 'SUCCESS', 0, 0, 0, 100,
                        2, 'FILE_SYSTEM', 'diagnostics/source/missing-snapshot.msgpack.gz',
                        100, 100
                    );",
                )
                .expect("request log seed should insert");

                let err = load_gateway_replay_source(13).await.unwrap_err();

                assert!(matches!(err, BaseError::ParamInvalid(Some(message)) if message.contains("missing request snapshot")));
            })
            .await;
    }

    #[tokio::test]
    async fn attempt_source_rejects_attempt_from_other_request_log() {
        let test_db_context = TestDbContext::new_sqlite("diagnostics-source-attempt-scope.sqlite");

        test_db_context
            .run_async(async {
                let DbConnection::Sqlite(mut conn) =
                    get_connection().expect("scoped sqlite connection should be available")
                else {
                    panic!("expected sqlite connection");
                };
                seed_api_key(&mut conn);
                conn.batch_execute(
                    "INSERT INTO request_log (
                        id, api_key_id, user_api_type, overall_status, attempt_count,
                        retry_count, fallback_count, request_received_at, created_at, updated_at
                    ) VALUES
                        (10, 7, 'OPENAI', 'SUCCESS', 0, 0, 0, 100, 100, 100),
                        (11, 7, 'OPENAI', 'SUCCESS', 1, 0, 0, 100, 100, 100);
                    INSERT INTO request_attempt (
                        id, request_log_id, attempt_index, candidate_position, attempt_status,
                        scheduler_action, response_started_to_client, created_at, updated_at
                    ) VALUES (
                        201, 11, 1, 1, 'SUCCESS', 'RETURN_SUCCESS', 0, 100, 100
                    );",
                )
                .expect("source scope seed should insert");

                let app_state = create_test_app_state(test_db_context.clone()).await;
                let err = load_attempt_replay_source(&app_state, 10, 201)
                    .await
                    .unwrap_err();

                assert!(matches!(err, BaseError::NotFound(Some(message)) if message.contains("does not belong to request_log 10")));
            })
            .await;
    }
}
