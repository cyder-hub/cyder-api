use std::cell::RefCell;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::TransformProtocol;
use super::capability::TransformValueKind;
use super::policy::{
    PolicyDecision, PolicyEngine, TransformAction, TransformDiagnosticKind, TransformLossLevel,
};
use super::stream::StreamTransformContext;
use super::unified::{
    UnifiedTransformDiagnostic, UnifiedTransformDiagnosticAction, UnifiedTransformDiagnosticKind,
    UnifiedTransformDiagnosticLossLevel,
};
use crate::schema::enum_def::LlmApiType;
use crate::utils::sse::SseEvent;

fn diagnostic_action(action: TransformAction) -> UnifiedTransformDiagnosticAction {
    match action {
        TransformAction::Send => UnifiedTransformDiagnosticAction::Send,
        TransformAction::Drop => UnifiedTransformDiagnosticAction::Drop,
        TransformAction::Reject => UnifiedTransformDiagnosticAction::Reject,
    }
}

fn diagnostic_loss_level(level: TransformLossLevel) -> UnifiedTransformDiagnosticLossLevel {
    match level {
        TransformLossLevel::Lossless => UnifiedTransformDiagnosticLossLevel::Lossless,
        TransformLossLevel::LossyMinor => UnifiedTransformDiagnosticLossLevel::LossyMinor,
        TransformLossLevel::LossyMajor => UnifiedTransformDiagnosticLossLevel::LossyMajor,
        TransformLossLevel::Reject => UnifiedTransformDiagnosticLossLevel::Reject,
    }
}

fn diagnostic_kind(kind: TransformDiagnosticKind) -> UnifiedTransformDiagnosticKind {
    match kind {
        TransformDiagnosticKind::FatalTransformError => {
            UnifiedTransformDiagnosticKind::FatalTransformError
        }
        TransformDiagnosticKind::LossyTransform => UnifiedTransformDiagnosticKind::LossyTransform,
        TransformDiagnosticKind::CapabilityDowngrade => {
            UnifiedTransformDiagnosticKind::CapabilityDowngrade
        }
    }
}

fn protocol_name(protocol: TransformProtocol) -> String {
    match protocol {
        TransformProtocol::Unified => "unified".to_string(),
        TransformProtocol::Api(api) => format!("{api:?}"),
    }
}

fn sha256_hex(body: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(body.as_ref()))
}

fn top_level_json_field_count(value: &Value) -> usize {
    match value {
        Value::Object(map) => map.len(),
        Value::Array(items) => items.len(),
        Value::Null => 0,
        Value::Bool(_) | Value::Number(_) | Value::String(_) => 1,
    }
}

pub(in crate::service::transform) fn json_value_log_summary(
    value: &Value,
) -> (usize, String, usize) {
    let serialized = serde_json::to_vec(value).unwrap_or_default();
    (
        serialized.len(),
        sha256_hex(&serialized),
        top_level_json_field_count(value),
    )
}

pub(in crate::service::transform) fn raw_payload_summary(raw_data: &str) -> String {
    let mut summary = format!("bytes={} sha256={}", raw_data.len(), sha256_hex(raw_data));
    if let Ok(value) = serde_json::from_str::<Value>(raw_data) {
        summary.push_str(&format!(
            " json_top_level_fields={}",
            top_level_json_field_count(&value)
        ));
    }
    summary
}

pub(in crate::service::transform) fn log_transform_diagnostic(
    diagnostic: &UnifiedTransformDiagnostic,
) {
    let diagnostic_kind = format!("{:?}", diagnostic.diagnostic_kind);
    let loss_level = format!("{:?}", diagnostic.loss_level);
    let action = format!("{:?}", diagnostic.action);
    let stage = diagnostic.stage.as_deref();
    let stream_id = diagnostic.stream_id.as_deref();

    match diagnostic.loss_level {
        UnifiedTransformDiagnosticLossLevel::Lossless => crate::debug_event!(
            "transform.diagnostic",
            diagnostic_kind = diagnostic_kind,
            loss_level = loss_level,
            source_api = &diagnostic.source,
            target_api = &diagnostic.target,
            semantic_unit = &diagnostic.semantic_unit,
            action = action,
            stage = stage,
            stream_id = stream_id,
        ),
        UnifiedTransformDiagnosticLossLevel::LossyMinor
        | UnifiedTransformDiagnosticLossLevel::LossyMajor => crate::warn_event!(
            "transform.diagnostic",
            diagnostic_kind = diagnostic_kind,
            loss_level = loss_level,
            source_api = &diagnostic.source,
            target_api = &diagnostic.target,
            semantic_unit = &diagnostic.semantic_unit,
            action = action,
            stage = stage,
            stream_id = stream_id,
        ),
        UnifiedTransformDiagnosticLossLevel::Reject => crate::error_event!(
            "transform.diagnostic",
            diagnostic_kind = diagnostic_kind,
            loss_level = loss_level,
            source_api = &diagnostic.source,
            target_api = &diagnostic.target,
            semantic_unit = &diagnostic.semantic_unit,
            action = action,
            stage = stage,
            stream_id = stream_id,
        ),
    }
}

