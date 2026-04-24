use std::collections::HashMap;

use crate::config::CacheBackendType;

pub(super) fn cache_backend_name(backend: CacheBackendType) -> &'static str {
    match backend {
        CacheBackendType::Memory => "memory",
        CacheBackendType::Redis => "redis",
    }
}

pub(super) fn increment_failure_counter(
    failures: &mut HashMap<&'static str, usize>,
    section: &'static str,
) {
    *failures.entry(section).or_default() += 1;
}

pub(super) fn summarize_failures(failures: &HashMap<&'static str, usize>) -> Option<String> {
    if failures.is_empty() {
        return None;
    }

    let mut sections = failures
        .iter()
        .map(|(section, count)| format!("{section}={count}"))
        .collect::<Vec<_>>();
    sections.sort();
    Some(sections.join(","))
}

pub(super) fn summarize_repo_names(names: &[&'static str]) -> Option<String> {
    if names.is_empty() {
        return None;
    }

    let mut names = names.to_vec();
    names.sort_unstable();
    Some(names.join(","))
}
