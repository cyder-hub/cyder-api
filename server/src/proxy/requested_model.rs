use crate::service::cache::types::CacheModelsCatalog;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedRequestedModelName {
    pub original_requested_name: String,
    pub base_requested_name: String,
    pub requested_suffix: Option<String>,
}

pub(crate) fn enabled_patch_suffixes(catalog: &CacheModelsCatalog) -> Vec<String> {
    let mut suffixes = catalog
        .request_patch_variants
        .iter()
        .filter(|variant| variant.enabled)
        .filter_map(|variant| variant.suffix.clone())
        .collect::<Vec<_>>();
    suffixes.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    suffixes.dedup();
    suffixes
}

pub(crate) fn parse_patch_suffix(
    requested_name: &str,
    suffixes: &[String],
) -> Option<ResolvedRequestedModelName> {
    let (direct_provider, suffix_target) = requested_name
        .split_once('/')
        .map_or((None, requested_name), |(provider, model)| {
            (Some(provider), model)
        });

    for suffix in suffixes {
        let marker = format!("-{suffix}");
        let Some(base_target) = suffix_target.strip_suffix(&marker) else {
            continue;
        };
        if base_target.is_empty() {
            continue;
        }
        let base_requested_name = match direct_provider {
            Some(provider) => format!("{provider}/{base_target}"),
            None => base_target.to_string(),
        };
        return Some(ResolvedRequestedModelName {
            original_requested_name: requested_name.to_string(),
            base_requested_name,
            requested_suffix: Some(suffix.clone()),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_model_names_are_not_split_by_this_helper() {
        let parsed = parse_patch_suffix(
            "openai/gpt-4o-mini-fast",
            &["fast-long".to_string(), "fast".to_string()],
        )
        .unwrap();
        assert_eq!(parsed.base_requested_name, "openai/gpt-4o-mini");
        assert_eq!(parsed.requested_suffix.as_deref(), Some("fast"));
    }

    #[test]
    fn longest_suffix_wins_stably() {
        let parsed = parse_patch_suffix(
            "gpt-fast-long",
            &["fast".to_string(), "fast-long".to_string()],
        )
        .unwrap();
        assert_eq!(parsed.base_requested_name, "gpt");
        assert_eq!(parsed.requested_suffix.as_deref(), Some("fast-long"));
    }
}
