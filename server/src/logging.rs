use std::fmt::{self, Write};
use std::future::Future;

use chrono::{DateTime, Local, SecondsFormat};
use log::{Level, LevelFilter, Log, Metadata, Record};

use crate::proxy::{ProxyError, ProxyLogLevel};

pub const THIRD_PARTY_DEBUG_ENV: &str = "CYDER_LOG_THIRD_PARTY_DEBUG";

static LOGGER: LocalLogger = LocalLogger;

tokio::task_local! {
    static REQUEST_ID_SCOPE: String;
}

fn event_message(event: &str) -> EventMessage {
    EventMessage::new(event)
}

pub(crate) async fn with_request_id_scope<F>(request_id: String, future: F) -> F::Output
where
    F: Future,
{
    REQUEST_ID_SCOPE.scope(request_id, future).await
}

#[doc(hidden)]
pub fn event_message_with_fields(event: &str, fields: &[(&str, Option<String>)]) -> EventMessage {
    let mut message = event_message(event);
    if !fields.iter().any(|(key, _)| *key == "request_id") {
        let _ = REQUEST_ID_SCOPE.try_with(|request_id| {
            message.push_field("request_id", request_id);
        });
    }
    for (key, value) in fields {
        if let Some(value) = value {
            message.push_field(key, value);
        }
    }
    message
}

fn proxy_error_event_message(
    event: &'static str,
    request_id: Option<&str>,
    log_id: Option<i64>,
    error: &ProxyError,
) -> EventMessage {
    let upstream_error = error.upstream_error();
    let mut fields = Vec::with_capacity(12);
    if let Some(request_id) = request_id {
        fields.push(("request_id", Some(request_id.to_string())));
    }
    fields.extend([
        ("log_id", log_id.map(|value| value.to_string())),
        ("error_code", Some(error.code().as_str().to_string())),
        ("stage", Some(error.stage().as_str().to_string())),
        (
            "timeout_phase",
            error
                .timeout_phase()
                .map(|phase| phase.as_str().to_string()),
        ),
        (
            "response_visibility",
            Some(error.response_visibility().as_str().to_string()),
        ),
        (
            "error_http_status",
            Some(error.status_code().as_u16().to_string()),
        ),
        (
            "upstream_status",
            upstream_error.map(|payload| payload.status().to_string()),
        ),
        (
            "upstream_payload_kind",
            upstream_error.map(|payload| payload.payload_kind().as_str().to_string()),
        ),
        (
            "upstream_captured_bytes",
            upstream_error.map(|payload| payload.captured_bytes().to_string()),
        ),
        (
            "upstream_limit_bytes",
            upstream_error.map(|payload| payload.limit_bytes().to_string()),
        ),
        (
            "upstream_truncated",
            upstream_error.map(|payload| payload.truncated().to_string()),
        ),
        (
            "operator_message",
            Some(error.operator_message().chars().take(2_000).collect()),
        ),
    ]);
    event_message_with_fields(event, &fields)
}

pub(crate) fn log_proxy_error_event(
    event: &'static str,
    request_id: Option<&str>,
    log_id: Option<i64>,
    error: &ProxyError,
) {
    let message = proxy_error_event_message(event, request_id, log_id, error);
    match error.operator_log_level() {
        ProxyLogLevel::Debug => log::debug!(target: "cyder_api::proxy", "{message}"),
        ProxyLogLevel::Warn => log::warn!(target: "cyder_api::proxy", "{message}"),
        ProxyLogLevel::Error => log::error!(target: "cyder_api::proxy", "{message}"),
    }
}

pub fn init(level: &str) {
    log::set_logger(&LOGGER).expect("local logger init");
    let level = parse_level(level).unwrap_or(LevelFilter::Info);
    log::set_max_level(level);
}

pub fn parse_level(level: &str) -> Result<LevelFilter, String> {
    match level.trim().to_ascii_lowercase().as_str() {
        "trace" => Ok(LevelFilter::Trace),
        "debug" => Ok(LevelFilter::Debug),
        "info" => Ok(LevelFilter::Info),
        "warn" => Ok(LevelFilter::Warn),
        "error" => Ok(LevelFilter::Error),
        _ => Err(format!(
            "invalid log level '{level}', expected trace, debug, info, warn, or error"
        )),
    }
}

