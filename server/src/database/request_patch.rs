use chrono::Utc;
use diesel::prelude::*;
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{DbResult, get_connection};
use crate::controller::BaseError;
use crate::database::model::Model;
use crate::database::upstream_source::UpstreamSource;
use crate::schema::enum_def::{RequestPatchOperation, RequestPatchPlacement};
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

const HARD_FORBIDDEN_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "api-key",
    "x-api-key",
    "x-goog-api-key",
    "anthropic-version",
    "cookie",
    "host",
    "content-length",
    "transfer-encoding",
    "accept-encoding",
    "x-request-id",
    "x-client-request-id",
];
const HARD_FORBIDDEN_BODY_TARGETS: &[&str] = &[
    "/model",
    "/stream",
    "/stream_options/include_usage",
    "/store",
    "/previous_response_id",
    "/conversation",
    "/background",
    "/generationConfig/candidateCount",
];
const HARD_FORBIDDEN_QUERY_TARGETS: &[&str] = &["key", "alt"];

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, Serialize)]
    #[diesel(table_name = request_patch_variant)]
    pub struct RequestPatchVariant {
        pub id: i64,
        pub source_id: i64,
        pub model_id: Option<i64>,
        pub suffix: Option<String>,
        pub enabled: bool,
        pub expose_in_models: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Debug, Clone)]
    #[diesel(table_name = request_patch_variant)]
    pub struct NewRequestPatchVariant {
        pub id: i64,
        pub source_id: i64,
        pub model_id: Option<i64>,
        pub suffix: Option<String>,
        pub enabled: bool,
        pub expose_in_models: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, Serialize)]
    #[diesel(table_name = request_patch_rule)]
    pub struct RequestPatchRule {
        pub id: i64,
        pub variant_id: i64,
        pub placement: RequestPatchPlacement,
        pub target: String,
        pub operation: RequestPatchOperation,
        pub value_json: Option<String>,
        pub description: Option<String>,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Debug, Clone)]
    #[diesel(table_name = request_patch_rule)]
    pub struct NewRequestPatchRule {
        pub id: i64,
        pub variant_id: i64,
        pub placement: RequestPatchPlacement,
        pub target: String,
        pub operation: RequestPatchOperation,
        pub value_json: Option<String>,
        pub description: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestPatchVariantAggregate {
    pub variant: RequestPatchVariant,
    pub rules: Vec<RequestPatchRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RequestPatchVariantInput {
    pub source_id: i64,
    pub model_id: Option<i64>,
    pub suffix: Option<String>,
    pub enabled: bool,
    pub expose_in_models: bool,
    pub rules: Vec<RequestPatchRuleInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RequestPatchRuleInput {
    pub placement: RequestPatchPlacement,
    pub target: String,
    pub operation: RequestPatchOperation,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub value_json: Option<Option<Value>>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestPatchPreviewConflict {
    pub existing_variant_id: i64,
    pub existing_model_id: Option<i64>,
    pub existing_suffix: Option<String>,
    pub placement: RequestPatchPlacement,
    pub candidate_target: String,
    pub existing_target: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestPatchVariantPreview {
    pub suffix: Option<String>,
    pub rule_count: usize,
    pub conflicts: Vec<RequestPatchPreviewConflict>,
    pub affected_model_count: usize,
    pub valid: bool,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct NormalizedRule {
    placement: RequestPatchPlacement,
    target: String,
    operation: RequestPatchOperation,
    value_json: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Clone)]
struct VariantSnapshot {
    variant: RequestPatchVariant,
    rules: Vec<RequestPatchRule>,
}

fn database_error(context: &str, error: impl std::fmt::Display) -> BaseError {
    BaseError::DatabaseFatal(Some(format!("{context}: {error}")))
}

fn map_write_error(context: &str, error: diesel::result::Error) -> BaseError {
    match error {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        ) => BaseError::DatabaseDup(Some(context.to_string())),
        other => database_error(context, other),
    }
}

fn json_scalar_to_string(value: &Value) -> DbResult<String> {
    match value {
        Value::Null => Ok("null".to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) => Ok(value.clone()),
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
    HeaderName::from_bytes(normalized.as_bytes()).map_err(|error| {
        BaseError::ParamInvalid(Some(format!("invalid HEADER target '{target}': {error}")))
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
    if target.chars().any(|character| {
        character.is_control()
            || character.is_whitespace()
            || matches!(character, '&' | '=' | '#' | '?')
    }) {
        return Err(BaseError::ParamInvalid(Some(format!(
            "invalid QUERY target '{target}'"
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
            "BODY target '{target}' must be a JSON Pointer"
        ))));
    }
    for segment in target.split('/').skip(1) {
        let mut characters = segment.chars().peekable();
        while let Some(character) = characters.next() {
            if character == '~' {
                match characters.next() {
                    Some('0') | Some('1') => {}
                    _ => {
                        return Err(BaseError::ParamInvalid(Some(format!(
                            "BODY target '{target}' contains an invalid JSON Pointer escape"
                        ))));
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn normalize_target(placement: RequestPatchPlacement, target: &str) -> DbResult<String> {
    match placement {
        RequestPatchPlacement::Header => normalize_header_target(target),
        RequestPatchPlacement::Query => normalize_query_target(target),
        RequestPatchPlacement::Body => {
            validate_json_pointer(target)?;
            Ok(target.to_string())
        }
    }
}

pub fn normalize_suffix(suffix: Option<&str>) -> DbResult<Option<String>> {
    let Some(suffix) = suffix else {
        return Ok(None);
    };
    if suffix.is_empty()
        || suffix.starts_with('-')
        || suffix.ends_with('-')
        || suffix.contains("--")
        || suffix.chars().any(|character| {
            !character.is_ascii_lowercase() && !character.is_ascii_digit() && character != '-'
        })
    {
        return Err(BaseError::ParamInvalid(Some(
            "request patch suffix must match [a-z0-9]+(?:-[a-z0-9]+)*".to_string(),
        )));
    }
    Ok(Some(suffix.to_string()))
}

fn validate_value_for_placement(
    placement: RequestPatchPlacement,
    operation: RequestPatchOperation,
    value_json: &Option<Option<Value>>,
) -> DbResult<Option<String>> {
    match operation {
        RequestPatchOperation::Remove => {
            if value_json.as_ref().is_some_and(Option::is_some) {
                return Err(BaseError::ParamInvalid(Some(
                    "REMOVE rules must not include value_json".to_string(),
                )));
            }
            Ok(None)
        }
        RequestPatchOperation::Set => {
            let null_value = Value::Null;
            let value = match value_json {
                Some(Some(value)) => value,
                Some(None) => &null_value,
                None => {
                    return Err(BaseError::ParamInvalid(Some(
                        "SET rules must include value_json".to_string(),
                    )));
                }
            };
            match placement {
                RequestPatchPlacement::Header => {
                    let rendered = json_scalar_to_string(value)?;
                    HeaderValue::from_str(&rendered).map_err(|error| {
                        BaseError::ParamInvalid(Some(format!(
                            "invalid HEADER value for target: {error}"
                        )))
                    })?;
                }
                RequestPatchPlacement::Query => {
                    json_scalar_to_string(value)?;
                }
                RequestPatchPlacement::Body => {}
            }
            serde_json::to_string(value).map(Some).map_err(|error| {
                BaseError::ParamInvalid(Some(format!("failed to serialize value_json: {error}")))
            })
        }
    }
}

fn matches_body_prefix(target: &str, prefix: &str) -> bool {
    target == prefix || target.starts_with(&format!("{prefix}/"))
}

fn is_body_ancestor_or_descendant(left: &str, right: &str) -> bool {
    left != right && (matches_body_prefix(left, right) || matches_body_prefix(right, left))
}

fn is_request_id_header(target: &str) -> bool {
    target == "request-id"
        || target.ends_with("-request-id")
        || matches!(target, "x-amzn-requestid" | "x-amz-request-id")
}

pub fn validate_reserved_target(placement: RequestPatchPlacement, target: &str) -> DbResult<()> {
    match placement {
        RequestPatchPlacement::Header => {
            if HARD_FORBIDDEN_HEADERS.contains(&target) || is_request_id_header(target) {
                return Err(BaseError::ParamInvalid(Some(format!(
                    "HEADER target '{target}' is reserved and cannot be modified"
                ))));
            }
        }
        RequestPatchPlacement::Query => {
            if HARD_FORBIDDEN_QUERY_TARGETS
                .iter()
                .any(|reserved| target.eq_ignore_ascii_case(reserved))
            {
                return Err(BaseError::ParamInvalid(Some(format!(
                    "QUERY target '{target}' is reserved and cannot be modified"
                ))));
            }
        }
        RequestPatchPlacement::Body => {
            if HARD_FORBIDDEN_BODY_TARGETS.iter().any(|reserved| {
                target == *reserved
                    || matches_body_prefix(target, reserved)
                    || matches_body_prefix(reserved, target)
            }) {
                return Err(BaseError::ParamInvalid(Some(format!(
                    "BODY target '{target}' is reserved and cannot be modified"
                ))));
            }
        }
    }
    Ok(())
}

fn normalize_rules(inputs: &[RequestPatchRuleInput]) -> DbResult<Vec<NormalizedRule>> {
    let mut identities: Vec<(RequestPatchPlacement, String)> = Vec::new();
    let mut normalized = Vec::with_capacity(inputs.len());

    for input in inputs {
        let target = normalize_target(input.placement, &input.target)?;
        if identities.contains(&(input.placement, target.clone())) {
            return Err(BaseError::ParamInvalid(Some(format!(
                "duplicate active request patch rule target: {:?} {target}",
                input.placement
            ))));
        }
        identities.push((input.placement, target.clone()));
        validate_reserved_target(input.placement, &target)?;
        let value_json =
            validate_value_for_placement(input.placement, input.operation, &input.value_json)?;
        normalized.push(NormalizedRule {
            placement: input.placement,
            target,
            operation: input.operation,
            value_json,
            description: input.description.clone(),
        });
    }

    for (left_index, left) in normalized.iter().enumerate() {
        for right in normalized.iter().skip(left_index + 1) {
            if left.placement == RequestPatchPlacement::Body
                && right.placement == RequestPatchPlacement::Body
                && is_body_ancestor_or_descendant(&left.target, &right.target)
            {
                return Err(BaseError::ParamInvalid(Some(format!(
                    "BODY targets '{}' and '{}' cannot coexist in one Variant",
                    left.target, right.target
                ))));
            }
        }
    }
    Ok(normalized)
}

fn validate_variant_shape(
    input: &RequestPatchVariantInput,
    suffix: &Option<String>,
    normalized_rules: &[NormalizedRule],
) -> DbResult<()> {
    if (suffix.is_none() || !input.enabled) && input.expose_in_models {
        return Err(BaseError::ParamInvalid(Some(
            "base or disabled request patch Variants cannot be exposed in models".to_string(),
        )));
    }
    if suffix.is_none() && normalized_rules.is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "an empty base Variant is not persisted".to_string(),
        )));
    }
    if suffix.is_some() && input.model_id.is_none() && input.enabled && normalized_rules.is_empty()
    {
        return Err(BaseError::ParamInvalid(Some(
            "an enabled Source suffix Variant requires at least one Rule".to_string(),
        )));
    }
    if suffix.is_some() && input.model_id.is_some() && !input.enabled {
        return Ok(());
    }
    if suffix.is_some() && input.model_id.is_some() && input.enabled && normalized_rules.is_empty()
    {
        // The transaction-level validator checks the Source suffix before write.
        return Ok(());
    }
    if normalized_rules.is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "an enabled request patch Variant requires at least one Rule".to_string(),
        )));
    }
    Ok(())
}

macro_rules! validate_owner {
    ($conn:expr, $source_id:expr, $model_id:expr) => {{
        let source_row = upstream_source::table
            .filter(upstream_source::dsl::id.eq($source_id))
            .select((
                upstream_source::dsl::id,
                upstream_source::dsl::provider_id,
                upstream_source::dsl::deleted_at,
            ))
            .first::<(i64, i64, Option<i64>)>($conn)
            .optional()
            .map_err(|error| database_error("failed to validate request patch Source", error))?
            .ok_or_else(|| {
                BaseError::NotFound(Some(format!("upstream source {} not found", $source_id)))
            })?;
        if source_row.2.is_some() {
            return Err(BaseError::NotFound(Some(format!(
                "upstream source {} is deleted",
                $source_id
            ))));
        }

        if let Some(model_id) = $model_id {
            let model_row = model::table
                .filter(model::dsl::id.eq(model_id))
                .select((
                    model::dsl::provider_id,
                    model::dsl::deleted_at,
                    model::dsl::source_selection_mode,
                ))
                .first::<(i64, Option<i64>, String)>($conn)
                .optional()
                .map_err(|error| database_error("failed to validate request patch Model", error))?
                .ok_or_else(|| BaseError::NotFound(Some(format!("model {model_id} not found"))))?;
            if model_row.1.is_some() {
                return Err(BaseError::NotFound(Some(format!(
                    "model {model_id} is deleted"
                ))));
            }
            if model_row.0 != source_row.1 {
                return Err(BaseError::ParamInvalid(Some(format!(
                    "model {model_id} and Source {} must belong to the same Provider",
                    $source_id
                ))));
            }
            if model_row.2 == "EXPLICIT" {
                let bound = model_source_binding::table
                    .filter(model_source_binding::dsl::model_id.eq(model_id))
                    .filter(model_source_binding::dsl::source_id.eq($source_id))
                    .select(model_source_binding::dsl::model_id)
                    .first::<i64>($conn)
                    .optional()
                    .map_err(|error| {
                        database_error("failed to validate explicit model Source binding", error)
                    })?;
                if bound.is_none() {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "model {model_id} is not explicitly bound to Source {}",
                        $source_id
                    ))));
                }
            } else if model_row.2 != "INHERIT_ALL" {
                return Err(BaseError::DatabaseFatal(Some(format!(
                    "model {model_id} has invalid source selection mode"
                ))));
            }
        }
        Ok::<(), BaseError>(())
    }};
}

macro_rules! load_variant_snapshot {
    ($conn:expr, $variant_id:expr, $include_deleted:expr) => {{
        let mut variant_query = request_patch_variant::table.into_boxed();
        variant_query = variant_query.filter(request_patch_variant::dsl::id.eq($variant_id));
        if !$include_deleted {
            variant_query = variant_query.filter(request_patch_variant::dsl::deleted_at.is_null());
        }
        let variant = variant_query
            .select(RequestPatchVariantDb::as_select())
            .first::<RequestPatchVariantDb>($conn)
            .map_err(|error| match error {
                diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                    "request patch Variant {} not found",
                    $variant_id
                ))),
                other => database_error("failed to load request patch Variant", other),
            })?
            .from_db();
        let rules = request_patch_rule::table
            .filter(request_patch_rule::dsl::variant_id.eq($variant_id))
            .filter(request_patch_rule::dsl::deleted_at.is_null())
            .order((
                request_patch_rule::dsl::placement.asc(),
                request_patch_rule::dsl::target.asc(),
                request_patch_rule::dsl::created_at.asc(),
                request_patch_rule::dsl::id.asc(),
            ))
            .select(RequestPatchRuleDb::as_select())
            .load::<RequestPatchRuleDb>($conn)
            .map_err(|error| database_error("failed to load request patch Rules", error))?
            .into_iter()
            .map(RequestPatchRuleDb::from_db)
            .collect::<Vec<_>>();
        Ok::<VariantSnapshot, BaseError>(VariantSnapshot { variant, rules })
    }};
}

