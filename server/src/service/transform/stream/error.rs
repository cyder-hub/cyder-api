use crate::service::transform::diagnostics::{
    build_fatal_stream_error_payload, log_transform_diagnostic,
};
use crate::service::transform::stream::StreamTransformer;
use crate::service::transform::unified::UnifiedTransformDiagnostic;
use crate::utils::sse::SseEvent;

pub(in crate::service::transform) fn controlled_error_sse(
    transformer: &mut StreamTransformer,
    stage: &'static str,
    message: String,
    raw_data: &str,
) -> Vec<SseEvent> {
    let payload = build_fatal_stream_error_payload(
        transformer.api_type,
        transformer.target_api_type,
        transformer.session.stream_id_clone(),
        stage,
        message,
        raw_data,
    );
    transformer.session.set_last_error(payload.clone());
    if let Ok(diagnostic) = serde_json::from_value::<UnifiedTransformDiagnostic>(payload.clone()) {
        log_transform_diagnostic(&diagnostic);
        transformer.session.record_diagnostic(diagnostic);
    }
    vec![SseEvent {
        event: Some("error".to_string()),
        data: serde_json::to_string(&payload).unwrap_or_else(|_| {
            "{\"type\":\"transform_error\",\"message\":\"serialization failure\"}".to_string()
        }),
        ..Default::default()
    }]
}