pub fn set_level(level: &str) -> Result<(), String> {
    let level = parse_level(level)?;
    set_level_filter(level);
    Ok(())
}

pub fn set_level_filter(level: LevelFilter) {
    log::set_max_level(level);
}

struct LocalLogger;

#[doc(hidden)]
pub struct EventMessage {
    event: String,
    fields: Vec<(String, String)>,
}

impl EventMessage {
    fn new(event: &str) -> Self {
        Self {
            event: event.to_string(),
            fields: Vec::new(),
        }
    }

    fn push_field(&mut self, key: &str, value: &str) {
        self.fields
            .push((key.to_string(), format_kv_value(&single_line(value))));
    }
}

impl fmt::Display for EventMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "event={}", self.event)?;
        for (key, value) in &self.fields {
            write!(f, " {key}={value}")?;
        }
        Ok(())
    }
}

impl Log for LocalLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        if metadata.level().to_level_filter() > log::max_level() {
            return false;
        }

        if metadata.level() <= Level::Info {
            return true;
        }

        third_party_debug_enabled() || is_app_target(metadata.target())
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let current_time: DateTime<Local> = Local::now();
        let time_string = current_time.to_rfc3339_opts(SecondsFormat::Millis, true);
        let output = format_log_line(record, &time_string);
        println!("{}", output);
    }

    fn flush(&self) {}
}

fn format_log_line(record: &Record<'_>, time_string: &str) -> String {
    let mut output = String::with_capacity(time_string.len() + record.target().len() + 64);
    write!(
        &mut output,
        "[{}] {:>5} target={} {}",
        time_string,
        record.metadata().level(),
        record.target(),
        normalize_log_body(&record.args().to_string())
    )
    .expect("format log line");
    output
}

fn normalize_log_body(message: &str) -> String {
    let message = single_line(message);
    if message.is_empty() {
        return "event=log.empty".to_string();
    }

    if message.starts_with("event=") {
        return message;
    }

    format!("event=log.legacy message={}", format_kv_value(&message))
}

fn single_line(value: impl AsRef<str>) -> String {
    let mut output = String::with_capacity(value.as_ref().len());
    for ch in value.as_ref().chars() {
        match ch {
            '\n' | '\r' | '\t' => output.push(' '),
            _ => output.push(ch),
        }
    }
    output.trim().to_string()
}

fn format_kv_value(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }

    if is_plain_value(value) {
        return value.to_string();
    }

    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}

fn is_plain_value(value: &str) -> bool {
    value.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/' | '@' | '+' | ',')
    })
}

