use chrono::Utc;
use diesel::prelude::*;
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{DbResult, get_connection};
use crate::controller::BaseError;
use crate::database::model::Model;
use crate::database::provider::Provider;
use crate::schema::enum_def::{RequestPatchOperation, RequestPatchPlacement};
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

const CONFIRM_DANGEROUS_TARGET_FIELD: &str = "confirm_dangerous_target";
const HARD_FORBIDDEN_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "transfer-encoding",
    "accept-encoding",
];
const DANGEROUS_HEADERS: &[&str] = &["authorization", "x-api-key", "x-goog-api-key"];
const HARD_FORBIDDEN_BODY_PREFIXES: &[&str] = &["/messages", "/tools", "/contents", "/input"];
const DANGEROUS_BODY_TARGETS: &[&str] = &["/model"];
const DANGEROUS_QUERY_TARGETS: &[&str] = &["key"];

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, Serialize)]
    #[diesel(table_name = request_patch_rule)]
    pub struct RequestPatchRule {
        pub id: i64,
        pub provider_id: Option<i64>,
        pub model_id: Option<i64>,
        pub placement: RequestPatchPlacement,
        pub target: String,
        pub operation: RequestPatchOperation,
        pub value_json: Option<String>,
        pub description: Option<String>,
        pub is_enabled: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Debug)]
    #[diesel(table_name = request_patch_rule)]
    pub struct NewRequestPatchRule {
        pub id: i64,
        pub provider_id: Option<i64>,
        pub model_id: Option<i64>,
        pub placement: RequestPatchPlacement,
        pub target: String,
        pub operation: RequestPatchOperation,
        pub value_json: Option<String>,
        pub description: Option<String>,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset, Debug, Default)]
    #[diesel(table_name = request_patch_rule)]
    pub struct UpdateRequestPatchRuleData {
        pub placement: Option<RequestPatchPlacement>,
        pub target: Option<String>,
        pub operation: Option<RequestPatchOperation>,
        pub value_json: Option<Option<String>>,
        pub description: Option<Option<String>>,
        pub is_enabled: Option<bool>,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RequestPatchScopeKind {
    Provider,
    Model,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RequestPatchRuleResponse {
    pub id: i64,
    pub provider_id: Option<i64>,
    pub model_id: Option<i64>,
    pub scope: RequestPatchScopeKind,
    pub placement: RequestPatchPlacement,
    pub target: String,
    pub operation: RequestPatchOperation,
    pub value_json: Option<Value>,
    pub description: Option<String>,
    pub is_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRequestPatchPayload {
    pub placement: RequestPatchPlacement,
    pub target: String,
    pub operation: RequestPatchOperation,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub value_json: Option<Option<Value>>,
    pub description: Option<String>,
    pub is_enabled: Option<bool>,
    pub confirm_dangerous_target: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateRequestPatchPayload {
    pub placement: Option<RequestPatchPlacement>,
    pub target: Option<String>,
    pub operation: Option<RequestPatchOperation>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub value_json: Option<Option<Value>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub description: Option<Option<String>>,
    pub is_enabled: Option<bool>,
    pub confirm_dangerous_target: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RequestPatchDangerousTargetConfirmation {
    pub placement: RequestPatchPlacement,
    pub target: String,
    pub reason: String,
    pub confirm_field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum RequestPatchMutationOutcome {
    Saved {
        rule: RequestPatchRuleResponse,
    },
    ConfirmationRequired {
        confirmation: RequestPatchDangerousTargetConfirmation,
    },
}

#[derive(Clone, Copy, Debug)]
enum RequestPatchScope {
    Provider(i64),
    Model(i64),
}

#[derive(Debug, Clone)]
struct NormalizedRequestPatchInput {
    placement: RequestPatchPlacement,
    target: String,
    operation: RequestPatchOperation,
    value_json: Option<Value>,
    description: Option<String>,
    is_enabled: bool,
    confirm_dangerous_target: bool,
}

fn parse_json_text(raw: &str, context: &str) -> DbResult<Value> {
    serde_json::from_str(raw).map_err(|err| {
        BaseError::DatabaseFatal(Some(format!("{context} contains invalid JSON: {err}")))
    })
}

fn build_rule_response(row: &RequestPatchRule) -> DbResult<RequestPatchRuleResponse> {
    let value_json = match row.value_json.as_deref() {
        Some(raw) => Some(parse_json_text(raw, "request_patch_rule.value_json")?),
        None => None,
    };

    let scope = if row.provider_id.is_some() {
        RequestPatchScopeKind::Provider
    } else {
        RequestPatchScopeKind::Model
    };

    Ok(RequestPatchRuleResponse {
        id: row.id,
        provider_id: row.provider_id,
        model_id: row.model_id,
        scope,
        placement: row.placement,
        target: row.target.clone(),
        operation: row.operation,
        value_json,
        description: row.description.clone(),
        is_enabled: row.is_enabled,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn json_input_to_value(input: Option<Option<Value>>) -> Option<Value> {
    match input {
        None => None,
        Some(None) => Some(Value::Null),
        Some(Some(value)) => Some(value),
    }
}

fn json_scalar_to_string(value: &Value) -> DbResult<String> {
    match value {
        Value::Null => Ok("null".to_string()),
        Value::Bool(v) => Ok(v.to_string()),
        Value::Number(v) => Ok(v.to_string()),
        Value::String(v) => Ok(v.clone()),
        Value::Array(_) | Value::Object(_) => Err(BaseError::ParamInvalid(Some(
            "HEADER and QUERY rules only accept JSON scalar values".to_string(),
        ))),
    }
}

fn normalize_header_target(target: &str) -> DbResult<String> {
    if target.is_empty() || target.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "request patch target cannot be empty".to_string(),
        )));
    }
    if target.trim() != target {
        return Err(BaseError::ParamInvalid(Some(
            "HEADER target cannot contain surrounding whitespace".to_string(),
        )));
    }

    let normalized = target.to_ascii_lowercase();
    HeaderName::from_bytes(normalized.as_bytes()).map_err(|err| {
        BaseError::ParamInvalid(Some(format!("Invalid HEADER target '{}': {}", target, err)))
    })?;
    Ok(normalized)
}

fn normalize_query_target(target: &str) -> DbResult<String> {
    if target.is_empty() || target.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "request patch target cannot be empty".to_string(),
        )));
    }
    if target.trim() != target {
        return Err(BaseError::ParamInvalid(Some(
            "QUERY target cannot contain surrounding whitespace".to_string(),
        )));
    }
    if target
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace() || matches!(ch, '&' | '=' | '#' | '?'))
    {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Invalid QUERY target '{}'",
            target
        ))));
    }
    Ok(target.to_string())
}

