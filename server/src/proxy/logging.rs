use crate::service::cache::types::{
    CacheBillingPlan, CacheModel, CacheProvider, CacheSystemApiKey,
};
use crate::{
    database::request_log::{NewRequestLog, RequestLog, UpdateRequestLogData},
    schema::enum_def::RequestStatus,
    service::storage::get_storage,
    utils::{
        billing::{populate_token_cost_fields, UsageInfo},
        storage::generate_storage_path_from_hash,
        ID_GENERATOR,
    },
};
use bytes::Bytes;
use chrono::Utc;
use cyder_tools::log::{debug, error};
use once_cell::sync::Lazy;
use reqwest::StatusCode;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

// server/src/proxy/logging.rs

// ... (imports)

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
    pub external_request_uri: Option<String>,
    pub channel: Option<String>,
    pub external_id: Option<String>,
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
    pub llm_request_body: Option<Bytes>,
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
        request_uri_path: &str,
        channel: &Option<String>,
        external_id: &Option<String>,
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
            external_request_uri: Some(request_uri_path.to_string()),
            channel: channel.clone(),
            external_id: external_id.clone(),
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
        let mut user_request_body_key = None;
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
                user_request_body_key = Some(key);
            }
        }

        let mut llm_request_body_hash = None;
        let mut llm_request_body_key = None;
        if let Some(body) = &context.llm_request_body {
            if Some(body) == context.user_request_body.as_ref() {
                llm_request_body_hash = user_request_body_hash.clone();
                llm_request_body_key = user_request_body_key.clone();
            } else {
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
                    llm_request_body_key = Some(key);
                }
            }
        }

        let mut llm_response_body_hash = None;
        let mut llm_response_body_key = None;
        if let Some(body) = &context.llm_response_body {
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
                llm_response_body_key = Some(key);
            }
        }

        let mut user_response_body_hash = None;
        let mut user_response_body_key = None;
        if let Some(body) = &context.user_response_body {
            if Some(body) == context.llm_response_body.as_ref() {
                user_response_body_hash = llm_response_body_hash.clone();
                user_response_body_key = llm_response_body_key.clone();
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
                    user_response_body_key = Some(key);
                }
            }
        }

        let now = Utc::now().timestamp_millis();
        let initial_log_data = NewRequestLog {
            id: context.id,
            system_api_key_id: context.system_api_key_id,
            provider_id: context.provider_id,
            model_id: context.model_id,
            provider_api_key_id: context.provider_api_key_id,
            model_name: context.model_name.clone(),
            real_model_name: context.real_model_name.clone(),
            request_received_at: context.request_received_at,
            client_ip: context.client_ip.clone(),
            external_request_uri: context.external_request_uri.clone(),
            status: RequestStatus::Pending,
            llm_request_sent_at: context.llm_request_sent_at.unwrap_or(now),
            created_at: context.request_received_at,
            updated_at: now,
            channel: context.channel.clone(),
            external_id: context.external_id.clone(),
        };

        if let Err(e) = RequestLog::create_initial_request(&initial_log_data) {
            error!(
                "LogManager: Failed to create initial request log for log_id {}: {:?}",
                log_id, e
            );
        }

        let mut update_data = UpdateRequestLogData {
            llm_request_uri: Some(context.request_url),
            llm_response_status: context.llm_status.map(|s| Some(s.as_u16() as i32)),
            is_stream: Some(context.is_stream),
            llm_response_first_chunk_at: context.first_chunk_ts,
            llm_response_completed_at: context.completion_ts,
            response_sent_to_client_at: context.completion_ts,
            status: Some(context.overall_status.clone()),
            storage_type: Some(Some(storage_type.clone())),
            user_request_body: user_request_body_hash.map(Some),
            llm_request_body: llm_request_body_hash.map(Some),
            llm_response_body: llm_response_body_hash.map(Some),
            user_response_body: user_response_body_hash.map(Some),
            ..Default::default()
        };

        populate_token_cost_fields(
            &mut update_data,
            context.usage.as_ref(),
            context.billing_plan.as_ref(),
        );

        debug!(
            "LogManager: Updating request log {} with status {:?}",
            log_id, context.overall_status
        );

        if let Err(e) = RequestLog::update_request_with_completion_details(log_id, &update_data) {
            error!(
                "LogManager: Failed to update request log for log_id {}: {:?}",
                log_id, e
            );
        }

        debug!(
            "LogManager: Log {} processed. Paths: user_request_body: {:?}, llm_request_body: {:?}, llm_response_body: {:?}, user_response_body: {:?}",
            log_id,
            user_request_body_key,
            llm_request_body_key,
            llm_response_body_key,
            user_response_body_key
        );
    }
}

static LOG_MANAGER: Lazy<LogManager> = Lazy::new(LogManager::new);

pub fn get_log_manager() -> &'static LogManager {
    &LOG_MANAGER
}