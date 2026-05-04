use serde_json::{Value, json};

use crate::controller::BaseError;
use crate::database::alert::{AlertEvent, mark_alert_notified};
use crate::database::notification::{
    NOTIFICATION_CHANNEL_TYPE_WEBHOOK, NewNotificationDelivery, NotificationChannel,
    NotificationDelivery, NotificationDeliveryListFilter, enqueue_delivery, get_channel_state,
    list_channels, list_deliveries, upsert_channel_state,
};
use crate::service::alerts::lifecycle::is_alert_suppressed;

use super::service::NotificationService;
use super::types::{
    NotificationDeliveryListInput, NotificationDeliveryListResponse, NotificationDeliveryResponse,
    NotificationEventType,
};

impl NotificationService {
    pub fn enqueue_alert_event(
        &self,
        alert: &AlertEvent,
        event_type: NotificationEventType,
        now_ms: i64,
        _fire_cooldown_seconds: u64,
    ) -> Result<usize, BaseError> {
        if !self.config().enabled {
            return Ok(0);
        }
        if event_type == NotificationEventType::AlertFired {
            if is_alert_suppressed(alert, now_ms) {
                return Ok(0);
            }
        }

        let channels = list_channels(false)?
            .into_iter()
            .filter(|channel| {
                channel.is_enabled && channel.channel_type == NOTIFICATION_CHANNEL_TYPE_WEBHOOK
            })
            .collect::<Vec<_>>();

        let mut enqueued = 0usize;
        for channel in channels {
            if event_type == NotificationEventType::AlertFired
                && is_channel_fire_cooldown_active(alert, &channel, now_ms)?
            {
                continue;
            }
            let payload = build_alert_webhook_payload(event_type, now_ms, &channel, alert);
            let payload_json = serde_json::to_string(&payload).map_err(|err| {
                BaseError::InternalServerError(Some(format!(
                    "failed to serialize notification payload: {err}"
                )))
            })?;
            enqueue_delivery(
                &NewNotificationDelivery {
                    channel_id: channel.id,
                    alert_id: alert.id,
                    alert_fingerprint: alert.fingerprint.clone(),
                    event_type: event_type.as_str().to_string(),
                    payload_json,
                    next_attempt_at: now_ms,
                },
                now_ms,
            )?;
            if event_type == NotificationEventType::AlertFired {
                upsert_channel_state(
                    alert.id,
                    &alert.fingerprint,
                    channel.id,
                    event_type.as_str(),
                    alert.reopened_count,
                    now_ms,
                )?;
            }
            enqueued += 1;
        }

        if enqueued > 0 && event_type == NotificationEventType::AlertFired {
            mark_alert_notified(alert.id, now_ms)?;
        }

        Ok(enqueued)
    }

    pub fn list_deliveries(
        &self,
        input: NotificationDeliveryListInput,
    ) -> Result<NotificationDeliveryListResponse, BaseError> {
        if let Some(status) = input.status.as_deref() {
            validate_delivery_status(status)?;
        }
        let limit = input.limit.unwrap_or(50).clamp(1, 100);
        let offset = input.offset.unwrap_or(0).max(0);
        let mut rows = list_deliveries(NotificationDeliveryListFilter {
            alert_id: input.alert_id,
            channel_id: input.channel_id,
            status: input.status,
            limit: Some(limit + 1),
            offset: Some(offset),
        })?;
        let has_more = rows.len() > limit as usize;
        if has_more {
            rows.truncate(limit as usize);
        }
        Ok(NotificationDeliveryListResponse {
            items: rows.into_iter().map(delivery_response).collect(),
            next_offset: has_more.then_some(offset + limit),
        })
    }
}

pub fn build_alert_webhook_payload(
    event_type: NotificationEventType,
    sent_at: i64,
    channel: &NotificationChannel,
    alert: &AlertEvent,
) -> Value {
    json!({
        "event_type": event_type.as_str(),
        "sent_at": sent_at,
        "channel_id": channel.id,
        "channel_key": channel.channel_key.as_str(),
        "alert_id": alert.id,
        "fingerprint": alert.fingerprint.as_str(),
        "rule_key": alert.rule_key.as_str(),
        "severity": alert.severity.as_str(),
        "status": alert.status.as_str(),
        "scope": {
            "type": alert.scope_type.as_str(),
            "id": alert.scope_id.as_str(),
        },
        "title": alert.title.as_str(),
        "summary": alert.summary.as_str(),
        "details": json_or_string(&alert.details_json),
        "metrics_snapshot": alert.metrics_snapshot_json.as_deref().map(json_or_string),
        "first_seen_at": alert.first_seen_at,
        "last_seen_at": alert.last_seen_at,
        "resolved_at": alert.resolved_at,
        "occurrence_count": alert.occurrence_count,
        "reopened_count": alert.reopened_count,
    })
}