fn validate_json_pointer(target: &str) -> DbResult<()> {
    if target.is_empty() || target.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "request patch target cannot be empty".to_string(),
        )));
    }
    if target.trim() != target {
        return Err(BaseError::ParamInvalid(Some(
            "BODY target cannot contain surrounding whitespace".to_string(),
        )));
    }
    if !target.starts_with('/') {
        return Err(BaseError::ParamInvalid(Some(format!(
            "BODY target '{}' must be a JSON Pointer",
            target
        ))));
    }

    for segment in target.split('/').skip(1) {
        let mut chars = segment.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '~' {
                match chars.next() {
                    Some('0') | Some('1') => {}
                    _ => {
                        return Err(BaseError::ParamInvalid(Some(format!(
                            "BODY target '{}' contains an invalid JSON Pointer escape",
                            target
                        ))));
                    }
                }
            }
        }
    }
    Ok(())
}

fn normalize_target(placement: RequestPatchPlacement, target: &str) -> DbResult<String> {
    match placement {
        RequestPatchPlacement::Header => normalize_header_target(target),
        RequestPatchPlacement::Query => normalize_query_target(target),
        RequestPatchPlacement::Body => {
            validate_json_pointer(target)?;
            Ok(target.to_string())
        }
    }
}

fn validate_value_for_placement(
    placement: RequestPatchPlacement,
    operation: RequestPatchOperation,
    value_json: &Option<Value>,
) -> DbResult<Option<String>> {
    match operation {
        RequestPatchOperation::Remove => {
            if value_json.is_some() {
                return Err(BaseError::ParamInvalid(Some(
                    "REMOVE rules must not include value_json".to_string(),
                )));
            }
            Ok(None)
        }
        RequestPatchOperation::Set => {
            let value = value_json.as_ref().ok_or_else(|| {
                BaseError::ParamInvalid(Some("SET rules must include value_json".to_string()))
            })?;

            match placement {
                RequestPatchPlacement::Header => {
                    let rendered = json_scalar_to_string(value)?;
                    HeaderValue::from_str(&rendered).map_err(|err| {
                        BaseError::ParamInvalid(Some(format!(
                            "Invalid HEADER value for target: {}",
                            err
                        )))
                    })?;
                    Ok(Some(serde_json::to_string(value).map_err(|err| {
                        BaseError::ParamInvalid(Some(format!(
                            "Failed to serialize value_json: {}",
                            err
                        )))
                    })?))
                }
                RequestPatchPlacement::Query => {
                    json_scalar_to_string(value)?;
                    Ok(Some(serde_json::to_string(value).map_err(|err| {
                        BaseError::ParamInvalid(Some(format!(
                            "Failed to serialize value_json: {}",
                            err
                        )))
                    })?))
                }
                RequestPatchPlacement::Body => {
                    Ok(Some(serde_json::to_string(value).map_err(|err| {
                        BaseError::ParamInvalid(Some(format!(
                            "Failed to serialize value_json: {}",
                            err
                        )))
                    })?))
                }
            }
        }
    }
}

