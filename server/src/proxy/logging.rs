use crate::{
    database::request_log::{NewRequestLog, RequestLog, UpdateRequestLogData},
    schema::enum_def::RequestStatus,
    utils::{
        billing::{populate_token_cost_fields, UsageInfo},
        ID_GENERATOR,
    },
};
use crate::service::cache::types::{CacheSystemApiKey, CacheProvider, CacheModel, CacheBillingPlan};
use chrono::Utc;
use cyder_tools::log::{debug, error};
use reqwest::StatusCode;

// Helper function to build and log the final update for a request
pub(super) fn log_final_update(
    log_id: i64,
    context_msg: &str,
    request_url: &str,
    request_body: &str,
    llm_status: Option<StatusCode>, // LLM response status
    // How to update llm_response_body:
    // None: don't touch the field in DB
    // Some(None): set to NULL in DB
    // Some(Some("body")): set to "body" in DB
    llm_body_update: Option<Option<String>>,
    is_stream_val: bool,
    first_chunk_ts: Option<i64>,
    completion_ts: i64,
    usage_opt: Option<&UsageInfo>,
    billing_plan: Option<&CacheBillingPlan>,
    overall_status: Option<RequestStatus>,
) {
    let is_error = overall_status == Some(RequestStatus::Error);

    let mut update_data = UpdateRequestLogData {
        llm_request_uri: Some(Some(request_url.to_string())),
        llm_request_body: if is_error {
            let truncated_body: String = request_body.chars().take(2000).collect();
            Some(Some(truncated_body))
        } else {
            None
        },
        llm_response_status: llm_status.map(|s| Some(s.as_u16() as i32)),
        llm_response_body: if is_error { llm_body_update } else { None },
        is_stream: Some(is_stream_val),
        llm_response_first_chunk_at: first_chunk_ts,
        llm_response_completed_at: Some(completion_ts),
        response_sent_to_client_at: Some(completion_ts),
        status: overall_status.clone(),
        ..Default::default()
    };
    populate_token_cost_fields(&mut update_data, usage_opt, billing_plan);

    debug!(
        "Updating request log {} (context: {}) with status {:?}",
        log_id, context_msg, overall_status
    );

    if let Err(e) = RequestLog::update_request_with_completion_details(log_id, &update_data) {
        error!(
            "Failed to update request log ({}) for log_id {}: {:?}",
            context_msg, log_id, e
        );
    }
}

// Creates an initial request log entry in the database.
pub(super) fn create_request_log(
    system_api_key: &CacheSystemApiKey,
    provider: &CacheProvider,
    model: &CacheModel,
    provider_api_key_id: i64,
    start_time: i64,
    client_ip_addr: &Option<String>,
    request_uri_path: &str,
    channel: &Option<String>,
    external_id: &Option<String>,
) -> i64 {
    let log_id = ID_GENERATOR.generate_id();
    let real_model_name = model
        .real_model_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&model.model_name);

    let initial_log_data = NewRequestLog {
        id: log_id,
        system_api_key_id: system_api_key.id,
        provider_id: provider.id,
        model_id: model.id,
        provider_api_key_id,
        model_name: model.model_name.clone(),
        real_model_name: real_model_name.to_string(),
        request_received_at: start_time,
        client_ip: client_ip_addr.clone(),
        external_request_uri: Some(request_uri_path.to_string()),
        status: RequestStatus::Pending,
        llm_request_sent_at: Utc::now().timestamp_millis(),
        created_at: start_time,
        updated_at: start_time,
        channel: channel.clone(),
        external_id: external_id.clone(),
    };

    debug!(
        "Creating initial request log {} for model {} (ID: {})",
        log_id, model.model_name, model.id
    );

    if let Err(e) = RequestLog::create_initial_request(&initial_log_data) {
        error!(
            "Failed to create initial request log for log_id {}: {:?}",
            log_id, e
        );
    }
    log_id
}
