use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::{DbResult, get_connection};
use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

pub const NOTIFICATION_CHANNEL_TYPE_WEBHOOK: &str = "webhook";
pub const NOTIFICATION_DELIVERY_STATUS_PENDING: &str = "pending";
pub const NOTIFICATION_DELIVERY_STATUS_IN_PROGRESS: &str = "in_progress";
pub const NOTIFICATION_DELIVERY_STATUS_RETRY_SCHEDULED: &str = "retry_scheduled";
pub const NOTIFICATION_DELIVERY_STATUS_SUCCEEDED: &str = "succeeded";
pub const NOTIFICATION_DELIVERY_STATUS_FAILED: &str = "failed";
pub const NOTIFICATION_DELIVERY_STATUS_SKIPPED: &str = "skipped";

db_object! {
    #[derive(Insertable, Queryable, Selectable, Debug, Clone, Serialize, Deserialize)]
    #[diesel(table_name = notification_channel)]
    pub struct NotificationChannel {
        pub id: i64,
        pub channel_key: String,
        pub channel_type: String,
        pub name: String,
        pub endpoint_url: String,
        pub signing_secret: Option<String>,
        pub headers_json: Option<String>,
        pub cooldown_seconds: i64,
        pub is_enabled: bool,
        pub last_test_at: Option<i64>,
        pub last_test_success: Option<bool>,
        pub last_test_error: Option<String>,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Queryable, Selectable, Debug, Clone, Serialize, Deserialize)]
    #[diesel(table_name = notification_delivery)]
    pub struct NotificationDelivery {
        pub id: i64,
        pub channel_id: i64,
        pub alert_id: i64,
        pub alert_fingerprint: String,
        pub event_type: String,
        pub status: String,
        pub payload_json: String,
        pub attempt_count: i32,
        pub next_attempt_at: i64,
        pub last_attempt_at: Option<i64>,
        pub delivered_at: Option<i64>,
        pub last_status_code: Option<i32>,
        pub last_error: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Queryable, Selectable, Debug, Clone, Serialize, Deserialize)]
    #[diesel(table_name = notification_channel_state)]
    pub struct NotificationChannelState {
        pub id: i64,
        pub alert_id: i64,
        pub alert_fingerprint: String,
        pub channel_id: i64,
        pub event_type: String,
        pub occurrence_key: i64,
        pub last_notification_at: i64,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNotificationChannel {
    pub channel_key: String,
    pub channel_type: String,
    pub name: String,
    pub endpoint_url: String,
    pub signing_secret: Option<String>,
    pub headers_json: Option<String>,
    pub cooldown_seconds: i64,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateNotificationChannel {
    pub name: Option<String>,
    pub endpoint_url: Option<String>,
    pub signing_secret: Option<Option<String>>,
    pub headers_json: Option<Option<String>>,
    pub cooldown_seconds: Option<i64>,
    pub is_enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNotificationDelivery {
    pub channel_id: i64,
    pub alert_id: i64,
    pub alert_fingerprint: String,
    pub event_type: String,
    pub payload_json: String,
    pub next_attempt_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationDeliveryListFilter {
    pub alert_id: Option<i64>,
    pub channel_id: Option<i64>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub fn create_channel(
    input: &NewNotificationChannel,
    now_ms: i64,
) -> DbResult<NotificationChannel> {
    let conn = &mut get_connection()?;
    let channel = NotificationChannel {
        id: ID_GENERATOR.generate_id(),
        channel_key: input.channel_key.clone(),
        channel_type: input.channel_type.clone(),
        name: input.name.clone(),
        endpoint_url: input.endpoint_url.clone(),
        signing_secret: input.signing_secret.clone(),
        headers_json: input.headers_json.clone(),
        cooldown_seconds: input.cooldown_seconds,
        is_enabled: input.is_enabled,
        last_test_at: None,
        last_test_success: None,
        last_test_error: None,
        deleted_at: None,
        created_at: now_ms,
        updated_at: now_ms,
    };

    db_execute!(conn, {
        diesel::insert_into(notification_channel::table)
            .values(NotificationChannelDb::to_db(&channel))
            .returning(NotificationChannelDb::as_returning())
            .get_result::<NotificationChannelDb>(conn)
            .map(NotificationChannelDb::from_db)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to create notification channel {}: {}",
                    input.channel_key, err
                )))
            })
    })
}