fn matches_body_prefix(target: &str, prefix: &str) -> bool {
    target == prefix || target.starts_with(&format!("{prefix}/"))
}

fn validate_reserved_target(
    placement: RequestPatchPlacement,
    target: &str,
) -> DbResult<Option<RequestPatchDangerousTargetConfirmation>> {
    match placement {
        RequestPatchPlacement::Header => {
            if HARD_FORBIDDEN_HEADERS.contains(&target) {
                return Err(BaseError::ParamInvalid(Some(format!(
                    "HEADER target '{}' is reserved and cannot be modified",
                    target
                ))));
            }
            if DANGEROUS_HEADERS.contains(&target) {
                return Ok(Some(RequestPatchDangerousTargetConfirmation {
                    placement,
                    target: target.to_string(),
                    reason: "This target changes upstream authentication semantics".to_string(),
                    confirm_field: CONFIRM_DANGEROUS_TARGET_FIELD.to_string(),
                }));
            }
        }
        RequestPatchPlacement::Query => {
            if DANGEROUS_QUERY_TARGETS.contains(&target) {
                return Ok(Some(RequestPatchDangerousTargetConfirmation {
                    placement,
                    target: target.to_string(),
                    reason: "This target changes query-based upstream credentials".to_string(),
                    confirm_field: CONFIRM_DANGEROUS_TARGET_FIELD.to_string(),
                }));
            }
        }
        RequestPatchPlacement::Body => {
            if HARD_FORBIDDEN_BODY_PREFIXES
                .iter()
                .any(|prefix| matches_body_prefix(target, prefix))
            {
                return Err(BaseError::ParamInvalid(Some(format!(
                    "BODY target '{}' is reserved and cannot be modified",
                    target
                ))));
            }
            if DANGEROUS_BODY_TARGETS.contains(&target) {
                return Ok(Some(RequestPatchDangerousTargetConfirmation {
                    placement,
                    target: target.to_string(),
                    reason: "This target changes upstream model routing semantics".to_string(),
                    confirm_field: CONFIRM_DANGEROUS_TARGET_FIELD.to_string(),
                }));
            }
        }
    }
    Ok(None)
}

fn requires_confirmation(
    existing: Option<&RequestPatchRule>,
    candidate: &NormalizedRequestPatchInput,
) -> DbResult<Option<RequestPatchDangerousTargetConfirmation>> {
    let dangerous = validate_reserved_target(candidate.placement, &candidate.target)?;
    if candidate.confirm_dangerous_target {
        return Ok(None);
    }

    let Some(confirmation) = dangerous else {
        return Ok(None);
    };

    if let Some(existing_rule) = existing {
        if existing_rule.placement == candidate.placement
            && existing_rule.target == candidate.target
        {
            return Ok(None);
        }
    }

    Ok(Some(confirmation))
}

fn is_body_ancestor_or_descendant(left: &str, right: &str) -> bool {
    matches_body_prefix(left, right) || matches_body_prefix(right, left)
}

fn detect_body_conflict<'a>(
    mut existing_targets: impl Iterator<Item = &'a str>,
    candidate_target: &str,
) -> Option<String> {
    existing_targets
        .find(|existing| is_body_ancestor_or_descendant(existing, candidate_target))
        .map(ToString::to_string)
}

