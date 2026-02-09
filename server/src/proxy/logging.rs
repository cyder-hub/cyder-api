use crate::service::cache::types::{
    CacheBillingPlan, CacheModel, CacheProvider, CacheSystemApiKey,
};
use crate::utils::billing::calculate_cost;
use crate::{
    database::request_log::RequestLog,
    schema::enum_def::RequestStatus,
    service::storage::{get_storage, types::PutObjectOptions, Storage},
    utils::{
        billing::UsageInfo,
        storage::{generate_storage_path_from_id, LogBodies},
        ID_GENERATOR,
    },
};
use bytes::Bytes;
use chrono::Utc;
use cyder_tools::log::{debug, error};
use flate2::{write::GzEncoder, Compression};
use once_cell::sync::Lazy;
use reqwest::StatusCode;
use rmp_serde::to_vec_named;
use serde::Serialize;
use std::io::Write;
use tokio::sync::mpsc;

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

    async fn store_bodies(
        storage: &dyn Storage,
        storage_type: &crate::schema::enum_def::StorageType,
        created_at: i64,
        id: i64,
        bodies: &LogBodies,
    ) -> bool {
        let serialized_body = match to_vec_named(bodies) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to serialize log bodies for log_id {}: {:?}", id, e);
                return false;
            }
        };

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        if let Err(e) = encoder.write_all(&serialized_body) {
            error!("Failed to gzip log bodies for log_id {}: {:?}", id, e);
            return false;
        };
        let compressed_body = match encoder.finish() {
            Ok(v) => Bytes::from(v),
            Err(e) => {
                error!("Failed to finish gzip for log_id {}: {:?}", id, e);
                return false;
            }
        };

        let key = generate_storage_path_from_id(created_at, id, storage_type);

        debug!("Storing log bodies for log_id {}: {:?}", id, key);

        storage
            .put_object(
                &key,
                compressed_body,
                Some(PutObjectOptions {
                    content_type: Some("application/msgpack"),
                    content_encoding: Some("gzip"),
                }),
            )
            .await
            .is_ok()
    }

    async fn process_log(context: RequestLogContext) {
        let log_id = context.id;
        let created_at = context.request_received_at;

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

        let bodies = LogBodies {
            user_request_body: context.user_request_body.clone(),
            llm_request_body: context.llm_request_body.clone(),
            llm_response_body: normalized_llm_response_body_opt.clone(),
            user_response_body: if normalized_llm_response_body_opt.as_ref()
                == context.llm_response_body.as_ref()
            {
                None
            } else {
                context.user_response_body.clone()
            },
        };

        let storage = get_storage().await;
        let mut final_storage_type = None;

        let storage_type = storage.get_storage_type();
        if Self::store_bodies(&**storage, &storage_type, created_at, log_id, &bodies).await {
            final_storage_type = Some(storage_type);
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
            storage_type: final_storage_type,
            user_request_body: None,
            llm_request_body: None,
            llm_response_body: None,
            user_response_body: None,
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
