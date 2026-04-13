use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::cost::{
    UsageNormalization,
    meter::{CostUnit, MeterKey},
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostLedger {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<CostLedgerItem>,
}

impl CostLedger {
    pub fn from_normalization(normalization: &UsageNormalization) -> Self {
        let mut ledger = Self::default();
        ledger.push_meter(
            MeterKey::LlmInputTextTokens,
            normalization.input_text_tokens,
        );
        ledger.push_meter(
            MeterKey::LlmOutputTextTokens,
            normalization.output_text_tokens,
        );
        ledger.push_meter(
            MeterKey::LlmInputImageTokens,
            normalization.input_image_tokens,
        );
        ledger.push_meter(
            MeterKey::LlmOutputImageTokens,
            normalization.output_image_tokens,
        );
        ledger.push_meter(
            MeterKey::LlmCacheReadTokens,
            normalization.cache_read_tokens,
        );
        ledger.push_meter(
            MeterKey::LlmCacheWriteTokens,
            normalization.cache_write_tokens,
        );
        ledger.push_meter(MeterKey::LlmReasoningTokens, normalization.reasoning_tokens);
        ledger.push_meter(MeterKey::InvokeRequestCalls, 1);
        ledger
    }

    pub fn push_meter(&mut self, meter_key: MeterKey, quantity: i64) {
        if quantity <= 0 {
            return;
        }

        self.items.push(CostLedgerItem {
            meter_key,
            quantity,
            unit: meter_key.unit(),
            attributes: BTreeMap::new(),
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostLedgerItem {
    pub meter_key: MeterKey,
    pub quantity: i64,
    pub unit: CostUnit,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
}

impl From<&UsageNormalization> for CostLedger {
    fn from(normalization: &UsageNormalization) -> Self {
        Self::from_normalization(normalization)
    }
}

impl From<UsageNormalization> for CostLedger {
    fn from(normalization: UsageNormalization) -> Self {
        Self::from_normalization(&normalization)
    }
}

#[cfg(test)]
mod tests {
    use super::CostLedger;
    use crate::cost::{CostUnit, MeterKey, UsageNormalization};

    #[test]
    fn builds_text_only_ledger_with_invocation_meter() {
        let normalization = UsageNormalization {
            input_text_tokens: 120,
            output_text_tokens: 80,
            total_input_tokens: 120,
            total_output_tokens: 80,
            ..Default::default()
        };

        let ledger = CostLedger::from(&normalization);

        assert_eq!(ledger.items.len(), 3);
        assert_eq!(ledger.items[0].meter_key, MeterKey::LlmInputTextTokens);
        assert_eq!(ledger.items[0].quantity, 120);
        assert_eq!(ledger.items[0].unit, CostUnit::Token);
        assert_eq!(ledger.items[1].meter_key, MeterKey::LlmOutputTextTokens);
        assert_eq!(ledger.items[1].quantity, 80);
        assert_eq!(ledger.items[2].meter_key, MeterKey::InvokeRequestCalls);
        assert_eq!(ledger.items[2].quantity, 1);
        assert_eq!(ledger.items[2].unit, CostUnit::Call);
    }

    #[test]
    fn builds_multimodal_ledger_without_total_token_meters() {
        let normalization = UsageNormalization {
            total_input_tokens: 100,
            total_output_tokens: 70,
            input_text_tokens: 60,
            output_text_tokens: 40,
            input_image_tokens: 30,
            output_image_tokens: 25,
            cache_read_tokens: 10,
            reasoning_tokens: 5,
            ..Default::default()
        };

        let ledger = CostLedger::from(normalization);
        let meters = ledger
            .items
            .iter()
            .map(|item| (item.meter_key, item.quantity))
            .collect::<Vec<_>>();

        assert_eq!(
            meters,
            vec![
                (MeterKey::LlmInputTextTokens, 60),
                (MeterKey::LlmOutputTextTokens, 40),
                (MeterKey::LlmInputImageTokens, 30),
                (MeterKey::LlmOutputImageTokens, 25),
                (MeterKey::LlmCacheReadTokens, 10),
                (MeterKey::LlmReasoningTokens, 5),
                (MeterKey::InvokeRequestCalls, 1),
            ]
        );
    }

    #[test]
    fn builds_image_output_ledger_for_generation_requests() {
        let normalization = UsageNormalization {
            total_output_tokens: 512,
            output_image_tokens: 512,
            ..Default::default()
        };

        let ledger = CostLedger::from(&normalization);

        assert_eq!(ledger.items.len(), 2);
        assert_eq!(ledger.items[0].meter_key, MeterKey::LlmOutputImageTokens);
        assert_eq!(ledger.items[0].quantity, 512);
        assert_eq!(ledger.items[1].meter_key, MeterKey::InvokeRequestCalls);
    }

    #[test]
    fn skips_zero_quantity_meters_but_keeps_invocation_meter() {
        let normalization = UsageNormalization::default();

        let ledger = CostLedger::from(&normalization);

        assert_eq!(ledger.items.len(), 1);
        assert_eq!(ledger.items[0].meter_key, MeterKey::InvokeRequestCalls);
        assert_eq!(ledger.items[0].quantity, 1);
        assert!(ledger.items[0].attributes.is_empty());
    }
}
