use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;

use crate::database::cost::{CostCatalogTemplateImportPayload, CostTemplateComponentImportPayload};

static BUILT_IN_TEMPLATES: OnceLock<Vec<CostTemplateDefinition>> = OnceLock::new();
const BUILT_IN_TEMPLATES_YAML: &str = include_str!("templates.yaml");

#[derive(Debug, Clone, Serialize)]
pub struct CostTemplateSummary {
    pub key: String,
    pub title: String,
    pub catalog_name: String,
    pub description: String,
    pub currency: String,
    pub version: String,
    pub source: String,
    pub effective_from: i64,
    pub effective_until: Option<i64>,
    pub tags: Vec<String>,
    pub supported_meters: Vec<String>,
    pub rounding_note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CostTemplateDefinition {
    pub key: String,
    pub title: String,
    pub catalog_name: String,
    pub description: String,
    pub currency: String,
    pub source: String,
    pub tags: Vec<String>,
    pub rounding_note: Option<String>,
    pub components: Vec<TemplateComponentDefinition>,
}

#[derive(Debug, Clone)]
pub struct TemplateComponentDefinition {
    pub meter_key: String,
    pub charge_kind: String,
    pub unit_price_nanos: Option<i64>,
    pub flat_fee_nanos: Option<i64>,
    pub tier_config_json: Option<String>,
    pub match_attributes_json: Option<String>,
    pub priority: i32,
    pub description: String,
}

#[derive(Debug, Deserialize)]
struct BuiltInTemplatesDocument {
    templates: Vec<YamlCostTemplateDefinition>,
}

#[derive(Debug, Deserialize)]
struct YamlCostTemplateDefinition {
    key: String,
    title: String,
    catalog_name: String,
    description: String,
    currency: String,
    source: String,
    #[serde(default)]
    tags: Vec<String>,
    rounding_note: Option<String>,
    components: Vec<YamlTemplateComponentEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum YamlTemplateComponentEntry {
    Full(YamlTemplateComponentDefinition),
    Short(ShortYamlTemplateComponentDefinition),
}

#[derive(Debug, Deserialize)]
struct YamlTemplateComponentDefinition {
    meter_key: String,
    charge_kind: Option<String>,
    unit_price_nanos: Option<YamlValue>,
    flat_fee_nanos: Option<YamlValue>,
    #[serde(default, alias = "tier_config_json")]
    tier_config: Option<YamlValue>,
    #[serde(default, alias = "match_attributes_json")]
    match_attributes: Option<YamlValue>,
    priority: Option<i32>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ShortYamlTemplateComponentDefinition(
    std::collections::BTreeMap<String, YamlTemplateComponentShortValue>,
);

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum YamlTemplateComponentShortValue {
    Detailed {
        charge_kind: Option<String>,
        unit_price_nanos: Option<YamlValue>,
        flat_fee_nanos: Option<YamlValue>,
        #[serde(default, alias = "tier_config_json")]
        tier_config: Option<YamlValue>,
        #[serde(default, alias = "match_attributes_json")]
        match_attributes: Option<YamlValue>,
        priority: Option<i32>,
        description: Option<String>,
    },
    UnitPrice(YamlValue),
}

#[derive(Debug, Clone)]
struct ResolvedTemplateMetadata {
    version: String,
    effective_from: i64,
}

impl CostTemplateDefinition {
    pub fn summary(&self) -> CostTemplateSummary {
        self.summary_at(Utc::now())
    }

    pub fn summary_at(&self, now: DateTime<Utc>) -> CostTemplateSummary {
        let metadata = resolve_template_metadata(now);
        CostTemplateSummary {
            key: self.key.clone(),
            title: self.title.clone(),
            catalog_name: self.catalog_name.clone(),
            description: self.description.clone(),
            currency: self.currency.clone(),
            version: metadata.version,
            source: self.source.clone(),
            effective_from: metadata.effective_from,
            effective_until: None,
            tags: self.tags.clone(),
            supported_meters: self
                .components
                .iter()
                .map(|component| component.meter_key.clone())
                .collect(),
            rounding_note: self.rounding_note.clone(),
        }
    }

