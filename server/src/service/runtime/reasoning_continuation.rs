use std::{
    collections::HashMap,
    fmt::Display,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use bb8_redis::redis::cmd;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{service::app_state::AppStoreError, service::redis::RedisPool};

pub const DEFAULT_REASONING_CONTINUATION_TTL: Duration = Duration::from_secs(30 * 60);
pub const DEFAULT_REASONING_CONTINUATION_MEMORY_CAPACITY: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReasoningContinuationScope {
    pub api_key_id: i64,
    pub provider_id: i64,
    pub model_id: i64,
    pub route_id: Option<i64>,
    pub route_name: Option<String>,
    pub candidate_position: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReasoningContinuationCacheKey {
    pub scope: ReasoningContinuationScope,
    pub tool_call_ids: Vec<String>,
    pub tool_calls_hash: String,
}

impl ReasoningContinuationCacheKey {
    pub fn new(
        scope: ReasoningContinuationScope,
        mut tool_call_ids: Vec<String>,
        tool_calls_hash: impl Into<String>,
    ) -> Self {
        tool_call_ids.sort();
        tool_call_ids.dedup();
        Self {
            scope,
            tool_call_ids,
            tool_calls_hash: tool_calls_hash.into(),
        }
    }

    fn lookup_key(&self) -> ReasoningContinuationLookupKey {
        ReasoningContinuationLookupKey {
            scope: self.scope.clone(),
            tool_call_ids: self.tool_call_ids.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct ReasoningContinuationLookupKey {
    scope: ReasoningContinuationScope,
    tool_call_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReasoningContinuationSnapshot {
    pub key: ReasoningContinuationCacheKey,
    pub reasoning_content: String,
    pub tool_calls: Value,
    pub observed_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReasoningContinuationRecord {
    pub key: ReasoningContinuationCacheKey,
    pub reasoning_content: String,
    pub tool_calls: Value,
    pub observed_at_ms: i64,
    pub expires_at_ms: i64,
}

impl ReasoningContinuationRecord {
    fn from_snapshot(snapshot: ReasoningContinuationSnapshot, now_ms: i64, ttl: Duration) -> Self {
        let ttl_ms = i64::try_from(ttl.as_millis()).unwrap_or(i64::MAX);
        Self {
            key: snapshot.key,
            reasoning_content: snapshot.reasoning_content,
            tool_calls: snapshot.tool_calls,
            observed_at_ms: snapshot.observed_at_ms,
            expires_at_ms: now_ms.saturating_add(ttl_ms.max(1)),
        }
    }

    fn is_expired(&self, now_ms: i64) -> bool {
        self.expires_at_ms <= now_ms
    }

    fn content_fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.reasoning_content.as_bytes());
        hasher.update([0]);
        hasher.update(serde_json::to_vec(&self.tool_calls).unwrap_or_default());
        format!("{:x}", hasher.finalize())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ReasoningContinuationLookupResult {
    Hit(ReasoningContinuationRecord),
    Miss,
    Expired { expired_count: usize },
    Ambiguous { matched_count: usize },
}

#[async_trait]
pub trait ReasoningContinuationStore: Send + Sync {
    async fn insert(
        &self,
        snapshot: ReasoningContinuationSnapshot,
        now_ms: i64,
    ) -> Result<(), AppStoreError>;

    async fn lookup(
        &self,
        key: &ReasoningContinuationCacheKey,
        now_ms: i64,
    ) -> Result<ReasoningContinuationLookupResult, AppStoreError>;
}

#[derive(Clone)]
pub struct MemoryReasoningContinuationStore {
    inner: Arc<Mutex<HashMap<ReasoningContinuationLookupKey, Vec<ReasoningContinuationRecord>>>>,
    ttl: Duration,
    capacity: usize,
}

impl MemoryReasoningContinuationStore {
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            ttl,
            capacity,
        }
    }

    pub fn default_with_capacity(capacity: usize) -> Self {
        Self::new(DEFAULT_REASONING_CONTINUATION_TTL, capacity)
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    fn prune_expired_locked(
        state: &mut HashMap<ReasoningContinuationLookupKey, Vec<ReasoningContinuationRecord>>,
        now_ms: i64,
    ) {
        state.retain(|_, records| {
            records.retain(|record| !record.is_expired(now_ms));
            !records.is_empty()
        });
    }

    fn prune_capacity_locked(
        state: &mut HashMap<ReasoningContinuationLookupKey, Vec<ReasoningContinuationRecord>>,
        capacity: usize,
    ) {
        if capacity == 0 {
            state.clear();
            return;
        }

        while total_record_count(state) > capacity {
            let oldest_key = state
                .iter()
                .filter_map(|(key, records)| {
                    records
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, record)| record.observed_at_ms)
                        .map(|(index, record)| (key.clone(), index, record.observed_at_ms))
                })
                .min_by_key(|(_, _, observed_at_ms)| *observed_at_ms)
                .map(|(key, index, _)| (key, index));

            let Some((key, index)) = oldest_key else {
                break;
            };
            if let Some(records) = state.get_mut(&key) {
                records.remove(index);
                if records.is_empty() {
                    state.remove(&key);
                }
            }
        }
    }
}

impl Default for MemoryReasoningContinuationStore {
    fn default() -> Self {
        Self::new(
            DEFAULT_REASONING_CONTINUATION_TTL,
            DEFAULT_REASONING_CONTINUATION_MEMORY_CAPACITY,
        )
    }
}

#[async_trait]
impl ReasoningContinuationStore for MemoryReasoningContinuationStore {
    async fn insert(
        &self,
        snapshot: ReasoningContinuationSnapshot,
        now_ms: i64,
    ) -> Result<(), AppStoreError> {
        let mut state = self.inner.lock().map_err(|err| {
            AppStoreError::LockError(format!("reasoning continuation lock poisoned: {err}"))
        })?;
        Self::prune_expired_locked(&mut state, now_ms);
        if self.capacity == 0 {
            return Ok(());
        }

        let record = ReasoningContinuationRecord::from_snapshot(snapshot, now_ms, self.ttl);
        let lookup_key = record.key.lookup_key();
        let fingerprint = record.content_fingerprint();
        let records = state.entry(lookup_key).or_default();
        records.retain(|existing| {
            existing.key.tool_calls_hash != record.key.tool_calls_hash
                || existing.content_fingerprint() != fingerprint
        });
        records.push(record);
        Self::prune_capacity_locked(&mut state, self.capacity);
        Ok(())
    }

    async fn lookup(
        &self,
        key: &ReasoningContinuationCacheKey,
        now_ms: i64,
    ) -> Result<ReasoningContinuationLookupResult, AppStoreError> {
        let mut state = self.inner.lock().map_err(|err| {
            AppStoreError::LockError(format!("reasoning continuation lock poisoned: {err}"))
        })?;
        let lookup_key = key.lookup_key();
        let Some(records) = state.get_mut(&lookup_key) else {
            return Ok(ReasoningContinuationLookupResult::Miss);
        };

        let before_count = records.len();
        records.retain(|record| !record.is_expired(now_ms));
        let expired_count = before_count.saturating_sub(records.len());
        if records.is_empty() {
            state.remove(&lookup_key);
            return Ok(ReasoningContinuationLookupResult::Expired { expired_count });
        }

        let matched = records
            .iter()
            .filter(|record| record.key.tool_calls_hash == key.tool_calls_hash)
            .cloned()
            .collect::<Vec<_>>();

        match matched.len() {
            0 => Ok(ReasoningContinuationLookupResult::Miss),
            1 => Ok(ReasoningContinuationLookupResult::Hit(
                matched.into_iter().next().expect("one match"),
            )),
            matched_count => Ok(ReasoningContinuationLookupResult::Ambiguous { matched_count }),
        }
    }
}

#[derive(Clone)]
pub struct RedisReasoningContinuationStore {
    pool: RedisPool,
    key_prefix: String,
    ttl: Duration,
}

impl RedisReasoningContinuationStore {
    pub fn new(pool: RedisPool, key_prefix: impl Into<String>, ttl: Duration) -> Self {
        Self {
            pool,
            key_prefix: key_prefix.into(),
            ttl,
        }
    }

    pub fn key_prefix(&self) -> &str {
        &self.key_prefix
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    fn cache_key(&self, key: &ReasoningContinuationCacheKey) -> String {
        let route_scope = match (key.scope.route_id, key.scope.route_name.as_deref()) {
            (Some(route_id), _) => format!("route:{route_id}"),
            (None, Some(route_name)) => format!("direct_name:{}", sha256_hex(route_name)),
            (None, None) => "direct".to_string(),
        };
        let tool_call_ids_hash =
            sha256_hex(serde_json::to_string(&key.tool_call_ids).unwrap_or_default());
        format!(
            "{}reasoning_continuation:{}:{}:{}:{}:{}:{}:{}",
            self.key_prefix,
            key.scope.api_key_id,
            key.scope.provider_id,
            key.scope.model_id,
            route_scope,
            key.scope.candidate_position,
            tool_call_ids_hash,
            key.tool_calls_hash
        )
    }

    fn ttl_seconds(&self) -> u64 {
        self.ttl.as_secs().max(1)
    }

    fn record_field(record: &ReasoningContinuationRecord) -> String {
        record.content_fingerprint()
    }

    fn serialize_record(record: &ReasoningContinuationRecord) -> Result<String, AppStoreError> {
        serde_json::to_string(record).map_err(|err| {
            AppStoreError::CacheError(format!(
                "failed to serialize reasoning continuation record: {err}"
            ))
        })
    }

    fn deserialize_record(value: &str) -> Result<ReasoningContinuationRecord, AppStoreError> {
        serde_json::from_str(value).map_err(|err| {
            AppStoreError::CacheError(format!(
                "failed to deserialize reasoning continuation record: {err}"
            ))
        })
    }

    fn lookup_records(
        records: &[ReasoningContinuationRecord],
        key: &ReasoningContinuationCacheKey,
        now_ms: i64,
    ) -> ReasoningContinuationLookupResult {
        if records.is_empty() {
            return ReasoningContinuationLookupResult::Miss;
        }

        let unexpired = records
            .iter()
            .filter(|record| !record.is_expired(now_ms))
            .collect::<Vec<_>>();
        if unexpired.is_empty() {
            return ReasoningContinuationLookupResult::Expired {
                expired_count: records.len(),
            };
        }

        let matched = unexpired
            .into_iter()
            .filter(|record| record.key.tool_calls_hash == key.tool_calls_hash)
            .cloned()
            .collect::<Vec<_>>();

        match matched.len() {
            0 => ReasoningContinuationLookupResult::Miss,
            1 => ReasoningContinuationLookupResult::Hit(
                matched.into_iter().next().expect("one match"),
            ),
            matched_count => ReasoningContinuationLookupResult::Ambiguous { matched_count },
        }
    }

    fn redis_error(context: &str, err: impl Display) -> AppStoreError {
        AppStoreError::CacheError(format!("{context}: {err}"))
    }
}

#[async_trait]
impl ReasoningContinuationStore for RedisReasoningContinuationStore {
    async fn insert(
        &self,
        snapshot: ReasoningContinuationSnapshot,
        now_ms: i64,
    ) -> Result<(), AppStoreError> {
        let record = ReasoningContinuationRecord::from_snapshot(snapshot, now_ms, self.ttl);
        let key = self.cache_key(&record.key);
        let field = Self::record_field(&record);
        let value = Self::serialize_record(&record)?;
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|err| Self::redis_error("failed to get redis connection", err))?;

        let key_type: String = cmd("TYPE")
            .arg(&key)
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_error("failed to inspect reasoning continuation", err))?;
        if key_type == "string" {
            let legacy_value: Option<String> =
                cmd("GET")
                    .arg(&key)
                    .query_async(&mut *conn)
                    .await
                    .map_err(|err| Self::redis_error("failed to load legacy continuation", err))?;
            let _: i64 = cmd("DEL")
                .arg(&key)
                .query_async(&mut *conn)
                .await
                .map_err(|err| {
                    Self::redis_error("failed to replace legacy reasoning continuation", err)
                })?;
            if let Some(legacy_value) = legacy_value {
                let legacy_record = Self::deserialize_record(&legacy_value)?;
                if !legacy_record.is_expired(now_ms) {
                    let legacy_field = Self::record_field(&legacy_record);
                    let legacy_value = Self::serialize_record(&legacy_record)?;
                    let _: i64 = cmd("HSET")
                        .arg(&key)
                        .arg(legacy_field)
                        .arg(legacy_value)
                        .query_async(&mut *conn)
                        .await
                        .map_err(|err| {
                            Self::redis_error(
                                "failed to migrate legacy reasoning continuation",
                                err,
                            )
                        })?;
                }
            }
        } else if key_type != "none" && key_type != "hash" {
            return Err(AppStoreError::CacheError(format!(
                "unexpected redis type for reasoning continuation: {key_type}"
            )));
        }

        let _: i64 = cmd("HSET")
            .arg(&key)
            .arg(field)
            .arg(value)
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_error("failed to store reasoning continuation", err))?;
        let _: i64 = cmd("EXPIRE")
            .arg(&key)
            .arg(self.ttl_seconds())
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_error("failed to expire reasoning continuation", err))?;
        Ok(())
    }

    async fn lookup(
        &self,
        key: &ReasoningContinuationCacheKey,
        now_ms: i64,
    ) -> Result<ReasoningContinuationLookupResult, AppStoreError> {
        let redis_key = self.cache_key(key);
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|err| Self::redis_error("failed to get redis connection", err))?;

        let key_type: String = cmd("TYPE")
            .arg(&redis_key)
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_error("failed to inspect reasoning continuation", err))?;
        if key_type == "none" {
            return Ok(ReasoningContinuationLookupResult::Miss);
        }

        if key_type == "string" {
            let value: Option<String> = cmd("GET")
                .arg(&redis_key)
                .query_async(&mut *conn)
                .await
                .map_err(|err| Self::redis_error("failed to load legacy continuation", err))?;
            let Some(value) = value else {
                return Ok(ReasoningContinuationLookupResult::Miss);
            };
            let record = Self::deserialize_record(&value)?;
            let records = vec![record];
            let result = Self::lookup_records(&records, key, now_ms);
            if matches!(result, ReasoningContinuationLookupResult::Expired { .. }) {
                let _: i64 = cmd("DEL")
                    .arg(&redis_key)
                    .query_async(&mut *conn)
                    .await
                    .map_err(|err| {
                        Self::redis_error("failed to delete expired reasoning continuation", err)
                    })?;
            }
            return Ok(result);
        }

        if key_type != "hash" {
            return Err(AppStoreError::CacheError(format!(
                "unexpected redis type for reasoning continuation: {key_type}"
            )));
        }

        let values: Vec<String> = cmd("HVALS")
            .arg(&redis_key)
            .query_async(&mut *conn)
            .await
            .map_err(|err| Self::redis_error("failed to load reasoning continuation", err))?;
        if values.is_empty() {
            return Ok(ReasoningContinuationLookupResult::Miss);
        }

        let mut records = Vec::with_capacity(values.len());
        for value in values {
            records.push(Self::deserialize_record(&value)?);
        }
        let expired_fields = records
            .iter()
            .filter(|record| record.is_expired(now_ms))
            .map(Self::record_field)
            .collect::<Vec<_>>();
        let result = Self::lookup_records(&records, key, now_ms);

        if matches!(result, ReasoningContinuationLookupResult::Expired { .. }) {
            let _: i64 = cmd("DEL")
                .arg(&redis_key)
                .query_async(&mut *conn)
                .await
                .map_err(|err| {
                    Self::redis_error("failed to delete expired reasoning continuation", err)
                })?;
        } else if !expired_fields.is_empty() {
            let _: i64 = cmd("HDEL")
                .arg(&redis_key)
                .arg(expired_fields)
                .query_async(&mut *conn)
                .await
                .map_err(|err| {
                    Self::redis_error("failed to prune expired reasoning continuation", err)
                })?;
        }

        Ok(result)
    }
}

