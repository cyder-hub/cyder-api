use std::{collections::BTreeMap, str::FromStr};

use serde::Deserialize;

use crate::{
    controller::BaseError,
    cost::{ChargeKind, CostDetailLine, CostLedger, CostLedgerItem, CostRatingResult, MeterKey},
    service::cache::types::{CacheCostCatalogVersion, CacheCostComponent},
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CostRatingContext {
    pub total_input_tokens: i64,
}

pub fn rate_cost(
    ledger: &CostLedger,
    context: &CostRatingContext,
    version: &CacheCostCatalogVersion,
) -> Result<CostRatingResult, BaseError> {
    let mut result = CostRatingResult {
        currency: version.currency.clone(),
        ..Default::default()
    };

    for item in &ledger.items {
        let Some(component) = find_matching_component(item, &version.components)? else {
            result.unmatched_items.push(item.meter_key.to_string());
            if should_emit_unmatched_warning(item.meter_key) {
                result.warnings.push(format!(
                    "no matching cost component for meter {}",
                    item.meter_key
                ));
            }
            continue;
        };

        let rated = rate_item(item, context, version.id, component)?;
        result.total_cost_nanos += rated.amount_nanos;
        result.detail_lines.push(rated);
    }

    Ok(result)
}

fn find_matching_component<'a>(
    item: &CostLedgerItem,
    components: &'a [CacheCostComponent],
) -> Result<Option<&'a CacheCostComponent>, BaseError> {
    let mut ordered = components.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|component| (component.priority, component.id));

    for meter_key in fallback_meter_keys(item.meter_key) {
        for component in &ordered {
            if component.meter_key != meter_key.to_string() {
                continue;
            }

            let Some(match_attributes) = parse_match_attributes(component)? else {
                return Ok(Some(component));
            };

            if match_attributes
                .iter()
                .all(|(key, value)| item.attributes.get(key) == Some(value))
            {
                return Ok(Some(component));
            }
        }
    }

    Ok(None)
}

fn should_emit_unmatched_warning(meter_key: MeterKey) -> bool {
    !matches!(meter_key, MeterKey::InvokeRequestCalls)
}

fn rate_item(
    item: &CostLedgerItem,
    context: &CostRatingContext,
    catalog_version_id: i64,
    component: &CacheCostComponent,
) -> Result<CostDetailLine, BaseError> {
    let charge_kind = ChargeKind::from_str(&component.charge_kind).map_err(|_| {
        BaseError::ParamInvalid(Some(format!(
            "Unsupported charge_kind '{}' on cost component {}",
            component.charge_kind, component.id
        )))
    })?;

    let amount_nanos;
    let mut unit_price_nanos = None;
    let mut attributes = item.attributes.clone();

    if component.meter_key != item.meter_key.to_string() {
        attributes.insert(
            "fallback_meter_key".to_string(),
            component.meter_key.clone(),
        );
    }

    match charge_kind {
        ChargeKind::PerUnit => {
            let price = component.unit_price_nanos.ok_or_else(|| {
                BaseError::ParamInvalid(Some(format!(
                    "Missing unit_price_nanos for per_unit cost component {}",
                    component.id
                )))
            })?;
            amount_nanos = item.quantity.checked_mul(price).ok_or_else(|| {
                BaseError::ParamInvalid(Some(format!(
                    "Cost overflow for per_unit component {}",
                    component.id
                )))
            })?;
            unit_price_nanos = Some(price);
        }
        ChargeKind::Flat => {
            amount_nanos = component.flat_fee_nanos.ok_or_else(|| {
                BaseError::ParamInvalid(Some(format!(
                    "Missing flat_fee_nanos for flat cost component {}",
                    component.id
                )))
            })?;
        }
        ChargeKind::TieredPerUnit => {
            let tier_config = parse_tier_config(component)?;
            let basis_value = match tier_config.basis {
                TierBasis::MeterQuantity => item.quantity,
                TierBasis::TotalInputTokens => context.total_input_tokens,
            };

            let tier = tier_config
                .tiers
                .iter()
                .find(|tier| tier.up_to.is_none_or(|up_to| basis_value <= up_to))
                .ok_or_else(|| {
                    BaseError::ParamInvalid(Some(format!(
                        "No tier matched basis value {} for cost component {}",
                        basis_value, component.id
                    )))
                })?;

            let price = tier.unit_price_nanos;
            amount_nanos = item.quantity.checked_mul(price).ok_or_else(|| {
                BaseError::ParamInvalid(Some(format!(
                    "Cost overflow for tiered_per_unit component {}",
                    component.id
                )))
            })?;
            unit_price_nanos = Some(price);
            attributes.insert("tier_basis".to_string(), tier_config.basis.to_string());
            if let Some(up_to) = tier.up_to {
                attributes.insert("tier_up_to".to_string(), up_to.to_string());
            } else {
                attributes.insert("tier_up_to".to_string(), "unbounded".to_string());
            }
        }
    }

    Ok(CostDetailLine {
        meter_key: item.meter_key,
        quantity: item.quantity,
        unit: item.unit,
        charge_kind,
        amount_nanos,
        unit_price_nanos,
        component_id: Some(component.id),
        catalog_version_id: Some(catalog_version_id),
        description: component.description.clone(),
        attributes,
    })
}

