fn hot_path_files() -> [(&'static str, &'static str); 8] {
    [
        ("auth.rs", include_str!("auth.rs")),
        ("core.rs", include_str!("core.rs")),
        ("gemini.rs", include_str!("gemini.rs")),
        ("generation.rs", include_str!("generation.rs")),
        ("orchestrator.rs", include_str!("orchestrator.rs")),
        ("pipeline.rs", include_str!("pipeline.rs")),
        ("request.rs", include_str!("request.rs")),
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
