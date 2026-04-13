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
    unit_price_nanos: Option<i64>,
    flat_fee_nanos: Option<i64>,
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
    UnitPrice(i64),
    Detailed {
        charge_kind: Option<String>,
        unit_price_nanos: Option<i64>,
        flat_fee_nanos: Option<i64>,
        #[serde(default, alias = "tier_config_json")]
        tier_config: Option<YamlValue>,
        #[serde(default, alias = "match_attributes_json")]
        match_attributes: Option<YamlValue>,
        priority: Option<i32>,
        description: Option<String>,
    },
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
        component.unit_price_nanos,
        component.flat_fee_nanos,
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

impl TryFrom<YamlCostTemplateDefinition> for CostTemplateDefinition {
    type Error = String;

    fn try_from(value: YamlCostTemplateDefinition) -> Result<Self, Self::Error> {
        let components = value
            .components
            .into_iter()
            .enumerate()
            .map(|(index, component)| {
                let component = normalize_component_entry(component)?;
                TemplateComponentDefinition::try_from((index, component))
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

impl TryFrom<(usize, YamlTemplateComponentDefinition)> for TemplateComponentDefinition {
    type Error = String;

    fn try_from(value: (usize, YamlTemplateComponentDefinition)) -> Result<Self, Self::Error> {
        let (index, component) = value;
        let charge_kind = infer_charge_kind(&component)?;
        let priority = component
            .priority
            .unwrap_or_else(|| default_priority(&component, index));
        let description = component
            .description
            .clone()
            .unwrap_or_else(|| default_component_description(&component));
        let meter_key = component.meter_key.clone();
        Ok(Self {
            charge_kind,
            unit_price_nanos: component.unit_price_nanos,
            flat_fee_nanos: component.flat_fee_nanos,
            tier_config_json: yaml_value_to_json_string("tier_config", component.tier_config)?,
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
        assert!(
            templates
                .iter()
                .all(|template| !template.tags.is_empty())
        );
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
      - input_text: 42
      - request:
          flat_fee_nanos: 0
      - output_text:
          charge_kind: tiered_per_unit
          tier_config:
            tiers:
              - up_to: 1000
                unit_price_nanos: 100
          match_attributes:
            model_family: demo
"#,
        );

        assert_eq!(templates.len(), 1);
        let components = &templates[0].components;
        assert_eq!(components[0].charge_kind, "per_unit");
        assert_eq!(components[0].priority, 100);
        assert_eq!(components[0].meter_key, "llm.input_text_tokens");
        assert_eq!(components[0].description, "Input text tokens");
        assert_eq!(components[1].charge_kind, "flat");
        assert_eq!(components[1].priority, 900);
        assert_eq!(components[1].meter_key, "invoke.request_calls");
        assert_eq!(components[1].description, "Per-request invocation baseline");
        assert_eq!(components[2].meter_key, "llm.output_text_tokens");
        assert_eq!(
            components[2].tier_config_json.as_deref(),
            Some(r#"{"tiers":[{"up_to":1000,"unit_price_nanos":100}]}"#)
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
    }
}