pub(in crate::service::transform) fn build_transform_diagnostic(
    diagnostic_kind_value: TransformDiagnosticKind,
    source: TransformProtocol,
    target: TransformProtocol,
    kind: TransformValueKind,
    decision: PolicyDecision,
    stream_id: Option<String>,
    stage: Option<&str>,
    context: Option<&str>,
    raw_data_summary: Option<String>,
    recovery_hint: Option<String>,
) -> UnifiedTransformDiagnostic {
    UnifiedTransformDiagnostic {
        type_: match diagnostic_kind_value {
            TransformDiagnosticKind::FatalTransformError => "transform_error".to_string(),
            TransformDiagnosticKind::LossyTransform
            | TransformDiagnosticKind::CapabilityDowngrade => "transform_diagnostic".to_string(),
        },
        diagnostic_kind: diagnostic_kind(diagnostic_kind_value),
        provider: protocol_name(source),
        target_provider: protocol_name(target),
        source: protocol_name(source),
        target: protocol_name(target),
        stream_id,
        stage: stage.map(ToString::to_string),
        loss_level: diagnostic_loss_level(decision.level),
        action: diagnostic_action(decision.action),
        semantic_unit: format!("{kind:?}"),
        reason: decision.reason.to_string(),
        context: context.map(ToString::to_string),
        raw_data_summary,
        recovery_hint,
    }
}

thread_local! {
    static TRANSFORM_DIAGNOSTIC_STACK: RefCell<Vec<Vec<UnifiedTransformDiagnostic>>> = const {
        RefCell::new(Vec::new())
    };
}

pub(in crate::service::transform) fn capture_transform_diagnostics<T>(
    f: impl FnOnce() -> T,
) -> (T, Vec<UnifiedTransformDiagnostic>) {
    TRANSFORM_DIAGNOSTIC_STACK.with(|stack| {
        stack.borrow_mut().push(Vec::new());
    });

    let result = f();
    let diagnostics =
        TRANSFORM_DIAGNOSTIC_STACK.with(|stack| stack.borrow_mut().pop().unwrap_or_default());

    (result, diagnostics)
}

pub(in crate::service::transform) fn record_captured_transform_diagnostic(
    diagnostic: &UnifiedTransformDiagnostic,
) {
    TRANSFORM_DIAGNOSTIC_STACK.with(|stack| {
        if let Some(active_capture) = stack.borrow_mut().last_mut() {
            active_capture.push(diagnostic.clone());
        }
    });
}

pub(in crate::service::transform) fn build_stream_diagnostic_sse(
    stream_context: &mut StreamTransformContext<'_>,
    source: TransformProtocol,
    target: TransformProtocol,
    kind: TransformValueKind,
    stage: &'static str,
    context_message: String,
    raw_data_summary: Option<String>,
    recovery_hint: Option<String>,
) -> SseEvent {
    let decision = PolicyEngine::evaluate(source, target, kind);
    let diagnostic = build_transform_diagnostic(
        decision.diagnostic_kind,
        source,
        target,
        kind,
        decision,
        stream_context.stream_id_clone(),
        Some(stage),
        Some(&context_message),
        raw_data_summary,
        recovery_hint.or_else(|| Some(decision.reason.to_string())),
    );
    let diagnostic_json = serde_json::to_string(&diagnostic).unwrap_or_else(|_| {
        "{\"type\":\"transform_diagnostic\",\"message\":\"serialization failure\"}".to_string()
    });
    log_transform_diagnostic(&diagnostic);
    stream_context.record_diagnostic(diagnostic);

    SseEvent {
        event: Some("transform_diagnostic".to_string()),
        data: diagnostic_json,
        ..Default::default()
    }
}

pub(in crate::service::transform) fn build_fatal_stream_error_payload(
    source_api: LlmApiType,
    target_api: LlmApiType,
    stream_id: Option<String>,
    stage: &'static str,
    message: String,
    raw_data: &str,
) -> Value {
    let raw_summary = raw_payload_summary(raw_data);
    let decision = PolicyDecision {
        diagnostic_kind: TransformDiagnosticKind::FatalTransformError,
        level: TransformLossLevel::Reject,
        action: TransformAction::Reject,
        reason: "A fatal transform error interrupted this streaming conversion.",
    };

    serde_json::to_value(build_transform_diagnostic(
        TransformDiagnosticKind::FatalTransformError,
        TransformProtocol::Api(source_api),
        TransformProtocol::Api(target_api),
        TransformValueKind::StreamError,
        decision,
        stream_id.clone(),
        Some(stage),
        Some(&message),
        Some(raw_summary.clone()),
        Some(
            "Inspect the raw summary and recent stream diagnostics to recover context.".to_string(),
        ),
    ))
    .unwrap_or_else(|_| {
        serde_json::json!({
            "type": "transform_error",
            "stage": stage,
            "provider": format!("{:?}", source_api),
            "target": format!("{:?}", target_api),
            "stream_id": stream_id,
            "message": message,
            "raw_data_summary": raw_summary
        })
    })
}
