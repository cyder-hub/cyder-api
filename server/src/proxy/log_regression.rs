fn hot_path_files() -> [(&'static str, &'static str); 12] {
    [
        ("auth.rs", include_str!("auth.rs")),
        ("gemini.rs", include_str!("gemini.rs")),
        ("generation.rs", include_str!("generation.rs")),
        ("pipeline.rs", include_str!("pipeline.rs")),
        ("request.rs", include_str!("request.rs")),
        ("runtime/executor.rs", include_str!("runtime/executor.rs")),
        ("runtime/facade.rs", include_str!("runtime/facade.rs")),
        ("runtime/scheduler.rs", include_str!("runtime/scheduler.rs")),
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