macro_rules! load_existing_variants {
    ($conn:expr, $source_id:expr, $exclude_variant_id:expr) => {{
        let mut query = request_patch_variant::table
            .filter(request_patch_variant::dsl::source_id.eq($source_id))
            .filter(request_patch_variant::dsl::deleted_at.is_null())
            .into_boxed();
        if let Some(variant_id) = $exclude_variant_id {
            query = query.filter(request_patch_variant::dsl::id.ne(variant_id));
        }
        let ids = query
            .select(request_patch_variant::dsl::id)
            .order(request_patch_variant::dsl::id.asc())
            .load::<i64>($conn)
            .map_err(|error| {
                database_error("failed to load existing request patch Variants", error)
            })?;
        ids.into_iter()
            .map(|variant_id| load_variant_snapshot!($conn, variant_id, false))
            .collect::<DbResult<Vec<_>>>()
    }};
}

macro_rules! load_variant_ids {
    ($conn:expr, $query:expr, $include_deleted:expr) => {{
        let ids = $query
            .select(request_patch_variant::dsl::id)
            .order((
                request_patch_variant::dsl::suffix.asc(),
                request_patch_variant::dsl::id.asc(),
            ))
            .load::<i64>($conn)
            .map_err(|error| database_error("failed to list request patch Variants", error))?;
        ids.into_iter()
            .map(|variant_id| load_variant_snapshot!($conn, variant_id, $include_deleted))
            .map(|result| {
                result.map(|snapshot| RequestPatchVariantAggregate {
                    variant: snapshot.variant,
                    rules: snapshot.rules,
                })
            })
            .collect::<DbResult<Vec<_>>>()
    }};
}

macro_rules! load_executable_model_ids_for_source {
    ($conn:expr, $provider_id:expr, $source_id:expr) => {{
        let model_rows = model::table
            .filter(model::dsl::provider_id.eq($provider_id))
            .filter(model::dsl::deleted_at.is_null())
            .filter(model::dsl::is_enabled.eq(true))
            .select((model::dsl::id, model::dsl::source_selection_mode))
            .load::<(i64, String)>($conn)
            .map_err(|error| {
                database_error(
                    "failed to load Models affected by request patch Source",
                    error,
                )
            })?;
        let explicitly_bound_model_ids = model_source_binding::table
            .filter(model_source_binding::dsl::source_id.eq($source_id))
            .select(model_source_binding::dsl::model_id)
            .load::<i64>($conn)
            .map_err(|error| {
                database_error("failed to load request patch Model Source bindings", error)
            })?;
        model_rows
            .into_iter()
            .filter_map(
                |(model_id, source_selection_mode)| match source_selection_mode.as_str() {
                    "INHERIT_ALL" => Some(Ok(model_id)),
                    "EXPLICIT" if explicitly_bound_model_ids.contains(&model_id) => {
                        Some(Ok(model_id))
                    }
                    "EXPLICIT" => None,
                    _ => Some(Err(BaseError::DatabaseFatal(Some(format!(
                        "model {model_id} has invalid source selection mode"
                    ))))),
                },
            )
            .collect::<DbResult<Vec<_>>>()
    }};
}