fn fallback_meter_keys(meter_key: MeterKey) -> &'static [MeterKey] {
    match meter_key {
        MeterKey::LlmInputTextTokens => &[MeterKey::LlmInputTextTokens],
        MeterKey::LlmOutputTextTokens => &[MeterKey::LlmOutputTextTokens],
        MeterKey::LlmInputImageTokens => {
            &[MeterKey::LlmInputImageTokens, MeterKey::LlmInputTextTokens]
        }
        MeterKey::LlmOutputImageTokens => &[
            MeterKey::LlmOutputImageTokens,
            MeterKey::LlmOutputTextTokens,
        ],
        MeterKey::LlmCacheReadTokens => {
            &[MeterKey::LlmCacheReadTokens, MeterKey::LlmInputTextTokens]
        }
        MeterKey::LlmCacheWriteTokens => {
            &[MeterKey::LlmCacheWriteTokens, MeterKey::LlmInputTextTokens]
        }
        MeterKey::LlmReasoningTokens => {
            &[MeterKey::LlmReasoningTokens, MeterKey::LlmOutputTextTokens]
        }
        MeterKey::InvokeRequestCalls => &[MeterKey::InvokeRequestCalls],
    }
}

fn parse_match_attributes(
    component: &CacheCostComponent,
) -> Result<Option<BTreeMap<String, String>>, BaseError> {
    let Some(raw) = component.match_attributes_json.as_deref() else {
        return Ok(None);
    };

    serde_json::from_str::<BTreeMap<String, String>>(raw)
        .map(Some)
        .map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "Invalid match_attributes_json for cost component {}: {}",
                component.id, err
            )))
        })
}

fn parse_tier_config(component: &CacheCostComponent) -> Result<TierConfig, BaseError> {
    let raw = component.tier_config_json.as_deref().ok_or_else(|| {
        BaseError::ParamInvalid(Some(format!(
            "Missing tier_config_json for tiered_per_unit cost component {}",
            component.id
        )))
    })?;

    let config = serde_json::from_str::<TierConfig>(raw).map_err(|err| {
        BaseError::ParamInvalid(Some(format!(
            "Invalid tier_config_json for cost component {}: {}",
            component.id, err
        )))
    })?;

    validate_tier_config(component.id, &config)?;

    Ok(config)
}

pub fn validate_component_config(
    meter_key: &str,
    charge_kind: &str,
    unit_price_nanos: Option<i64>,
    flat_fee_nanos: Option<i64>,
    tier_config_json: Option<&str>,
    match_attributes_json: Option<&str>,
) -> Result<(), BaseError> {
    MeterKey::from_str(meter_key).map_err(|_| {
        BaseError::ParamInvalid(Some(format!("Unsupported meter_key '{}'", meter_key)))
    })?;

    let charge_kind = ChargeKind::from_str(charge_kind).map_err(|_| {
        BaseError::ParamInvalid(Some(format!("Unsupported charge_kind '{}'", charge_kind)))
    })?;

    if let Some(raw) = match_attributes_json {
        serde_json::from_str::<BTreeMap<String, String>>(raw).map_err(|err| {
            BaseError::ParamInvalid(Some(format!("Invalid match_attributes_json: {}", err)))
        })?;
    }

    match charge_kind {
        ChargeKind::PerUnit => {
            let Some(unit_price_nanos) = unit_price_nanos else {
                return Err(BaseError::ParamInvalid(Some(
                    "per_unit requires unit_price_nanos".to_string(),
                )));
            };
            validate_non_negative("unit_price_nanos", unit_price_nanos)?;
            reject_present("flat_fee_nanos", flat_fee_nanos)?;
            reject_present("tier_config_json", tier_config_json)?;
        }
        ChargeKind::Flat => {
            let Some(flat_fee_nanos) = flat_fee_nanos else {
                return Err(BaseError::ParamInvalid(Some(
                    "flat requires flat_fee_nanos".to_string(),
                )));
            };
            validate_non_negative("flat_fee_nanos", flat_fee_nanos)?;
            reject_present("unit_price_nanos", unit_price_nanos)?;
            reject_present("tier_config_json", tier_config_json)?;
        }
        ChargeKind::TieredPerUnit => {
            let Some(tier_config_json) = tier_config_json else {
                return Err(BaseError::ParamInvalid(Some(
                    "tiered_per_unit requires tier_config_json".to_string(),
                )));
            };
            reject_present("unit_price_nanos", unit_price_nanos)?;
            reject_present("flat_fee_nanos", flat_fee_nanos)?;
            let config = serde_json::from_str::<TierConfig>(tier_config_json).map_err(|err| {
                BaseError::ParamInvalid(Some(format!("Invalid tier_config_json: {}", err)))
            })?;
            validate_tier_config(0, &config)?;
        }
    }

    Ok(())
}

