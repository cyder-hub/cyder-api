use crate::controller::BaseError;
use crate::database::alert::{
    AlertEvent, AlertFireRecord, AlertListFilter, AlertRuleState, acknowledge_alert, fire_alert,
    get_alert, list_alerts, resolve_alert, suppress_alert, unsuppress_alert, upsert_rule_state,
};

use super::service::AlertsService;
use super::types::{AlertAcknowledgeInput, AlertFireInput, AlertSuppressInput};

impl AlertsService {
    pub fn fire_alert(&self, input: AlertFireInput, now_ms: i64) -> Result<AlertEvent, BaseError> {
        fire_alert(&AlertFireRecord::from(input), now_ms)
    }

    pub fn list_alerts(&self, filter: AlertListFilter) -> Result<Vec<AlertEvent>, BaseError> {
        list_alerts(filter)
    }

    pub fn get_alert(&self, alert_id: i64) -> Result<AlertEvent, BaseError> {
        get_alert(alert_id)
    }

    pub fn acknowledge_alert(
        &self,
        alert_id: i64,
        input: AlertAcknowledgeInput,
        now_ms: i64,
    ) -> Result<AlertEvent, BaseError> {
        acknowledge_alert(alert_id, input.note, now_ms)
    }

    pub fn suppress_alert(
        &self,
        alert_id: i64,
        input: AlertSuppressInput,
        now_ms: i64,
    ) -> Result<AlertEvent, BaseError> {
        suppress_alert(alert_id, input.suppressed_until, input.reason, now_ms)
    }

    pub fn unsuppress_alert(&self, alert_id: i64, now_ms: i64) -> Result<AlertEvent, BaseError> {
        unsuppress_alert(alert_id, now_ms)
    }

    pub fn resolve_alert(&self, alert_id: i64, now_ms: i64) -> Result<AlertEvent, BaseError> {
        resolve_alert(alert_id, now_ms)
    }

    pub fn upsert_rule_state(&self, state: &AlertRuleState) -> Result<AlertRuleState, BaseError> {
        upsert_rule_state(state)
    }
}

impl From<AlertFireInput> for AlertFireRecord {
    fn from(value: AlertFireInput) -> Self {
        Self {
            fingerprint: value.fingerprint,
            rule_key: value.rule_key,
            severity: value.severity.as_str().to_string(),
            scope_type: value.scope_type.as_str().to_string(),
            scope_id: value.scope_id,
            title: value.title,
            summary: value.summary,
            details_json: value.details_json,
            metrics_snapshot_json: value.metrics_snapshot_json,
        }
    }
}

pub fn is_alert_suppressed(alert: &AlertEvent, now_ms: i64) -> bool {
    alert
        .suppressed_until
        .is_some_and(|suppressed_until| suppressed_until > now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AlertsConfig;
    use crate::database::TestDbContext;
    use crate::database::alert::{ALERT_STATUS_ACTIVE, ALERT_STATUS_RESOLVED};
    use crate::service::alerts::types::{
        AlertAcknowledgeInput, AlertFireInput, AlertScopeType, AlertSeverity, AlertSuppressInput,
    };

    #[test]
    fn lifecycle_acknowledge_suppress_and_resolve_are_orthogonal() {
        let context = TestDbContext::new_sqlite("alert-lifecycle.sqlite");
        context.run_sync(|| {
            let service = AlertsService::new(AlertsConfig::default());
            let alert = service.fire_alert(sample_fire_input(), 1_000).unwrap();

            let acknowledged = service
                .acknowledge_alert(
                    alert.id,
                    AlertAcknowledgeInput {
                        note: Some("operator saw it".to_string()),
                    },
                    1_100,
                )
                .unwrap();
            assert_eq!(acknowledged.status, ALERT_STATUS_ACTIVE);
            assert_eq!(acknowledged.acknowledged_at, Some(1_100));

            let suppressed = service
                .suppress_alert(
                    alert.id,
                    AlertSuppressInput {
                        suppressed_until: 2_000,
                        reason: Some("maintenance".to_string()),
                    },
                    1_200,
                )
                .unwrap();
            assert_eq!(suppressed.status, ALERT_STATUS_ACTIVE);
            assert!(is_alert_suppressed(&suppressed, 1_500));
            assert!(!is_alert_suppressed(&suppressed, 2_000));

            let resolved = service.resolve_alert(alert.id, 1_800).unwrap();
            assert_eq!(resolved.status, ALERT_STATUS_RESOLVED);
            assert_eq!(resolved.resolved_at, Some(1_800));
            assert_eq!(resolved.acknowledged_at, Some(1_100));

            let reopened = service.fire_alert(sample_fire_input(), 1_900).unwrap();
            assert_eq!(reopened.status, ALERT_STATUS_ACTIVE);
            assert_eq!(reopened.reopened_count, 1);
            assert_eq!(reopened.acknowledged_at, None);
            assert!(is_alert_suppressed(&reopened, 1_950));

            let unsuppressed = service.unsuppress_alert(alert.id, 1_960).unwrap();
            assert!(!is_alert_suppressed(&unsuppressed, 1_970));
        });
    }

    fn sample_fire_input() -> AlertFireInput {
        AlertFireInput {
            fingerprint: "provider_open:provider:7".to_string(),
            rule_key: "provider_open".to_string(),
            severity: AlertSeverity::Critical,
            scope_type: AlertScopeType::Provider,
            scope_id: "7".to_string(),
            title: "Provider open".to_string(),
            summary: "Provider circuit is open".to_string(),
            details_json: "{}".to_string(),
            metrics_snapshot_json: Some("{\"error_count\":10}".to_string()),
        }
    }
}