fn third_party_debug_enabled() -> bool {
    std::env::var(THIRD_PARTY_DEBUG_ENV)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn is_app_target(target: &str) -> bool {
    target.starts_with("cyder_api") || target.starts_with("cyder_tools")
}

#[doc(hidden)]
pub trait EventFieldValue {
    fn into_event_field_value(self) -> Option<String>;
}

impl<T> EventFieldValue for &Option<T>
where
    T: fmt::Display,
{
    fn into_event_field_value(self) -> Option<String> {
        self.as_ref().map(|value| single_line(value.to_string()))
    }
}

impl<T> EventFieldValue for &&T
where
    T: fmt::Display + ?Sized,
{
    fn into_event_field_value(self) -> Option<String> {
        Some(single_line(self.to_string()))
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! __event_message {
    ($event:literal $(,)?) => {{
        $crate::logging::event_message_with_fields($event, &[])
    }};
    ($event:literal $(, $key:ident = $value:expr )* $(,)?) => {{
        use $crate::logging::EventFieldValue as _;
        let fields = [$( (stringify!($key), (&&$value).into_event_field_value()) ),*];
        $crate::logging::event_message_with_fields($event, &fields)
    }};
}

#[macro_export]
macro_rules! debug_event {
    ($($tt:tt)*) => {
        ::log::debug!("{}", $crate::__event_message!($($tt)*))
    };
}

#[macro_export]
macro_rules! info_event {
    ($($tt:tt)*) => {
        ::log::info!("{}", $crate::__event_message!($($tt)*))
    };
}

#[macro_export]
macro_rules! warn_event {
    ($($tt:tt)*) => {
        ::log::warn!("{}", $crate::__event_message!($($tt)*))
    };
}

#[macro_export]
macro_rules! error_event {
    ($($tt:tt)*) => {
        ::log::error!("{}", $crate::__event_message!($($tt)*))
    };
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use log::{Level, LevelFilter, Record};

    use super::{
        THIRD_PARTY_DEBUG_ENV, format_log_line, is_app_target, parse_level,
        proxy_error_event_message, set_level, third_party_debug_enabled, with_request_id_scope,
    };
    use crate::proxy::{
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, TimeoutPhase,
        classify_upstream_status,
    };
    use axum::http::{HeaderValue, StatusCode};

    static LOG_LEVEL_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn app_targets_are_recognized() {
        assert!(is_app_target("cyder_api::proxy::core"));
        assert!(is_app_target("cyder_tools::auth"));
        assert!(!is_app_target("aws_smithy_runtime::client"));
        assert!(!is_app_target("hyper_util::client::legacy::pool"));
    }

    #[test]
    fn legal_log_levels_parse() {
        assert_eq!(parse_level("trace"), Ok(LevelFilter::Trace));
        assert_eq!(parse_level("debug"), Ok(LevelFilter::Debug));
        assert_eq!(parse_level("info"), Ok(LevelFilter::Info));
        assert_eq!(parse_level("warn"), Ok(LevelFilter::Warn));
        assert_eq!(parse_level("error"), Ok(LevelFilter::Error));
        assert!(parse_level("off").is_err());
        assert!(parse_level("invalid").is_err());
    }

    #[test]
    fn set_level_updates_global_max_level_and_can_be_restored() {
        let _guard = LOG_LEVEL_TEST_LOCK.lock().expect("log level lock");
        let previous = log::max_level();

        set_level("debug").expect("debug level should apply");
        assert_eq!(log::max_level(), LevelFilter::Debug);

        log::set_max_level(previous);
    }

    #[test]
    fn third_party_debug_flag_is_opt_in() {
        unsafe {
            std::env::remove_var(THIRD_PARTY_DEBUG_ENV);
        }
        assert!(!third_party_debug_enabled());

        unsafe {
            std::env::set_var(THIRD_PARTY_DEBUG_ENV, "1");
        }
        assert!(third_party_debug_enabled());

        unsafe {
            std::env::remove_var(THIRD_PARTY_DEBUG_ENV);
        }
    }

    #[test]
    fn format_log_line_includes_target_and_structured_event_body() {
        let message = crate::__event_message!(
            "startup.server_started",
            target_addr = "127.0.0.1:8080",
            base_path = "/ai",
            log_level = "debug",
        )
        .to_string();
        let args = format_args!("{message}");
        let record = Record::builder()
            .args(args)
            .level(Level::Info)
            .target("cyder_api::main")
            .build();

        let rendered = format_log_line(&record, "2026-04-23T10:00:00.000+08:00");
        assert_eq!(
            rendered,
            "[2026-04-23T10:00:00.000+08:00]  INFO target=cyder_api::main event=startup.server_started target_addr=127.0.0.1:8080 base_path=/ai log_level=debug"
        );
    }

    #[test]
    fn format_log_line_wraps_legacy_messages_with_default_event() {
        let message =
            "Third-party debug logs are muted;\nset CYDER_LOG_THIRD_PARTY_DEBUG=1".to_string();
        let args = format_args!("{message}");
        let record = Record::builder()
            .args(args)
            .level(Level::Info)
            .target("cyder_api::main")
            .build();

        let rendered = format_log_line(&record, "2026-04-23T10:00:00.000+08:00");
        assert_eq!(
            rendered,
            "[2026-04-23T10:00:00.000+08:00]  INFO target=cyder_api::main event=log.legacy message=\"Third-party debug logs are muted; set CYDER_LOG_THIRD_PARTY_DEBUG=1\""
        );
    }

    #[test]
    fn structured_event_macro_omits_none_fields_and_accepts_trailing_comma() {
        let route_id: Option<i64> = None;
        let route_name = Some("primary");
        let message = crate::__event_message!(
            "proxy.request_failed",
            log_id = 42,
            route_id = route_id,
            route_name = route_name,
            error_code = Some("server_error"),
        )
        .to_string();

        assert_eq!(
            message,
            "event=proxy.request_failed log_id=42 route_name=primary error_code=server_error"
        );
    }

    #[test]
    fn structured_event_macro_supports_zero_fields() {
        let message = crate::__event_message!("logging.flush_waiter_dropped").to_string();
        assert_eq!(message, "event=logging.flush_waiter_dropped");
    }

    #[tokio::test]
    async fn request_id_scope_enriches_events_without_overriding_explicit_fields() {
        let (implicit, explicit) = with_request_id_scope("gateway-request".to_string(), async {
            (
                crate::__event_message!("proxy.request_received").to_string(),
                crate::__event_message!(
                    "proxy.request_completed",
                    request_id = "late-request",
                    status = 200,
                )
                .to_string(),
            )
        })
        .await;

        assert_eq!(
            implicit,
            "event=proxy.request_received request_id=gateway-request"
        );
        assert_eq!(
            explicit,
            "event=proxy.request_completed request_id=late-request status=200"
        );
        assert_eq!(
            crate::__event_message!("outside.request_scope").to_string(),
            "event=outside.request_scope"
        );
    }

    #[tokio::test]
    async fn proxy_error_event_contains_stable_facts_without_upstream_body() {
        let content_type = HeaderValue::from_static("application/json");
        let upstream_body =
            br#"{"error":{"message":"quota exceeded","details":["provider-secret-detail"]}}"#;
        let error = classify_upstream_status(
            StatusCode::TOO_MANY_REQUESTS,
            Some(&content_type),
            upstream_body,
            65_536,
            ResponseVisibility::NotVisible,
        );

        let message = with_request_id_scope("gateway-request".to_string(), async {
            proxy_error_event_message("proxy.error_response", None, Some(42), &error).to_string()
        })
        .await;

        for expected in [
            "event=proxy.error_response",
            "request_id=gateway-request",
            "log_id=42",
            "error_code=upstream_rate_limit_error",
            "stage=upstream_response",
            "response_visibility=not_visible",
            "error_http_status=429",
            "upstream_status=429",
            "upstream_payload_kind=json",
            "upstream_limit_bytes=65536",
            "upstream_truncated=false",
            "operator_message=\"Upstream returned 429: JSON error body with a message field (75 captured bytes)\"",
        ] {
            assert!(message.contains(expected), "missing {expected}: {message}");
        }
        assert!(message.contains(&format!("upstream_captured_bytes={}", upstream_body.len())));
        assert!(!message.contains("quota exceeded"));
        assert!(!message.contains("provider-secret-detail"));
    }

    #[test]
    fn proxy_timeout_event_contains_the_typed_timeout_phase() {
        let error = ProxyError::upstream_timeout(
            TimeoutPhase::FirstByte,
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::NotVisible,
            "first byte timeout",
        );
        let message = proxy_error_event_message("proxy.timeout", None, None, &error).to_string();

        assert!(message.contains("error_code=upstream_timeout_error"));
        assert!(message.contains("timeout_phase=first_byte"));
        assert!(!message.contains("timeout_phase=total"));
        assert_eq!(error.code(), ProxyErrorCode::UpstreamTimeoutError);
    }

    #[tokio::test]
    async fn concurrent_request_id_scopes_do_not_leak_between_tasks() {
        let (first, second) = tokio::join!(
            with_request_id_scope("request-a".to_string(), async {
                tokio::task::yield_now().await;
                crate::__event_message!("proxy.concurrent").to_string()
            }),
            with_request_id_scope("request-b".to_string(), async {
                tokio::task::yield_now().await;
                crate::__event_message!("proxy.concurrent").to_string()
            }),
        );

        assert_eq!(
            first, "event=proxy.concurrent request_id=request-a",
            "first scope must retain its own request id"
        );
        assert_eq!(
            second, "event=proxy.concurrent request_id=request-b",
            "second scope must retain its own request id"
        );
    }

    #[tokio::test]
    async fn explicit_request_id_survives_after_request_scope_ends() {
        with_request_id_scope("gateway-request".to_string(), async {
            assert_eq!(
                crate::__event_message!("proxy.handler_finished").to_string(),
                "event=proxy.handler_finished request_id=gateway-request"
            );
        })
        .await;

        let late_event = crate::__event_message!(
            "logging.request_log_inserted",
            request_id = "gateway-request",
            log_id = 42,
        )
        .to_string();

        assert_eq!(
            late_event,
            "event=logging.request_log_inserted request_id=gateway-request log_id=42"
        );
    }
}