macro_rules! validate_source_post_state {
    ($conn:expr, $source_id:expr, $snapshots:expr, $force_enabled:expr) => {{
        let source_row = upstream_source::table
            .filter(upstream_source::dsl::id.eq($source_id))
            .filter(upstream_source::dsl::deleted_at.is_null())
            .select((
                upstream_source::dsl::provider_id,
                upstream_source::dsl::is_enabled,
            ))
            .first::<(i64, bool)>($conn)
            .optional()
            .map_err(|error| {
                database_error("failed to load request patch Source lifecycle state", error)
            })?
            .ok_or_else(|| {
                BaseError::NotFound(Some(format!("upstream source {} not found", $source_id)))
            })?;
        if $force_enabled || source_row.1 {
            let model_ids = load_executable_model_ids_for_source!($conn, source_row.0, $source_id)?;
            validate_snapshot_combinations($snapshots, &model_ids)?;
        }
        Ok::<(), BaseError>(())
    }};
}

macro_rules! validate_model_suffix_inheritance {
    ($conn:expr, $input:expr, $suffix:expr, $normalized_rules:expr) => {{
        if $input.model_id.is_some()
            && $suffix.is_some()
            && $input.enabled
            && $normalized_rules.is_empty()
        {
            let source_variant_id = request_patch_variant::table
                .filter(request_patch_variant::dsl::source_id.eq($input.source_id))
                .filter(request_patch_variant::dsl::model_id.is_null())
                .filter(request_patch_variant::dsl::suffix.eq($suffix.clone()))
                .filter(request_patch_variant::dsl::enabled.eq(true))
                .filter(request_patch_variant::dsl::deleted_at.is_null())
                .select(request_patch_variant::dsl::id)
                .first::<i64>($conn)
                .optional()
                .map_err(|error| {
                    database_error("failed to validate inherited suffix Variant", error)
                })?;
            let Some(source_variant_id) = source_variant_id else {
                return Err(BaseError::ParamInvalid(Some(
                    "an enabled empty Model suffix Variant requires an enabled Source suffix with Rules"
                        .to_string(),
                )));
            };
            let rule_count = request_patch_rule::table
                .filter(request_patch_rule::dsl::variant_id.eq(source_variant_id))
                .filter(request_patch_rule::dsl::deleted_at.is_null())
                .select(request_patch_rule::dsl::id)
                .count()
                .get_result::<i64>($conn)
                .map_err(|error| {
                    database_error("failed to count inherited suffix Rules", error)
                })?;
            if rule_count == 0 {
                return Err(BaseError::ParamInvalid(Some(
                    "an enabled empty Model suffix Variant requires an effective Source Rule"
                        .to_string(),
                )));
            }
        }
        Ok::<(), BaseError>(())
    }};
}

fn variants_can_coexist(left: &RequestPatchVariant, right: &RequestPatchVariant) -> bool {
    if left.source_id != right.source_id {
        return false;
    }
    if left.model_id.is_some() && right.model_id.is_some() && left.model_id != right.model_id {
        return false;
    }
    match (&left.suffix, &right.suffix) {
        (Some(left), Some(right)) => left == right,
        _ => true,
    }
}

fn validate_cross_layer_conflicts(
    candidate: &RequestPatchVariant,
    candidate_rules: &[NormalizedRule],
    existing: &[VariantSnapshot],
) -> DbResult<()> {
    if let Some(conflict) = collect_cross_layer_conflicts(candidate, candidate_rules, existing)
        .into_iter()
        .next()
    {
        return Err(BaseError::ParamInvalid(Some(format!(
            "BODY target '{}' conflicts with existing Variant {} target '{}'",
            conflict.candidate_target, conflict.existing_variant_id, conflict.existing_target
        ))));
    }
    Ok(())
}

fn collect_cross_layer_conflicts(
    candidate: &RequestPatchVariant,
    candidate_rules: &[NormalizedRule],
    existing: &[VariantSnapshot],
) -> Vec<RequestPatchPreviewConflict> {
    if !candidate.enabled || candidate_rules.is_empty() {
        return Vec::new();
    }

    existing
        .iter()
        .filter(|snapshot| {
            snapshot.variant.id != candidate.id
                && snapshot.variant.enabled
                && variants_can_coexist(candidate, &snapshot.variant)
        })
        .flat_map(|snapshot| {
            candidate_rules.iter().flat_map(move |candidate_rule| {
                snapshot.rules.iter().filter_map(move |existing_rule| {
                    (candidate_rule.placement == RequestPatchPlacement::Body
                        && existing_rule.placement == RequestPatchPlacement::Body
                        && is_body_ancestor_or_descendant(
                            &candidate_rule.target,
                            &existing_rule.target,
                        ))
                    .then(|| RequestPatchPreviewConflict {
                        existing_variant_id: snapshot.variant.id,
                        existing_model_id: snapshot.variant.model_id,
                        existing_suffix: snapshot.variant.suffix.clone(),
                        placement: RequestPatchPlacement::Body,
                        candidate_target: candidate_rule.target.clone(),
                        existing_target: existing_rule.target.clone(),
                        reason: "BODY target has an ancestor/descendant conflict".to_string(),
                    })
                })
            })
        })
        .collect()
}

fn normalize_variant_input(
    input: &RequestPatchVariantInput,
) -> DbResult<(Option<String>, Vec<NormalizedRule>)> {
    normalize_variant_input_inner(input, false)
}

fn normalize_variant_input_for_replace(
    input: &RequestPatchVariantInput,
) -> DbResult<(Option<String>, Vec<NormalizedRule>)> {
    normalize_variant_input_inner(input, true)
}

fn normalize_variant_input_inner(
    input: &RequestPatchVariantInput,
    allow_empty_base: bool,
) -> DbResult<(Option<String>, Vec<NormalizedRule>)> {
    if input.source_id <= 0 {
        return Err(BaseError::ParamInvalid(Some(
            "request patch source_id must be positive".to_string(),
        )));
    }
    if input.model_id.is_some_and(|model_id| model_id <= 0) {
        return Err(BaseError::ParamInvalid(Some(
            "request patch model_id must be positive".to_string(),
        )));
    }
    let suffix = normalize_suffix(input.suffix.as_deref())?;
    let normalized_rules = normalize_rules(&input.rules)?;
    if allow_empty_base && suffix.is_none() && normalized_rules.is_empty() {
        if input.expose_in_models {
            return Err(BaseError::ParamInvalid(Some(
                "base request patch Variants cannot be exposed in models".to_string(),
            )));
        }
    } else {
        validate_variant_shape(input, &suffix, &normalized_rules)?;
    }
    Ok((suffix, normalized_rules))
}

fn build_insert_rules(
    variant_id: i64,
    rules: &[NormalizedRule],
    now: i64,
) -> Vec<NewRequestPatchRule> {
    rules
        .iter()
        .map(|rule| NewRequestPatchRule {
            id: ID_GENERATOR.generate_id(),
            variant_id,
            placement: rule.placement,
            target: rule.target.clone(),
            operation: rule.operation,
            value_json: rule.value_json.clone(),
            description: rule.description.clone(),
            created_at: now,
            updated_at: now,
        })
        .collect()
}

fn candidate_snapshot(
    candidate: &RequestPatchVariant,
    rules: &[NormalizedRule],
) -> VariantSnapshot {
    VariantSnapshot {
        variant: candidate.clone(),
        rules: rules
            .iter()
            .enumerate()
            .map(|(index, rule)| RequestPatchRule {
                id: -(index as i64) - 1,
                variant_id: candidate.id,
                placement: rule.placement,
                target: rule.target.clone(),
                operation: rule.operation,
                value_json: rule.value_json.clone(),
                description: rule.description.clone(),
                deleted_at: None,
                created_at: 0,
                updated_at: 0,
            })
            .collect(),
    }
}

fn validate_snapshot_combinations(
    snapshots: &[VariantSnapshot],
    model_ids: &[i64],
) -> DbResult<()> {
    let validate_for_model = |model_id: Option<i64>| {
        let aggregates = snapshots
            .iter()
            .filter(|snapshot| {
                snapshot.variant.model_id.is_none() || snapshot.variant.model_id == model_id
            })
            .map(|snapshot| RequestPatchVariantAggregate {
                variant: snapshot.variant.clone(),
                rules: snapshot.rules.clone(),
            })
            .collect::<Vec<_>>();
        validate_active_variant_aggregates(&aggregates)
    };

    validate_for_model(None)?;
    for model_id in model_ids {
        validate_for_model(Some(*model_id))?;
    }
    Ok(())
}

fn duplicate_variant_identity(
    candidate: &RequestPatchVariant,
    existing: &[VariantSnapshot],
) -> bool {
    existing.iter().any(|snapshot| {
        snapshot.variant.source_id == candidate.source_id
            && snapshot.variant.model_id == candidate.model_id
            && snapshot.variant.suffix == candidate.suffix
    })
}

pub struct RequestPatchVariantRepository;

