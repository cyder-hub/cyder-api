fn hot_path_files() -> [(&'static str, &'static str); 11] {
    [
        ("auth.rs", include_str!("auth.rs")),
        ("gemini.rs", include_str!("gemini.rs")),
        ("generation.rs", include_str!("generation.rs")),
        ("pipeline.rs", include_str!("pipeline.rs")),
        ("request.rs", include_str!("request.rs")),
        ("runtime/executor.rs", include_str!("runtime/executor.rs")),
        ("runtime/facade.rs", include_str!("runtime/facade.rs")),
        (
            "runtime/transport/mod.rs",
            include_str!("runtime/transport/mod.rs"),
        ),
        (
            "runtime/transport/non_stream.rs",
            include_str!("runtime/transport/non_stream.rs"),
        ),
        (
            "runtime/transport/stream.rs",
            include_str!("runtime/transport/stream.rs"),
        ),
        ("utility.rs", include_str!("utility.rs")),
    ]
}

#[test]
fn ordinary_success_path_modules_do_not_emit_info_logs() {
    for (path, contents) in hot_path_files() {
        assert!(
            !contents.contains("info!(") && !contents.contains("info_event!("),
            "{path} should not emit info-level logs on the ordinary request path"
        );
    }
}

#[test]
fn hot_path_modules_do_not_use_structured_builder_api() {
    for (path, contents) in hot_path_files() {
        assert!(
            !contents.contains("event_message(") && !contents.contains(".field("),
            "{path} should not use the structured log builder API directly"
        );
    }
}

#[test]
fn request_log_builder_ignores_transient_error_stage_and_visibility() {
    let source = include_str!("logging.rs");
    let builder = source
        .split_once("fn build_request_log")
        .expect("request log builder should exist")
        .1
        .split_once("#[derive(Default)]")
        .expect("request log builder should end before CostOutcome")
        .0;

    assert!(
        !builder.contains("final_error_stage"),
        "transient error stage must not enter the Diesel RequestLog payload"
    );
    assert!(
        !builder.contains("response_visibility"),
        "transient response visibility must not enter the Diesel RequestLog payload"
    );
}

#[test]
fn proxy_error_events_never_log_client_payload_or_upstream_body() {
    let logging_source = include_str!("../logging.rs");
    let helper = logging_source
        .split_once("fn proxy_error_event_message")
        .expect("proxy error event helper should exist")
        .1
        .split_once("pub(crate) fn log_proxy_error_event")
        .expect("proxy error event helper should have a bounded body")
        .0;

    for forbidden in [
        "client_payload",
        "response_body",
        "body_text",
        "body_base64",
    ] {
        assert!(
            !helper.contains(forbidden),
            "proxy error event helper must not access {forbidden}"
        );
    }
}
