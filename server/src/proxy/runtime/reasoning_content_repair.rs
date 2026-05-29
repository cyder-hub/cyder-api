use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::service::{
    app_state::AppStoreError,
    runtime::{
        ReasoningContinuationCacheKey, ReasoningContinuationLookupResult,
        ReasoningContinuationScope, ReasoningContinuationSnapshot, ReasoningContinuationStore,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReasoningContentRepairResultKey {
    Disabled,
    NotApplicable,
    CacheMiss,
    Ambiguous,
    Matched,
    AlreadyPresent,
    Expired,
    ParseFailed,
    ExplicitReasoningDisabled,
}

impl ReasoningContentRepairResultKey {
    pub(crate) fn as_key(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::NotApplicable => "not_applicable",
            Self::CacheMiss => "cache_miss",
            Self::Ambiguous => "ambiguous",
            Self::Matched => "matched",
            Self::AlreadyPresent => "already_present",
            Self::Expired => "expired",
            Self::ParseFailed => "parse_failed",
            Self::ExplicitReasoningDisabled => "explicit_reasoning_disabled",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReasoningContentRepairDiagnostic {
    pub result: ReasoningContentRepairResultKey,
    pub message_index: Option<usize>,
    pub tool_call_ids: Vec<String>,
    pub tool_calls_hash: Option<String>,
    pub content_hash: Option<String>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReasoningContentRepairReport {
    pub repaired_count: usize,
    pub diagnostics: Vec<ReasoningContentRepairDiagnostic>,
}

pub(crate) struct ReasoningContentRepairRequest<'a> {
    pub body: &'a mut Value,
    pub scope: ReasoningContinuationScope,
    pub store: &'a dyn ReasoningContinuationStore,
    pub feature_enabled: bool,
    pub target_is_openai_compatible_generation: bool,
    pub explicit_reasoning_disabled: bool,
    pub now_ms: i64,
}

struct AssistantToolCallMessage {
    index: usize,
    tool_call_ids: Vec<String>,
    tool_calls_hash: String,
    content_hash: Option<String>,
    already_has_reasoning: bool,
}

pub(crate) async fn repair_openai_reasoning_content(
    request: ReasoningContentRepairRequest<'_>,
) -> Result<ReasoningContentRepairReport, AppStoreError> {
    if !request.feature_enabled {
        return Ok(single_result(ReasoningContentRepairResultKey::Disabled));
    }
    if !request.target_is_openai_compatible_generation {
        return Ok(single_result(
            ReasoningContentRepairResultKey::NotApplicable,
        ));
    }
    if request.explicit_reasoning_disabled {
        return Ok(single_result(
            ReasoningContentRepairResultKey::ExplicitReasoningDisabled,
        ));
    }

    let mut messages = match assistant_tool_call_messages(request.body) {
        Ok(messages) => messages,
        Err(detail) => {
            return Ok(single_result_with_detail(
                ReasoningContentRepairResultKey::ParseFailed,
                detail,
            ));
        }
    };

    if messages.is_empty() {
        return Ok(single_result(
            ReasoningContentRepairResultKey::NotApplicable,
        ));
    }

    let mut report = ReasoningContentRepairReport::default();
    for message in messages.drain(..) {
        if message.already_has_reasoning {
            report.diagnostics.push(message_diagnostic(
                ReasoningContentRepairResultKey::AlreadyPresent,
                &message,
                None,
            ));
            continue;
        }

        let cache_key = ReasoningContinuationCacheKey::new(
            request.scope.clone(),
            message.tool_call_ids.clone(),
            message.tool_calls_hash.clone(),
        );
        let lookup_result = match request.store.lookup(&cache_key, request.now_ms).await {
            Ok(result) => result,
            Err(err) => {
                report.diagnostics.push(message_diagnostic(
                    ReasoningContentRepairResultKey::CacheMiss,
                    &message,
                    Some(format!("store_error={err}")),
                ));
                continue;
            }
        };

        match lookup_result {
            ReasoningContinuationLookupResult::Hit(record) => {
                if record.reasoning_content.is_empty() {
                    report.diagnostics.push(message_diagnostic(
                        ReasoningContentRepairResultKey::CacheMiss,
                        &message,
                        Some("empty_reasoning_content".to_string()),
                    ));
                    continue;
                }
                if insert_reasoning_content(request.body, message.index, record.reasoning_content) {
                    report.repaired_count += 1;
                    report.diagnostics.push(message_diagnostic(
                        ReasoningContentRepairResultKey::Matched,
                        &message,
                        None,
                    ));
                } else {
                    report.diagnostics.push(message_diagnostic(
                        ReasoningContentRepairResultKey::ParseFailed,
                        &message,
                        Some("message_mutation_failed".to_string()),
                    ));
                }
            }
            ReasoningContinuationLookupResult::Miss => {
                report.diagnostics.push(message_diagnostic(
                    ReasoningContentRepairResultKey::CacheMiss,
                    &message,
                    None,
                ));
            }
            ReasoningContinuationLookupResult::Expired { expired_count } => {
                report.diagnostics.push(message_diagnostic(
                    ReasoningContentRepairResultKey::Expired,
                    &message,
                    Some(format!("expired_count={expired_count}")),
                ));
            }
            ReasoningContinuationLookupResult::Ambiguous { matched_count } => {
                report.diagnostics.push(message_diagnostic(
                    ReasoningContentRepairResultKey::Ambiguous,
                    &message,
                    Some(format!("matched_count={matched_count}")),
                ));
            }
        }
    }

    Ok(report)
}

pub(crate) fn continuation_snapshot_from_assistant_message(
    scope: ReasoningContinuationScope,
    message: &Value,
    observed_at_ms: i64,
) -> Result<Option<ReasoningContinuationSnapshot>, ReasoningContentRepairResultKey> {
    let Some(message_obj) = message.as_object() else {
        return Err(ReasoningContentRepairResultKey::ParseFailed);
    };
    if message_obj.get("role").and_then(Value::as_str) != Some("assistant") {
        return Ok(None);
    }
    let Some(reasoning_content) = message_obj
        .get("reasoning_content")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let Some(tool_calls) = message_obj.get("tool_calls") else {
        return Ok(None);
    };
    continuation_snapshot_from_parts(scope, reasoning_content, tool_calls, observed_at_ms)
}

pub(crate) fn continuation_snapshots_from_openai_response_body(
    scope: ReasoningContinuationScope,
    body: &[u8],
    observed_at_ms: i64,
) -> Result<Vec<ReasoningContinuationSnapshot>, ReasoningContentRepairResultKey> {
    let value = serde_json::from_slice::<Value>(body)
        .map_err(|_| ReasoningContentRepairResultKey::ParseFailed)?;
    let choices = value
        .as_object()
        .and_then(|body| body.get("choices"))
        .and_then(Value::as_array)
        .ok_or(ReasoningContentRepairResultKey::ParseFailed)?;

    let mut snapshots = Vec::new();
    for choice in choices {
        let Some(message) = choice.as_object().and_then(|choice| choice.get("message")) else {
            continue;
        };
        if let Some(snapshot) =
            continuation_snapshot_from_assistant_message(scope.clone(), message, observed_at_ms)?
        {
            snapshots.push(snapshot);
        }
    }

    Ok(snapshots)
}

pub(crate) fn continuation_snapshot_from_parts(
    scope: ReasoningContinuationScope,
    reasoning_content: &str,
    tool_calls: &Value,
    observed_at_ms: i64,
) -> Result<Option<ReasoningContinuationSnapshot>, ReasoningContentRepairResultKey> {
    if reasoning_content.is_empty() {
        return Ok(None);
    }
    let Some(tool_call_ids) = tool_call_ids(tool_calls) else {
        return Err(ReasoningContentRepairResultKey::ParseFailed);
    };
    if tool_call_ids.is_empty() {
        return Ok(None);
    }
    let tool_calls_hash = canonical_tool_calls_hash(tool_calls)
        .ok_or(ReasoningContentRepairResultKey::ParseFailed)?;
    Ok(Some(ReasoningContinuationSnapshot {
        key: ReasoningContinuationCacheKey::new(scope, tool_call_ids, tool_calls_hash),
        reasoning_content: reasoning_content.to_string(),
        tool_calls: tool_calls.clone(),
        observed_at_ms,
    }))
}

pub(crate) fn canonical_tool_calls_hash(tool_calls: &Value) -> Option<String> {
    if !tool_calls.is_array() {
        return None;
    }
    let canonical = canonicalize_json_value(tool_calls);
    serde_json::to_vec(&canonical).ok().map(sha256_hex)
}

fn assistant_tool_call_messages(body: &Value) -> Result<Vec<AssistantToolCallMessage>, String> {
    let body_obj = body
        .as_object()
        .ok_or_else(|| "body_not_object".to_string())?;
    let messages = body_obj
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "messages_not_array".to_string())?;
    let mut result = Vec::new();

    for (index, message) in messages.iter().enumerate() {
        let Some(message_obj) = message.as_object() else {
            continue;
        };
        if message_obj.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(tool_calls) = message_obj.get("tool_calls") else {
            continue;
        };
        let Some(tool_call_ids) = tool_call_ids(tool_calls) else {
            return Err("assistant_tool_calls_invalid".to_string());
        };
        if tool_call_ids.is_empty() {
            continue;
        }
        let Some(tool_calls_hash) = canonical_tool_calls_hash(tool_calls) else {
            return Err("assistant_tool_calls_invalid".to_string());
        };
        result.push(AssistantToolCallMessage {
            index,
            tool_call_ids,
            tool_calls_hash,
            content_hash: message_obj.get("content").map(canonical_value_hash),
            already_has_reasoning: message_obj
                .get("reasoning_content")
                .is_some_and(reasoning_content_is_present),
        });
    }

    Ok(result)
}

fn insert_reasoning_content(
    body: &mut Value,
    message_index: usize,
    reasoning_content: String,
) -> bool {
    let Some(messages) = body
        .as_object_mut()
        .and_then(|body| body.get_mut("messages"))
        .and_then(Value::as_array_mut)
    else {
        return false;
    };
    let Some(message) = messages
        .get_mut(message_index)
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    if message
        .get("reasoning_content")
        .is_some_and(reasoning_content_is_present)
    {
        return false;
    }
    message.insert(
        "reasoning_content".to_string(),
        Value::String(reasoning_content),
    );
    true
}

fn reasoning_content_is_present(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(text) => !text.is_empty(),
        _ => true,
    }
}

fn tool_call_ids(tool_calls: &Value) -> Option<Vec<String>> {
    let calls = tool_calls.as_array()?;
    let mut ids = Vec::with_capacity(calls.len());
    for call in calls {
        let id = call.get("id").and_then(Value::as_str)?;
        if id.is_empty() {
            return None;
        }
        ids.push(id.to_string());
    }
    ids.sort();
    ids.dedup();
    Some(ids)
}

fn canonicalize_json_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_json_value).collect()),
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            let mut canonical = Map::new();
            for key in keys {
                canonical.insert(key.clone(), canonicalize_json_value(&map[key]));
            }
            Value::Object(canonical)
        }
        other => other.clone(),
    }
}

