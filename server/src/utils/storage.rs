use crate::schema::enum_def::StorageType;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LogBodyCaptureState {
    Complete,
    Incomplete,
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
    use super::{LogBodyCaptureState, LogBundle, StorageType, generate_storage_path_from_id};
    use bytes::Bytes;

    #[test]
    fn log_bundle_stores_response_capture_state() {
        let bundle = LogBundle {
            version: 1,
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
    fn generate_storage_path_from_id_uses_expected_layout() {
        let filesystem_path =
            generate_storage_path_from_id(1_744_100_800_000, 123456, &StorageType::FileSystem);
        assert_eq!(filesystem_path, "2025/04/08/12/123456.mp.gz");

        let s3_path = generate_storage_path_from_id(1_744_100_800_000, 123456, &StorageType::S3);
        assert_eq!(s3_path, "logs/2025/04/08/123456.mp.gz");
    }
}