fn validate_non_negative(field_name: &str, value: i64) -> Result<(), BaseError> {
    if value < 0 {
        return Err(BaseError::ParamInvalid(Some(format!(
            "{} cannot be negative",
            field_name
        ))));
    }
    Ok(())
}

fn reject_present<T>(field_name: &str, value: Option<T>) -> Result<(), BaseError> {
    if value.is_some() {
        return Err(BaseError::ParamInvalid(Some(format!(
            "{} is not allowed for this charge_kind",
            field_name
        ))));
    }
    Ok(())
}

fn validate_tier_config(component_id: i64, config: &TierConfig) -> Result<(), BaseError> {
    if config.tiers.is_empty() {
        return Err(BaseError::ParamInvalid(Some(format!(
            "tier_config_json must define at least one tier for cost component {}",
            component_id
        ))));
    }

    let mut previous_up_to = None;
    let mut saw_unbounded_tier = false;

    for tier in &config.tiers {
        validate_non_negative("tier.unit_price_nanos", tier.unit_price_nanos)?;

        if saw_unbounded_tier {
            return Err(BaseError::ParamInvalid(Some(format!(
                "tier_config_json cannot define tiers after an unbounded tier for cost component {}",
                component_id
            ))));
        }

        match tier.up_to {
            Some(up_to) => {
                validate_non_negative("tier.up_to", up_to)?;
                if let Some(previous_up_to) = previous_up_to
                    && up_to <= previous_up_to
                {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "tier_config_json tiers must be strictly increasing for cost component {}",
                        component_id
                    ))));
                }
                previous_up_to = Some(up_to);
            }
            None => {
                saw_unbounded_tier = true;
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct TierConfig {
    basis: TierBasis,
    tiers: Vec<TierDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TierBasis {
    MeterQuantity,
    TotalInputTokens,
}

impl std::fmt::Display for TierBasis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MeterQuantity => f.write_str("meter_quantity"),
            Self::TotalInputTokens => f.write_str("total_input_tokens"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct TierDefinition {
    up_to: Option<i64>,
    unit_price_nanos: i64,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{CostRatingContext, rate_cost, validate_component_config};
    use crate::{
        cost::{CostLedger, CostLedgerItem, CostUnit, MeterKey},
        service::cache::types::{CacheCostCatalogVersion, CacheCostComponent},
    };

    #[test]
    fn rates_per_unit_component() {
        let ledger = CostLedger {
            items: vec![ledger_item(MeterKey::LlmInputTextTokens, 1200)],
        };
        let version = version_with_components(vec![component(
            11,
            "llm.input_text_tokens",
            "per_unit",
            Some(3),
            None,
            None,
            None,
            1,
        )]);

        let result = rate_cost(&ledger, &CostRatingContext::default(), &version).unwrap();

        assert_eq!(result.total_cost_nanos, 3600);
        assert_eq!(result.detail_lines.len(), 1);
        assert_eq!(result.detail_lines[0].unit_price_nanos, Some(3));
        assert_eq!(result.detail_lines[0].component_id, Some(11));
    }

    #[test]
    fn rates_flat_component() {
        let ledger = CostLedger {
            items: vec![ledger_item(MeterKey::InvokeRequestCalls, 1)],
        };
        let version = version_with_components(vec![component(
            22,
            "invoke.request_calls",
            "flat",
            None,
            Some(2500),
            None,
            None,
            1,
        )]);

        let result = rate_cost(&ledger, &CostRatingContext::default(), &version).unwrap();

        assert_eq!(result.total_cost_nanos, 2500);
        assert_eq!(result.detail_lines[0].amount_nanos, 2500);
        assert_eq!(result.detail_lines[0].unit_price_nanos, None);
    }

    #[test]
    fn rates_tiered_per_unit_by_meter_quantity() {
        let ledger = CostLedger {
            items: vec![ledger_item(MeterKey::LlmOutputTextTokens, 1500)],
        };
        let version = version_with_components(vec![component(
            33,
            "llm.output_text_tokens",
            "tiered_per_unit",
            None,
            None,
            Some(r#"{"basis":"meter_quantity","tiers":[{"up_to":1000,"unit_price_nanos":4},{"up_to":null,"unit_price_nanos":2}]}"#.to_string()),
            None,
            1,
        )]);

        let result = rate_cost(&ledger, &CostRatingContext::default(), &version).unwrap();

        assert_eq!(result.total_cost_nanos, 3000);
        assert_eq!(result.detail_lines[0].unit_price_nanos, Some(2));
        assert_eq!(
            result.detail_lines[0].attributes.get("tier_basis"),
            Some(&"meter_quantity".to_string())
        );
    }

    #[test]
    fn rates_tiered_per_unit_by_total_input_tokens() {
        let ledger = CostLedger {
            items: vec![ledger_item(MeterKey::LlmOutputTextTokens, 200)],
        };
        let version = version_with_components(vec![component(
            44,
            "llm.output_text_tokens",
            "tiered_per_unit",
            None,
            None,
            Some(r#"{"basis":"total_input_tokens","tiers":[{"up_to":1000,"unit_price_nanos":6},{"up_to":null,"unit_price_nanos":4}]}"#.to_string()),
            None,
            1,
        )]);

        let result = rate_cost(
            &ledger,
            &CostRatingContext {
                total_input_tokens: 1500,
            },
            &version,
        )
        .unwrap();

        assert_eq!(result.total_cost_nanos, 800);
        assert_eq!(result.detail_lines[0].unit_price_nanos, Some(4));
        assert_eq!(
            result.detail_lines[0].attributes.get("tier_basis"),
            Some(&"total_input_tokens".to_string())
        );
    }

    #[test]
    fn rates_mixed_scene_and_records_unmatched_items() {
        let mut image_attrs = BTreeMap::new();
        image_attrs.insert("spec_key".to_string(), "1024x1024".to_string());

        let ledger = CostLedger {
            items: vec![
                ledger_item(MeterKey::LlmInputTextTokens, 100),
                CostLedgerItem {
                    meter_key: MeterKey::LlmOutputImageTokens,
                    quantity: 50,
                    unit: CostUnit::Token,
                    attributes: image_attrs,
                },
                ledger_item(MeterKey::LlmReasoningTokens, 25),
            ],
        };
        let version = version_with_components(vec![
            component(
                55,
                "llm.input_text_tokens",
                "per_unit",
                Some(2),
                None,
                None,
                None,
                1,
            ),
            component(
                56,
                "llm.output_image_tokens",
                "per_unit",
                Some(9),
                None,
                None,
                Some(r#"{"spec_key":"1024x1024"}"#.to_string()),
                1,
            ),
        ]);

        let result = rate_cost(
            &ledger,
            &CostRatingContext {
                total_input_tokens: 100,
            },
            &version,
        )
        .unwrap();

        assert_eq!(result.total_cost_nanos, 650);
        assert_eq!(result.detail_lines.len(), 2);
        assert_eq!(result.unmatched_items, vec!["llm.reasoning_tokens"]);
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn falls_back_output_side_meters_to_output_text_price() {
        let ledger = CostLedger {
            items: vec![
                ledger_item(MeterKey::LlmOutputImageTokens, 40),
                ledger_item(MeterKey::LlmReasoningTokens, 25),
            ],
        };
        let version = version_with_components(vec![component(
            77,
            "llm.output_text_tokens",
            "per_unit",
            Some(7),
            None,
            None,
            None,
            1,
        )]);

        let result = rate_cost(&ledger, &CostRatingContext::default(), &version).unwrap();

        assert_eq!(result.total_cost_nanos, 455);
        assert!(result.unmatched_items.is_empty());
        assert_eq!(result.detail_lines.len(), 2);
        assert_eq!(
            result.detail_lines[0].attributes.get("fallback_meter_key"),
            Some(&"llm.output_text_tokens".to_string())
        );
        assert_eq!(
            result.detail_lines[1].attributes.get("fallback_meter_key"),
            Some(&"llm.output_text_tokens".to_string())
        );
    }

    #[test]
    fn falls_back_input_side_meters_to_input_text_price() {
        let ledger = CostLedger {
            items: vec![
                ledger_item(MeterKey::LlmInputImageTokens, 30),
                ledger_item(MeterKey::LlmCacheReadTokens, 20),
                ledger_item(MeterKey::LlmCacheWriteTokens, 10),
            ],
        };
        let version = version_with_components(vec![component(
            88,
            "llm.input_text_tokens",
            "per_unit",
            Some(5),
            None,
            None,
            None,
            1,
        )]);

        let result = rate_cost(&ledger, &CostRatingContext::default(), &version).unwrap();

        assert_eq!(result.total_cost_nanos, 300);
        assert!(result.unmatched_items.is_empty());
        assert_eq!(result.detail_lines.len(), 3);
        for detail in &result.detail_lines {
            assert_eq!(
                detail.attributes.get("fallback_meter_key"),
                Some(&"llm.input_text_tokens".to_string())
            );
        }
    }

    #[test]
    fn prefers_exact_meter_before_fallback_meter() {
        let ledger = CostLedger {
            items: vec![ledger_item(MeterKey::LlmReasoningTokens, 10)],
        };
        let version = version_with_components(vec![
            component(
                91,
                "llm.output_text_tokens",
                "per_unit",
                Some(7),
                None,
                None,
                None,
                1,
            ),
            component(
                92,
                "llm.reasoning_tokens",
                "per_unit",
                Some(11),
                None,
                None,
                None,
                1,
            ),
        ]);

        let result = rate_cost(&ledger, &CostRatingContext::default(), &version).unwrap();

        assert_eq!(result.total_cost_nanos, 110);
        assert!(
            !result.detail_lines[0]
                .attributes
                .contains_key("fallback_meter_key")
        );
        assert_eq!(result.detail_lines[0].component_id, Some(92));
    }

    #[test]
    fn large_quantities_keep_integer_precision() {
        let ledger = CostLedger {
            items: vec![ledger_item(MeterKey::LlmInputTextTokens, 2_000_000_000)],
        };
        let version = version_with_components(vec![component(
            66,
            "llm.input_text_tokens",
            "per_unit",
            Some(3),
            None,
            None,
            None,
            1,
        )]);

        let result = rate_cost(&ledger, &CostRatingContext::default(), &version).unwrap();

        assert_eq!(result.total_cost_nanos, 6_000_000_000);
    }

    #[test]
    fn unmatched_request_calls_do_not_emit_warning() {
        let ledger = CostLedger {
            items: vec![ledger_item(MeterKey::InvokeRequestCalls, 1)],
        };
        let version = version_with_components(vec![]);

        let result = rate_cost(&ledger, &CostRatingContext::default(), &version).unwrap();

        assert_eq!(result.unmatched_items, vec!["invoke.request_calls"]);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn component_validation_rejects_invalid_field_combinations() {
        let err = validate_component_config(
            "llm.input_text_tokens",
            "flat",
            Some(3),
            Some(10),
            None,
            None,
        )
        .expect_err("flat should reject unit price");

        assert!(matches!(
            err,
            crate::controller::BaseError::ParamInvalid(Some(message))
                if message.contains("unit_price_nanos is not allowed")
        ));
    }

    #[test]
    fn component_validation_accepts_valid_tiered_config() {
        validate_component_config(
            "llm.input_text_tokens",
            "tiered_per_unit",
            None,
            None,
            Some(
                r#"{"basis":"total_input_tokens","tiers":[{"up_to":1000,"unit_price_nanos":3},{"unit_price_nanos":2}]}"#,
            ),
            Some(r#"{"region":"global"}"#),
        )
        .expect("tiered component should validate");
    }

    fn ledger_item(meter_key: MeterKey, quantity: i64) -> CostLedgerItem {
        CostLedgerItem {
            meter_key,
            quantity,
            unit: meter_key.unit(),
            attributes: BTreeMap::new(),
        }
    }

    fn version_with_components(components: Vec<CacheCostComponent>) -> CacheCostCatalogVersion {
        CacheCostCatalogVersion {
            id: 999,
            catalog_id: 888,
            version: "v1".to_string(),
            currency: "USD".to_string(),
            source: None,
            effective_from: 0,
            effective_until: None,
            is_enabled: true,
            components,
        }
    }

    fn component(
        id: i64,
        meter_key: &str,
        charge_kind: &str,
        unit_price_nanos: Option<i64>,
        flat_fee_nanos: Option<i64>,
        tier_config_json: Option<String>,
        match_attributes_json: Option<String>,
        priority: i32,
    ) -> CacheCostComponent {
        CacheCostComponent {
            id,
            catalog_version_id: 999,
            meter_key: meter_key.to_string(),
            charge_kind: charge_kind.to_string(),
            unit_price_nanos,
            flat_fee_nanos,
            tier_config_json,
            match_attributes_json,
            priority,
            description: None,
        }
    }
}