pub fn update_channel(
    channel_id: i64,
    input: &UpdateNotificationChannel,
    now_ms: i64,
) -> DbResult<NotificationChannel> {
    let current = get_channel(channel_id)?;
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::update(notification_channel::table.find(channel_id))
            .set((
                notification_channel::dsl::name
                    .eq(input.name.clone().unwrap_or(current.name.clone())),
                notification_channel::dsl::endpoint_url.eq(input
                    .endpoint_url
                    .clone()
                    .unwrap_or(current.endpoint_url.clone())),
                notification_channel::dsl::signing_secret.eq(input
                    .signing_secret
                    .clone()
                    .unwrap_or(current.signing_secret.clone())),
                notification_channel::dsl::headers_json.eq(input
                    .headers_json
                    .clone()
                    .unwrap_or(current.headers_json.clone())),
                notification_channel::dsl::cooldown_seconds
                    .eq(input.cooldown_seconds.unwrap_or(current.cooldown_seconds)),
                notification_channel::dsl::is_enabled
                    .eq(input.is_enabled.unwrap_or(current.is_enabled)),
                notification_channel::dsl::updated_at.eq(now_ms),
            ))
            .returning(NotificationChannelDb::as_returning())
            .get_result::<NotificationChannelDb>(conn)
            .map(NotificationChannelDb::from_db)
            .map_err(|err| map_channel_update_error(channel_id, "update", err))
    })
}

pub fn get_channel(channel_id: i64) -> DbResult<NotificationChannel> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        notification_channel::table
            .filter(notification_channel::dsl::id.eq(channel_id))
            .filter(notification_channel::dsl::deleted_at.is_null())
            .select(NotificationChannelDb::as_select())
            .first::<NotificationChannelDb>(conn)
            .map(NotificationChannelDb::from_db)
            .map_err(|err| {
                if matches!(err, diesel::result::Error::NotFound) {
                    BaseError::ParamInvalid(Some(format!(
                        "Notification channel {} not found",
                        channel_id
                    )))
                } else {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to get notification channel {}: {}",
                        channel_id, err
                    )))
                }
            })
    })
}

pub fn list_channels(include_deleted: bool) -> DbResult<Vec<NotificationChannel>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        let mut query = notification_channel::table.into_boxed();
        if !include_deleted {
            query = query.filter(notification_channel::dsl::deleted_at.is_null());
        }
        query
            .order(notification_channel::dsl::created_at.desc())
            .select(NotificationChannelDb::as_select())
            .load::<NotificationChannelDb>(conn)
            .map(|rows| {
                rows.into_iter()
                    .map(NotificationChannelDb::from_db)
                    .collect()
            })
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to list notification channels: {}",
                    err
                )))
            })
    })
}

pub fn get_channel_state(
    alert_id: i64,
    channel_id: i64,
    event_type: &str,
) -> DbResult<Option<NotificationChannelState>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        notification_channel_state::table
            .filter(notification_channel_state::dsl::alert_id.eq(alert_id))
            .filter(notification_channel_state::dsl::channel_id.eq(channel_id))
            .filter(notification_channel_state::dsl::event_type.eq(event_type))
            .select(NotificationChannelStateDb::as_select())
            .first::<NotificationChannelStateDb>(conn)
            .optional()
            .map(|row| row.map(NotificationChannelStateDb::from_db))
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to get notification channel state alert={} channel={} event={}: {}",
                    alert_id, channel_id, event_type, err
                )))
            })
    })
}

