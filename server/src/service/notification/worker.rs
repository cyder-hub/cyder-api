use reqwest::Client;

use crate::controller::BaseError;
use crate::database::notification::{
    NotificationDelivery, claim_due_delivery, list_due_deliveries, mark_delivery_failed,
    mark_delivery_retry_scheduled, mark_delivery_skipped, mark_delivery_succeeded,
};

use super::service::NotificationService;
use super::types::{NotificationWebhookTestResult, NotificationWorkerTickResult};
use super::webhook::{WebhookRequestOptions, send_webhook_json};

impl NotificationService {
    pub async fn process_due_deliveries(
        &self,
        client: &Client,
        now_ms: i64,
        limit: i64,
    ) -> Result<NotificationWorkerTickResult, BaseError> {
        if !self.config().enabled {
            return Ok(NotificationWorkerTickResult::default());
        }

        let deliveries = list_due_deliveries(now_ms, limit.clamp(1, 500))?;
        let mut result = NotificationWorkerTickResult::default();
        for due_delivery in deliveries {
            let Some(delivery) = claim_due_delivery(due_delivery.id, now_ms)? else {
                result.skipped += 1;
                continue;
            };
            result.processed += 1;
            match self.process_one_delivery(client, delivery, now_ms).await? {
                DeliveryOutcome::Succeeded => result.succeeded += 1,
                DeliveryOutcome::RetryScheduled => result.retry_scheduled += 1,
                DeliveryOutcome::Failed => result.failed += 1,
                DeliveryOutcome::Skipped => result.skipped += 1,
            }
        }
        Ok(result)
    }

    async fn process_one_delivery(
        &self,
        client: &Client,
        delivery: NotificationDelivery,
        now_ms: i64,
    ) -> Result<DeliveryOutcome, BaseError> {
        let next_attempt_count = delivery.attempt_count.saturating_add(1);
        let channel = match self.get_channel_secret(delivery.channel_id) {
            Ok(channel) if channel.is_enabled => channel,
            Ok(_) => {
                mark_delivery_skipped(
                    delivery.id,
                    delivery.attempt_count,
                    Some("notification channel is disabled".to_string()),
                    now_ms,
                )?;
                return Ok(DeliveryOutcome::Skipped);
            }
            Err(err) => {
                mark_delivery_skipped(
                    delivery.id,
                    delivery.attempt_count,
                    Some(format!("notification channel is unavailable: {err:?}")),
                    now_ms,
                )?;
                return Ok(DeliveryOutcome::Skipped);
            }
        };

        let payload = match serde_json::from_str::<serde_json::Value>(&delivery.payload_json) {
            Ok(payload) => payload,
            Err(err) => {
                mark_delivery_failed(
                    delivery.id,
                    next_attempt_count,
                    None,
                    Some(format!("invalid notification payload JSON: {err}")),
                    now_ms,
                )?;
                return Ok(DeliveryOutcome::Failed);
            }
        };

        let send_result = send_webhook_json(
            client,
            &channel.endpoint_url,
            &payload,
            self.config().webhook_timeout_seconds,
            WebhookRequestOptions {
                event_type: &delivery.event_type,
                alert_fingerprint: Some(&delivery.alert_fingerprint),
                signing_secret: channel.signing_secret.as_deref(),
                headers_json: channel.headers_json.as_deref(),
            },
        )
        .await;

        match send_result {
            Ok(response) if response.success => {
                mark_delivery_succeeded(
                    delivery.id,
                    next_attempt_count,
                    response.status.map(i32::from),
                    now_ms,
                )?;
                Ok(DeliveryOutcome::Succeeded)
            }
            Ok(response) => self.record_delivery_failure(
                &delivery,
                next_attempt_count,
                response.status.map(i32::from),
                delivery_error(response),
                now_ms,
            ),
            Err(err) => self.record_delivery_failure(
                &delivery,
                next_attempt_count,
                None,
                Some(format!("{err:?}")),
                now_ms,
            ),
        }
    }

