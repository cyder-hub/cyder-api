use crate::service::cache::types::{
    CacheBillingPlan, CacheModel, CacheProvider, CacheSystemApiKey,
};
use crate::utils::billing::calculate_cost;
use crate::{
    database::request_log::RequestLog,
    schema::enum_def::RequestStatus,
    service::storage::get_storage,
    utils::{
        billing::UsageInfo,
        storage::generate_storage_path_from_hash,
        ID_GENERATOR,
    },
};
use bytes::Bytes;
use chrono::Utc;
use cyder_tools::log::error;
use once_cell::sync::Lazy;
use reqwest::StatusCode;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

// server/src/proxy/logging.rs

// ... (imports)

#[derive(Debug, Clone)]
pub enum RequestBodyVariant {
    Full(Bytes),
    Patch(Bytes),
}

#[derive(Debug, Clone)]
pub struct RequestLogContext {
    // from create_request_log
    pub id: i64,
    pub system_api_key_id: i64,
    pub provider_id: i64,
    pub model_id: i64,
    pub provider_api_key_id: i64,
    pub model_name: String,
    pub real_model_name: String,
    pub request_received_at: i64,
    pub client_ip: Option<String>,
    pub llm_request_sent_at: Option<i64>,

    // from log_final_update
    pub request_url: Option<String>,
    pub llm_status: Option<StatusCode>,
    pub is_stream: bool,
    pub first_chunk_ts: Option<i64>,
    pub completion_ts: Option<i64>,
    pub usage: Option<UsageInfo>,
    pub billing_plan: Option<CacheBillingPlan>,
    pub overall_status: RequestStatus,
    pub user_request_body: Option<Bytes>,
    pub llm_request_body: Option<RequestBodyVariant>,
    pub llm_response_body: Option<Bytes>,
    pub user_response_body: Option<Bytes>,
}

impl RequestLogContext {
    pub fn new(
        system_api_key: &CacheSystemApiKey,
        provider: &CacheProvider,
        model: &CacheModel,
        provider_api_key_id: i64,
        start_time: i64,
        client_ip_addr: &Option<String>,
    ) -> Self {
        let real_model_name = model
            .real_model_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&model.model_name);

        Self {
            id: ID_GENERATOR.generate_id(),
            system_api_key_id: system_api_key.id,
            provider_id: provider.id,
            model_id: model.id,
            provider_api_key_id,
            model_name: model.model_name.clone(),
            real_model_name: real_model_name.to_string(),
            request_received_at: start_time,
            client_ip: client_ip_addr.clone(),
            llm_request_sent_at: None,
            request_url: None,
            llm_status: None,
            is_stream: false,
            first_chunk_ts: None,
            completion_ts: None,
            usage: None,
            billing_plan: None,
            overall_status: RequestStatus::Pending,
            user_request_body: None,
            llm_request_body: None,
            llm_response_body: None,
            user_response_body: None,
        }
    }
}

pub struct LogManager {
    sender: mpsc::Sender<RequestLogContext>,
}

impl LogManager {
    fn new() -> Self {
        let (sender, mut receiver) = mpsc::channel::<RequestLogContext>(100);

        tokio::spawn(async move {
            while let Some(context) = receiver.recv().await {
                Self::process_log(context).await;
            }
        });

        Self { sender }
    }

    pub async fn log(&self, context: RequestLogContext) {
        if let Err(e) = self.sender.send(context).await {
            error!("Failed to send log to LogManager: {:?}", e);
        }
    }