pub fn delivery_response(delivery: NotificationDelivery) -> NotificationDeliveryResponse {
    NotificationDeliveryResponse {
        id: delivery.id,
        channel_id: delivery.channel_id,
        alert_id: delivery.alert_id,
        alert_fingerprint: delivery.alert_fingerprint,
        event_type: delivery.event_type,
        status: delivery.status,
        payload_json: delivery.payload_json,
        attempt_count: delivery.attempt_count,
        next_attempt_at: delivery.next_attempt_at,
        last_attempt_at: delivery.last_attempt_at,
        delivered_at: delivery.delivered_at,
        last_status_code: delivery.last_status_code,
        last_error: delivery.last_error,
        created_at: delivery.created_at,
        updated_at: delivery.updated_at,
    }
}

fn is_channel_fire_cooldown_active(
    alert: &AlertEvent,
    channel: &NotificationChannel,
    now_ms: i64,
) -> Result<bool, BaseError> {
    let Some(state) = get_channel_state(
        alert.id,
        channel.id,
        NotificationEventType::AlertFired.as_str(),
    )?
    else {
        return Ok(alert
            .last_notification_at
            .is_some_and(|last_notification_at| {
                is_fire_cooldown_active(last_notification_at, channel, now_ms)
            }));
    };
    if state.occurrence_key != alert.reopened_count {
        return Ok(false);
    }
    Ok(is_fire_cooldown_active(
        state.last_notification_at,
        channel,
        now_ms,
    ))
}

fn is_fire_cooldown_active(
    last_notification_at: i64,
    channel: &NotificationChannel,
    now_ms: i64,
) -> bool {
    let fire_cooldown_seconds = channel.cooldown_seconds.max(0);
    let cooldown_ms = fire_cooldown_seconds.saturating_mul(1_000);
    last_notification_at.saturating_add(cooldown_ms) > now_ms
}

fn json_or_string(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}

