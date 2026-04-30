use cyder_tools::log::debug;

use crate::{
    cost::{
        COST_SNAPSHOT_SCHEMA_VERSION_V1, CostLedger, CostRatingContext, CostSnapshot,
        UsageNormalization, rate_cost,
    },
    service::{
        cache::types::CacheCostCatalogVersion,
        diagnostics::replay::transport::AttemptReplayExecutionOutcome,
    },
};

pub(crate) fn usage_totals_for_run(
    normalization: &UsageNormalization,
) -> (Option<i32>, Option<i32>, Option<i32>, Option<i32>) {
    let total_tokens = normalization.total_input_tokens + normalization.total_output_tokens;
    (
        i64_to_i32(normalization.total_input_tokens),
        i64_to_i32(normalization.total_output_tokens),
        i64_to_i32(normalization.reasoning_tokens),
        i64_to_i32(total_tokens),
    )
}

pub(crate) fn rate_replay_cost(
    normalization: &UsageNormalization,
    version: &CacheCostCatalogVersion,
) -> (Option<i64>, Option<String>) {
    let ledger = CostLedger::from(normalization);
    let rating = match rate_cost(
        &ledger,
        &CostRatingContext {
            total_input_tokens: normalization.total_input_tokens,
        },
        version,
    ) {
        Ok(rating) => rating,
        Err(err) => {
            debug!(
                "Replay cost rating failed for version {}: {:?}",
                version.id, err
            );
            return (None, None);
        }
    };

    let snapshot = CostSnapshot {
        schema_version: COST_SNAPSHOT_SCHEMA_VERSION_V1,
        cost_catalog_id: version.catalog_id,
        cost_catalog_version_id: version.id,
        total_cost_nanos: rating.total_cost_nanos,
        currency: rating.currency.clone(),
        detail_lines: rating.detail_lines,
        unmatched_items: rating.unmatched_items,
        warnings: rating.warnings,
    };

    if serde_json::to_string(&snapshot).is_ok() {
        (Some(snapshot.total_cost_nanos), Some(snapshot.currency))
    } else {
        (None, None)
    }
}

pub(crate) fn apply_usage_cost_to_outcome(
    outcome: &mut AttemptReplayExecutionOutcome,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
) {
    let Some(normalization) = outcome.usage_normalization.as_ref() else {
        return;
    };

    let (estimated_cost_nanos, estimated_cost_currency) = cost_catalog_version
        .map(|version| rate_replay_cost(normalization, version))
        .unwrap_or((None, None));
    let (total_input_tokens, total_output_tokens, reasoning_tokens, total_tokens) =
        usage_totals_for_run(normalization);

    outcome.estimated_cost_nanos = estimated_cost_nanos;
    outcome.estimated_cost_currency = estimated_cost_currency;
    outcome.total_input_tokens = total_input_tokens;
    outcome.total_output_tokens = total_output_tokens;
    outcome.reasoning_tokens = reasoning_tokens;
    outcome.total_tokens = total_tokens;
}

fn i64_to_i32(value: i64) -> Option<i32> {
    i32::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use crate::{
        cost::UsageNormalization,
        schema::enum_def::RequestReplayStatus,
        service::{
            cache::types::{CacheCostCatalogVersion, CacheCostComponent},
            diagnostics::replay::transport::AttemptReplayExecutionOutcome,
        },
    };

    use super::*;

    #[test]
    fn replay_cost_rating_populates_run_token_and_cost_fields() {
        let normalization = UsageNormalization {
            total_input_tokens: 12,
            total_output_tokens: 5,
            input_text_tokens: 12,
            output_text_tokens: 5,
            reasoning_tokens: 2,
            ..Default::default()
        };
        let mut outcome = outcome_with_usage(normalization);
        let version = version_with_components(vec![
            component(11, "llm.input_text_tokens", Some(3)),
            component(12, "llm.output_text_tokens", Some(7)),
            component(13, "llm.reasoning_tokens", Some(11)),
        ]);

        apply_usage_cost_to_outcome(&mut outcome, Some(&version));

        assert_eq!(outcome.total_input_tokens, Some(12));
        assert_eq!(outcome.total_output_tokens, Some(5));
        assert_eq!(outcome.reasoning_tokens, Some(2));
        assert_eq!(outcome.total_tokens, Some(17));
        assert_eq!(outcome.estimated_cost_nanos, Some(93));
        assert_eq!(outcome.estimated_cost_currency.as_deref(), Some("USD"));
    }

    #[test]
    fn replay_cost_rating_keeps_cost_empty_without_catalog_version() {
        let normalization = UsageNormalization {
            total_input_tokens: 1,
            total_output_tokens: 2,
            input_text_tokens: 1,
            output_text_tokens: 2,
            ..Default::default()
        };
        let mut outcome = outcome_with_usage(normalization);

        apply_usage_cost_to_outcome(&mut outcome, None);

        assert_eq!(outcome.total_input_tokens, Some(1));
        assert_eq!(outcome.total_output_tokens, Some(2));
        assert_eq!(outcome.total_tokens, Some(3));
        assert_eq!(outcome.estimated_cost_nanos, None);
        assert_eq!(outcome.estimated_cost_currency, None);
    }

    #[test]
    fn replay_cost_rating_drops_overflowing_run_token_fields() {
        let normalization = UsageNormalization {
            total_input_tokens: i64::from(i32::MAX) + 1,
            total_output_tokens: 2,
            input_text_tokens: i64::from(i32::MAX) + 1,
            output_text_tokens: 2,
            ..Default::default()
        };
        let mut outcome = outcome_with_usage(normalization);

        apply_usage_cost_to_outcome(&mut outcome, None);

        assert_eq!(outcome.total_input_tokens, None);
        assert_eq!(outcome.total_output_tokens, Some(2));
        assert_eq!(outcome.total_tokens, None);
    }

    fn outcome_with_usage(normalization: UsageNormalization) -> AttemptReplayExecutionOutcome {
        AttemptReplayExecutionOutcome {
            status: RequestReplayStatus::Success,
            http_status: Some(200),
            first_byte_at: Some(1),
            error_code: None,
            error_message: None,
            response_headers: Vec::new(),
            response_body: None,
            response_body_bytes: None,
            response_body_capture_state: None,
            response_body_capture: None,
            usage_normalization: Some(normalization),
            transform_diagnostics: Vec::new(),
            estimated_cost_nanos: None,
            estimated_cost_currency: None,
            total_input_tokens: None,
            total_output_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
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

    fn component(id: i64, meter_key: &str, unit_price_nanos: Option<i64>) -> CacheCostComponent {
        CacheCostComponent {
            id,
            catalog_version_id: 999,
            meter_key: meter_key.to_string(),
            charge_kind: "per_unit".to_string(),
            unit_price_nanos,
            flat_fee_nanos: None,
            tier_config_json: None,
            match_attributes_json: None,
            priority: 1,
            description: None,
        }
    }
}