impl RequestPatchVariantRepository {
    pub fn validate_source_reactivation(source_id: i64) -> DbResult<()> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let snapshots = load_existing_variants!(conn, source_id, None::<i64>)?;
            validate_source_post_state!(conn, source_id, &snapshots, true)
        })
    }

    pub fn validate_model_source_reactivation(model_id: i64, source_id: i64) -> DbResult<()> {
        let model = Model::get_by_id(model_id)?;
        let source = UpstreamSource::get_active_by_id_for_provider(source_id, model.provider_id)?;
        if !source.is_enabled {
            return Ok(());
        }
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let snapshots = load_existing_variants!(conn, source_id, None::<i64>)?;
            validate_snapshot_combinations(&snapshots, &[model_id])
        })
    }

    pub fn preview(
        input: &RequestPatchVariantInput,
        exclude_variant_id: Option<i64>,
    ) -> DbResult<RequestPatchVariantPreview> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            conn.transaction::<RequestPatchVariantPreview, BaseError, _>(|conn| {
                validate_owner!(conn, input.source_id, input.model_id)?;
                if let Some(variant_id) = exclude_variant_id {
                    let owner = request_patch_variant::table
                        .filter(request_patch_variant::dsl::id.eq(variant_id))
                        .filter(request_patch_variant::dsl::deleted_at.is_null())
                        .select((
                            request_patch_variant::dsl::source_id,
                            request_patch_variant::dsl::model_id,
                        ))
                        .first::<(i64, Option<i64>)>(conn)
                        .optional()
                        .map_err(|error| {
                            database_error(
                                "failed to load request patch Variant preview owner",
                                error,
                            )
                        })?
                        .ok_or_else(|| {
                            BaseError::NotFound(Some(format!(
                                "request patch Variant {variant_id} not found"
                            )))
                        })?;
                    if owner != (input.source_id, input.model_id) {
                        return Err(BaseError::ParamInvalid(Some(
                            "request patch Variant preview owner cannot change".to_string(),
                        )));
                    }
                }
                for rule in &input.rules {
                    let normalized_target = normalize_target(rule.placement, &rule.target)?;
                    validate_reserved_target(rule.placement, &normalized_target)?;
                }
                let (suffix, normalized_rules) = if exclude_variant_id.is_some() {
                    normalize_variant_input_for_replace(input)?
                } else {
                    normalize_variant_input(input)?
                };
                validate_model_suffix_inheritance!(conn, input, &suffix, &normalized_rules)?;
                let candidate = RequestPatchVariant {
                    id: exclude_variant_id.unwrap_or_default(),
                    source_id: input.source_id,
                    model_id: input.model_id,
                    suffix: suffix.clone(),
                    enabled: input.enabled,
                    expose_in_models: input.expose_in_models,
                    deleted_at: None,
                    created_at: 0,
                    updated_at: 0,
                };
                let existing = load_existing_variants!(conn, input.source_id, exclude_variant_id)?;
                let duplicate_identity = duplicate_variant_identity(&candidate, &existing);
                let conflicts =
                    collect_cross_layer_conflicts(&candidate, &normalized_rules, &existing);
                let provider_id = upstream_source::table
                    .filter(upstream_source::dsl::id.eq(input.source_id))
                    .select(upstream_source::dsl::provider_id)
                    .first::<i64>(conn)
                    .map_err(|error| {
                        database_error("failed to load Source provider for Variant preview", error)
                    })?;
                let affected_model_count = if input.model_id.is_some() {
                    1
                } else {
                    model::table
                        .filter(model::dsl::provider_id.eq(provider_id))
                        .filter(model::dsl::deleted_at.is_null())
                        .select(model::dsl::id)
                        .count()
                        .get_result::<i64>(conn)
                        .map_err(|error| database_error("failed to count affected Models", error))?
                        as usize
                };
                let failure_reason = if duplicate_identity {
                    Some(
                        "an active request patch Variant already has this owner and suffix"
                            .to_string(),
                    )
                } else if !conflicts.is_empty() {
                    Some("BODY targets have an ancestor/descendant conflict".to_string())
                } else {
                    None
                };
                Ok(RequestPatchVariantPreview {
                    suffix,
                    rule_count: normalized_rules.len(),
                    conflicts,
                    affected_model_count,
                    valid: failure_reason.is_none(),
                    failure_reason,
                })
            })
        })
    }

    pub fn create(input: &RequestPatchVariantInput) -> DbResult<RequestPatchVariantAggregate> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            conn.transaction::<RequestPatchVariantAggregate, BaseError, _>(|conn| {
                validate_owner!(conn, input.source_id, input.model_id)?;
                let variant_id = ID_GENERATOR.generate_id();
                let (suffix, normalized_rules) = normalize_variant_input(input)?;
                validate_model_suffix_inheritance!(conn, input, &suffix, &normalized_rules)?;
                let candidate = RequestPatchVariant {
                    id: variant_id,
                    source_id: input.source_id,
                    model_id: input.model_id,
                    suffix: suffix.clone(),
                    enabled: input.enabled,
                    expose_in_models: input.expose_in_models,
                    deleted_at: None,
                    created_at: 0,
                    updated_at: 0,
                };
                let existing = load_existing_variants!(conn, input.source_id, None::<i64>)?;
                validate_cross_layer_conflicts(&candidate, &normalized_rules, &existing)?;
                if candidate.model_id.is_none() {
                    let mut post_state = existing.clone();
                    post_state.push(candidate_snapshot(&candidate, &normalized_rules));
                    validate_source_post_state!(conn, input.source_id, &post_state, false)?;
                }

                let now = Utc::now().timestamp_millis();
                let new_variant = NewRequestPatchVariant {
                    id: variant_id,
                    source_id: input.source_id,
                    model_id: input.model_id,
                    suffix,
                    enabled: input.enabled,
                    expose_in_models: input.expose_in_models,
                    created_at: now,
                    updated_at: now,
                };
                diesel::insert_into(request_patch_variant::table)
                    .values(NewRequestPatchVariantDb::to_db(&new_variant))
                    .execute(conn)
                    .map_err(|error| {
                        map_write_error("failed to create request patch Variant", error)
                    })?;
                let rows = build_insert_rules(variant_id, &normalized_rules, now);
                if !rows.is_empty() {
                    diesel::insert_into(request_patch_rule::table)
                        .values(
                            rows.iter()
                                .map(NewRequestPatchRuleDb::to_db)
                                .collect::<Vec<_>>(),
                        )
                        .execute(conn)
                        .map_err(|error| {
                            map_write_error("failed to create request patch Rules", error)
                        })?;
                }
                load_variant_snapshot!(conn, variant_id, false).map(|snapshot| {
                    RequestPatchVariantAggregate {
                        variant: snapshot.variant,
                        rules: snapshot.rules,
                    }
                })
            })
        })
    }

    pub fn replace(
        variant_id: i64,
        input: &RequestPatchVariantInput,
    ) -> DbResult<RequestPatchVariantAggregate> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            conn.transaction::<RequestPatchVariantAggregate, BaseError, _>(|conn| {
                let current = request_patch_variant::table
                    .filter(request_patch_variant::dsl::id.eq(variant_id))
                    .filter(request_patch_variant::dsl::deleted_at.is_null())
                    .select(RequestPatchVariantDb::as_select())
                    .first::<RequestPatchVariantDb>(conn)
                    .map(RequestPatchVariantDb::from_db)
                    .map_err(|error| match error {
                        diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                            "request patch Variant {variant_id} not found"
                        ))),
                        other => database_error("failed to load request patch Variant", other),
                    })?;
                if current.source_id != input.source_id || current.model_id != input.model_id {
                    return Err(BaseError::ParamInvalid(Some(
                        "request patch Variant owner cannot change".to_string(),
                    )));
                }
                validate_owner!(conn, input.source_id, input.model_id)?;
                let (suffix, normalized_rules) = normalize_variant_input_for_replace(input)?;
                validate_model_suffix_inheritance!(conn, input, &suffix, &normalized_rules)?;
                let candidate = RequestPatchVariant {
                    id: variant_id,
                    source_id: input.source_id,
                    model_id: input.model_id,
                    suffix: suffix.clone(),
                    enabled: input.enabled,
                    expose_in_models: input.expose_in_models,
                    deleted_at: None,
                    created_at: current.created_at,
                    updated_at: 0,
                };
                let existing = load_existing_variants!(conn, input.source_id, Some(variant_id))?;
                validate_cross_layer_conflicts(&candidate, &normalized_rules, &existing)?;
                let deletes_empty_base = suffix.is_none() && normalized_rules.is_empty();
                if candidate.model_id.is_none() {
                    let mut post_state = existing.clone();
                    if !deletes_empty_base {
                        post_state.push(candidate_snapshot(&candidate, &normalized_rules));
                    }
                    validate_source_post_state!(conn, input.source_id, &post_state, false)?;
                }

                let now = Utc::now().timestamp_millis();
                if deletes_empty_base {
                    diesel::update(request_patch_variant::table.find(variant_id))
                        .set((
                            request_patch_variant::dsl::deleted_at.eq(Some(now)),
                            request_patch_variant::dsl::enabled.eq(false),
                            request_patch_variant::dsl::expose_in_models.eq(false),
                            request_patch_variant::dsl::updated_at.eq(now),
                        ))
                        .execute(conn)
                        .map_err(|error| {
                            database_error("failed to delete empty base Variant", error)
                        })?;
                    diesel::update(
                        request_patch_rule::table
                            .filter(request_patch_rule::dsl::variant_id.eq(variant_id))
                            .filter(request_patch_rule::dsl::deleted_at.is_null()),
                    )
                    .set(request_patch_rule::dsl::deleted_at.eq(Some(now)))
                    .execute(conn)
                    .map_err(|error| {
                        database_error("failed to retire empty base Variant Rules", error)
                    })?;
                    return load_variant_snapshot!(conn, variant_id, true).map(|snapshot| {
                        RequestPatchVariantAggregate {
                            variant: snapshot.variant,
                            rules: snapshot.rules,
                        }
                    });
                }
                diesel::update(request_patch_variant::table.find(variant_id))
                    .set((
                        request_patch_variant::dsl::suffix.eq(suffix),
                        request_patch_variant::dsl::enabled.eq(input.enabled),
                        request_patch_variant::dsl::expose_in_models.eq(input.expose_in_models),
                        request_patch_variant::dsl::updated_at.eq(now),
                    ))
                    .execute(conn)
                    .map_err(|error| {
                        map_write_error("failed to replace request patch Variant", error)
                    })?;
                diesel::update(
                    request_patch_rule::table
                        .filter(request_patch_rule::dsl::variant_id.eq(variant_id))
                        .filter(request_patch_rule::dsl::deleted_at.is_null()),
                )
                .set(request_patch_rule::dsl::deleted_at.eq(Some(now)))
                .execute(conn)
                .map_err(|error| {
                    database_error("failed to retire old request patch Rules", error)
                })?;
                let rows = build_insert_rules(variant_id, &normalized_rules, now);
                if !rows.is_empty() {
                    diesel::insert_into(request_patch_rule::table)
                        .values(
                            rows.iter()
                                .map(NewRequestPatchRuleDb::to_db)
                                .collect::<Vec<_>>(),
                        )
                        .execute(conn)
                        .map_err(|error| {
                            map_write_error("failed to replace request patch Rules", error)
                        })?;
                }
                load_variant_snapshot!(conn, variant_id, false).map(|snapshot| {
                    RequestPatchVariantAggregate {
                        variant: snapshot.variant,
                        rules: snapshot.rules,
                    }
                })
            })
        })
    }

    pub fn get(variant_id: i64) -> DbResult<RequestPatchVariantAggregate> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            load_variant_snapshot!(conn, variant_id, false).map(|snapshot| {
                RequestPatchVariantAggregate {
                    variant: snapshot.variant,
                    rules: snapshot.rules,
                }
            })
        })
    }

    pub fn list_by_source(source_id: i64) -> DbResult<Vec<RequestPatchVariantAggregate>> {
        Self::list_by_owner(source_id, None)
    }

    pub fn list_by_source_ids(source_ids: &[i64]) -> DbResult<Vec<RequestPatchVariantAggregate>> {
        if source_ids.is_empty() {
            return Ok(Vec::new());
        }
        Self::list_by_ids(source_ids, Some(&[]), false)
    }

    pub fn list_by_source_ids_including_deleted(
        source_ids: &[i64],
    ) -> DbResult<Vec<RequestPatchVariantAggregate>> {
        if source_ids.is_empty() {
            return Ok(Vec::new());
        }
        Self::list_by_ids(source_ids, Some(&[]), true)
    }

    pub fn list_by_model_source(
        model_id: i64,
        source_id: i64,
    ) -> DbResult<Vec<RequestPatchVariantAggregate>> {
        Self::list_by_owner(source_id, Some(model_id))
    }

    pub fn list_by_model_ids(model_ids: &[i64]) -> DbResult<Vec<RequestPatchVariantAggregate>> {
        if model_ids.is_empty() {
            return Ok(Vec::new());
        }
        Self::list_by_ids(&[], Some(model_ids), false)
    }

    pub fn list_by_model_ids_including_deleted(
        model_ids: &[i64],
    ) -> DbResult<Vec<RequestPatchVariantAggregate>> {
        Self::list_by_ids(&[], Some(model_ids), true)
    }

    fn list_by_owner(
        source_id: i64,
        model_id: Option<i64>,
    ) -> DbResult<Vec<RequestPatchVariantAggregate>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let mut query = request_patch_variant::table
                .filter(request_patch_variant::dsl::source_id.eq(source_id))
                .filter(request_patch_variant::dsl::deleted_at.is_null())
                .into_boxed();
            query = match model_id {
                Some(model_id) => query.filter(request_patch_variant::dsl::model_id.eq(model_id)),
                None => query.filter(request_patch_variant::dsl::model_id.is_null()),
            };
            load_variant_ids!(conn, query, false)
        })
    }

    fn list_by_ids(
        source_ids: &[i64],
        model_ids: Option<&[i64]>,
        include_deleted: bool,
    ) -> DbResult<Vec<RequestPatchVariantAggregate>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let mut query = request_patch_variant::table.into_boxed();
            if !source_ids.is_empty() {
                query = query.filter(request_patch_variant::dsl::source_id.eq_any(source_ids));
            }
            if let Some(model_ids) = model_ids {
                if model_ids.is_empty() {
                    query = query.filter(request_patch_variant::dsl::model_id.is_null());
                } else {
                    query = query.filter(request_patch_variant::dsl::model_id.eq_any(model_ids));
                }
            }
            if !include_deleted {
                query = query.filter(request_patch_variant::dsl::deleted_at.is_null());
            }
            load_variant_ids!(conn, query, include_deleted)
        })
    }

    pub fn soft_delete(variant_id: i64) -> DbResult<RequestPatchVariantAggregate> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            conn.transaction::<RequestPatchVariantAggregate, BaseError, _>(|conn| {
                let current = load_variant_snapshot!(conn, variant_id, false)?;
                if current.variant.model_id.is_none() {
                    let post_state =
                        load_existing_variants!(conn, current.variant.source_id, Some(variant_id))?;
                    validate_source_post_state!(
                        conn,
                        current.variant.source_id,
                        &post_state,
                        false
                    )?;
                }
                let now = Utc::now().timestamp_millis();
                diesel::update(request_patch_variant::table.find(variant_id))
                    .set((
                        request_patch_variant::dsl::deleted_at.eq(Some(now)),
                        request_patch_variant::dsl::enabled.eq(false),
                        request_patch_variant::dsl::expose_in_models.eq(false),
                        request_patch_variant::dsl::updated_at.eq(now),
                    ))
                    .execute(conn)
                    .map_err(|error| {
                        database_error("failed to delete request patch Variant", error)
                    })?;
                diesel::update(
                    request_patch_rule::table
                        .filter(request_patch_rule::dsl::variant_id.eq(variant_id))
                        .filter(request_patch_rule::dsl::deleted_at.is_null()),
                )
                .set(request_patch_rule::dsl::deleted_at.eq(Some(now)))
                .execute(conn)
                .map_err(|error| database_error("failed to delete request patch Rules", error))?;
                let mut deleted = current.variant;
                deleted.deleted_at = Some(now);
                deleted.enabled = false;
                deleted.expose_in_models = false;
                deleted.updated_at = now;
                Ok(RequestPatchVariantAggregate {
                    variant: deleted,
                    rules: current.rules,
                })
            })
        })
    }

    pub fn soft_delete_by_source_id(source_id: i64) -> DbResult<usize> {
        Self::soft_delete_by_filter(Some(source_id), None)
    }

    pub fn soft_delete_by_model_id(model_id: i64) -> DbResult<usize> {
        Self::soft_delete_by_filter(None, Some(model_id))
    }

    fn soft_delete_by_filter(source_id: Option<i64>, model_id: Option<i64>) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            conn.transaction::<usize, BaseError, _>(|conn| {
                let now = Utc::now().timestamp_millis();
                let mut query = request_patch_variant::table
                    .filter(request_patch_variant::dsl::deleted_at.is_null())
                    .into_boxed();
                if let Some(source_id) = source_id {
                    query = query.filter(request_patch_variant::dsl::source_id.eq(source_id));
                }
                if let Some(model_id) = model_id {
                    query = query.filter(request_patch_variant::dsl::model_id.eq(model_id));
                }
                let ids = query
                    .select(request_patch_variant::dsl::id)
                    .load::<i64>(conn)
                    .map_err(|error| {
                        database_error("failed to find request patch Variants to delete", error)
                    })?;
                if ids.is_empty() {
                    return Ok(0);
                }
                diesel::update(
                    request_patch_variant::table
                        .filter(request_patch_variant::dsl::id.eq_any(&ids)),
                )
                .set((
                    request_patch_variant::dsl::deleted_at.eq(Some(now)),
                    request_patch_variant::dsl::enabled.eq(false),
                    request_patch_variant::dsl::expose_in_models.eq(false),
                    request_patch_variant::dsl::updated_at.eq(now),
                ))
                .execute(conn)
                .map_err(|error| {
                    database_error("failed to delete request patch Variants", error)
                })?;
                diesel::update(
                    request_patch_rule::table
                        .filter(request_patch_rule::dsl::variant_id.eq_any(&ids))
                        .filter(request_patch_rule::dsl::deleted_at.is_null()),
                )
                .set(request_patch_rule::dsl::deleted_at.eq(Some(now)))
                .execute(conn)
                .map_err(|error| database_error("failed to delete request patch Rules", error))?;
                Ok(ids.len())
            })
        })
    }
}