    pub fn import_payload(
        &self,
        catalog_name_override: Option<&str>,
    ) -> CostCatalogTemplateImportPayload {
        self.import_payload_at(Utc::now(), catalog_name_override)
    }

    pub fn import_payload_at(
        &self,
        now: DateTime<Utc>,
        catalog_name_override: Option<&str>,
    ) -> CostCatalogTemplateImportPayload {
        let metadata = resolve_template_metadata(now);
        CostCatalogTemplateImportPayload {
            catalog_name: catalog_name_override
                .unwrap_or(&self.catalog_name)
                .trim()
                .to_string(),
            catalog_description: Some(self.description.clone()),
            version: metadata.version,
            currency: self.currency.clone(),
            source: Some(self.source.clone()),
            effective_from: metadata.effective_from,
            effective_until: None,
            is_enabled: true,
            components: self
                .components
                .iter()
                .map(|component| CostTemplateComponentImportPayload {
                    meter_key: component.meter_key.clone(),
                    charge_kind: component.charge_kind.clone(),
                    unit_price_nanos: component.unit_price_nanos,
                    flat_fee_nanos: component.flat_fee_nanos,
                    tier_config_json: component.tier_config_json.clone(),
                    match_attributes_json: component.match_attributes_json.clone(),
                    priority: component.priority,
                    description: Some(component.description.clone()),
                })
                .collect(),
        }
    }
}

pub fn list_templates() -> Vec<CostTemplateSummary> {
    let now = Utc::now();
    built_in_templates()
        .iter()
        .map(|template| template.summary_at(now.clone()))
        .collect()
}

pub fn find_template(key: &str) -> Option<CostTemplateDefinition> {
    built_in_templates()
        .iter()
        .find(|template| template.key == key)
        .cloned()
}

fn built_in_templates() -> &'static [CostTemplateDefinition] {
    BUILT_IN_TEMPLATES
        .get_or_init(|| parse_templates_document(BUILT_IN_TEMPLATES_YAML))
        .as_slice()
}

fn parse_templates_document(source: &str) -> Vec<CostTemplateDefinition> {
    let document: BuiltInTemplatesDocument =
        serde_yaml::from_str(source).expect("invalid built-in cost template yaml");

    document
        .templates
        .into_iter()
        .map(CostTemplateDefinition::try_from)
        .collect::<Result<Vec<_>, _>>()
        .expect("invalid built-in cost template definition")
}

fn resolve_template_metadata(now: DateTime<Utc>) -> ResolvedTemplateMetadata {
    ResolvedTemplateMetadata {
        version: now.format("%Y-%m-%d").to_string(),
        effective_from: now.timestamp_millis(),
    }
}

fn infer_charge_kind(component: &YamlTemplateComponentDefinition) -> Result<String, String> {
    match (
        component.charge_kind.as_deref(),
        component.unit_price_nanos.as_ref(),
        component.flat_fee_nanos.as_ref(),
    ) {
        (Some(charge_kind), _, _) => Ok(charge_kind.to_string()),
        (None, Some(_), None) => Ok("per_unit".to_string()),
        (None, None, Some(_)) => Ok("flat".to_string()),
        (None, Some(_), Some(_)) => Err(format!(
            "component '{}' must declare charge_kind when both unit_price_nanos and flat_fee_nanos are set",
            component.meter_key
        )),
        (None, None, None) => Err(format!(
            "component '{}' must provide unit_price_nanos or flat_fee_nanos",
            component.meter_key
        )),
    }
}

fn default_priority(component: &YamlTemplateComponentDefinition, index: usize) -> i32 {
    if component.meter_key == "invoke.request_calls" && component.flat_fee_nanos.is_some() {
        900
    } else {
        100 + (index as i32 * 10)
    }
}

fn default_component_description(component: &YamlTemplateComponentDefinition) -> String {
    match component.meter_key.as_str() {
        "llm.input_text_tokens" => "Input text tokens".to_string(),
        "llm.output_text_tokens" => "Output text tokens".to_string(),
        "llm.cache_read_tokens" => "Cache read tokens".to_string(),
        "llm.cache_write_tokens" => "Cache write tokens".to_string(),
        "llm.input_image_tokens" => "Input image tokens".to_string(),
        "llm.output_image_tokens" => "Output image tokens".to_string(),
        "llm.reasoning_tokens" => "Reasoning tokens".to_string(),
        "invoke.request_calls" if component.flat_fee_nanos.is_some() => {
            "Per-request invocation baseline".to_string()
        }
        _ => component.meter_key.replace('.', " "),
    }
}

fn yaml_value_to_json_string(
    field_name: &str,
    value: Option<YamlValue>,
) -> Result<Option<String>, String> {
    value
        .map(|value| match value {
            YamlValue::Null => Ok(None),
            YamlValue::String(raw) => Ok(Some(raw)),
            other => serde_json::to_string(&other)
                .map(Some)
                .map_err(|err| format!("failed to serialize {} to json: {}", field_name, err)),
        })
        .transpose()
        .map(|value| value.flatten())
}

fn currency_minor_unit_digits(currency: &str) -> u32 {
    match currency.to_uppercase().as_str() {
        "BHD" | "JOD" | "KWD" | "OMR" | "TND" => 3,
        "CLP" | "DJF" | "GNF" | "ISK" | "JPY" | "KMF" | "KRW" | "PYG" | "RWF" | "UGX" | "VND"
        | "VUV" | "XAF" | "XOF" | "XPF" => 0,
        _ => 2,
    }
}

fn is_per_million_meter(meter_key: &str) -> bool {
    meter_key.starts_with("llm.")
}

fn price_scale_digits(currency: &str) -> u32 {
    currency_minor_unit_digits(currency) + 9
}

fn rate_scale_digits(currency: &str) -> u32 {
    currency_minor_unit_digits(currency) + 3
}

fn yaml_scalar_to_string(field_name: &str, value: &YamlValue) -> Result<String, String> {
    match value {
        YamlValue::Number(number) => Ok(number.to_string()),
        YamlValue::String(raw) => Ok(raw.trim().to_string()),
        YamlValue::Null => Err(format!("{} cannot be null", field_name)),
        _ => Err(format!(
            "{} must be a numeric scalar or stringified decimal",
            field_name
        )),
    }
}

fn parse_decimal_to_scaled_i64(
    field_name: &str,
    raw: &str,
    scale_digits: u32,
) -> Result<i64, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!("{} cannot be empty", field_name));
    }

    if trimmed.starts_with('-') {
        return Err(format!("{} cannot be negative", field_name));
    }

    let (integer_part, fraction_part) = trimmed
        .split_once('.')
        .map_or((trimmed, ""), |(left, right)| (left, right));

    if integer_part.is_empty() || !integer_part.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(format!("{} must be a decimal number", field_name));
    }

    if !fraction_part.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(format!("{} must be a decimal number", field_name));
    }

    if fraction_part.len() > scale_digits as usize {
        return Err(format!(
            "{} supports at most {} decimal places",
            field_name, scale_digits
        ));
    }

    let scale = 10_i64.pow(scale_digits);
    let integer = integer_part
        .parse::<i64>()
        .map_err(|_| format!("{} is too large", field_name))?;
    let fraction = if fraction_part.is_empty() {
        0
    } else {
        fraction_part
            .parse::<i64>()
            .map_err(|_| format!("{} is too large", field_name))?
            * 10_i64.pow(scale_digits - fraction_part.len() as u32)
    };

    integer
        .checked_mul(scale)
        .and_then(|value| value.checked_add(fraction))
        .ok_or_else(|| format!("{} is too large", field_name))
}

