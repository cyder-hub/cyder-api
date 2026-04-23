use std::{
    fmt::{self, Write},
    str::FromStr,
};

use chrono::{DateTime, Local, SecondsFormat};
use log::{Level, LevelFilter, Log, Metadata, Record};

pub const THIRD_PARTY_DEBUG_ENV: &str = "CYDER_LOG_THIRD_PARTY_DEBUG";

static LOGGER: LocalLogger = LocalLogger;

fn event_message(event: &str) -> EventMessage {
    EventMessage::new(event)
}

#[doc(hidden)]
pub fn event_message_with_fields(event: &str, fields: &[(&str, Option<String>)]) -> EventMessage {
    let mut message = event_message(event);
    for (key, value) in fields {
        if let Some(value) = value {
            message.push_field(key, value);
        }
    }
    message
}

pub fn init(level: &str) {
    log::set_logger(&LOGGER).expect("local logger init");
    let level = LevelFilter::from_str(level).unwrap_or(LevelFilter::Info);
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
    target.starts_with("cyder_api")
        || target.starts_with("cyder_tools")
        || target.starts_with("xtask")
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
    use log::{Level, Record};

    use super::{THIRD_PARTY_DEBUG_ENV, format_log_line, is_app_target, third_party_debug_enabled};

    #[test]
    fn app_targets_are_recognized() {
        assert!(is_app_target("cyder_api::proxy::core"));
        assert!(is_app_target("cyder_tools::auth"));
        assert!(!is_app_target("aws_smithy_runtime::client"));
        assert!(!is_app_target("hyper_util::client::legacy::pool"));
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
}
