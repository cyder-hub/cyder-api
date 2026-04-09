use std::str::FromStr;

use chrono::{DateTime, Local, SecondsFormat};
use log::{Level, LevelFilter, Log, Metadata, Record};

pub const THIRD_PARTY_DEBUG_ENV: &str = "CYDER_LOG_THIRD_PARTY_DEBUG";

static LOGGER: LocalLogger = LocalLogger;

pub fn init(level: &str) {
    log::set_logger(&LOGGER).expect("local logger init");
    let level = LevelFilter::from_str(level).unwrap_or(LevelFilter::Info);
    log::set_max_level(level);
}

struct LocalLogger;

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
        let output = format!(
            "[{}] {:>5}: {}",
            time_string,
            record.metadata().level(),
            record.args()
        );
        println!("{}", output);
    }

    fn flush(&self) {}
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

#[cfg(test)]
mod tests {
    use super::{THIRD_PARTY_DEBUG_ENV, is_app_target, third_party_debug_enabled};

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
}