    fn record_delivery_failure(
        &self,
        delivery: &NotificationDelivery,
        next_attempt_count: i32,
        status_code: Option<i32>,
        error: Option<String>,
        now_ms: i64,
    ) -> Result<DeliveryOutcome, BaseError> {
        if delivery_should_fail(self.config().max_delivery_attempts, next_attempt_count) {
            mark_delivery_failed(delivery.id, next_attempt_count, status_code, error, now_ms)?;
            return Ok(DeliveryOutcome::Failed);
        }

        let delay_seconds = next_retry_delay_seconds(
            self.config().retry_base_backoff_seconds,
            self.config().retry_max_backoff_seconds,
            next_attempt_count,
        );
        let next_attempt_at = now_ms.saturating_add((delay_seconds as i64).saturating_mul(1_000));
        mark_delivery_retry_scheduled(
            delivery.id,
            next_attempt_count,
            status_code,
            error,
            next_attempt_at,
            now_ms,
        )?;
        Ok(DeliveryOutcome::RetryScheduled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryOutcome {
    Succeeded,
    RetryScheduled,
    Failed,
    Skipped,
}

pub fn next_retry_delay_seconds(base_seconds: u64, max_seconds: u64, attempt_count: i32) -> u64 {
    let base = base_seconds.max(1);
    let max = max_seconds.max(base);
    let exponent = attempt_count.saturating_sub(1).clamp(0, 32) as u32;
    base.saturating_mul(2_u64.saturating_pow(exponent)).min(max)
}

pub fn delivery_should_fail(max_attempts: u32, next_attempt_count: i32) -> bool {
    next_attempt_count >= max_attempts.max(1) as i32
}

fn delivery_error(response: NotificationWebhookTestResult) -> Option<String> {
    response.error.or_else(|| {
        response
            .status
            .map(|status| format!("webhook returned HTTP {status}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NotificationConfig;
    use crate::database::TestDbContext;
    use crate::database::alert::{AlertFireRecord, fire_alert};
    use crate::database::notification::{
        NOTIFICATION_CHANNEL_TYPE_WEBHOOK, NOTIFICATION_DELIVERY_STATUS_FAILED,
        NOTIFICATION_DELIVERY_STATUS_RETRY_SCHEDULED, NOTIFICATION_DELIVERY_STATUS_SKIPPED,
        NOTIFICATION_DELIVERY_STATUS_SUCCEEDED, NewNotificationChannel, create_channel,
        list_deliveries,
    };
    use axum::{
        Router,
        body::Bytes,
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn retry_backoff_is_exponential_and_capped() {
        assert_eq!(next_retry_delay_seconds(30, 900, 1), 30);
        assert_eq!(next_retry_delay_seconds(30, 900, 2), 60);
        assert_eq!(next_retry_delay_seconds(30, 900, 3), 120);
        assert_eq!(next_retry_delay_seconds(30, 90, 4), 90);
        assert!(delivery_should_fail(3, 3));
        assert!(!delivery_should_fail(3, 2));
    }

    #[tokio::test]
    async fn worker_marks_2xx_delivery_succeeded() {
        let context = TestDbContext::new_sqlite("notification-worker-success.sqlite");
        context
            .run_async(async {
                let endpoint_url = spawn_webhook(StatusCode::NO_CONTENT).await;
                let service = NotificationService::new(NotificationConfig::default());
                let alert = seed_delivery(&service, &endpoint_url, 1_000);

                let result = service
                    .process_due_deliveries(&Client::new(), 2_000, 10)
                    .await
                    .unwrap();
                assert_eq!(result.processed, 1);
                assert_eq!(result.succeeded, 1);

                let deliveries = list_deliveries(
                    crate::database::notification::NotificationDeliveryListFilter {
                        alert_id: Some(alert.id),
                        ..crate::database::notification::NotificationDeliveryListFilter::default()
                    },
                )
                .unwrap();
                assert_eq!(deliveries[0].status, NOTIFICATION_DELIVERY_STATUS_SUCCEEDED);
                assert_eq!(deliveries[0].attempt_count, 1);
                assert_eq!(deliveries[0].last_status_code, Some(204));
                assert_eq!(deliveries[0].delivered_at, Some(2_000));
            })
            .await;
    }

    #[tokio::test]
    async fn worker_schedules_retry_then_marks_failed_after_max_attempts() {
        let context = TestDbContext::new_sqlite("notification-worker-retry.sqlite");
        context
            .run_async(async {
                let endpoint_url = spawn_webhook(StatusCode::INTERNAL_SERVER_ERROR).await;
                let service = NotificationService::new(NotificationConfig {
                    max_delivery_attempts: 2,
                    retry_base_backoff_seconds: 30,
                    retry_max_backoff_seconds: 900,
                    ..NotificationConfig::default()
                });
                let alert = seed_delivery(&service, &endpoint_url, 1_000);

                let first = service
                    .process_due_deliveries(&Client::new(), 2_000, 10)
                    .await
                    .unwrap();
                assert_eq!(first.retry_scheduled, 1);
                let deliveries = list_deliveries(
                    crate::database::notification::NotificationDeliveryListFilter {
                        alert_id: Some(alert.id),
                        ..crate::database::notification::NotificationDeliveryListFilter::default()
                    },
                )
                .unwrap();
                assert_eq!(
                    deliveries[0].status,
                    NOTIFICATION_DELIVERY_STATUS_RETRY_SCHEDULED
                );
                assert_eq!(deliveries[0].next_attempt_at, 32_000);
                assert_eq!(deliveries[0].last_status_code, Some(500));

                let second = service
                    .process_due_deliveries(&Client::new(), 32_000, 10)
                    .await
                    .unwrap();
                assert_eq!(second.failed, 1);
                let deliveries = list_deliveries(
                    crate::database::notification::NotificationDeliveryListFilter {
                        alert_id: Some(alert.id),
                        ..crate::database::notification::NotificationDeliveryListFilter::default()
                    },
                )
                .unwrap();
                assert_eq!(deliveries[0].status, NOTIFICATION_DELIVERY_STATUS_FAILED);
                assert_eq!(deliveries[0].attempt_count, 2);
            })
            .await;
    }

    #[tokio::test]
    async fn concurrent_workers_claim_due_delivery_once_and_send_signed_headers() {
        let context = TestDbContext::new_sqlite("notification-worker-claim.sqlite");
        context
            .run_async(async {
                let observed = Arc::new(ObservedWebhook::default());
                let endpoint_url =
                    spawn_observed_webhook(StatusCode::NO_CONTENT, Arc::clone(&observed)).await;
                let service = NotificationService::new(NotificationConfig::default());
                let alert = seed_delivery_with_channel(
                    &service,
                    &endpoint_url,
                    1_000,
                    Some("secret"),
                    Some(r#"{"X-Ops":"primary"}"#),
                    true,
                );

                let client = Client::new();
                let (left, right) = tokio::join!(
                    service.process_due_deliveries(&client, 2_000, 10),
                    service.process_due_deliveries(&client, 2_000, 10),
                );
                let total_succeeded = left.unwrap().succeeded + right.unwrap().succeeded;
                assert_eq!(total_succeeded, 1);
                assert_eq!(observed.count.load(Ordering::SeqCst), 1);
                assert_eq!(
                    observed.event.lock().unwrap().as_deref(),
                    Some("alert_fired")
                );
                assert_eq!(
                    observed.custom_header.lock().unwrap().as_deref(),
                    Some("primary")
                );
                let body = observed.body.lock().unwrap().clone();
                assert!(body.contains(r#""fingerprint":"provider_open:provider:7""#));
                let expected_signature =
                    super::super::webhook::hmac_sha256_header_value("secret", body.as_bytes())
                        .unwrap();
                assert_eq!(
                    observed.signature.lock().unwrap().as_deref(),
                    Some(expected_signature.as_str())
                );

                let deliveries = list_deliveries(
                    crate::database::notification::NotificationDeliveryListFilter {
                        alert_id: Some(alert.id),
                        ..crate::database::notification::NotificationDeliveryListFilter::default()
                    },
                )
                .unwrap();
                assert_eq!(deliveries[0].status, NOTIFICATION_DELIVERY_STATUS_SUCCEEDED);
            })
            .await;
    }

    #[tokio::test]
    async fn worker_marks_disabled_channel_delivery_skipped() {
        let context = TestDbContext::new_sqlite("notification-worker-skipped.sqlite");
        context
            .run_async(async {
                let endpoint_url = spawn_webhook(StatusCode::NO_CONTENT).await;
                let service = NotificationService::new(NotificationConfig::default());
                let alert =
                    seed_delivery_with_channel(&service, &endpoint_url, 1_000, None, None, false);

                let result = service
                    .process_due_deliveries(&Client::new(), 2_000, 10)
                    .await
                    .unwrap();
                assert_eq!(result.processed, 1);
                assert_eq!(result.skipped, 1);

                let deliveries = list_deliveries(
                    crate::database::notification::NotificationDeliveryListFilter {
                        alert_id: Some(alert.id),
                        ..crate::database::notification::NotificationDeliveryListFilter::default()
                    },
                )
                .unwrap();
                assert_eq!(deliveries[0].status, NOTIFICATION_DELIVERY_STATUS_SKIPPED);
                assert_eq!(
                    deliveries[0].last_error.as_deref(),
                    Some("notification channel is disabled")
                );
            })
            .await;
    }

    async fn spawn_webhook(status: StatusCode) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test webhook listener should bind");
        let addr = listener
            .local_addr()
            .expect("test webhook local addr should be available");
        tokio::spawn(async move {
            let app = Router::new().route("/", post(move || async move { (status, "ok") }));
            axum::serve(listener, app)
                .await
                .expect("test webhook should serve");
        });
        format!("http://{addr}/")
    }

    #[derive(Default)]
    struct ObservedWebhook {
        count: AtomicUsize,
        body: std::sync::Mutex<String>,
        event: std::sync::Mutex<Option<String>>,
        custom_header: std::sync::Mutex<Option<String>>,
        signature: std::sync::Mutex<Option<String>>,
    }

    async fn spawn_observed_webhook(status: StatusCode, observed: Arc<ObservedWebhook>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test webhook listener should bind");
        let addr = listener
            .local_addr()
            .expect("test webhook local addr should be available");
        tokio::spawn(async move {
            let app = Router::new()
                .route(
                    "/",
                    post(
                        move |State(observed): State<Arc<ObservedWebhook>>,
                              headers: HeaderMap,
                              body: Bytes| async move {
                            observed.count.fetch_add(1, Ordering::SeqCst);
                            *observed.body.lock().unwrap() =
                                String::from_utf8_lossy(&body).to_string();
                            *observed.event.lock().unwrap() = headers
                                .get("x-cyder-event")
                                .and_then(|value| value.to_str().ok())
                                .map(str::to_string);
                            *observed.custom_header.lock().unwrap() = headers
                                .get("x-ops")
                                .and_then(|value| value.to_str().ok())
                                .map(str::to_string);
                            *observed.signature.lock().unwrap() = headers
                                .get("x-cyder-signature")
                                .and_then(|value| value.to_str().ok())
                                .map(str::to_string);
                            (status, "ok")
                        },
                    ),
                )
                .with_state(observed);
            axum::serve(listener, app)
                .await
                .expect("test webhook should serve");
        });
        format!("http://{addr}/")
    }

    fn seed_delivery(
        service: &NotificationService,
        endpoint_url: &str,
        now_ms: i64,
    ) -> crate::database::alert::AlertEvent {
        seed_delivery_with_channel(service, endpoint_url, now_ms, None, None, true)
    }

    fn seed_delivery_with_channel(
        service: &NotificationService,
        endpoint_url: &str,
        now_ms: i64,
        signing_secret: Option<&str>,
        headers_json: Option<&str>,
        channel_enabled_after_enqueue: bool,
    ) -> crate::database::alert::AlertEvent {
        let channel = create_channel(
            &NewNotificationChannel {
                channel_key: "ops".to_string(),
                channel_type: NOTIFICATION_CHANNEL_TYPE_WEBHOOK.to_string(),
                name: "Ops".to_string(),
                endpoint_url: endpoint_url.to_string(),
                signing_secret: signing_secret.map(str::to_string),
                headers_json: headers_json.map(str::to_string),
                cooldown_seconds: 900,
                is_enabled: true,
            },
            now_ms,
        )
        .unwrap();
        let alert = fire_alert(&sample_fire_record(), now_ms).unwrap();
        let enqueued = service
            .enqueue_alert_event(
                &alert,
                super::super::types::NotificationEventType::AlertFired,
                now_ms,
                60,
            )
            .unwrap();
        assert_eq!(enqueued, 1);
        if !channel_enabled_after_enqueue {
            crate::database::notification::update_channel(
                channel.id,
                &crate::database::notification::UpdateNotificationChannel {
                    is_enabled: Some(false),
                    ..crate::database::notification::UpdateNotificationChannel::default()
                },
                now_ms + 1,
            )
            .unwrap();
        }
        alert
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
            details_json: "{}".to_string(),
            metrics_snapshot_json: None,
        }
    }
}