fn canonical_value_hash(value: &Value) -> String {
    let canonical = canonicalize_json_value(value);
    sha256_hex(serde_json::to_vec(&canonical).unwrap_or_default())
}

fn sha256_hex(input: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(input.as_ref()))
}

fn single_result(result: ReasoningContentRepairResultKey) -> ReasoningContentRepairReport {
    ReasoningContentRepairReport {
        repaired_count: 0,
        diagnostics: vec![ReasoningContentRepairDiagnostic {
            result,
            message_index: None,
            tool_call_ids: vec![],
            tool_calls_hash: None,
            content_hash: None,
            detail: None,
        }],
    }
}

fn single_result_with_detail(
    result: ReasoningContentRepairResultKey,
    detail: String,
) -> ReasoningContentRepairReport {
    ReasoningContentRepairReport {
        repaired_count: 0,
        diagnostics: vec![ReasoningContentRepairDiagnostic {
            result,
            message_index: None,
            tool_call_ids: vec![],
            tool_calls_hash: None,
            content_hash: None,
            detail: Some(detail),
        }],
    }
}

fn message_diagnostic(
    result: ReasoningContentRepairResultKey,
    message: &AssistantToolCallMessage,
    detail: Option<String>,
) -> ReasoningContentRepairDiagnostic {
    ReasoningContentRepairDiagnostic {
        result,
        message_index: Some(message.index),
        tool_call_ids: message.tool_call_ids.clone(),
        tool_calls_hash: Some(message.tool_calls_hash.clone()),
        content_hash: message.content_hash.clone(),
        detail,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;

    use async_trait::async_trait;
    use serde_json::{Value, json};

    use super::*;
    use crate::service::runtime::MemoryReasoningContinuationStore;

    fn scope() -> ReasoningContinuationScope {
        ReasoningContinuationScope {
            api_key_id: 11,
            provider_id: 22,
            model_id: 33,
            route_id: Some(44),
            route_name: Some("primary".to_string()),
            candidate_position: 0,
        }
    }

    fn tool_calls() -> Value {
        json!([
            {
                "type": "function",
                "function": { "arguments": "{\"city\":\"Paris\"}", "name": "weather" },
                "id": "call-weather"
            }
        ])
    }

    fn request_body_without_reasoning() -> Value {
        json!({
            "model": "deepseek-reasoner",
            "messages": [
                { "role": "user", "content": "weather" },
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": tool_calls()
                },
                { "role": "tool", "tool_call_id": "call-weather", "content": "{}" }
            ]
        })
    }

    async fn seed_store(
        store: &MemoryReasoningContinuationStore,
        reasoning_content: &str,
        now_ms: i64,
    ) {
        let snapshot =
            continuation_snapshot_from_parts(scope(), reasoning_content, &tool_calls(), now_ms)
                .expect("snapshot should parse")
                .expect("snapshot should exist");
        store
            .insert(snapshot, now_ms)
            .await
            .expect("store insert should succeed");
    }

    #[tokio::test]
    async fn missing_reasoning_unique_match_inserts_message_level_reasoning_content() {
        let store = MemoryReasoningContinuationStore::new(Duration::from_secs(60), 16);
        seed_store(&store, "SECRET_REASONING_CONTENT", 100).await;
        let mut body = request_body_without_reasoning();

        let report = repair_openai_reasoning_content(ReasoningContentRepairRequest {
            body: &mut body,
            scope: scope(),
            store: &store,
            feature_enabled: true,
            target_is_openai_compatible_generation: true,
            explicit_reasoning_disabled: false,
            now_ms: 200,
        })
        .await
        .expect("repair should succeed");

        assert_eq!(report.repaired_count, 1);
        assert_eq!(
            report.diagnostics[0].result,
            ReasoningContentRepairResultKey::Matched
        );
        assert_eq!(
            body["messages"][1]["reasoning_content"],
            "SECRET_REASONING_CONTENT"
        );
    }

    #[tokio::test]
    async fn already_present_does_not_overwrite_or_read_store() {
        let store = CountingStore::default();
        let mut body = request_body_without_reasoning();
        body["messages"][1]["reasoning_content"] = json!("CLIENT_REASONING");

        let report = repair_openai_reasoning_content(ReasoningContentRepairRequest {
            body: &mut body,
            scope: scope(),
            store: &store,
            feature_enabled: true,
            target_is_openai_compatible_generation: true,
            explicit_reasoning_disabled: false,
            now_ms: 200,
        })
        .await
        .expect("repair should succeed");

        assert_eq!(
            report.diagnostics[0].result,
            ReasoningContentRepairResultKey::AlreadyPresent
        );
        assert_eq!(body["messages"][1]["reasoning_content"], "CLIENT_REASONING");
        assert_eq!(store.lookup_count(), 0);
    }

    #[tokio::test]
    async fn ambiguous_cache_match_does_not_modify_body() {
        let store = MemoryReasoningContinuationStore::new(Duration::from_secs(60), 16);
        seed_store(&store, "first reasoning", 100).await;
        seed_store(&store, "second reasoning", 101).await;
        let mut body = request_body_without_reasoning();

        let report = repair_openai_reasoning_content(ReasoningContentRepairRequest {
            body: &mut body,
            scope: scope(),
            store: &store,
            feature_enabled: true,
            target_is_openai_compatible_generation: true,
            explicit_reasoning_disabled: false,
            now_ms: 200,
        })
        .await
        .expect("repair should succeed");

        assert_eq!(
            report.diagnostics[0].result,
            ReasoningContentRepairResultKey::Ambiguous
        );
        assert!(body["messages"][1].get("reasoning_content").is_none());
    }

    #[tokio::test]
    async fn hash_mismatch_is_cache_miss_and_does_not_modify_body() {
        let store = MemoryReasoningContinuationStore::new(Duration::from_secs(60), 16);
        seed_store(&store, "reasoning", 100).await;
        let mut body = request_body_without_reasoning();
        body["messages"][1]["tool_calls"][0]["function"]["arguments"] =
            json!("{\"city\":\"Rome\"}");

        let report = repair_openai_reasoning_content(ReasoningContentRepairRequest {
            body: &mut body,
            scope: scope(),
            store: &store,
            feature_enabled: true,
            target_is_openai_compatible_generation: true,
            explicit_reasoning_disabled: false,
            now_ms: 200,
        })
        .await
        .expect("repair should succeed");

        assert_eq!(
            report.diagnostics[0].result,
            ReasoningContentRepairResultKey::CacheMiss
        );
        assert!(body["messages"][1].get("reasoning_content").is_none());
    }

    #[tokio::test]
    async fn invalid_body_shape_is_parse_failed() {
        let store = MemoryReasoningContinuationStore::new(Duration::from_secs(60), 16);
        let mut body = json!({ "messages": "not-array" });

        let report = repair_openai_reasoning_content(ReasoningContentRepairRequest {
            body: &mut body,
            scope: scope(),
            store: &store,
            feature_enabled: true,
            target_is_openai_compatible_generation: true,
            explicit_reasoning_disabled: false,
            now_ms: 200,
        })
        .await
        .expect("repair should succeed");

        assert_eq!(
            report.diagnostics[0].result,
            ReasoningContentRepairResultKey::ParseFailed
        );
    }

    #[tokio::test]
    async fn disabled_not_applicable_and_explicit_disabled_do_not_read_store() {
        let cases = [
            (
                false,
                true,
                false,
                ReasoningContentRepairResultKey::Disabled,
            ),
            (
                true,
                false,
                false,
                ReasoningContentRepairResultKey::NotApplicable,
            ),
            (
                true,
                true,
                true,
                ReasoningContentRepairResultKey::ExplicitReasoningDisabled,
            ),
        ];

        for (feature_enabled, target, explicit_disabled, expected) in cases {
            let store = CountingStore::default();
            let mut body = request_body_without_reasoning();
            let report = repair_openai_reasoning_content(ReasoningContentRepairRequest {
                body: &mut body,
                scope: scope(),
                store: &store,
                feature_enabled,
                target_is_openai_compatible_generation: target,
                explicit_reasoning_disabled: explicit_disabled,
                now_ms: 200,
            })
            .await
            .expect("repair should succeed");

            assert_eq!(report.diagnostics[0].result, expected);
            assert_eq!(store.lookup_count(), 0);
        }
    }

    #[test]
    fn canonical_tool_calls_hash_is_stable_for_object_key_order() {
        let left = json!([
            {
                "id": "call-weather",
                "type": "function",
                "function": { "name": "weather", "arguments": "{}" }
            }
        ]);
        let right = json!([
            {
                "function": { "arguments": "{}", "name": "weather" },
                "type": "function",
                "id": "call-weather"
            }
        ]);

        assert_eq!(
            canonical_tool_calls_hash(&left),
            canonical_tool_calls_hash(&right)
        );
    }

    #[test]
    fn assistant_message_snapshot_requires_reasoning_and_tool_calls() {
        let message = json!({
            "role": "assistant",
            "reasoning_content": "observed reasoning",
            "tool_calls": tool_calls()
        });
        let snapshot = continuation_snapshot_from_assistant_message(scope(), &message, 123)
            .expect("snapshot parsing should succeed")
            .expect("snapshot should exist");

        assert_eq!(snapshot.reasoning_content, "observed reasoning");
        assert_eq!(snapshot.key.tool_call_ids, vec!["call-weather"]);
    }

    #[test]
    fn openai_response_body_snapshots_parse_choice_messages() {
        let body = json!({
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "reasoning_content": "observed response reasoning",
                        "tool_calls": tool_calls()
                    }
                }
            ]
        });

        let snapshots = continuation_snapshots_from_openai_response_body(
            scope(),
            body.to_string().as_bytes(),
            321,
        )
        .expect("response body should parse");

        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].reasoning_content,
            "observed response reasoning"
        );
        assert_eq!(snapshots[0].key.tool_call_ids, vec!["call-weather"]);
    }

    #[derive(Default)]
    struct CountingStore {
        lookup_count: Arc<AtomicUsize>,
    }

    impl CountingStore {
        fn lookup_count(&self) -> usize {
            self.lookup_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl ReasoningContinuationStore for CountingStore {
        async fn insert(
            &self,
            _snapshot: ReasoningContinuationSnapshot,
            _now_ms: i64,
        ) -> Result<(), AppStoreError> {
            Ok(())
        }

        async fn lookup(
            &self,
            _key: &ReasoningContinuationCacheKey,
            _now_ms: i64,
        ) -> Result<ReasoningContinuationLookupResult, AppStoreError> {
            self.lookup_count.fetch_add(1, Ordering::SeqCst);
            Ok(ReasoningContinuationLookupResult::Miss)
        }
    }
}