fn scope_identity_conflict_message(
    scope: RequestPatchScope,
    placement: RequestPatchPlacement,
    target: &str,
) -> String {
    match scope {
        RequestPatchScope::Provider(provider_id) => format!(
            "provider {} already has an active {:?} rule for target '{}'",
            provider_id, placement, target
        ),
        RequestPatchScope::Model(model_id) => format!(
            "model {} already has an active {:?} rule for target '{}'",
            model_id, placement, target
        ),
    }
}

fn scope_body_conflict_message(
    scope: RequestPatchScope,
    candidate: &str,
    existing: &str,
) -> String {
    match scope {
        RequestPatchScope::Provider(provider_id) => format!(
            "provider {} BODY target '{}' conflicts with existing BODY target '{}'",
            provider_id, candidate, existing
        ),
        RequestPatchScope::Model(model_id) => format!(
            "model {} BODY target '{}' conflicts with existing BODY target '{}'",
            model_id, candidate, existing
        ),
    }
}

fn map_write_error(context: &str, err: diesel::result::Error) -> BaseError {
    match err {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        ) => BaseError::DatabaseDup(Some(context.to_string())),
        other => BaseError::DatabaseFatal(Some(format!("{context}: {other}"))),
    }
}

impl RequestPatchScope {
    fn ensure_exists(self) -> DbResult<()> {
        match self {
            Self::Provider(provider_id) => Provider::get_by_id(provider_id).map(|_| ()),
            Self::Model(model_id) => Model::get_by_id(model_id).map(|_| ()),
        }
    }

    fn provider_id(self) -> Option<i64> {
        match self {
            Self::Provider(provider_id) => Some(provider_id),
            Self::Model(_) => None,
        }
    }

    fn model_id(self) -> Option<i64> {
        match self {
            Self::Provider(_) => None,
            Self::Model(model_id) => Some(model_id),
        }
    }
}

fn resolve_create_payload(
    payload: &CreateRequestPatchPayload,
) -> DbResult<NormalizedRequestPatchInput> {
    let placement = payload.placement;
    let target = normalize_target(placement, &payload.target)?;
    let operation = payload.operation;
    let value_json = json_input_to_value(payload.value_json.clone());
    validate_value_for_placement(placement, operation, &value_json)?;

    Ok(NormalizedRequestPatchInput {
        placement,
        target,
        operation,
        value_json,
        description: payload.description.clone(),
        is_enabled: payload.is_enabled.unwrap_or(true),
        confirm_dangerous_target: payload.confirm_dangerous_target.unwrap_or(false),
    })
}

fn resolve_update_payload(
    existing: &RequestPatchRule,
    payload: &UpdateRequestPatchPayload,
) -> DbResult<NormalizedRequestPatchInput> {
    let placement = payload.placement.unwrap_or(existing.placement);
    let target_source = payload.target.as_deref().unwrap_or(&existing.target);
    let target = normalize_target(placement, target_source)?;
    let operation = payload.operation.unwrap_or(existing.operation);
    let provided_value_json = json_input_to_value(payload.value_json.clone());
    let existing_value_json = match existing.value_json.as_deref() {
        Some(raw) => Some(parse_json_text(raw, "request_patch_rule.value_json")?),
        None => None,
    };
    let value_json = match payload.value_json {
        Some(_) => provided_value_json,
        None => existing_value_json,
    };

    if operation == RequestPatchOperation::Remove && payload.value_json.is_some() {
        return Err(BaseError::ParamInvalid(Some(
            "REMOVE rules must not include value_json".to_string(),
        )));
    }
    if operation == RequestPatchOperation::Set
        && value_json.is_none()
        && existing.operation == RequestPatchOperation::Remove
    {
        return Err(BaseError::ParamInvalid(Some(
            "SET rules must include value_json".to_string(),
        )));
    }

    validate_value_for_placement(placement, operation, &value_json)?;

    Ok(NormalizedRequestPatchInput {
        placement,
        target,
        operation,
        value_json: if operation == RequestPatchOperation::Remove {
            None
        } else {
            value_json
        },
        description: match payload.description.clone() {
            Some(description) => description,
            None => existing.description.clone(),
        },
        is_enabled: payload.is_enabled.unwrap_or(existing.is_enabled),
        confirm_dangerous_target: payload.confirm_dangerous_target.unwrap_or(false),
    })
}