fn convert_amount_to_nanos(
    field_name: &str,
    value: &YamlValue,
    scale_digits: u32,
) -> Result<i64, String> {
    let raw = yaml_scalar_to_string(field_name, value)?;
    parse_decimal_to_scaled_i64(field_name, &raw, scale_digits)
}

fn convert_unit_price_to_nanos(
    field_name: &str,
    value: &YamlValue,
    meter_key: &str,
    currency: &str,
) -> Result<i64, String> {
    let scale_digits = if is_per_million_meter(meter_key) {
        rate_scale_digits(currency)
    } else {
        price_scale_digits(currency)
    };
    convert_amount_to_nanos(field_name, value, scale_digits)
}

fn convert_flat_fee_to_nanos(
    field_name: &str,
    value: &YamlValue,
    currency: &str,
) -> Result<i64, String> {
    convert_amount_to_nanos(field_name, value, price_scale_digits(currency))
}

fn normalize_tier_config_prices(
    value: YamlValue,
    meter_key: &str,
    currency: &str,
) -> Result<YamlValue, String> {
    match value {
        YamlValue::Mapping(mapping) => {
            let normalized = mapping
                .into_iter()
                .map(|(key, nested_value)| {
                    let normalized_value = if matches!(&key, YamlValue::String(name) if name == "unit_price_nanos")
                    {
                        let nanos = convert_unit_price_to_nanos(
                            "tier_config.unit_price_nanos",
                            &nested_value,
                            meter_key,
                            currency,
                        )?;
                        Ok(YamlValue::Number(serde_yaml::Number::from(nanos)))
                    } else {
                        normalize_tier_config_prices(nested_value, meter_key, currency)
                    }?;

                    Ok((key, normalized_value))
                })
                .collect::<Result<serde_yaml::Mapping, String>>()?;
            Ok(YamlValue::Mapping(normalized))
        }
        YamlValue::Sequence(sequence) => Ok(YamlValue::Sequence(
            sequence
                .into_iter()
                .map(|item| normalize_tier_config_prices(item, meter_key, currency))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        other => Ok(other),
    }
}

impl TryFrom<YamlCostTemplateDefinition> for CostTemplateDefinition {
    type Error = String;

    fn try_from(value: YamlCostTemplateDefinition) -> Result<Self, Self::Error> {
        let currency = value.currency.clone();
        let components = value
            .components
            .into_iter()
            .enumerate()
            .map(|(index, component)| {
                let component = normalize_component_entry(component)?;
                TemplateComponentDefinition::try_from((index, component, currency.as_str()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            key: value.key,
            title: value.title,
            catalog_name: value.catalog_name,
            description: value.description,
            currency: value.currency,
            source: value.source,
            tags: value.tags,
            rounding_note: value.rounding_note,
            components,
        })
    }
}

fn normalize_component_entry(
    entry: YamlTemplateComponentEntry,
) -> Result<YamlTemplateComponentDefinition, String> {
    match entry {
        YamlTemplateComponentEntry::Full(mut component) => {
            component.meter_key = resolve_meter_key_alias(&component.meter_key)?;
            Ok(component)
        }
        YamlTemplateComponentEntry::Short(component) => {
            let ShortYamlTemplateComponentDefinition(entries) = component;
            if entries.len() != 1 {
                return Err(
                    "component shorthand must contain exactly one meter key per list item"
                        .to_string(),
                );
            }

            let (meter_key, value) = entries
                .into_iter()
                .next()
                .expect("short component should contain one entry");
            let meter_key = resolve_meter_key_alias(&meter_key)?;

            let component = match value {
                YamlTemplateComponentShortValue::UnitPrice(unit_price_nanos) => {
                    YamlTemplateComponentDefinition {
                        meter_key,
                        charge_kind: None,
                        unit_price_nanos: Some(unit_price_nanos),
                        flat_fee_nanos: None,
                        tier_config: None,
                        match_attributes: None,
                        priority: None,
                        description: None,
                    }
                }
                YamlTemplateComponentShortValue::Detailed {
                    charge_kind,
                    unit_price_nanos,
                    flat_fee_nanos,
                    tier_config,
                    match_attributes,
                    priority,
                    description,
                } => YamlTemplateComponentDefinition {
                    meter_key,
                    charge_kind,
                    unit_price_nanos,
                    flat_fee_nanos,
                    tier_config,
                    match_attributes,
                    priority,
                    description,
                },
            };

            Ok(component)
        }
    }
}

fn resolve_meter_key_alias(value: &str) -> Result<String, String> {
    let normalized = value.trim();
    let resolved = match normalized {
        "llm.input_text_tokens" | "input_text" | "text_in" => "llm.input_text_tokens",
        "llm.output_text_tokens" | "output_text" | "text_out" => "llm.output_text_tokens",
        "llm.cache_read_tokens" | "cache_read" | "cached_input" => "llm.cache_read_tokens",
        "llm.cache_write_tokens" | "cache_write" | "cache_create" => "llm.cache_write_tokens",
        "llm.input_image_tokens" | "input_image" | "image_in" => "llm.input_image_tokens",
        "llm.output_image_tokens" | "output_image" | "image_out" => "llm.output_image_tokens",
        "llm.reasoning_tokens" | "reasoning" | "thinking" => "llm.reasoning_tokens",
        "invoke.request_calls" | "request" | "request_call" | "request_calls" => {
            "invoke.request_calls"
        }
        _ if normalized.is_empty() => {
            return Err("component meter_key cannot be empty".to_string());
        }
        _ => normalized,
    };

    Ok(resolved.to_string())
}

impl TryFrom<(usize, YamlTemplateComponentDefinition, &str)> for TemplateComponentDefinition {
    type Error = String;

    fn try_from(
        value: (usize, YamlTemplateComponentDefinition, &str),
    ) -> Result<Self, Self::Error> {
        let (index, component, currency) = value;
        let charge_kind = infer_charge_kind(&component)?;
        let priority = component
            .priority
            .unwrap_or_else(|| default_priority(&component, index));
        let description = component
            .description
            .clone()
            .unwrap_or_else(|| default_component_description(&component));
        let meter_key = component.meter_key.clone();
        let unit_price_nanos = component
            .unit_price_nanos
            .as_ref()
            .map(|value| {
                convert_unit_price_to_nanos("unit_price_nanos", value, meter_key.as_str(), currency)
            })
            .transpose()?;
        let flat_fee_nanos = component
            .flat_fee_nanos
            .as_ref()
            .map(|value| convert_flat_fee_to_nanos("flat_fee_nanos", value, currency))
            .transpose()?;
        let tier_config = component
            .tier_config
            .map(|value| normalize_tier_config_prices(value, meter_key.as_str(), currency))
            .transpose()?;
        Ok(Self {
            charge_kind,
            unit_price_nanos,
            flat_fee_nanos,
            tier_config_json: yaml_value_to_json_string("tier_config", tier_config)?,
            match_attributes_json: yaml_value_to_json_string(
                "match_attributes",
                component.match_attributes,
            )?,
            priority,
            description,
            meter_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{find_template, list_templates, parse_templates_document};

    #[test]
    fn built_in_templates_expose_required_metadata() {
        let templates = list_templates();

        assert!(
            templates
                .iter()
                .any(|template| template.tags.iter().any(|tag| tag == "text"))
        );
        assert!(templates.iter().all(|template| !template.tags.is_empty()));
        assert!(
            templates
                .iter()
                .all(|template| !template.version.trim().is_empty())
        );
        assert!(
            templates
                .iter()
                .all(|template| !template.source.trim().is_empty())
        );
        assert!(
            templates
                .iter()
                .all(|template| !template.supported_meters.is_empty())
        );
        assert!(
            templates
                .iter()
                .all(|template| template.effective_until.is_none())
        );
    }

    #[test]
    fn template_lookup_returns_payload_ready_definition() {
        let template = find_template("google.gemini-2.5-pro.text").expect("template should exist");
        let now = Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap();
        let payload = template.import_payload_at(now, None);

        assert_eq!(payload.catalog_name, "Google / Gemini 2.5 Pro");
        assert_eq!(payload.version, "2026-04-13");
        assert_eq!(payload.currency, "USD");
        assert_eq!(payload.effective_from, now.timestamp_millis());
        assert_eq!(payload.effective_until, None);
        assert!(!payload.components.is_empty());
    }

    #[test]
    fn yaml_templates_support_simplified_component_defaults() {
        let templates = parse_templates_document(
            r#"
templates:
  - key: demo.template
    title: Demo Template
    catalog_name: Demo / Template
    description: Demo template.
    currency: USD
    source: https://example.com/pricing
    components:
      - input_text: 2.5
      - request:
          flat_fee_nanos: 1.25
      - output_text:
          charge_kind: tiered_per_unit
          tier_config:
            tiers:
              - up_to: 1000
                unit_price_nanos: 0.75
          match_attributes:
            model_family: demo
"#,
        );

        assert_eq!(templates.len(), 1);
        let components = &templates[0].components;
        assert_eq!(components[0].charge_kind, "per_unit");
        assert_eq!(components[0].priority, 100);
        assert_eq!(components[0].meter_key, "llm.input_text_tokens");
        assert_eq!(components[0].unit_price_nanos, Some(250000));
        assert_eq!(components[0].description, "Input text tokens");
        assert_eq!(components[1].charge_kind, "flat");
        assert_eq!(components[1].priority, 900);
        assert_eq!(components[1].meter_key, "invoke.request_calls");
        assert_eq!(components[1].flat_fee_nanos, Some(125000000000));
        assert_eq!(components[1].description, "Per-request invocation baseline");
        assert_eq!(components[2].meter_key, "llm.output_text_tokens");
        assert_eq!(
            components[2].tier_config_json.as_deref(),
            Some(r#"{"tiers":[{"up_to":1000,"unit_price_nanos":75000}]}"#)
        );
        assert_eq!(
            components[2].match_attributes_json.as_deref(),
            Some(r#"{"model_family":"demo"}"#)
        );
    }

    #[test]
    fn full_component_definition_accepts_meter_aliases() {
        let templates = parse_templates_document(
            r#"
templates:
  - key: demo.template
    title: Demo Template
    catalog_name: Demo / Template
    description: Demo template.
    currency: USD
    source: https://example.com/pricing
    components:
      - meter_key: reasoning
        unit_price_nanos: 7
"#,
        );

        assert_eq!(templates[0].components[0].meter_key, "llm.reasoning_tokens");
        assert_eq!(templates[0].components[0].unit_price_nanos, Some(700000));
    }

    #[test]
    fn tier_config_prices_are_converted_from_major_unit_decimals() {
        let templates = parse_templates_document(
            r#"
templates:
  - key: demo.template
    title: Demo Template
    catalog_name: Demo / Template
    description: Demo template.
    currency: CNY
    source: https://example.com/pricing
    components:
      - meter_key: llm.cache_read_tokens
        charge_kind: tiered_per_unit
        tier_config:
          basis: total_input_tokens
          tiers:
            - up_to: 32000
              unit_price_nanos: 1.3
            - unit_price_nanos: "2"
"#,
        );

        assert_eq!(
            templates[0].components[0].tier_config_json.as_deref(),
            Some(
                r#"{"basis":"total_input_tokens","tiers":[{"up_to":32000,"unit_price_nanos":130000},{"unit_price_nanos":200000}]}"#
            )
        );
    }
}