fn validate_delivery_status(status: &str) -> Result<(), BaseError> {
    match status {
        "pending" | "in_progress" | "retry_scheduled" | "succeeded" | "failed" | "skipped" => {
            Ok(())
        }
        _ => Err(BaseError::ParamInvalid(Some(format!(
            "invalid notification delivery status '{status}'"
        )))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NotificationConfig;
    use crate::database::TestDbContext;
    use crate::database::alert::{AlertFireRecord, fire_alert, get_alert};
    use crate::database::notification::{
        NOTIFICATION_DELIVERY_STATUS_PENDING, NotificationDeliveryListFilter,
        list_deliveries as list_delivery_rows,
    };

    #[test]
    fn alert_fire_enqueues_enabled_webhook_and_marks_notified() {
        let context = TestDbContext::new_sqlite("notification-delivery-enqueue.sqlite");
        context.run_sync(|| {
            let service = NotificationService::new(NotificationConfig::default());
            create_enabled_channel("ops", 900);
            let alert = fire_alert(&sample_fire_record(), 1_000).unwrap();

            let enqueued = service
                .enqueue_alert_event(&alert, NotificationEventType::AlertFired, 2_000, 60)
                .unwrap();
            assert_eq!(enqueued, 1);

            let deliveries = list_delivery_rows(NotificationDeliveryListFilter {
                alert_id: Some(alert.id),
                ..NotificationDeliveryListFilter::default()
            })
            .unwrap();
            assert_eq!(deliveries.len(), 1);
            assert_eq!(deliveries[0].status, NOTIFICATION_DELIVERY_STATUS_PENDING);
            assert_eq!(deliveries[0].event_type, "alert_fired");
            let notified = get_alert(alert.id).unwrap();
            assert_eq!(notified.last_notification_at, Some(2_000));
        });
    }

    #[test]
    fn suppressed_fire_and_cooldown_fire_are_skipped_but_recovery_enqueues() {
        let context = TestDbContext::new_sqlite("notification-delivery-cooldown.sqlite");
        context.run_sync(|| {
            let service = NotificationService::new(NotificationConfig::default());
            create_enabled_channel("ops", 900);
            let alert = fire_alert(&sample_fire_record(), 1_000).unwrap();
            crate::database::alert::suppress_alert(alert.id, 10_000, None, 1_500).unwrap();
            let suppressed = get_alert(alert.id).unwrap();

            let suppressed_count = service
                .enqueue_alert_event(&suppressed, NotificationEventType::AlertFired, 2_000, 60)
                .unwrap();
            assert_eq!(suppressed_count, 0);

            crate::database::alert::unsuppress_alert(alert.id, 2_500).unwrap();
            let active = get_alert(alert.id).unwrap();
            let first = service
                .enqueue_alert_event(&active, NotificationEventType::AlertFired, 3_000, 60)
                .unwrap();
            assert_eq!(first, 1);

            let cooled_down = get_alert(alert.id).unwrap();
            let skipped = service
                .enqueue_alert_event(&cooled_down, NotificationEventType::AlertFired, 3_500, 60)
                .unwrap();
            assert_eq!(skipped, 0);

            let recovered = crate::database::alert::resolve_alert(alert.id, 4_000).unwrap();
            let recovery = service
                .enqueue_alert_event(&recovered, NotificationEventType::AlertRecovered, 4_100, 60)
                .unwrap();
            assert_eq!(recovery, 1);

            let reopened = fire_alert(&sample_fire_record(), 4_500).unwrap();
            assert_eq!(reopened.last_notification_at, None);
            let refired = service
                .enqueue_alert_event(&reopened, NotificationEventType::AlertFired, 4_600, 60)
                .unwrap();
            assert_eq!(refired, 1);

            let deliveries = list_delivery_rows(NotificationDeliveryListFilter {
                alert_id: Some(alert.id),
                ..NotificationDeliveryListFilter::default()
            })
            .unwrap();
            assert_eq!(deliveries.len(), 3);
            assert!(
                deliveries
                    .iter()
                    .any(|delivery| delivery.event_type == "alert_recovered")
            );
            assert_eq!(
                deliveries
                    .iter()
                    .filter(|delivery| delivery.event_type == "alert_fired")
                    .count(),
                2
            );
        });
    }

    #[test]
    fn fire_cooldown_is_tracked_per_channel() {
        let context = TestDbContext::new_sqlite("notification-delivery-channel-cooldown.sqlite");
        context.run_sync(|| {
            let service = NotificationService::new(NotificationConfig::default());
            let fast = create_enabled_channel_with_cooldown("fast", 1, 900);
            let slow = create_enabled_channel_with_cooldown("slow", 10, 901);
            let alert = fire_alert(&sample_fire_record(), 1_000).unwrap();

            let first = service
                .enqueue_alert_event(&alert, NotificationEventType::AlertFired, 1_000, 60)
                .unwrap();
            assert_eq!(first, 2);

            let repeated = fire_alert(&sample_fire_record(), 2_500).unwrap();
            let second = service
                .enqueue_alert_event(&repeated, NotificationEventType::AlertFired, 2_500, 60)
                .unwrap();
            assert_eq!(second, 1);

            let repeated_again = fire_alert(&sample_fire_record(), 11_500).unwrap();
            let third = service
                .enqueue_alert_event(
                    &repeated_again,
                    NotificationEventType::AlertFired,
                    11_500,
                    60,
                )
                .unwrap();
            assert_eq!(third, 2);

            let deliveries = list_delivery_rows(NotificationDeliveryListFilter {
                alert_id: Some(alert.id),
                ..NotificationDeliveryListFilter::default()
            })
            .unwrap();
            assert_eq!(deliveries.len(), 5);
            assert_eq!(
                deliveries
                    .iter()
                    .filter(|delivery| delivery.channel_id == fast.id)
                    .count(),
                3
            );
            assert_eq!(
                deliveries
                    .iter()
                    .filter(|delivery| delivery.channel_id == slow.id)
                    .count(),
                2
            );
        });
    }

    #[test]
    fn legacy_alert_notification_timestamp_preserves_cooldown_without_channel_state() {
        let context = TestDbContext::new_sqlite("notification-delivery-legacy-cooldown.sqlite");
        context.run_sync(|| {
            let service = NotificationService::new(NotificationConfig::default());
            create_enabled_channel_with_cooldown("ops", 10, 900);
            let alert = fire_alert(&sample_fire_record(), 1_000).unwrap();
            let legacy_notified = mark_alert_notified(alert.id, 2_000).unwrap();

            let skipped = service
                .enqueue_alert_event(
                    &legacy_notified,
                    NotificationEventType::AlertFired,
                    11_999,
                    60,
                )
                .unwrap();
            assert_eq!(skipped, 0);

            let deliveries = list_delivery_rows(NotificationDeliveryListFilter {
                alert_id: Some(alert.id),
                ..NotificationDeliveryListFilter::default()
            })
            .unwrap();
            assert!(deliveries.is_empty());

            let expired = service
                .enqueue_alert_event(
                    &legacy_notified,
                    NotificationEventType::AlertFired,
                    12_000,
                    60,
                )
                .unwrap();
            assert_eq!(expired, 1);
        });
    }

    fn create_enabled_channel(channel_key: &str, now_ms: i64) {
        create_enabled_channel_with_cooldown(channel_key, 900, now_ms);
    }

    fn create_enabled_channel_with_cooldown(
        channel_key: &str,
        cooldown_seconds: i64,
        now_ms: i64,
    ) -> crate::database::notification::NotificationChannel {
        crate::database::notification::create_channel(
            &crate::database::notification::NewNotificationChannel {
                channel_key: channel_key.to_string(),
                channel_type: NOTIFICATION_CHANNEL_TYPE_WEBHOOK.to_string(),
                name: "Ops".to_string(),
                endpoint_url: "https://example.com/webhook".to_string(),
                signing_secret: None,
                headers_json: None,
                cooldown_seconds,
                is_enabled: true,
            },
            now_ms,
        )
        .unwrap()
    }

    #[test]
    fn alert_webhook_payload_uses_top_level_contract() {
        let channel = NotificationChannel {
            id: 7,
            channel_key: "ops".to_string(),
            channel_type: NOTIFICATION_CHANNEL_TYPE_WEBHOOK.to_string(),
            name: "Ops".to_string(),
            endpoint_url: "https://example.com/webhook".to_string(),
            signing_secret: None,
            headers_json: None,
            cooldown_seconds: 900,
            is_enabled: true,
            last_test_at: None,
            last_test_success: None,
            last_test_error: None,
            deleted_at: None,
            created_at: 1,
            updated_at: 1,
        };
        let alert = AlertEvent {
            id: 9,
            fingerprint: "high_error_rate:global:global".to_string(),
            rule_key: "high_error_rate".to_string(),
            severity: "critical".to_string(),
            status: "active".to_string(),
            scope_type: "global".to_string(),
            scope_id: "global".to_string(),
            title: "High error rate".to_string(),
            summary: "Errors are high".to_string(),
            details_json: "{\"error_rate\":0.5}".to_string(),
            metrics_snapshot_json: None,
            first_seen_at: 1_000,
            last_seen_at: 2_000,
            resolved_at: None,
            acknowledged_at: None,
            acknowledged_note: None,
            suppressed_until: None,
            suppressed_reason: None,
            occurrence_count: 1,
            reopened_count: 0,
            last_notification_at: None,
            created_at: 1_000,
            updated_at: 2_000,
        };

        let payload =
            build_alert_webhook_payload(NotificationEventType::AlertFired, 3_000, &channel, &alert);
        assert_eq!(
            payload.get("event_type").and_then(|value| value.as_str()),
            Some("alert_fired")
        );
        assert_eq!(
            payload.get("fingerprint").and_then(|value| value.as_str()),
            Some("high_error_rate:global:global")
        );
        assert!(payload.get("data").is_none());
        assert_eq!(
            payload
                .get("scope")
                .and_then(|scope| scope.get("type"))
                .and_then(|value| value.as_str()),
            Some("global")
        );
    }

    fn sample_fire_record() -> AlertFireRecord {
        AlertFireRecord {
            fingerprint: "provider_open:provider:7".to_string(),
            rule_key: "provider_open".to_string(),
            severity: "critical".to_string(),
            scope_type: "provider".to_string(),
            scope_id: "7".to_string(),
            title: "Provider open".to_string(),
            summary: "Provider circuit is open".to_string(),
            details_json: "{\"provider_id\":7}".to_string(),
            metrics_snapshot_json: Some("{\"request_count\":10}".to_string()),
        }
    }
}