pub fn upsert_channel_state(
    alert_id: i64,
    alert_fingerprint: &str,
    channel_id: i64,
    event_type: &str,
    occurrence_key: i64,
    now_ms: i64,
) -> DbResult<NotificationChannelState> {
    let conn = &mut get_connection()?;
    let state = NotificationChannelState {
        id: ID_GENERATOR.generate_id(),
        alert_id,
        alert_fingerprint: alert_fingerprint.to_string(),
        channel_id,
        event_type: event_type.to_string(),
        occurrence_key,
        last_notification_at: now_ms,
        created_at: now_ms,
        updated_at: now_ms,
    };
    db_execute!(conn, {
        diesel::insert_into(notification_channel_state::table)
            .values(NotificationChannelStateDb::to_db(&state))
            .on_conflict((
                notification_channel_state::dsl::alert_id,
                notification_channel_state::dsl::channel_id,
                notification_channel_state::dsl::event_type,
            ))
            .do_update()
            .set((
                notification_channel_state::dsl::alert_fingerprint.eq(alert_fingerprint),
                notification_channel_state::dsl::occurrence_key.eq(occurrence_key),
                notification_channel_state::dsl::last_notification_at.eq(now_ms),
                notification_channel_state::dsl::updated_at.eq(now_ms),
            ))
            .returning(NotificationChannelStateDb::as_returning())
            .get_result::<NotificationChannelStateDb>(conn)
            .map(NotificationChannelStateDb::from_db)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to upsert notification channel state alert={} channel={} event={}: {}",
                    alert_id, channel_id, event_type, err
                )))
            })
    })
}

pub fn delete_channel(channel_id: i64, now_ms: i64) -> DbResult<NotificationChannel> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::update(notification_channel::table.find(channel_id))
            .set((
                notification_channel::dsl::deleted_at.eq(Some(now_ms)),
                notification_channel::dsl::is_enabled.eq(false),
                notification_channel::dsl::updated_at.eq(now_ms),
            ))
            .returning(NotificationChannelDb::as_returning())
            .get_result::<NotificationChannelDb>(conn)
            .map(NotificationChannelDb::from_db)
            .map_err(|err| map_channel_update_error(channel_id, "delete", err))
    })
}

pub fn record_channel_test_result(
    channel_id: i64,
    success: bool,
    error: Option<String>,
    now_ms: i64,
) -> DbResult<NotificationChannel> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::update(notification_channel::table.find(channel_id))
            .set((
                notification_channel::dsl::last_test_at.eq(Some(now_ms)),
                notification_channel::dsl::last_test_success.eq(Some(success)),
                notification_channel::dsl::last_test_error.eq(error),
                notification_channel::dsl::updated_at.eq(now_ms),
            ))
            .returning(NotificationChannelDb::as_returning())
            .get_result::<NotificationChannelDb>(conn)
            .map(NotificationChannelDb::from_db)
            .map_err(|err| map_channel_update_error(channel_id, "record test result", err))
    })
}

fn map_channel_update_error(
    channel_id: i64,
    action: &'static str,
    err: diesel::result::Error,
) -> BaseError {
    if matches!(err, diesel::result::Error::NotFound) {
        BaseError::ParamInvalid(Some(format!(
            "Notification channel {} not found",
            channel_id
        )))
    } else {
        BaseError::DatabaseFatal(Some(format!(
            "Failed to {} notification channel {}: {}",
            action, channel_id, err
        )))
    }
}

pub fn enqueue_delivery(
    input: &NewNotificationDelivery,
    now_ms: i64,
) -> DbResult<NotificationDelivery> {
    let conn = &mut get_connection()?;
    let delivery = NotificationDelivery {
        id: ID_GENERATOR.generate_id(),
        channel_id: input.channel_id,
        alert_id: input.alert_id,
        alert_fingerprint: input.alert_fingerprint.clone(),
        event_type: input.event_type.clone(),
        status: NOTIFICATION_DELIVERY_STATUS_PENDING.to_string(),
        payload_json: input.payload_json.clone(),
        attempt_count: 0,
        next_attempt_at: input.next_attempt_at,
        last_attempt_at: None,
        delivered_at: None,
        last_status_code: None,
        last_error: None,
        created_at: now_ms,
        updated_at: now_ms,
    };
    db_execute!(conn, {
        diesel::insert_into(notification_delivery::table)
            .values(NotificationDeliveryDb::to_db(&delivery))
            .returning(NotificationDeliveryDb::as_returning())
            .get_result::<NotificationDeliveryDb>(conn)
            .map(NotificationDeliveryDb::from_db)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to enqueue notification delivery for alert {} channel {}: {}",
                    input.alert_id, input.channel_id, err
                )))
            })
    })
}