    async fn process_log(context: RequestLogContext) {
        let log_id = context.id;
        let created_at = context.request_received_at;

        let storage = get_storage().await;
        let storage_type = storage.get_storage_type();

        let mut user_request_body_hash = None;
        if let Some(body) = &context.user_request_body {
            let mut hasher = Sha256::new();
            hasher.update(body);
            let hash = format!("{:x}", hasher.finalize());
            let key = generate_storage_path_from_hash(created_at, &hash, &storage_type);
            if storage
                .put_object(&key, body.clone(), Some("application/json"))
                .await
                .is_ok()
            {
                user_request_body_hash = Some(hash);
            }
        }

        let mut llm_request_body_hash = None;
        if let Some(variant) = &context.llm_request_body {
            let body = match variant {
                RequestBodyVariant::Full(b) => b,
                RequestBodyVariant::Patch(b) => b,
            };

            let mut should_compute_hash = true;
            if let RequestBodyVariant::Full(full_body) = variant {
                if Some(full_body) == context.user_request_body.as_ref() {
                    llm_request_body_hash = user_request_body_hash.clone();
                    should_compute_hash = false;
                }
            }

            if should_compute_hash {
                let mut hasher = Sha256::new();
                hasher.update(body);
                let hash = format!("{:x}", hasher.finalize());
                let key = generate_storage_path_from_hash(created_at, &hash, &storage_type);
                if storage
                    .put_object(&key, body.clone(), Some("application/json"))
                    .await
                    .is_ok()
                {
                    llm_request_body_hash = Some(hash);
                }
            }
        }

        let normalized_llm_response_body_opt = context.llm_response_body.as_ref().map(|body| {
            // Fast path: if there's no '\r', no conversion is needed.
            if !body.contains(&b'\r') {
                return body.clone();
            }
            // Efficiently convert CRLF to LF at the byte level.
            let mut result = Vec::with_capacity(body.len());
            let mut i = 0;
            while i < body.len() {
                if body.get(i) == Some(&b'\r') && body.get(i + 1) == Some(&b'\n') {
                    result.push(b'\n');
                    i += 2;
                } else {
                    result.push(body[i]);
                    i += 1;
                }
            }
            Bytes::from(result)
        });

        let mut llm_response_body_hash = None;
        if let Some(body) = &normalized_llm_response_body_opt {
            let mut hasher = Sha256::new();
            hasher.update(body);
            let hash = format!("{:x}", hasher.finalize());
            let key = generate_storage_path_from_hash(created_at, &hash, &storage_type);
            if storage
                .put_object(&key, body.clone(), Some("text/plain"))
                .await
                .is_ok()
            {
                llm_response_body_hash = Some(hash);
            }
        }

        let mut user_response_body_hash = None;
        if let Some(body) = &context.user_response_body {
            // Compare user body with the *normalized* LLM body to check for equality.
            if normalized_llm_response_body_opt.as_ref() == Some(body) {
                user_response_body_hash = llm_response_body_hash.clone();
            } else {
                let mut hasher = Sha256::new();
                hasher.update(body);
                let hash = format!("{:x}", hasher.finalize());
                let key = generate_storage_path_from_hash(created_at, &hash, &storage_type);
                if storage
                    .put_object(&key, body.clone(), Some("text/plain"))
                    .await
                    .is_ok()
                {
                    user_response_body_hash = Some(hash);
                }
            }
        }

        let now = Utc::now().timestamp_millis();

        let request_log = RequestLog {
            id: context.id,
            system_api_key_id: context.system_api_key_id,
            provider_id: context.provider_id,
            model_id: context.model_id,
            provider_api_key_id: context.provider_api_key_id,
            model_name: context.model_name.clone(),
            real_model_name: context.real_model_name.clone(),
            request_received_at: context.request_received_at,
            client_ip: context.client_ip.clone(),
            llm_request_sent_at: context.llm_request_sent_at.unwrap_or(now),
            created_at: context.request_received_at,
            updated_at: now,
            llm_response_status: context.llm_status.map(|s| s.as_u16() as i32),
            is_stream: context.is_stream,
            llm_response_first_chunk_at: context.first_chunk_ts,
            llm_response_completed_at: context.completion_ts,
            status: Some(context.overall_status.clone()),
            storage_type: Some(storage_type.clone()),
            user_request_body: user_request_body_hash,
            llm_request_body: llm_request_body_hash,
            llm_response_body: llm_response_body_hash,
            user_response_body: user_response_body_hash,
            input_tokens: context.usage.as_ref().map(|u| u.input_tokens),
            output_tokens: context.usage.as_ref().map(|u| u.output_tokens),
            reasoning_tokens: context.usage.as_ref().map(|u| u.reasoning_tokens),
            total_tokens: context.usage.as_ref().map(|u| u.total_tokens),
            calculated_cost: match (&context.usage, &context.billing_plan) {
                (Some(usage), Some(plan)) => Some(calculate_cost(usage, &plan.price_rules)),
                _ => None,
            },
            ..Default::default()
        };

        if let Err(e) = RequestLog::insert(&request_log) {
            error!(
                "LogManager: Failed to insert request log for log_id {}: {:?}",
                log_id, e
            );
        }
    }
}

static LOG_MANAGER: Lazy<LogManager> = Lazy::new(LogManager::new);

pub fn get_log_manager() -> &'static LogManager {
    &LOG_MANAGER
}