fn total_record_count(
    state: &HashMap<ReasoningContinuationLookupKey, Vec<ReasoningContinuationRecord>>,
) -> usize {
    state.values().map(Vec::len).sum()
}

fn sha256_hex(input: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(input.as_ref()))
}

pub fn current_time_ms() -> i64 {
    Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb8::Pool;
    use bb8_redis::RedisConnectionManager;
    use serde_json::json;

    fn scope(candidate_position: usize) -> ReasoningContinuationScope {
        ReasoningContinuationScope {
            api_key_id: 10,
            provider_id: 20,
            model_id: 30,
            route_id: Some(40),
            route_name: Some("primary-route".to_string()),
            candidate_position,
        }
    }

    fn cache_key(hash: &str) -> ReasoningContinuationCacheKey {
        ReasoningContinuationCacheKey::new(
            scope(0),
            vec!["call-b".to_string(), "call-a".to_string()],
            hash,
        )
    }

    fn snapshot(
        hash: &str,
        reasoning_content: &str,
        observed_at_ms: i64,
    ) -> ReasoningContinuationSnapshot {
        ReasoningContinuationSnapshot {
            key: cache_key(hash),
            reasoning_content: reasoning_content.to_string(),
            tool_calls: json!([
                {"id":"call-a","type":"function","function":{"name":"a","arguments":"{}"}},
                {"id":"call-b","type":"function","function":{"name":"b","arguments":"{}"}}
            ]),
            observed_at_ms,
        }
    }

    #[tokio::test]
    async fn memory_store_returns_hit_and_normalizes_tool_call_ids() {
        let store = MemoryReasoningContinuationStore::new(Duration::from_secs(60), 16);
        store
            .insert(snapshot("hash-1", "reasoning one", 100), 100)
            .await
            .expect("insert should succeed");

        let result = store
            .lookup(
                &ReasoningContinuationCacheKey::new(
                    scope(0),
                    vec!["call-a".to_string(), "call-b".to_string()],
                    "hash-1",
                ),
                200,
            )
            .await
            .expect("lookup should succeed");

        match result {
            ReasoningContinuationLookupResult::Hit(record) => {
                assert_eq!(record.reasoning_content, "reasoning one");
                assert_eq!(record.key.tool_call_ids, vec!["call-a", "call-b"]);
            }
            other => panic!("expected hit, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn memory_store_returns_miss_for_unknown_or_hash_mismatch() {
        let store = MemoryReasoningContinuationStore::new(Duration::from_secs(60), 16);
        store
            .insert(snapshot("hash-1", "reasoning one", 100), 100)
            .await
            .expect("insert should succeed");

        assert_eq!(
            store.lookup(&cache_key("hash-2"), 200).await.unwrap(),
            ReasoningContinuationLookupResult::Miss
        );
        assert_eq!(
            store
                .lookup(
                    &ReasoningContinuationCacheKey::new(
                        scope(1),
                        vec!["call-a".to_string()],
                        "hash-1"
                    ),
                    200,
                )
                .await
                .unwrap(),
            ReasoningContinuationLookupResult::Miss
        );
    }

    #[tokio::test]
    async fn memory_store_reports_expired_and_prunes() {
        let store = MemoryReasoningContinuationStore::new(Duration::from_millis(10), 16);
        store
            .insert(snapshot("hash-1", "reasoning one", 100), 100)
            .await
            .expect("insert should succeed");

        assert_eq!(
            store.lookup(&cache_key("hash-1"), 111).await.unwrap(),
            ReasoningContinuationLookupResult::Expired { expired_count: 1 }
        );
        assert_eq!(
            store.lookup(&cache_key("hash-1"), 112).await.unwrap(),
            ReasoningContinuationLookupResult::Miss
        );
    }

    #[tokio::test]
    async fn memory_store_reports_ambiguous_when_same_hash_has_distinct_records() {
        let store = MemoryReasoningContinuationStore::new(Duration::from_secs(60), 16);
        store
            .insert(snapshot("hash-1", "reasoning one", 100), 100)
            .await
            .expect("insert one should succeed");
        store
            .insert(snapshot("hash-1", "reasoning two", 101), 101)
            .await
            .expect("insert two should succeed");

        assert_eq!(
            store.lookup(&cache_key("hash-1"), 200).await.unwrap(),
            ReasoningContinuationLookupResult::Ambiguous { matched_count: 2 }
        );
    }

    #[test]
    fn redis_record_field_deduplicates_only_identical_records() {
        let first = ReasoningContinuationRecord::from_snapshot(
            snapshot("hash-1", "reasoning one", 100),
            100,
            Duration::from_secs(60),
        );
        let duplicate = ReasoningContinuationRecord::from_snapshot(
            snapshot("hash-1", "reasoning one", 101),
            101,
            Duration::from_secs(60),
        );
        let distinct = ReasoningContinuationRecord::from_snapshot(
            snapshot("hash-1", "reasoning two", 102),
            102,
            Duration::from_secs(60),
        );

        assert_eq!(
            RedisReasoningContinuationStore::record_field(&first),
            RedisReasoningContinuationStore::record_field(&duplicate)
        );
        assert_ne!(
            RedisReasoningContinuationStore::record_field(&first),
            RedisReasoningContinuationStore::record_field(&distinct)
        );
    }

    #[test]
    fn redis_lookup_records_reports_ambiguous_for_same_hash_distinct_records() {
        let records = vec![
            ReasoningContinuationRecord::from_snapshot(
                snapshot("hash-1", "reasoning one", 100),
                100,
                Duration::from_secs(60),
            ),
            ReasoningContinuationRecord::from_snapshot(
                snapshot("hash-1", "reasoning two", 101),
                101,
                Duration::from_secs(60),
            ),
        ];

        assert_eq!(
            RedisReasoningContinuationStore::lookup_records(&records, &cache_key("hash-1"), 200),
            ReasoningContinuationLookupResult::Ambiguous { matched_count: 2 }
        );
    }

    #[tokio::test]
    async fn memory_store_prunes_oldest_records_by_capacity() {
        let store = MemoryReasoningContinuationStore::new(Duration::from_secs(60), 1);
        store
            .insert(snapshot("hash-1", "reasoning one", 100), 100)
            .await
            .expect("insert one should succeed");
        store
            .insert(snapshot("hash-2", "reasoning two", 101), 101)
            .await
            .expect("insert two should succeed");

        assert_eq!(
            store.lookup(&cache_key("hash-1"), 102).await.unwrap(),
            ReasoningContinuationLookupResult::Miss
        );
        assert!(matches!(
            store.lookup(&cache_key("hash-2"), 102).await.unwrap(),
            ReasoningContinuationLookupResult::Hit(_)
        ));
    }

    #[tokio::test]
    async fn redis_store_uses_runtime_prefix_and_state_ttl() {
        let manager = RedisConnectionManager::new("redis://127.0.0.1:1")
            .expect("redis test URL should be valid");
        let pool = Pool::builder().build_unchecked(manager);
        let store =
            RedisReasoningContinuationStore::new(pool, "cyder:runtime:", Duration::from_secs(123));

        assert_eq!(store.key_prefix(), "cyder:runtime:");
        assert_eq!(store.ttl(), Duration::from_secs(123));
        assert!(
            store
                .cache_key(&cache_key("hash-1"))
                .contains("reasoning_continuation:10:20:30:route:40:0:")
        );
    }
}