pub fn list_due_deliveries(now_ms: i64, limit: i64) -> DbResult<Vec<NotificationDelivery>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        notification_delivery::table
            .filter(
                notification_delivery::dsl::status
                    .eq(NOTIFICATION_DELIVERY_STATUS_PENDING)
                    .or(notification_delivery::dsl::status
                        .eq(NOTIFICATION_DELIVERY_STATUS_RETRY_SCHEDULED)),
            )
            .filter(notification_delivery::dsl::next_attempt_at.le(now_ms))
            .order(notification_delivery::dsl::next_attempt_at.asc())
            .limit(limit)
            .select(NotificationDeliveryDb::as_select())
            .load::<NotificationDeliveryDb>(conn)
            .map(|rows| {
                rows.into_iter()
                    .map(NotificationDeliveryDb::from_db)
                    .collect()
            })
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to list due notification deliveries: {}",
                    err
                )))
            })
    })
}

pub fn claim_due_delivery(delivery_id: i64, now_ms: i64) -> DbResult<Option<NotificationDelivery>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::update(
            notification_delivery::table
                .filter(notification_delivery::dsl::id.eq(delivery_id))
                .filter(
                    notification_delivery::dsl::status
                        .eq(NOTIFICATION_DELIVERY_STATUS_PENDING)
                        .or(notification_delivery::dsl::status
                            .eq(NOTIFICATION_DELIVERY_STATUS_RETRY_SCHEDULED)),
                )
                .filter(notification_delivery::dsl::next_attempt_at.le(now_ms)),
        )
        .set((
            notification_delivery::dsl::status.eq(NOTIFICATION_DELIVERY_STATUS_IN_PROGRESS),
            notification_delivery::dsl::last_attempt_at.eq(Some(now_ms)),
            notification_delivery::dsl::updated_at.eq(now_ms),
        ))
        .returning(NotificationDeliveryDb::as_returning())
        .get_result::<NotificationDeliveryDb>(conn)
        .optional()
        .map(|row| row.map(NotificationDeliveryDb::from_db))
        .map_err(|err| {
            BaseError::DatabaseFatal(Some(format!(
                "Failed to claim notification delivery {}: {}",
                delivery_id, err
            )))
        })
    })
}

pub fn list_deliveries(
    filter: NotificationDeliveryListFilter,
) -> DbResult<Vec<NotificationDelivery>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        let mut query = notification_delivery::table.into_boxed();
        if let Some(alert_id) = filter.alert_id {
            query = query.filter(notification_delivery::dsl::alert_id.eq(alert_id));
        }
        if let Some(channel_id) = filter.channel_id {
            query = query.filter(notification_delivery::dsl::channel_id.eq(channel_id));
        }
        if let Some(status) = filter.status.as_deref() {
            query = query.filter(notification_delivery::dsl::status.eq(status));
        }
        query
            .order(notification_delivery::dsl::created_at.desc())
            .limit(filter.limit.unwrap_or(50))
            .offset(filter.offset.unwrap_or(0))
            .select(NotificationDeliveryDb::as_select())
            .load::<NotificationDeliveryDb>(conn)
            .map(|rows| {
                rows.into_iter()
                    .map(NotificationDeliveryDb::from_db)
                    .collect()
            })
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to list notification deliveries: {}",
                    err
                )))
            })
    })
}

pub fn mark_delivery_succeeded(
    delivery_id: i64,
    attempt_count: i32,
    status_code: Option<i32>,
    now_ms: i64,
) -> DbResult<NotificationDelivery> {
    update_delivery_result(
        delivery_id,
        NOTIFICATION_DELIVERY_STATUS_SUCCEEDED,
        attempt_count,
        now_ms,
        status_code,
        None,
        Some(now_ms),
        now_ms,
    )
}

pub fn mark_delivery_retry_scheduled(
    delivery_id: i64,
    attempt_count: i32,
    status_code: Option<i32>,
    error: Option<String>,
    next_attempt_at: i64,
    now_ms: i64,
) -> DbResult<NotificationDelivery> {
    update_delivery_result(
        delivery_id,
        NOTIFICATION_DELIVERY_STATUS_RETRY_SCHEDULED,
        attempt_count,
        next_attempt_at,
        status_code,
        error,
        None,
        now_ms,
    )
}