impl RequestPatchRule {
    pub fn list_all() -> DbResult<Vec<RequestPatchRuleResponse>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = request_patch_rule::table
                .filter(request_patch_rule::dsl::deleted_at.is_null())
                .order((
                    request_patch_rule::dsl::provider_id.asc(),
                    request_patch_rule::dsl::model_id.asc(),
                    request_patch_rule::dsl::placement.asc(),
                    request_patch_rule::dsl::target.asc(),
                    request_patch_rule::dsl::created_at.asc(),
                    request_patch_rule::dsl::id.asc(),
                ))
                .select(RequestPatchRuleDb::as_select())
                .load::<RequestPatchRuleDb>(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list all request patch rules: {}",
                        err
                    )))
                })?;

            rows.into_iter()
                .map(|row| build_rule_response(&row.from_db()))
                .collect()
        })
    }

    fn list_by_scope(scope: RequestPatchScope) -> DbResult<Vec<RequestPatchRuleResponse>> {
        scope.ensure_exists()?;
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let mut query = request_patch_rule::table
                .into_boxed()
                .filter(request_patch_rule::dsl::deleted_at.is_null());

            query = match scope {
                RequestPatchScope::Provider(provider_id) => query
                    .filter(request_patch_rule::dsl::provider_id.eq(provider_id))
                    .filter(request_patch_rule::dsl::model_id.is_null()),
                RequestPatchScope::Model(model_id) => query
                    .filter(request_patch_rule::dsl::model_id.eq(model_id))
                    .filter(request_patch_rule::dsl::provider_id.is_null()),
            };

            let rows = query
                .order((
                    request_patch_rule::dsl::created_at.asc(),
                    request_patch_rule::dsl::id.asc(),
                ))
                .select(RequestPatchRuleDb::as_select())
                .load::<RequestPatchRuleDb>(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list request patch rules: {}",
                        err
                    )))
                })?;

            rows.into_iter()
                .map(|row| build_rule_response(&row.from_db()))
                .collect()
        })
    }

    fn get_by_scope(scope: RequestPatchScope, rule_id: i64) -> DbResult<RequestPatchRule> {
        scope.ensure_exists()?;
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let mut query = request_patch_rule::table
                .into_boxed()
                .filter(request_patch_rule::dsl::id.eq(rule_id))
                .filter(request_patch_rule::dsl::deleted_at.is_null());

            query = match scope {
                RequestPatchScope::Provider(provider_id) => query
                    .filter(request_patch_rule::dsl::provider_id.eq(provider_id))
                    .filter(request_patch_rule::dsl::model_id.is_null()),
                RequestPatchScope::Model(model_id) => query
                    .filter(request_patch_rule::dsl::model_id.eq(model_id))
                    .filter(request_patch_rule::dsl::provider_id.is_null()),
            };

            query
                .select(RequestPatchRuleDb::as_select())
                .first::<RequestPatchRuleDb>(conn)
                .map(|row| row.from_db())
                .map_err(|err| match err {
                    diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                        "Request patch rule {} not found",
                        rule_id
                    ))),
                    other => BaseError::DatabaseFatal(Some(format!(
                        "Failed to fetch request patch rule {}: {}",
                        rule_id, other
                    ))),
                })
        })
    }

    fn validate_scope_conflicts(
        scope: RequestPatchScope,
        candidate: &NormalizedRequestPatchInput,
        exclude_rule_id: Option<i64>,
    ) -> DbResult<()> {
        if !candidate.is_enabled {
            return Ok(());
        }

        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let mut identity_query = request_patch_rule::table
                .into_boxed()
                .filter(request_patch_rule::dsl::deleted_at.is_null())
                .filter(request_patch_rule::dsl::is_enabled.eq(true))
                .filter(request_patch_rule::dsl::placement.eq(candidate.placement))
                .filter(request_patch_rule::dsl::target.eq(&candidate.target));

            identity_query = match scope {
                RequestPatchScope::Provider(provider_id) => identity_query
                    .filter(request_patch_rule::dsl::provider_id.eq(provider_id))
                    .filter(request_patch_rule::dsl::model_id.is_null()),
                RequestPatchScope::Model(model_id) => identity_query
                    .filter(request_patch_rule::dsl::model_id.eq(model_id))
                    .filter(request_patch_rule::dsl::provider_id.is_null()),
            };

            if let Some(rule_id) = exclude_rule_id {
                identity_query = identity_query.filter(request_patch_rule::dsl::id.ne(rule_id));
            }

            let identity_conflict_count = identity_query
                .select(diesel::dsl::count_star())
                .first::<i64>(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to validate request patch identity conflicts: {}",
                        err
                    )))
                })?;

            if identity_conflict_count > 0 {
                return Err(BaseError::DatabaseDup(Some(
                    scope_identity_conflict_message(scope, candidate.placement, &candidate.target),
                )));
            }

            if candidate.placement == RequestPatchPlacement::Body {
                let mut body_query = request_patch_rule::table
                    .into_boxed()
                    .filter(request_patch_rule::dsl::deleted_at.is_null())
                    .filter(request_patch_rule::dsl::is_enabled.eq(true))
                    .filter(request_patch_rule::dsl::placement.eq(RequestPatchPlacement::Body));

                body_query = match scope {
                    RequestPatchScope::Provider(provider_id) => body_query
                        .filter(request_patch_rule::dsl::provider_id.eq(provider_id))
                        .filter(request_patch_rule::dsl::model_id.is_null()),
                    RequestPatchScope::Model(model_id) => body_query
                        .filter(request_patch_rule::dsl::model_id.eq(model_id))
                        .filter(request_patch_rule::dsl::provider_id.is_null()),
                };

                if let Some(rule_id) = exclude_rule_id {
                    body_query = body_query.filter(request_patch_rule::dsl::id.ne(rule_id));
                }

                let rows = body_query
                    .order(request_patch_rule::dsl::id.asc())
                    .select(RequestPatchRuleDb::as_select())
                    .load::<RequestPatchRuleDb>(conn)
                    .map_err(|err| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to validate BODY request patch conflicts: {}",
                            err
                        )))
                    })?;

                if let Some(conflict_target) = detect_body_conflict(
                    rows.iter()
                        .map(|row| row.clone().from_db().target)
                        .collect::<Vec<_>>()
                        .iter()
                        .map(|target| target.as_str()),
                    &candidate.target,
                ) {
                    return Err(BaseError::ParamInvalid(Some(scope_body_conflict_message(
                        scope,
                        &candidate.target,
                        &conflict_target,
                    ))));
                }
            }

            Ok(())
        })
    }

    fn create_by_scope(
        scope: RequestPatchScope,
        payload: &CreateRequestPatchPayload,
    ) -> DbResult<RequestPatchMutationOutcome> {
        scope.ensure_exists()?;
        let normalized = resolve_create_payload(payload)?;

        if let Some(confirmation) = requires_confirmation(None, &normalized)? {
            return Ok(RequestPatchMutationOutcome::ConfirmationRequired { confirmation });
        }

        Self::validate_scope_conflicts(scope, &normalized, None)?;

        let now = Utc::now().timestamp_millis();
        let value_json = validate_value_for_placement(
            normalized.placement,
            normalized.operation,
            &normalized.value_json,
        )?;
        let new_row = NewRequestPatchRule {
            id: ID_GENERATOR.generate_id(),
            provider_id: scope.provider_id(),
            model_id: scope.model_id(),
            placement: normalized.placement,
            target: normalized.target,
            operation: normalized.operation,
            value_json,
            description: normalized.description,
            is_enabled: normalized.is_enabled,
            created_at: now,
            updated_at: now,
        };

        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let inserted = diesel::insert_into(request_patch_rule::table)
                .values(NewRequestPatchRuleDb::to_db(&new_row))
                .returning(RequestPatchRuleDb::as_returning())
                .get_result::<RequestPatchRuleDb>(conn)
                .map_err(|err| map_write_error("Failed to create request patch rule", err))?;

            Ok(RequestPatchMutationOutcome::Saved {
                rule: build_rule_response(&inserted.from_db())?,
            })
        })
    }

    fn update_by_scope(
        scope: RequestPatchScope,
        rule_id: i64,
        payload: &UpdateRequestPatchPayload,
    ) -> DbResult<RequestPatchMutationOutcome> {
        let existing = Self::get_by_scope(scope, rule_id)?;
        let normalized = resolve_update_payload(&existing, payload)?;

        if let Some(confirmation) = requires_confirmation(Some(&existing), &normalized)? {
            return Ok(RequestPatchMutationOutcome::ConfirmationRequired { confirmation });
        }

        Self::validate_scope_conflicts(scope, &normalized, Some(rule_id))?;

        let update_row = UpdateRequestPatchRuleData {
            placement: Some(normalized.placement),
            target: Some(normalized.target),
            operation: Some(normalized.operation),
            value_json: Some(validate_value_for_placement(
                normalized.placement,
                normalized.operation,
                &normalized.value_json,
            )?),
            description: Some(normalized.description),
            is_enabled: Some(normalized.is_enabled),
        };
        let now = Utc::now().timestamp_millis();
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let updated = diesel::update(request_patch_rule::table.find(rule_id))
                .set((
                    UpdateRequestPatchRuleDataDb::to_db(&update_row),
                    request_patch_rule::dsl::updated_at.eq(now),
                ))
                .returning(RequestPatchRuleDb::as_returning())
                .get_result::<RequestPatchRuleDb>(conn)
                .map_err(|err| map_write_error("Failed to update request patch rule", err))?;

            Ok(RequestPatchMutationOutcome::Saved {
                rule: build_rule_response(&updated.from_db())?,
            })
        })
    }

    fn delete_by_scope(scope: RequestPatchScope, rule_id: i64) -> DbResult<usize> {
        let _existing = Self::get_by_scope(scope, rule_id)?;
        let now = Utc::now().timestamp_millis();
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            diesel::update(request_patch_rule::table.find(rule_id))
                .set((
                    request_patch_rule::dsl::deleted_at.eq(now),
                    request_patch_rule::dsl::is_enabled.eq(false),
                    request_patch_rule::dsl::updated_at.eq(now),
                ))
                .execute(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete request patch rule {}: {}",
                        rule_id, err
                    )))
                })
        })
    }

    pub fn list_by_provider_id(provider_id: i64) -> DbResult<Vec<RequestPatchRuleResponse>> {
        Self::list_by_scope(RequestPatchScope::Provider(provider_id))
    }

    pub fn list_by_model_id(model_id: i64) -> DbResult<Vec<RequestPatchRuleResponse>> {
        Self::list_by_scope(RequestPatchScope::Model(model_id))
    }

    pub fn get_provider_rule(provider_id: i64, rule_id: i64) -> DbResult<RequestPatchRuleResponse> {
        build_rule_response(&Self::get_by_scope(
            RequestPatchScope::Provider(provider_id),
            rule_id,
        )?)
    }

    pub fn get_model_rule(model_id: i64, rule_id: i64) -> DbResult<RequestPatchRuleResponse> {
        build_rule_response(&Self::get_by_scope(
            RequestPatchScope::Model(model_id),
            rule_id,
        )?)
    }

    pub fn create_for_provider(
        provider_id: i64,
        payload: &CreateRequestPatchPayload,
    ) -> DbResult<RequestPatchMutationOutcome> {
        Self::create_by_scope(RequestPatchScope::Provider(provider_id), payload)
    }

    pub fn create_for_model(
        model_id: i64,
        payload: &CreateRequestPatchPayload,
    ) -> DbResult<RequestPatchMutationOutcome> {
        Self::create_by_scope(RequestPatchScope::Model(model_id), payload)
    }

    pub fn update_for_provider(
        provider_id: i64,
        rule_id: i64,
        payload: &UpdateRequestPatchPayload,
    ) -> DbResult<RequestPatchMutationOutcome> {
        Self::update_by_scope(RequestPatchScope::Provider(provider_id), rule_id, payload)
    }

    pub fn update_for_model(
        model_id: i64,
        rule_id: i64,
        payload: &UpdateRequestPatchPayload,
    ) -> DbResult<RequestPatchMutationOutcome> {
        Self::update_by_scope(RequestPatchScope::Model(model_id), rule_id, payload)
    }

    pub fn delete_for_provider(provider_id: i64, rule_id: i64) -> DbResult<usize> {
        Self::delete_by_scope(RequestPatchScope::Provider(provider_id), rule_id)
    }

    pub fn delete_for_model(model_id: i64, rule_id: i64) -> DbResult<usize> {
        Self::delete_by_scope(RequestPatchScope::Model(model_id), rule_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_rule(
        placement: RequestPatchPlacement,
        target: &str,
        operation: RequestPatchOperation,
        value_json: Option<&str>,
    ) -> RequestPatchRule {
        RequestPatchRule {
            id: 1,
            provider_id: Some(10),
            model_id: None,
            placement,
            target: target.to_string(),
            operation,
            value_json: value_json.map(ToString::to_string),
            description: None,
            is_enabled: true,
            deleted_at: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn header_targets_are_normalized_to_lowercase() {
        let target = normalize_target(RequestPatchPlacement::Header, "X-API-Key")
            .expect("header target should normalize");
        assert_eq!(target, "x-api-key");
    }

    #[test]
    fn query_targets_reject_invalid_delimiters() {
        assert!(normalize_target(RequestPatchPlacement::Query, "bad=value").is_err());
        assert!(normalize_target(RequestPatchPlacement::Query, "bad value").is_err());
        assert!(normalize_target(RequestPatchPlacement::Query, "good_key").is_ok());
    }

    #[test]
    fn body_targets_require_valid_json_pointer() {
        assert!(normalize_target(RequestPatchPlacement::Body, "messages").is_err());
        assert!(normalize_target(RequestPatchPlacement::Body, "/messages/~x").is_err());
        assert!(normalize_target(RequestPatchPlacement::Body, "/messages/0").is_ok());
    }

    #[test]
    fn header_and_query_values_must_be_scalar() {
        let object_value = Some(json!({"a": 1}));
        assert!(
            validate_value_for_placement(
                RequestPatchPlacement::Header,
                RequestPatchOperation::Set,
                &object_value,
            )
            .is_err()
        );
        assert!(
            validate_value_for_placement(
                RequestPatchPlacement::Query,
                RequestPatchOperation::Set,
                &Some(json!(["a"])),
            )
            .is_err()
        );
        assert!(
            validate_value_for_placement(
                RequestPatchPlacement::Header,
                RequestPatchOperation::Set,
                &Some(json!("token")),
            )
            .is_ok()
        );
        assert!(
            validate_value_for_placement(
                RequestPatchPlacement::Query,
                RequestPatchOperation::Set,
                &Some(json!(false)),
            )
            .is_ok()
        );
    }

    #[test]
    fn hard_forbidden_targets_are_rejected() {
        assert!(validate_reserved_target(RequestPatchPlacement::Header, "host").is_err());
        assert!(validate_reserved_target(RequestPatchPlacement::Body, "/messages").is_err());
        assert!(validate_reserved_target(RequestPatchPlacement::Body, "/messages/0").is_err());
    }

    #[test]
    fn dangerous_targets_require_confirmation_when_changed() {
        let candidate = NormalizedRequestPatchInput {
            placement: RequestPatchPlacement::Header,
            target: "authorization".to_string(),
            operation: RequestPatchOperation::Set,
            value_json: Some(json!("Bearer demo")),
            description: None,
            is_enabled: true,
            confirm_dangerous_target: false,
        };

        let confirmation = requires_confirmation(None, &candidate)
            .expect("validation should succeed")
            .expect("confirmation should be required");
        assert_eq!(confirmation.target, "authorization");
        assert_eq!(confirmation.confirm_field, CONFIRM_DANGEROUS_TARGET_FIELD);

        let existing = sample_rule(
            RequestPatchPlacement::Header,
            "authorization",
            RequestPatchOperation::Set,
            Some("\"Bearer old\""),
        );
        assert!(
            requires_confirmation(Some(&existing), &candidate)
                .expect("validation should succeed")
                .is_none()
        );
    }

    #[test]
    fn body_ancestor_and_descendant_conflicts_are_detected() {
        let conflict = detect_body_conflict(
            ["/generation_config", "/metadata/tenant"].into_iter(),
            "/generation_config/temperature",
        )
        .expect("ancestor conflict should be detected");
        assert_eq!(conflict, "/generation_config");

        let reverse_conflict = detect_body_conflict(
            ["/generation_config/temperature"].into_iter(),
            "/generation_config",
        )
        .expect("descendant conflict should be detected");
        assert_eq!(reverse_conflict, "/generation_config/temperature");
    }

    #[test]
    fn build_rule_response_parses_json_null() {
        let row = sample_rule(
            RequestPatchPlacement::Body,
            "/metadata/tag",
            RequestPatchOperation::Set,
            Some("null"),
        );

        let response = build_rule_response(&row).expect("response should build");
        assert_eq!(response.value_json, Some(Value::Null));
    }

    #[test]
    fn update_payload_can_set_json_null_without_losing_state() {
        let existing = sample_rule(
            RequestPatchPlacement::Body,
            "/model",
            RequestPatchOperation::Set,
            Some("\"gpt-4.1\""),
        );
        let payload = UpdateRequestPatchPayload {
            value_json: Some(None),
            ..Default::default()
        };

        let normalized =
            resolve_update_payload(&existing, &payload).expect("update payload should resolve");
        assert_eq!(normalized.value_json, Some(Value::Null));
    }
}
