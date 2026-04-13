pub mod engine;
pub mod ledger;
pub mod meter;
pub mod normalize;
pub mod snapshot;
pub mod templates;

pub use engine::{CostRatingContext, rate_cost, validate_component_config};
pub use ledger::{CostLedger, CostLedgerItem};
pub use meter::{ChargeKind, CostUnit, MeterKey};
pub use normalize::UsageNormalization;
pub use snapshot::{
    COST_SNAPSHOT_SCHEMA_VERSION_V1, CostDetailLine, CostRatingResult, CostSnapshot,
};
pub use templates::{CostTemplateSummary, find_template, list_templates};

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::{
        COST_SNAPSHOT_SCHEMA_VERSION_V1, ChargeKind, CostDetailLine, CostSnapshot, CostUnit,
        MeterKey,
    };

    #[test]
    fn meter_key_serializes_to_stable_wire_value() {
        let serialized = serde_json::to_value(MeterKey::LlmInputTextTokens).unwrap();
        assert_eq!(
            serialized,
            Value::String("llm.input_text_tokens".to_string())
        );

        let round_trip: MeterKey =
            serde_json::from_value(Value::String("invoke.request_calls".to_string())).unwrap();
        assert_eq!(round_trip, MeterKey::InvokeRequestCalls);
    }

    #[test]
    fn cost_snapshot_serializes_with_detail_lines() {
        let mut attributes = BTreeMap::new();
        attributes.insert("tier".to_string(), "default".to_string());

        let snapshot = CostSnapshot {
            schema_version: COST_SNAPSHOT_SCHEMA_VERSION_V1,
            cost_catalog_id: 11,
            cost_catalog_version_id: 22,
            total_cost_nanos: 3200,
            currency: "USD".to_string(),
            detail_lines: vec![CostDetailLine {
                meter_key: MeterKey::LlmOutputTextTokens,
                quantity: 1600,
                unit: CostUnit::Token,
                charge_kind: ChargeKind::PerUnit,
                amount_nanos: 3200,
                unit_price_nanos: Some(2),
                component_id: Some(33),
                catalog_version_id: Some(22),
                description: Some("output token cost".to_string()),
                attributes,
            }],
            unmatched_items: vec!["llm.cache_write_tokens".to_string()],
            warnings: vec!["missing cache_write component".to_string()],
        };

        let serialized = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(
            serialized,
            json!({
                "schema_version": 1,
                "cost_catalog_id": 11,
                "cost_catalog_version_id": 22,
                "total_cost_nanos": 3200,
                "currency": "USD",
                "detail_lines": [{
                    "meter_key": "llm.output_text_tokens",
                    "quantity": 1600,
                    "unit": "token",
                    "charge_kind": "per_unit",
                    "amount_nanos": 3200,
                    "unit_price_nanos": 2,
                    "component_id": 33,
                    "catalog_version_id": 22,
                    "description": "output token cost",
                    "attributes": {
                        "tier": "default"
                    }
                }],
                "unmatched_items": ["llm.cache_write_tokens"],
                "warnings": ["missing cache_write component"]
            })
        );
    }
}