pub fn mark_delivery_failed(
    delivery_id: i64,
    attempt_count: i32,
    status_code: Option<i32>,
    error: Option<String>,
    now_ms: i64,
) -> DbResult<NotificationDelivery> {
    update_delivery_result(
        delivery_id,
        NOTIFICATION_DELIVERY_STATUS_FAILED,
        attempt_count,
        now_ms,
        status_code,
        error,
        None,
        now_ms,
    )
}

pub fn mark_delivery_skipped(
    delivery_id: i64,
    attempt_count: i32,
    error: Option<String>,
    now_ms: i64,
) -> DbResult<NotificationDelivery> {
    update_delivery_result(
        delivery_id,
        NOTIFICATION_DELIVERY_STATUS_SKIPPED,
        attempt_count,
        now_ms,
        None,
        error,
        None,
        now_ms,
    )
}

fn update_delivery_result(
    delivery_id: i64,
    status: &str,
    attempt_count: i32,
    next_attempt_at: i64,
    status_code: Option<i32>,
    error: Option<String>,
    delivered_at: Option<i64>,
    now_ms: i64,
) -> DbResult<NotificationDelivery> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::update(
            notification_delivery::table
                .filter(notification_delivery::dsl::id.eq(delivery_id))
                .filter(
                    notification_delivery::dsl::status.eq(NOTIFICATION_DELIVERY_STATUS_IN_PROGRESS),
                ),
        )
        .set((
            notification_delivery::dsl::status.eq(status),
            notification_delivery::dsl::attempt_count.eq(attempt_count),
            notification_delivery::dsl::next_attempt_at.eq(next_attempt_at),
            notification_delivery::dsl::last_attempt_at.eq(Some(now_ms)),
            notification_delivery::dsl::delivered_at.eq(delivered_at),
            notification_delivery::dsl::last_status_code.eq(status_code),
            notification_delivery::dsl::last_error.eq(error),
            notification_delivery::dsl::updated_at.eq(now_ms),
        ))
        .returning(NotificationDeliveryDb::as_returning())
        .get_result::<NotificationDeliveryDb>(conn)
        .map(NotificationDeliveryDb::from_db)
        .map_err(|err| {
            if matches!(err, diesel::result::Error::NotFound) {
                BaseError::ParamInvalid(Some(format!(
                    "Notification delivery {} is not in progress",
                    delivery_id
                )))
            } else {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to update notification delivery {}: {}",
                    delivery_id, err
                )))
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::TestDbContext;

    #[test]
    fn channel_crud_soft_deletes_and_records_test_result() {
        let context = TestDbContext::new_sqlite("notification-channel.sqlite");
        context.run_sync(|| {
            let channel = create_channel(
                &NewNotificationChannel {
                    channel_key: "ops".to_string(),
                    channel_type: NOTIFICATION_CHANNEL_TYPE_WEBHOOK.to_string(),
                    name: "Ops".to_string(),
                    endpoint_url: "https://example.com/webhook".to_string(),
                    signing_secret: Some("secret".to_string()),
                    headers_json: Some(r#"{"X-Ops":"primary"}"#.to_string()),
                    cooldown_seconds: 300,
                    is_enabled: true,
                },
                1_000,
            )
            .unwrap();

            assert_eq!(list_channels(false).unwrap().len(), 1);
            assert_eq!(channel.signing_secret.as_deref(), Some("secret"));
            assert_eq!(
                channel.headers_json.as_deref(),
                Some(r#"{"X-Ops":"primary"}"#)
            );
            assert_eq!(channel.cooldown_seconds, 300);

            let updated = update_channel(
                channel.id,
                &UpdateNotificationChannel {
                    name: Some("Ops Updated".to_string()),
                    signing_secret: Some(None),
                    headers_json: Some(None),
                    cooldown_seconds: Some(120),
                    is_enabled: Some(false),
                    ..UpdateNotificationChannel::default()
                },
                2_000,
            )
            .unwrap();
            assert_eq!(updated.name, "Ops Updated");
            assert_eq!(updated.signing_secret, None);
            assert_eq!(updated.headers_json, None);
            assert_eq!(updated.cooldown_seconds, 120);
            assert!(!updated.is_enabled);

            let tested = record_channel_test_result(
                channel.id,
                false,
                Some("webhook failed".to_string()),
                3_000,
            )
            .unwrap();
            assert_eq!(tested.last_test_at, Some(3_000));
            assert_eq!(tested.last_test_success, Some(false));

            let deleted = delete_channel(channel.id, 4_000).unwrap();
            assert_eq!(deleted.deleted_at, Some(4_000));
            assert!(list_channels(false).unwrap().is_empty());
            assert_eq!(list_channels(true).unwrap().len(), 1);
        });
    }

    #[test]
    fn delivery_queue_lists_due_rows_and_records_terminal_status() {
        let context = TestDbContext::new_sqlite("notification-delivery-db.sqlite");
        context.run_sync(|| {
            let channel = create_channel(
                &NewNotificationChannel {
                    channel_key: "ops".to_string(),
                    channel_type: NOTIFICATION_CHANNEL_TYPE_WEBHOOK.to_string(),
                    name: "Ops".to_string(),
                    endpoint_url: "https://example.com/webhook".to_string(),
                    signing_secret: None,
                    headers_json: None,
                    cooldown_seconds: 900,
                    is_enabled: true,
                },
                1_000,
            )
            .unwrap();
            let alert = crate::database::alert::fire_alert(
                &crate::database::alert::AlertFireRecord {
                    fingerprint: "provider_open:provider:7".to_string(),
                    rule_key: "provider_open".to_string(),
                    severity: "critical".to_string(),
                    scope_type: "provider".to_string(),
                    scope_id: "7".to_string(),
                    title: "Provider open".to_string(),
                    summary: "Provider circuit is open".to_string(),
                    details_json: "{}".to_string(),
                    metrics_snapshot_json: None,
                },
                1_100,
            )
            .unwrap();

            let future = enqueue_delivery(
                &NewNotificationDelivery {
                    channel_id: channel.id,
                    alert_id: alert.id,
                    alert_fingerprint: alert.fingerprint.clone(),
                    event_type: "alert_fired".to_string(),
                    payload_json: "{}".to_string(),
                    next_attempt_at: 5_000,
                },
                1_200,
            )
            .unwrap();
            assert!(list_due_deliveries(4_999, 10).unwrap().is_empty());
            assert_eq!(list_due_deliveries(5_000, 10).unwrap().len(), 1);
            let claimed = claim_due_delivery(future.id, 5_000).unwrap().unwrap();
            assert_eq!(claimed.status, NOTIFICATION_DELIVERY_STATUS_IN_PROGRESS);
            assert!(claim_due_delivery(future.id, 5_000).unwrap().is_none());

            let retry = mark_delivery_retry_scheduled(
                claimed.id,
                1,
                Some(500),
                Some("webhook returned HTTP 500".to_string()),
                30_000,
                5_000,
            )
            .unwrap();
            assert_eq!(retry.status, NOTIFICATION_DELIVERY_STATUS_RETRY_SCHEDULED);
            assert_eq!(retry.attempt_count, 1);
            assert_eq!(retry.next_attempt_at, 30_000);

            let claimed_retry = claim_due_delivery(retry.id, 30_000).unwrap().unwrap();
            let failed = mark_delivery_failed(
                claimed_retry.id,
                2,
                Some(500),
                Some("max attempts exceeded".to_string()),
                30_000,
            )
            .unwrap();
            assert_eq!(failed.status, NOTIFICATION_DELIVERY_STATUS_FAILED);
            assert!(list_due_deliveries(30_000, 10).unwrap().is_empty());

            let rows = list_deliveries(NotificationDeliveryListFilter {
                alert_id: Some(alert.id),
                status: Some(NOTIFICATION_DELIVERY_STATUS_FAILED.to_string()),
                ..NotificationDeliveryListFilter::default()
            })
            .unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].id, failed.id);
        });
    }
}