fn validate_active_variant_aggregates(aggregates: &[RequestPatchVariantAggregate]) -> DbResult<()> {
    let snapshots = aggregates
        .iter()
        .map(|aggregate| VariantSnapshot {
            variant: aggregate.variant.clone(),
            rules: aggregate.rules.clone(),
        })
        .collect::<Vec<_>>();

    for snapshot in &snapshots {
        if !snapshot.variant.enabled {
            continue;
        }
        let normalized_rules = snapshot
            .rules
            .iter()
            .filter(|rule| rule.deleted_at.is_none())
            .map(|rule| NormalizedRule {
                placement: rule.placement,
                target: rule.target.clone(),
                operation: rule.operation,
                value_json: rule.value_json.clone(),
                description: rule.description.clone(),
            })
            .collect::<Vec<_>>();

        if normalized_rules.is_empty() {
            if let Some(suffix) = snapshot.variant.suffix.as_deref() {
                if snapshot.variant.model_id.is_some()
                    && snapshots.iter().any(|source_snapshot| {
                        source_snapshot.variant.model_id.is_none()
                            && source_snapshot.variant.suffix.as_deref() == Some(suffix)
                            && source_snapshot.variant.enabled
                            && !source_snapshot.rules.is_empty()
                    })
                {
                    continue;
                }
            }
            return Err(BaseError::ParamInvalid(Some(format!(
                "enabled request patch Variant {} has no effective Rules",
                snapshot.variant.id
            ))));
        }

        for (left_index, left) in normalized_rules.iter().enumerate() {
            for right in normalized_rules.iter().skip(left_index + 1) {
                if left.placement == RequestPatchPlacement::Body
                    && right.placement == RequestPatchPlacement::Body
                    && is_body_ancestor_or_descendant(&left.target, &right.target)
                {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "BODY targets '{}' and '{}' conflict in Variant {}",
                        left.target, right.target, snapshot.variant.id
                    ))));
                }
            }
        }

        let existing = snapshots
            .iter()
            .filter(|candidate| candidate.variant.id != snapshot.variant.id)
            .cloned()
            .collect::<Vec<_>>();
        validate_cross_layer_conflicts(&snapshot.variant, &normalized_rules, &existing)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::TestDbContext;
    use crate::database::model::Model;
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::upstream_source::{
        NewUpstreamSource, UpdateUpstreamSourceData, UpstreamSource,
    };
    use crate::schema::enum_def::{ModelKind, ProviderApiKeyMode, UpstreamProfileType};
    use serde_json::json;

    fn rule(
        placement: RequestPatchPlacement,
        target: &str,
        operation: RequestPatchOperation,
        value_json: Option<Value>,
    ) -> RequestPatchRuleInput {
        RequestPatchRuleInput {
            placement,
            target: target.to_string(),
            operation,
            value_json: value_json.map(Some),
            description: None,
        }
    }

    #[test]
    fn suffix_accepts_free_form_lowercase_tokens_without_prefix() {
        assert_eq!(
            normalize_suffix(Some("tool-use-v2")).expect("suffix should normalize"),
            Some("tool-use-v2".to_string())
        );
    }

    #[test]
    fn suffix_rejects_invalid_tokens() {
        for suffix in [
            "",
            "-high",
            "high-",
            "high--fast",
            "High",
            "tool_use",
            "a.b",
        ] {
            assert!(
                normalize_suffix(Some(suffix)).is_err(),
                "{suffix} should be rejected"
            );
        }
    }

    #[test]
    fn header_targets_are_normalized_and_reserved_targets_are_rejected() {
        assert_eq!(
            normalize_target(RequestPatchPlacement::Header, "X-Test")
                .expect("header should normalize"),
            "x-test"
        );
        assert!(validate_reserved_target(RequestPatchPlacement::Header, "host").is_err());
        assert!(validate_reserved_target(RequestPatchPlacement::Header, "x-request-id").is_err());
        assert!(
            validate_reserved_target(RequestPatchPlacement::Header, "anthropic-version").is_err()
        );
        assert!(validate_reserved_target(RequestPatchPlacement::Header, "anthropic-beta").is_ok());
    }

    #[test]
    fn rule_input_rejects_duplicate_and_body_ancestor_targets() {
        let duplicate = vec![
            rule(
                RequestPatchPlacement::Header,
                "X-Test",
                RequestPatchOperation::Set,
                Some(json!(true)),
            ),
            rule(
                RequestPatchPlacement::Header,
                "x-test",
                RequestPatchOperation::Set,
                Some(json!(false)),
            ),
        ];
        assert!(normalize_rules(&duplicate).is_err());

        let body_conflict = vec![
            rule(
                RequestPatchPlacement::Body,
                "/generation_config",
                RequestPatchOperation::Set,
                Some(json!({"temperature": 1})),
            ),
            rule(
                RequestPatchPlacement::Body,
                "/generation_config/temperature",
                RequestPatchOperation::Set,
                Some(json!(1)),
            ),
        ];
        assert!(normalize_rules(&body_conflict).is_err());
    }

    #[test]
    fn reserved_targets_are_always_rejected_without_confirmation_override() {
        for (placement, target) in [
            (RequestPatchPlacement::Header, "authorization"),
            (RequestPatchPlacement::Header, "proxy-authorization"),
            (RequestPatchPlacement::Header, "api-key"),
            (RequestPatchPlacement::Header, "x-api-key"),
            (RequestPatchPlacement::Header, "x-goog-api-key"),
            (RequestPatchPlacement::Header, "anthropic-version"),
            (RequestPatchPlacement::Header, "cookie"),
            (RequestPatchPlacement::Header, "openai-request-id"),
            (RequestPatchPlacement::Query, "key"),
            (RequestPatchPlacement::Query, "Key"),
            (RequestPatchPlacement::Query, "alt"),
            (RequestPatchPlacement::Query, "ALT"),
            (RequestPatchPlacement::Body, "/model"),
            (RequestPatchPlacement::Body, "/stream"),
            (RequestPatchPlacement::Body, "/stream_options"),
            (RequestPatchPlacement::Body, "/stream_options/include_usage"),
            (
                RequestPatchPlacement::Body,
                "/generationConfig/candidateCount",
            ),
            (RequestPatchPlacement::Body, "/generationConfig"),
        ] {
            let rules = vec![rule(
                placement,
                target,
                RequestPatchOperation::Set,
                Some(json!("forbidden")),
            )];
            assert!(normalize_rules(&rules).is_err(), "{placement:?} {target}");
        }

        assert!(
            normalize_rules(&[rule(
                RequestPatchPlacement::Body,
                "/messages/0/content",
                RequestPatchOperation::Set,
                Some(json!("allowed")),
            )])
            .is_ok()
        );
    }

    #[test]
    fn responses_stateless_body_targets_and_descendants_are_reserved_for_all_operations() {
        for operation in [RequestPatchOperation::Set, RequestPatchOperation::Remove] {
            for target in [
                "/store",
                "/store/enabled",
                "/previous_response_id",
                "/previous_response_id/value",
                "/conversation",
                "/conversation/id",
                "/background",
                "/background/enabled",
            ] {
                let value = (operation == RequestPatchOperation::Set).then(|| json!(true));
                assert!(
                    normalize_rules(&[
                        rule(RequestPatchPlacement::Body, target, operation, value,)
                    ])
                    .is_err(),
                    "{operation:?} {target}"
                );
            }
        }
    }

    #[test]
    fn operation_value_contract_is_strict() {
        assert!(
            normalize_rules(&[rule(
                RequestPatchPlacement::Header,
                "x-test",
                RequestPatchOperation::Remove,
                Some(json!("not-allowed")),
            )])
            .is_err()
        );
        assert!(
            normalize_rules(&[rule(
                RequestPatchPlacement::Header,
                "x-test",
                RequestPatchOperation::Set,
                Some(json!({"object": true})),
            )])
            .is_err()
        );
        assert!(
            normalize_rules(&[rule(
                RequestPatchPlacement::Body,
                "/metadata/tag",
                RequestPatchOperation::Set,
                Some(Value::Null),
            )])
            .is_ok()
        );
    }

    #[test]
    fn rule_input_deserialization_preserves_explicit_json_null_for_set() {
        let explicit_null = serde_json::from_value::<RequestPatchRuleInput>(json!({
            "placement": "BODY",
            "target": "/metadata/tag",
            "operation": "SET",
            "value_json": null,
            "description": null
        }))
        .expect("explicit JSON null should deserialize");
        assert_eq!(explicit_null.value_json, Some(None));
        let normalized = normalize_rules(&[explicit_null])
            .expect("explicit JSON null must satisfy the SET value contract");
        assert_eq!(normalized[0].value_json.as_deref(), Some("null"));

        let missing = serde_json::from_value::<RequestPatchRuleInput>(json!({
            "placement": "BODY",
            "target": "/metadata/tag",
            "operation": "SET",
            "description": null
        }))
        .expect("missing value_json should deserialize for domain validation");
        assert_eq!(missing.value_json, None);
        assert!(normalize_rules(&[missing]).is_err());
    }

    #[test]
    fn variant_shape_requires_rules_for_base_and_source_suffix() {
        let base = RequestPatchVariantInput {
            source_id: 1,
            model_id: None,
            suffix: None,
            enabled: true,
            expose_in_models: false,
            rules: Vec::new(),
        };
        assert!(validate_variant_shape(&base, &None, &[]).is_err());

        let source_suffix = RequestPatchVariantInput {
            suffix: Some("tools".to_string()),
            ..base.clone()
        };
        assert!(validate_variant_shape(&source_suffix, &source_suffix.suffix, &[]).is_err());

        let tombstone = RequestPatchVariantInput {
            model_id: Some(2),
            enabled: false,
            ..source_suffix
        };
        assert!(validate_variant_shape(&tombstone, &tombstone.suffix, &[]).is_ok());
    }

    #[test]
    fn exact_body_targets_do_not_conflict_but_ancestors_do() {
        let left = RequestPatchVariant {
            id: 1,
            source_id: 10,
            model_id: None,
            suffix: None,
            enabled: true,
            expose_in_models: false,
            deleted_at: None,
            created_at: 1,
            updated_at: 1,
        };
        let right = RequestPatchVariant {
            id: 2,
            ..left.clone()
        };
        let exact = rule(
            RequestPatchPlacement::Body,
            "/metadata/tag",
            RequestPatchOperation::Set,
            Some(json!(1)),
        );
        let exact_normalized = normalize_rules(&[exact]).expect("exact rule should normalize");
        let exact_existing = VariantSnapshot {
            variant: right.clone(),
            rules: vec![RequestPatchRule {
                id: 3,
                variant_id: right.id,
                placement: RequestPatchPlacement::Body,
                target: "/metadata/tag".to_string(),
                operation: RequestPatchOperation::Set,
                value_json: Some("2".to_string()),
                description: None,
                deleted_at: None,
                created_at: 1,
                updated_at: 1,
            }],
        };
        assert!(
            validate_cross_layer_conflicts(&left, &exact_normalized, &[exact_existing]).is_ok()
        );

        let ancestor = rule(
            RequestPatchPlacement::Body,
            "/metadata",
            RequestPatchOperation::Set,
            Some(json!({})),
        );
        let ancestor_normalized = normalize_rules(&[ancestor]).expect("ancestor should normalize");
        assert!(
            validate_cross_layer_conflicts(
                &left,
                &ancestor_normalized,
                &[VariantSnapshot {
                    variant: right,
                    rules: vec![RequestPatchRule {
                        id: 4,
                        variant_id: 2,
                        placement: RequestPatchPlacement::Body,
                        target: "/metadata/tag".to_string(),
                        operation: RequestPatchOperation::Set,
                        value_json: Some("2".to_string()),
                        description: None,
                        deleted_at: None,
                        created_at: 1,
                        updated_at: 1,
                    }],
                }],
            )
            .is_err()
        );
    }

    #[test]
    fn reactivation_validation_rejects_body_ancestor_conflicts_in_either_order() {
        let source_variant = RequestPatchVariant {
            id: 11,
            source_id: 12,
            model_id: None,
            suffix: Some("tools".to_string()),
            enabled: true,
            expose_in_models: true,
            deleted_at: None,
            created_at: 1,
            updated_at: 1,
        };
        let model_variant = RequestPatchVariant {
            id: 13,
            model_id: Some(14),
            ..source_variant.clone()
        };
        let source_rules = vec![RequestPatchRule {
            id: 15,
            variant_id: source_variant.id,
            placement: RequestPatchPlacement::Body,
            target: "/metadata".to_string(),
            operation: RequestPatchOperation::Set,
            value_json: Some("{}".to_string()),
            description: None,
            deleted_at: None,
            created_at: 1,
            updated_at: 1,
        }];
        let model_rules = vec![RequestPatchRule {
            id: 16,
            variant_id: model_variant.id,
            placement: RequestPatchPlacement::Body,
            target: "/metadata/tag".to_string(),
            operation: RequestPatchOperation::Set,
            value_json: Some("1".to_string()),
            description: None,
            deleted_at: None,
            created_at: 1,
            updated_at: 1,
        }];
        let source_aggregate = RequestPatchVariantAggregate {
            variant: source_variant,
            rules: source_rules,
        };
        let model_aggregate = RequestPatchVariantAggregate {
            variant: model_variant,
            rules: model_rules,
        };
        assert!(
            validate_active_variant_aggregates(&[
                source_aggregate.clone(),
                model_aggregate.clone()
            ])
            .is_err()
        );
        assert!(validate_active_variant_aggregates(&[model_aggregate, source_aggregate]).is_err());
    }

    #[test]
    fn reactivation_validation_allows_empty_model_suffix_only_with_source_rules() {
        let source_variant = RequestPatchVariant {
            id: 21,
            source_id: 22,
            model_id: None,
            suffix: Some("fast".to_string()),
            enabled: true,
            expose_in_models: true,
            deleted_at: None,
            created_at: 1,
            updated_at: 1,
        };
        let model_variant = RequestPatchVariant {
            id: 23,
            model_id: Some(24),
            ..source_variant.clone()
        };
        let source_aggregate = RequestPatchVariantAggregate {
            variant: source_variant.clone(),
            rules: vec![RequestPatchRule {
                id: 25,
                variant_id: source_variant.id,
                placement: RequestPatchPlacement::Body,
                target: "/options/temperature".to_string(),
                operation: RequestPatchOperation::Set,
                value_json: Some("0.2".to_string()),
                description: None,
                deleted_at: None,
                created_at: 1,
                updated_at: 1,
            }],
        };
        let model_aggregate = RequestPatchVariantAggregate {
            variant: model_variant,
            rules: Vec::new(),
        };
        assert!(validate_active_variant_aggregates(&[source_aggregate, model_aggregate]).is_ok());
    }

    fn seed_provider(provider_id: i64, source_id: i64) -> (Provider, UpstreamSource) {
        let aggregate = Provider::create(
            &NewProvider {
                id: provider_id,
                provider_key: format!("provider-{provider_id}"),
                name: format!("Provider {provider_id}"),
                is_enabled: true,
                created_at: 1,
                updated_at: 1,
                provider_api_key_mode: ProviderApiKeyMode::Queue,
            },
            &NewUpstreamSource {
                id: source_id,
                provider_id,
                profile_type: UpstreamProfileType::Openai,
                base_url: format!("https://source-{source_id}.example/v1"),
                use_proxy: false,
                is_enabled: true,
                is_default: true,
                created_at: 1,
                updated_at: 1,
                ..NewUpstreamSource::test_defaults(UpstreamProfileType::Openai)
            },
        )
        .expect("provider seed should succeed");
        (aggregate.provider, aggregate.upstream_sources[0].clone())
    }

    fn variant_input(
        source_id: i64,
        model_id: Option<i64>,
        suffix: Option<&str>,
        enabled: bool,
        expose_in_models: bool,
        rules: Vec<RequestPatchRuleInput>,
    ) -> RequestPatchVariantInput {
        RequestPatchVariantInput {
            source_id,
            model_id,
            suffix: suffix.map(str::to_string),
            enabled,
            expose_in_models,
            rules,
        }
    }

    fn body_set(target: &str, value: Value) -> RequestPatchRuleInput {
        rule(
            RequestPatchPlacement::Body,
            target,
            RequestPatchOperation::Set,
            Some(value),
        )
    }

    #[tokio::test]
    async fn repository_writes_and_replaces_full_variant_atomically() {
        let database = TestDbContext::new_sqlite("request-patch-variant-repository.sqlite");
        database
            .run_async(async {
                let (_provider, source) = seed_provider(8101, 8111);
                let created = RequestPatchVariantRepository::create(&variant_input(
                    source.id,
                    None,
                    Some("fast"),
                    true,
                    true,
                    vec![body_set("/options/temperature", json!(0.2))],
                ))
                .expect("variant should be created");
                assert_eq!(created.variant.source_id, source.id);
                assert_eq!(created.variant.suffix.as_deref(), Some("fast"));
                assert_eq!(created.rules.len(), 1);

                let replaced = RequestPatchVariantRepository::replace(
                    created.variant.id,
                    &variant_input(
                        source.id,
                        None,
                        Some("fast"),
                        true,
                        false,
                        vec![body_set("/options/top_p", json!(0.8))],
                    ),
                )
                .expect("variant replacement should succeed");
                assert_eq!(replaced.variant.id, created.variant.id);
                assert!(!replaced.variant.expose_in_models);
                assert_eq!(replaced.rules.len(), 1);
                assert_eq!(replaced.rules[0].target, "/options/top_p");
                assert_ne!(replaced.rules[0].id, created.rules[0].id);
            })
            .await;
    }

    #[tokio::test]
    async fn repository_rejects_invalid_write_without_partial_rows() {
        let database = TestDbContext::new_sqlite("request-patch-variant-rollback.sqlite");
        database
            .run_async(async {
                let (_provider, source) = seed_provider(8201, 8211);
                let invalid = variant_input(
                    source.id,
                    None,
                    None,
                    true,
                    false,
                    vec![rule(
                        RequestPatchPlacement::Header,
                        "Authorization",
                        RequestPatchOperation::Set,
                        Some(json!("secret")),
                    )],
                );
                assert!(RequestPatchVariantRepository::create(&invalid).is_err());
                assert!(
                    RequestPatchVariantRepository::list_by_source(source.id)
                        .expect("variant list should succeed")
                        .is_empty()
                );
            })
            .await;
    }

    #[tokio::test]
    async fn repository_enforces_owner_and_suffix_uniqueness_contracts() {
        let database = TestDbContext::new_sqlite("request-patch-variant-owners.sqlite");
        database
            .run_async(async {
                let (_provider, source) = seed_provider(8301, 8311);
                let (_other_provider, other_source) = seed_provider(8302, 8312);
                let model =
                    Model::create(source.provider_id, "model-a", None, ModelKind::Chat, true)
                        .expect("model should be created");
                let first = RequestPatchVariantRepository::create(&variant_input(
                    source.id,
                    None,
                    Some("fast"),
                    true,
                    true,
                    vec![body_set("/options/temperature", json!(0.2))],
                ))
                .expect("first suffix should be created");
                let duplicate = RequestPatchVariantRepository::create(&variant_input(
                    source.id,
                    None,
                    Some("fast"),
                    true,
                    false,
                    vec![body_set("/options/top_p", json!(0.8))],
                ));
                assert!(duplicate.is_err());

                let cross_provider = RequestPatchVariantRepository::create(&variant_input(
                    other_source.id,
                    Some(model.id),
                    Some("cross"),
                    true,
                    false,
                    vec![body_set("/options/top_p", json!(0.8))],
                ));
                assert!(cross_provider.is_err());
                assert_eq!(first.variant.source_id, source.id);
            })
            .await;
    }

    #[tokio::test]
    async fn repository_soft_delete_removes_active_rules_and_keeps_audit_rows() {
        let database = TestDbContext::new_sqlite("request-patch-variant-soft-delete.sqlite");
        database
            .run_async(async {
                let (_provider, source) = seed_provider(8401, 8411);
                let created = RequestPatchVariantRepository::create(&variant_input(
                    source.id,
                    None,
                    Some("fast"),
                    true,
                    false,
                    vec![body_set("/options/temperature", json!(0.2))],
                ))
                .expect("variant should be created");
                let deleted = RequestPatchVariantRepository::soft_delete(created.variant.id)
                    .expect("variant should be deleted");
                assert!(deleted.variant.deleted_at.is_some());
                assert!(RequestPatchVariantRepository::get(created.variant.id).is_err());
                assert!(
                    RequestPatchVariantRepository::list_by_source(source.id)
                        .expect("active list should succeed")
                        .is_empty()
                );
                let historical =
                    RequestPatchVariantRepository::list_by_source_ids_including_deleted(&[
                        source.id
                    ])
                    .expect("historical list should succeed");
                assert_eq!(historical.len(), 1);
                assert!(historical[0].rules.is_empty());
            })
            .await;
    }

    #[tokio::test]
    async fn replacing_empty_base_variant_retires_the_complete_aggregate() {
        let database = TestDbContext::new_sqlite("request-patch-empty-base-delete.sqlite");
        database
            .run_async(async {
                let (_provider, source) = seed_provider(8451, 8461);
                let created = RequestPatchVariantRepository::create(&variant_input(
                    source.id,
                    None,
                    None,
                    true,
                    false,
                    vec![body_set("/options/temperature", json!(0.2))],
                ))
                .expect("base Variant should be created");

                let deleted = RequestPatchVariantRepository::replace(
                    created.variant.id,
                    &variant_input(source.id, None, None, true, false, Vec::new()),
                )
                .expect("empty base replacement should delete the aggregate");

                assert!(deleted.variant.deleted_at.is_some());
                assert!(deleted.rules.is_empty());
                let historical =
                    RequestPatchVariantRepository::list_by_source_ids_including_deleted(&[
                        source.id
                    ])
                    .expect("historical aggregate should load");
                assert_eq!(historical.len(), 1);
                assert!(historical[0].rules.is_empty());
            })
            .await;
    }

    #[tokio::test]
    async fn source_suffix_mutations_reject_post_states_that_break_model_inheritance() {
        let database =
            TestDbContext::new_sqlite("request-patch-source-dependent-validation.sqlite");
        database
            .run_async(async {
                let (_provider, source) = seed_provider(8471, 8481);
                let model = Model::create(
                    source.provider_id,
                    "dependent-model",
                    None,
                    ModelKind::Chat,
                    true,
                )
                .expect("model should be created");
                let source_variant = RequestPatchVariantRepository::create(&variant_input(
                    source.id,
                    None,
                    Some("fast"),
                    true,
                    true,
                    vec![body_set("/options/temperature", json!(0.2))],
                ))
                .expect("source suffix should be created");
                RequestPatchVariantRepository::create(&variant_input(
                    source.id,
                    Some(model.id),
                    Some("fast"),
                    true,
                    false,
                    Vec::new(),
                ))
                .expect("empty Model suffix may inherit Source Rules");

                for invalid_replacement in [
                    variant_input(
                        source.id,
                        None,
                        Some("fast"),
                        false,
                        false,
                        vec![body_set("/options/temperature", json!(0.2))],
                    ),
                    variant_input(
                        source.id,
                        None,
                        Some("slow"),
                        true,
                        true,
                        vec![body_set("/options/temperature", json!(0.2))],
                    ),
                ] {
                    assert!(
                        RequestPatchVariantRepository::replace(
                            source_variant.variant.id,
                            &invalid_replacement,
                        )
                        .is_err()
                    );
                }
                assert!(
                    RequestPatchVariantRepository::soft_delete(source_variant.variant.id).is_err()
                );

                let persisted = RequestPatchVariantRepository::get(source_variant.variant.id)
                    .expect("failed mutations must leave the Source suffix intact");
                assert!(persisted.variant.enabled);
                assert_eq!(persisted.variant.suffix.as_deref(), Some("fast"));
            })
            .await;
    }

    #[tokio::test]
    async fn source_reactivation_validates_model_variants_on_the_source() {
        let database = TestDbContext::new_sqlite("request-patch-source-reactivation.sqlite");
        database
            .run_async(async {
                let (_provider, source) = seed_provider(8491, 8501);
                let model = Model::create(
                    source.provider_id,
                    "reactivated-model",
                    None,
                    ModelKind::Chat,
                    true,
                )
                .expect("model should be created");
                let source_variant = RequestPatchVariantRepository::create(&variant_input(
                    source.id,
                    None,
                    Some("fast"),
                    true,
                    true,
                    vec![body_set("/options/temperature", json!(0.2))],
                ))
                .expect("source suffix should be created");
                RequestPatchVariantRepository::create(&variant_input(
                    source.id,
                    Some(model.id),
                    Some("fast"),
                    true,
                    false,
                    Vec::new(),
                ))
                .expect("empty Model suffix may inherit Source Rules");
                UpstreamSource::update(
                    source.id,
                    source.provider_id,
                    &UpdateUpstreamSourceData {
                        base_url: None,
                        use_proxy: None,
                        is_enabled: Some(false),
                        is_default: None,
                        updated_at: 2,
                        ..UpdateUpstreamSourceData::test_defaults()
                    },
                )
                .expect("Source should be disabled");
                RequestPatchVariantRepository::soft_delete(source_variant.variant.id)
                    .expect("dormant Source suffix may be deleted while Source is disabled");

                assert!(
                    RequestPatchVariantRepository::validate_source_reactivation(source.id).is_err(),
                    "reactivation must reject the now-empty effective Model suffix"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn repository_preview_reports_conflicts_and_bounded_model_impact_without_writing() {
        let database = TestDbContext::new_sqlite("request-patch-variant-preview.sqlite");
        database
            .run_async(async {
                let (_provider, source) = seed_provider(8501, 8511);
                let model = Model::create(
                    source.provider_id,
                    "preview-model",
                    None,
                    ModelKind::Chat,
                    true,
                )
                .expect("model should be created");
                RequestPatchVariantRepository::create(&variant_input(
                    source.id,
                    None,
                    Some("fast"),
                    true,
                    true,
                    vec![body_set("/options", json!({"temperature": 0.2}))],
                ))
                .expect("source Variant should be created");

                let preview = RequestPatchVariantRepository::preview(
                    &variant_input(
                        source.id,
                        Some(model.id),
                        Some("fast"),
                        true,
                        false,
                        vec![body_set("/options/temperature", json!(0.8))],
                    ),
                    None,
                )
                .expect("preview should return conflicts as data");
                assert!(!preview.valid);
                assert_eq!(preview.conflicts.len(), 1);
                assert_eq!(
                    preview.conflicts[0].existing_suffix.as_deref(),
                    Some("fast")
                );
                assert_eq!(preview.affected_model_count, 1);
                assert!(preview.failure_reason.is_some());
                assert!(
                    RequestPatchVariantRepository::list_by_model_source(model.id, source.id)
                        .expect("model Variants should load")
                        .is_empty()
                );
            })
            .await;
    }

    #[tokio::test]
    async fn repository_preview_rejects_duplicate_variant_identity() {
        let database = TestDbContext::new_sqlite("request-patch-preview-identity.sqlite");
        database
            .run_async(async {
                let (_provider, source) = seed_provider(8521, 8531);
                RequestPatchVariantRepository::create(&variant_input(
                    source.id,
                    None,
                    Some("fast"),
                    true,
                    true,
                    vec![body_set("/options/temperature", json!(0.2))],
                ))
                .expect("existing suffix should be created");

                let preview = RequestPatchVariantRepository::preview(
                    &variant_input(
                        source.id,
                        None,
                        Some("fast"),
                        true,
                        false,
                        vec![body_set("/options/top_p", json!(0.8))],
                    ),
                    None,
                )
                .expect("duplicate identity should be reported as Preview data");
                assert!(!preview.valid);
                assert!(
                    preview
                        .failure_reason
                        .as_deref()
                        .is_some_and(|reason| reason.contains("owner and suffix"))
                );
            })
            .await;
    }
}
