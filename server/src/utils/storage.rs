use crate::schema::enum_def::StorageType;
use chrono::{DateTime, Utc};

pub fn generate_storage_path_from_hash(
    created_at_millis: i64,
    content_hash: &str,
    storage_type: &StorageType,
) -> String {
    // 1. Convert timestamp to YYYY-MM-DD
    let dt = DateTime::from_timestamp_millis(created_at_millis).unwrap_or_else(|| Utc::now());
    let date_str = dt.format("%Y-%m-%d").to_string();

    // 2. Construct path based on the storage type
    match storage_type {
        StorageType::FileSystem => {
            let hash_prefix = &content_hash[..2];
            // Format: {date}/{hash_prefix}/{full_hash}
            format!("{}/{}/{}", date_str, hash_prefix, content_hash)
        }
        StorageType::S3 => {
            // Format: logs/{date}/{full_hash}
            format!("logs/{}/{}", date_str, content_hash)
        }
    }
}
