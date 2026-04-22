use crate::schema::enum_def::{LlmApiType, StorageType};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub const REQUEST_LOG_BUNDLE_V1_VERSION: u32 = 1;
pub const REQUEST_LOG_BUNDLE_V2_VERSION: u32 = 2;

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
pub struct LogBundle {
    pub version: u32,
    pub log_id: i64,
    pub created_at: i64,
    pub user_request_body: Option<Bytes>,
    pub llm_request_body: Option<Bytes>,
    pub llm_response_body: Option<Bytes>,
    pub llm_response_capture_state: Option<LogBodyCaptureState>,
    pub user_response_body: Option<Bytes>,
    pub user_response_capture_state: Option<LogBodyCaptureState>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
pub struct RequestLogBundleV2 {
    pub version: u32,
    pub log_id: i64,
    pub created_at: i64,
    pub request_section: RequestLogBundleRequestSection,
    pub attempt_sections: Vec<RequestLogBundleAttemptSection>,
    pub blob_pool: Vec<RequestLogBundleBlob>,
    pub patch_pool: Vec<RequestLogBundlePatch>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
pub struct RequestLogBundleRequestSection {
    pub user_request_blob_id: Option<i32>,
    pub user_response_blob_id: Option<i32>,
    pub user_response_capture_state: Option<LogBodyCaptureState>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
pub struct RequestLogBundleAttemptSection {
    pub attempt_id: Option<i64>,
    pub attempt_index: i32,
    pub llm_request_blob_id: Option<i32>,
    pub llm_request_patch_id: Option<i32>,
    pub llm_response_blob_id: Option<i32>,
    pub llm_response_capture_state: Option<LogBodyCaptureState>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RequestLogBundleBlob {
    pub blob_id: i32,
    pub media_type: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub body: Bytes,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RequestLogBundlePatch {
    pub patch_id: i32,
    pub format: String,
    pub target_sha256: String,
    pub target_size_bytes: i64,
    pub patch_body: Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestLogBundleRequestBodyRef {
    pub blob_id: i32,
    pub patch_id: Option<i32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LogBodyCaptureState {
    Complete,
    Incomplete,
    NotCaptured,
}

#[derive(Default)]
pub struct RequestLogBundleV2Builder {
    blob_pool: Vec<RequestLogBundleBlob>,
    blob_ids_by_key: HashMap<(String, String), i32>,
    patch_pool: Vec<RequestLogBundlePatch>,
    user_request_base: Option<RequestBodyPatchBase>,
    attempt_request_bases: Vec<RequestBodyPatchBase>,
}

#[derive(Debug, Clone)]
struct RequestBodyPatchBase {
    blob_id: i32,
    body: Bytes,
    llm_api_type: Option<LlmApiType>,
}

impl RequestLogBundleV2Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_blob(&mut self, media_type: &str, body: Bytes) -> i32 {
        let digest = sha256_hex(&body);
        let media_type = media_type.to_string();
        let key = (digest.clone(), media_type.clone());
        if let Some(blob_id) = self.blob_ids_by_key.get(&key) {
            return *blob_id;
        }

        let blob_id = i32::try_from(self.blob_pool.len() + 1).unwrap_or(i32::MAX);
        self.blob_pool.push(RequestLogBundleBlob {
            blob_id,
            media_type,
            sha256: digest.clone(),
            size_bytes: body.len() as i64,
            body,
        });
        self.blob_ids_by_key.insert(key, blob_id);
        blob_id
    }

    pub fn add_response_body(&mut self, body: Bytes) -> i32 {
        self.add_blob("application/octet-stream", body)
    }

    pub fn add_user_request_body(&mut self, body: Bytes) -> i32 {
        let canonical_body = canonical_json_bytes(body);
        let blob_id = self.add_blob("application/json", canonical_body.clone());
        self.user_request_base = Some(RequestBodyPatchBase {
            blob_id,
            body: canonical_body,
            llm_api_type: None,
        });
        blob_id
    }

    pub fn add_llm_request_body(
        &mut self,
        user_api_type: LlmApiType,
        llm_api_type: LlmApiType,
        attempt_index: i32,
        body: Bytes,
    ) -> RequestLogBundleRequestBodyRef {
        let canonical_body = canonical_json_bytes(body);

        if let Some(base) = self.user_request_base.clone() {
            if base.body == canonical_body {
                self.register_attempt_request_base(
                    attempt_index,
                    llm_api_type,
                    base.blob_id,
                    canonical_body,
                );
                return RequestLogBundleRequestBodyRef {
                    blob_id: base.blob_id,
                    patch_id: None,
                };
            }

            if user_api_type == llm_api_type {
                if let Some(patch_body) = build_json_patch_body(&base.body, &canonical_body) {
                    if patch_body.len() < canonical_body.len() {
                        let patch_id = self.add_patch(&base.body, patch_body);
                        return RequestLogBundleRequestBodyRef {
                            blob_id: base.blob_id,
                            patch_id: Some(patch_id),
                        };
                    }
                }
            }
        }

        if let Some(base) = self
            .attempt_request_bases
            .iter()
            .find(|base| base.llm_api_type == Some(llm_api_type) && base.body == canonical_body)
            .cloned()
        {
            self.register_attempt_request_base(
                attempt_index,
                llm_api_type,
                base.blob_id,
                canonical_body,
            );
            return RequestLogBundleRequestBodyRef {
                blob_id: base.blob_id,
                patch_id: None,
            };
        }

        let mut best_patch: Option<(usize, i32, Bytes, Bytes)> = None;
        for base in self
            .attempt_request_bases
            .iter()
            .filter(|base| base.llm_api_type == Some(llm_api_type))
        {
            let Some(patch_body) = build_json_patch_body(&base.body, &canonical_body) else {
                continue;
            };
            if patch_body.len() >= canonical_body.len() {
                continue;
            }
            let candidate = (
                patch_body.len(),
                base.blob_id,
                base.body.clone(),
                patch_body,
            );
            if best_patch
                .as_ref()
                .map_or(true, |best| candidate.0 < best.0)
            {
                best_patch = Some(candidate);
            }
        }
        if let Some((_size, blob_id, base_body, patch_body)) = best_patch {
            let patch_id = self.add_patch(&base_body, patch_body);
            return RequestLogBundleRequestBodyRef {
                blob_id,
                patch_id: Some(patch_id),
            };
        }

        let blob_id = self.add_blob("application/json", canonical_body.clone());
        self.register_attempt_request_base(attempt_index, llm_api_type, blob_id, canonical_body);
        RequestLogBundleRequestBodyRef {
            blob_id,
            patch_id: None,
        }
    }

    pub fn add_patch(&mut self, target_body: &[u8], patch_body: Bytes) -> i32 {
        let patch_id = i32::try_from(self.patch_pool.len() + 1).unwrap_or(i32::MAX);
        self.patch_pool.push(RequestLogBundlePatch {
            patch_id,
            format: "application/json-patch+json".to_string(),
            target_sha256: sha256_hex(target_body),
            target_size_bytes: target_body.len() as i64,
            patch_body,
        });
        patch_id
    }

    fn register_attempt_request_base(
        &mut self,
        _attempt_index: i32,
        llm_api_type: LlmApiType,
        blob_id: i32,
        body: Bytes,
    ) {
        self.attempt_request_bases.push(RequestBodyPatchBase {
            blob_id,
            body,
            llm_api_type: Some(llm_api_type),
        });
    }

    pub fn finish(
        self,
        log_id: i64,
        created_at: i64,
        request_section: RequestLogBundleRequestSection,
        attempt_sections: Vec<RequestLogBundleAttemptSection>,
    ) -> RequestLogBundleV2 {
        RequestLogBundleV2 {
            version: REQUEST_LOG_BUNDLE_V2_VERSION,
            log_id,
            created_at,
            request_section,
            attempt_sections,
            blob_pool: self.blob_pool,
            patch_pool: self.patch_pool,
        }
    }
}

fn sha256_hex(body: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(body.as_ref()))
}

fn canonical_json_bytes(body: Bytes) -> Bytes {
    serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|value| serde_json::to_vec(&value).ok())
        .map(Bytes::from)
        .unwrap_or(body)
}

fn build_json_patch_body(base_body: &[u8], final_body: &[u8]) -> Option<Bytes> {
    let base_value = serde_json::from_slice::<Value>(base_body).ok()?;
    let final_value = serde_json::from_slice::<Value>(final_body).ok()?;
    let patch = json_patch::diff(&base_value, &final_value);
    if patch.is_empty() {
        return None;
    }
    serde_json::to_vec(&patch).ok().map(Bytes::from)
}

pub fn generate_storage_path_from_id(
    created_at_millis: i64,
    id: i64,
    storage_type: &StorageType,
) -> String {
    let dt = DateTime::from_timestamp_millis(created_at_millis).unwrap_or_else(|| Utc::now());
    let date_str = dt.format("%Y/%m/%d").to_string();
    let id_str = id.to_string();

    match storage_type {
        StorageType::FileSystem => {
            let len = id_str.len();
            if len >= 6 {
                let sub_dir = &id_str[len - 6..len - 4];
                format!("{}/{}/{}.mp.gz", date_str, sub_dir, id_str)
            } else {
                format!("{}/{}.mp.gz", date_str, id_str)
            }
        }
        StorageType::S3 => {
            format!("logs/{}/{}.mp.gz", date_str, id_str)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LogBodyCaptureState, LogBundle, REQUEST_LOG_BUNDLE_V1_VERSION,
        REQUEST_LOG_BUNDLE_V2_VERSION, RequestLogBundleAttemptSection,
        RequestLogBundleRequestSection, RequestLogBundleV2Builder, StorageType,
        generate_storage_path_from_id,
    };
    use crate::schema::enum_def::LlmApiType;
    use bytes::Bytes;
    use serde_json::{Value, json};

    #[test]
    fn log_bundle_stores_response_capture_state() {
        let bundle = LogBundle {
            version: REQUEST_LOG_BUNDLE_V1_VERSION,
            log_id: 42,
            created_at: 1_744_100_800_000,
            user_request_body: Some(Bytes::from_static(b"user request")),
            llm_request_body: Some(Bytes::from_static(b"llm request")),
            llm_response_body: Some(Bytes::from_static(b"llm response")),
            llm_response_capture_state: Some(LogBodyCaptureState::Incomplete),
            user_response_body: Some(Bytes::from_static(b"user response")),
            user_response_capture_state: Some(LogBodyCaptureState::Complete),
        };

        assert_eq!(
            bundle.llm_response_capture_state,
            Some(LogBodyCaptureState::Incomplete)
        );
        assert_eq!(
            bundle.user_response_capture_state,
            Some(LogBodyCaptureState::Complete)
        );
    }

    #[test]
    fn request_log_bundle_v2_assigns_stable_blob_ids_and_reuses_body_blobs() {
        let mut builder = RequestLogBundleV2Builder::new();
        let user_request_blob_id =
            builder.add_user_request_body(Bytes::from_static(br#"{"input":"hi"}"#));
        let repeated_request_body_ref = builder.add_llm_request_body(
            LlmApiType::Openai,
            LlmApiType::Openai,
            1,
            Bytes::from_static(br#"{"input":"hi"}"#),
        );
        let response_blob_id = builder.add_response_body(Bytes::from_static(b"ok"));

        let bundle = builder.finish(
            42,
            1_744_100_800_000,
            RequestLogBundleRequestSection {
                user_request_blob_id: Some(user_request_blob_id),
                user_response_blob_id: Some(response_blob_id),
                user_response_capture_state: Some(LogBodyCaptureState::Complete),
            },
            vec![RequestLogBundleAttemptSection {
                attempt_id: Some(101),
                attempt_index: 1,
                llm_request_blob_id: Some(repeated_request_body_ref.blob_id),
                llm_request_patch_id: repeated_request_body_ref.patch_id,
                llm_response_blob_id: Some(response_blob_id),
                llm_response_capture_state: Some(LogBodyCaptureState::Complete),
            }],
        );

        assert_eq!(bundle.version, REQUEST_LOG_BUNDLE_V2_VERSION);
        assert_eq!(user_request_blob_id, 1);
        assert_eq!(repeated_request_body_ref.blob_id, user_request_blob_id);
        assert_eq!(repeated_request_body_ref.patch_id, None);
        assert_eq!(response_blob_id, 2);
        assert_eq!(bundle.blob_pool.len(), 2);
        assert_eq!(bundle.patch_pool.len(), 0);
        assert_eq!(
            bundle.attempt_sections[0].llm_request_blob_id,
            Some(user_request_blob_id)
        );
    }

    #[test]
    fn request_log_bundle_v2_reuses_identical_response_bodies_without_patches() {
        let mut builder = RequestLogBundleV2Builder::new();
        let user_response_blob_id = builder.add_response_body(Bytes::from_static(b"same response"));
        let llm_response_blob_id = builder.add_response_body(Bytes::from_static(b"same response"));

        let bundle = builder.finish(
            42,
            1_744_100_800_000,
            RequestLogBundleRequestSection {
                user_request_blob_id: None,
                user_response_blob_id: Some(user_response_blob_id),
                user_response_capture_state: Some(LogBodyCaptureState::Complete),
            },
            vec![RequestLogBundleAttemptSection {
                attempt_id: Some(101),
                attempt_index: 1,
                llm_request_blob_id: None,
                llm_request_patch_id: None,
                llm_response_blob_id: Some(llm_response_blob_id),
                llm_response_capture_state: Some(LogBodyCaptureState::Complete),
            }],
        );

        assert_eq!(user_response_blob_id, llm_response_blob_id);
        assert_eq!(bundle.blob_pool.len(), 1);
        assert_eq!(bundle.patch_pool.len(), 0);
        assert_eq!(
            bundle.request_section.user_response_capture_state,
            Some(LogBodyCaptureState::Complete)
        );
        assert_eq!(
            bundle.attempt_sections[0].llm_response_capture_state,
            Some(LogBodyCaptureState::Complete)
        );
    }

    #[test]
    fn request_log_bundle_v2_uses_user_request_patch_when_smaller() {
        let user_value = json!({
            "model": "old-model",
            "messages": [{
                "role": "user",
                "content": "This long prompt makes the final body larger than a model-only JSON patch."
            }]
        });
        let final_value = json!({
            "model": "new-model",
            "messages": [{
                "role": "user",
                "content": "This long prompt makes the final body larger than a model-only JSON patch."
            }]
        });
        let user_body = Bytes::from(serde_json::to_vec(&user_value).unwrap());
        let final_body = Bytes::from(serde_json::to_vec(&final_value).unwrap());

        let mut builder = RequestLogBundleV2Builder::new();
        let user_blob_id = builder.add_user_request_body(user_body);
        let request_ref =
            builder.add_llm_request_body(LlmApiType::Openai, LlmApiType::Openai, 1, final_body);
        let bundle = builder.finish(
            42,
            1_744_100_800_000,
            RequestLogBundleRequestSection {
                user_request_blob_id: Some(user_blob_id),
                user_response_blob_id: None,
                user_response_capture_state: None,
            },
            vec![RequestLogBundleAttemptSection {
                attempt_id: Some(101),
                attempt_index: 1,
                llm_request_blob_id: Some(request_ref.blob_id),
                llm_request_patch_id: request_ref.patch_id,
                llm_response_blob_id: None,
                llm_response_capture_state: None,
            }],
        );

        assert_eq!(request_ref.blob_id, user_blob_id);
        assert!(request_ref.patch_id.is_some());
        assert_eq!(bundle.blob_pool.len(), 1);
        assert_eq!(bundle.patch_pool.len(), 1);

        let mut reconstructed: Value = serde_json::from_slice(&bundle.blob_pool[0].body).unwrap();
        let patch: json_patch::Patch =
            serde_json::from_slice(&bundle.patch_pool[0].patch_body).unwrap();
        json_patch::patch(&mut reconstructed, &patch).unwrap();
        assert_eq!(reconstructed, final_value);
    }

    #[test]
    fn request_log_bundle_v2_writes_full_blob_when_user_patch_is_cross_api() {
        let user_value = json!({
            "model": "old-model",
            "messages": [{"role": "user", "content": "same prompt"}]
        });
        let final_value = json!({
            "model": "new-model",
            "messages": [{"role": "user", "content": "same prompt"}]
        });

        let mut builder = RequestLogBundleV2Builder::new();
        let user_blob_id =
            builder.add_user_request_body(Bytes::from(serde_json::to_vec(&user_value).unwrap()));
        let request_ref = builder.add_llm_request_body(
            LlmApiType::Openai,
            LlmApiType::Anthropic,
            1,
            Bytes::from(serde_json::to_vec(&final_value).unwrap()),
        );

        assert_eq!(user_blob_id, 1);
        assert_eq!(request_ref.blob_id, 2);
        assert_eq!(request_ref.patch_id, None);
    }

    #[test]
    fn generate_storage_path_from_id_uses_expected_layout() {
        let filesystem_path =
            generate_storage_path_from_id(1_744_100_800_000, 123456, &StorageType::FileSystem);
        assert_eq!(filesystem_path, "2025/04/08/12/123456.mp.gz");

        let s3_path = generate_storage_path_from_id(1_744_100_800_000, 123456, &StorageType::S3);
        assert_eq!(s3_path, "logs/2025/04/08/123456.mp.gz");
    }
}
