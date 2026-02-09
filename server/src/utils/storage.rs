use crate::schema::enum_def::StorageType;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Serialize, Default, Debug, Clone)]
pub struct LogBodies {
    pub user_request_body: Option<Bytes>,
    pub llm_request_body: Option<Bytes>,
    pub llm_response_body: Option<Bytes>,
    pub user_response_body: Option<Bytes>,
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
