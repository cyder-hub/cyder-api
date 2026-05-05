use serde_json::Value;

use crate::schema::enum_def::ProviderType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenAiVariant {
    Standard,
    GeminiCompat,
}

#[derive(Clone, Copy)]
pub(crate) struct DefaultInjectionRule {
    field: &'static str,
    value: fn() -> Value,
}

#[derive(Clone, Copy)]
pub(crate) struct ChannelSchemaPolicy {
    allowed_top_level_fields: &'static [&'static str],
    forbidden_top_level_fields: &'static [&'static str],
    default_injections: &'static [DefaultInjectionRule],
}

#[derive(Clone, Copy)]
pub(crate) struct OpenAiVariantPolicy {
    #[cfg(test)]
    variant: OpenAiVariant,
    schema: ChannelSchemaPolicy,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct OpenAiSanitizeReport {
    pub removed_fields: Vec<String>,
    pub injected_defaults: Vec<String>,
}

const GEMINI_COMPAT_ALLOWED_FIELDS: &[&str] = &[
    "messages",
    "model",
    "detail",
    "max_completion_tokens",
    "modalities",
    "max_tokens",
    "n",
    "frequency_penalty",
    "presence_penalty",
    "reasoning_effort",
    "response_format",
    "seed",
    "stop",
    "stream",
    "stream_options",
    "temperature",
    "top_p",
    "tools",
    "tool_choice",
    "web_search_options",
    "function_call",
    "functions",
];

const EMPTY_FIELDS: &[&str] = &[];
const EMPTY_DEFAULT_INJECTIONS: &[DefaultInjectionRule] = &[];

impl ChannelSchemaPolicy {
    fn standard_core() -> Self {
        Self {
            allowed_top_level_fields: EMPTY_FIELDS,
            forbidden_top_level_fields: EMPTY_FIELDS,
            default_injections: EMPTY_DEFAULT_INJECTIONS,
        }
    }

    fn gemini_compat() -> Self {
        Self {
            allowed_top_level_fields: GEMINI_COMPAT_ALLOWED_FIELDS,
            forbidden_top_level_fields: EMPTY_FIELDS,
            default_injections: EMPTY_DEFAULT_INJECTIONS,
        }
    }

    fn for_variant(variant: OpenAiVariant) -> Self {
        match variant {
            OpenAiVariant::Standard => Self::standard_core(),
            OpenAiVariant::GeminiCompat => Self::gemini_compat(),
        }
    }
}

impl OpenAiVariantPolicy {
    pub(crate) fn for_variant(variant: OpenAiVariant) -> Self {
        Self {
            #[cfg(test)]
            variant,
            schema: ChannelSchemaPolicy::for_variant(variant),
        }
    }

    #[cfg(test)]
    pub(crate) fn variant(&self) -> OpenAiVariant {
        self.variant
    }

    pub(crate) fn sanitize_request_payload(&self, payload: &mut Value) -> OpenAiSanitizeReport {
        let policy = self.schema;
        let mut report = OpenAiSanitizeReport::default();
        let Some(obj) = payload.as_object_mut() else {
            return report;
        };

        if !policy.forbidden_top_level_fields.is_empty() {
            for field in policy.forbidden_top_level_fields {
                if obj.remove(*field).is_some() {
                    report.removed_fields.push((*field).to_string());
                }
            }
        }

        if !policy.allowed_top_level_fields.is_empty() {
            let keys_to_remove: Vec<String> = obj
                .keys()
                .filter(|key| !policy.allowed_top_level_fields.contains(&key.as_str()))
                .cloned()
                .collect();
            for key in keys_to_remove {
                obj.remove(&key);
                report.removed_fields.push(key);
            }
        }

        for injection in policy.default_injections {
            if !obj.contains_key(injection.field) {
                obj.insert(injection.field.to_string(), (injection.value)());
                report.injected_defaults.push(injection.field.to_string());
            }
        }

        report.removed_fields.sort();
        report.removed_fields.dedup();
        report.injected_defaults.sort();
        report.injected_defaults.dedup();
        report
    }
}

pub(crate) fn determine_openai_variant(
    provider_type: &ProviderType,
    downstream_path: &str,
) -> OpenAiVariant {
    match provider_type {
        ProviderType::VertexOpenai | ProviderType::GeminiOpenai
            if downstream_path == "chat/completions" =>
        {
            OpenAiVariant::GeminiCompat
        }
        _ => OpenAiVariant::Standard,
    }
}

#[cfg(test)]
pub(crate) fn resolve_openai_variant_policy(
    provider_type: &ProviderType,
    downstream_path: &str,
) -> OpenAiVariantPolicy {
    OpenAiVariantPolicy::for_variant(determine_openai_variant(provider_type, downstream_path))
}

pub(crate) fn finalize_openai_compatible_request_payload(
    payload: &mut Value,
    provider_type: &ProviderType,
    downstream_path: &str,
) -> (OpenAiVariant, OpenAiSanitizeReport) {
    let variant = determine_openai_variant(provider_type, downstream_path);
    let report = sanitize_openai_request_payload(payload, variant);
    (variant, report)
}

pub(crate) fn sanitize_openai_request_payload(
    payload: &mut Value,
    variant: OpenAiVariant,
) -> OpenAiSanitizeReport {
    OpenAiVariantPolicy::for_variant(variant).sanitize_request_payload(payload)
}
