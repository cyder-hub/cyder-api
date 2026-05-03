use diesel::prelude::*;
use diesel::upsert::excluded;
use serde::{Deserialize, Serialize};

use super::{DbResult, get_connection};
use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

pub const ALERT_STATUS_ACTIVE: &str = "active";
pub const ALERT_STATUS_RESOLVED: &str = "resolved";

db_object! {
    #[derive(Insertable, Queryable, Selectable, Debug, Clone, Serialize, Deserialize)]
    #[diesel(table_name = alert_event)]
    pub struct AlertEvent {
        pub id: i64,
        pub fingerprint: String,
        pub rule_key: String,
        pub severity: String,
        pub status: String,
        pub scope_type: String,
        pub scope_id: String,
        pub title: String,
        pub summary: String,
        pub details_json: String,
        pub metrics_snapshot_json: Option<String>,
        pub first_seen_at: i64,
        pub last_seen_at: i64,
        pub resolved_at: Option<i64>,
        pub acknowledged_at: Option<i64>,
        pub acknowledged_note: Option<String>,
        pub suppressed_until: Option<i64>,
        pub suppressed_reason: Option<String>,
        pub occurrence_count: i64,
        pub reopened_count: i64,
        pub last_notification_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Queryable, Selectable, Debug, Clone, Serialize, Deserialize)]
    #[diesel(table_name = alert_rule_state)]
    #[diesel(primary_key(rule_key, scope_type, scope_id))]
    pub struct AlertRuleState {
        pub rule_key: String,
        pub scope_type: String,
        pub scope_id: String,
        pub last_evaluated_at: i64,
        pub last_fired_at: Option<i64>,
        pub last_resolved_at: Option<i64>,
        pub cooldown_until: Option<i64>,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertFireRecord {
    pub fingerprint: String,
    pub rule_key: String,
    pub severity: String,
    pub scope_type: String,
    pub scope_id: String,
    pub title: String,
    pub summary: String,
    pub details_json: String,
    pub metrics_snapshot_json: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlertListFilter {
    pub status: Option<String>,
    pub severity: Option<String>,
    pub rule_key: Option<String>,
    pub scope_type: Option<String>,
    pub scope_id: Option<String>,
    pub acknowledged: Option<bool>,
    pub suppressed: Option<bool>,
    pub seen_from: Option<i64>,
    pub seen_to: Option<i64>,
    pub now_ms: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub fn fire_alert(record: &AlertFireRecord, now_ms: i64) -> DbResult<AlertEvent> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        conn.transaction::<AlertEvent, BaseError, _>(|conn| {
            let existing = alert_event::table
                .filter(alert_event::dsl::fingerprint.eq(&record.fingerprint))
                .select(AlertEventDb::as_select())
                .first::<AlertEventDb>(conn)
                .optional()
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to load alert by fingerprint {}: {}",
                        record.fingerprint, err
                    )))
                })?
                .map(AlertEventDb::from_db);

            if let Some(existing) = existing {
                let was_resolved = existing.status == ALERT_STATUS_RESOLVED;
                let acknowledged_at = if was_resolved {
                    None
                } else {
                    existing.acknowledged_at
                };
                let acknowledged_note = if was_resolved {
                    None
                } else {
                    existing.acknowledged_note.clone()
                };
                let last_notification_at = if was_resolved {
                    None
                } else {
                    existing.last_notification_at
                };
                let occurrence_count = existing.occurrence_count + 1;
                let reopened_count = if was_resolved {
                    existing.reopened_count + 1
                } else {
                    existing.reopened_count
                };
                return diesel::update(alert_event::table.find(existing.id))
                    .set((
                        alert_event::dsl::rule_key.eq(&record.rule_key),
                        alert_event::dsl::severity.eq(&record.severity),
                        alert_event::dsl::status.eq(ALERT_STATUS_ACTIVE),
                        alert_event::dsl::scope_type.eq(&record.scope_type),
                        alert_event::dsl::scope_id.eq(&record.scope_id),
                        alert_event::dsl::title.eq(&record.title),
                        alert_event::dsl::summary.eq(&record.summary),
                        alert_event::dsl::details_json.eq(&record.details_json),
                        alert_event::dsl::metrics_snapshot_json
                            .eq(record.metrics_snapshot_json.clone()),
                        alert_event::dsl::last_seen_at.eq(now_ms),
                        alert_event::dsl::resolved_at.eq::<Option<i64>>(None),
                        alert_event::dsl::acknowledged_at.eq(acknowledged_at),
                        alert_event::dsl::acknowledged_note.eq(acknowledged_note),
                        alert_event::dsl::occurrence_count.eq(occurrence_count),
                        alert_event::dsl::reopened_count.eq(reopened_count),
                        alert_event::dsl::last_notification_at.eq(last_notification_at),
                        alert_event::dsl::updated_at.eq(now_ms),
                    ))
                    .returning(AlertEventDb::as_returning())
                    .get_result::<AlertEventDb>(conn)
                    .map(AlertEventDb::from_db)
                    .map_err(|err| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to update alert {}: {}",
                            existing.id, err
                        )))
                    });
            }

            let alert = AlertEvent {
                id: ID_GENERATOR.generate_id(),
                fingerprint: record.fingerprint.clone(),
                rule_key: record.rule_key.clone(),
                severity: record.severity.clone(),
                status: ALERT_STATUS_ACTIVE.to_string(),
                scope_type: record.scope_type.clone(),
                scope_id: record.scope_id.clone(),
                title: record.title.clone(),
                summary: record.summary.clone(),
                details_json: record.details_json.clone(),
                metrics_snapshot_json: record.metrics_snapshot_json.clone(),
                first_seen_at: now_ms,
                last_seen_at: now_ms,
                resolved_at: None,
                acknowledged_at: None,
                acknowledged_note: None,
                suppressed_until: None,
                suppressed_reason: None,
                occurrence_count: 1,
                reopened_count: 0,
                last_notification_at: None,
                created_at: now_ms,
                updated_at: now_ms,
            };

            diesel::insert_into(alert_event::table)
                .values(AlertEventDb::to_db(&alert))
                .returning(AlertEventDb::as_returning())
                .get_result::<AlertEventDb>(conn)
                .map(AlertEventDb::from_db)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to insert alert {}: {}",
                        record.fingerprint, err
                    )))
                })
        })
    })
}

pub fn get_alert(alert_id: i64) -> DbResult<AlertEvent> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        alert_event::table
            .find(alert_id)
            .select(AlertEventDb::as_select())
            .first::<AlertEventDb>(conn)
            .map(AlertEventDb::from_db)
            .map_err(|err| {
                if matches!(err, diesel::result::Error::NotFound) {
                    BaseError::ParamInvalid(Some(format!("Alert {} not found", alert_id)))
                } else {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to get alert {}: {}",
                        alert_id, err
                    )))
                }
            })
    })
}

pub fn get_alert_by_fingerprint(fingerprint: &str) -> DbResult<Option<AlertEvent>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        alert_event::table
            .filter(alert_event::dsl::fingerprint.eq(fingerprint))
            .select(AlertEventDb::as_select())
            .first::<AlertEventDb>(conn)
            .optional()
            .map(|row| row.map(AlertEventDb::from_db))
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to get alert by fingerprint {}: {}",
                    fingerprint, err
                )))
            })
    })
}

pub fn list_alerts(filter: AlertListFilter) -> DbResult<Vec<AlertEvent>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        let mut query = alert_event::table.into_boxed();

        if let Some(status) = filter.status.as_deref() {
            query = query.filter(alert_event::dsl::status.eq(status));
        }
        if let Some(severity) = filter.severity.as_deref() {
            query = query.filter(alert_event::dsl::severity.eq(severity));
        }
        if let Some(rule_key) = filter.rule_key.as_deref() {
            query = query.filter(alert_event::dsl::rule_key.eq(rule_key));
        }
        if let Some(scope_type) = filter.scope_type.as_deref() {
            query = query.filter(alert_event::dsl::scope_type.eq(scope_type));
        }
        if let Some(scope_id) = filter.scope_id.as_deref() {
            query = query.filter(alert_event::dsl::scope_id.eq(scope_id));
        }
        if let Some(acknowledged) = filter.acknowledged {
            query = if acknowledged {
                query.filter(alert_event::dsl::acknowledged_at.is_not_null())
            } else {
                query.filter(alert_event::dsl::acknowledged_at.is_null())
            };
        }
        if let Some(suppressed) = filter.suppressed {
            let now_ms = filter.now_ms.unwrap_or(0);
            query = if suppressed {
                query.filter(alert_event::dsl::suppressed_until.gt(now_ms))
            } else {
                query.filter(
                    alert_event::dsl::suppressed_until
                        .is_null()
                        .or(alert_event::dsl::suppressed_until.le(now_ms)),
                )
            };
        }
        if let Some(seen_from) = filter.seen_from {
            query = query.filter(alert_event::dsl::last_seen_at.ge(seen_from));
        }
        if let Some(seen_to) = filter.seen_to {
            query = query.filter(alert_event::dsl::last_seen_at.lt(seen_to));
        }

        query
            .order(alert_event::dsl::last_seen_at.desc())
            .limit(filter.limit.unwrap_or(50))
            .offset(filter.offset.unwrap_or(0))
            .select(AlertEventDb::as_select())
            .load::<AlertEventDb>(conn)
            .map(|rows| rows.into_iter().map(AlertEventDb::from_db).collect())
            .map_err(|err| BaseError::DatabaseFatal(Some(format!("Failed to list alerts: {err}"))))
    })
}

pub fn acknowledge_alert(alert_id: i64, note: Option<String>, now_ms: i64) -> DbResult<AlertEvent> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::update(alert_event::table.find(alert_id))
            .set((
                alert_event::dsl::acknowledged_at.eq(Some(now_ms)),
                alert_event::dsl::acknowledged_note.eq(note),
                alert_event::dsl::updated_at.eq(now_ms),
            ))
            .returning(AlertEventDb::as_returning())
            .get_result::<AlertEventDb>(conn)
            .map(AlertEventDb::from_db)
            .map_err(|err| map_alert_update_error(alert_id, "acknowledge", err))
    })
}

pub fn suppress_alert(
    alert_id: i64,
    suppressed_until: i64,
    reason: Option<String>,
    now_ms: i64,
) -> DbResult<AlertEvent> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::update(alert_event::table.find(alert_id))
            .set((
                alert_event::dsl::suppressed_until.eq(Some(suppressed_until)),
                alert_event::dsl::suppressed_reason.eq(reason),
                alert_event::dsl::updated_at.eq(now_ms),
            ))
            .returning(AlertEventDb::as_returning())
            .get_result::<AlertEventDb>(conn)
            .map(AlertEventDb::from_db)
            .map_err(|err| map_alert_update_error(alert_id, "suppress", err))
    })
}

pub fn unsuppress_alert(alert_id: i64, now_ms: i64) -> DbResult<AlertEvent> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::update(alert_event::table.find(alert_id))
            .set((
                alert_event::dsl::suppressed_until.eq::<Option<i64>>(None),
                alert_event::dsl::suppressed_reason.eq::<Option<String>>(None),
                alert_event::dsl::updated_at.eq(now_ms),
            ))
            .returning(AlertEventDb::as_returning())
            .get_result::<AlertEventDb>(conn)
            .map(AlertEventDb::from_db)
            .map_err(|err| map_alert_update_error(alert_id, "unsuppress", err))
    })
}

pub fn resolve_alert(alert_id: i64, now_ms: i64) -> DbResult<AlertEvent> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::update(alert_event::table.find(alert_id))
            .set((
                alert_event::dsl::status.eq(ALERT_STATUS_RESOLVED),
                alert_event::dsl::resolved_at.eq(Some(now_ms)),
                alert_event::dsl::updated_at.eq(now_ms),
            ))
            .returning(AlertEventDb::as_returning())
            .get_result::<AlertEventDb>(conn)
            .map(AlertEventDb::from_db)
            .map_err(|err| map_alert_update_error(alert_id, "resolve", err))
    })
}

pub fn mark_alert_notified(alert_id: i64, now_ms: i64) -> DbResult<AlertEvent> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::update(alert_event::table.find(alert_id))
            .set((
                alert_event::dsl::last_notification_at.eq(Some(now_ms)),
                alert_event::dsl::updated_at.eq(now_ms),
            ))
            .returning(AlertEventDb::as_returning())
            .get_result::<AlertEventDb>(conn)
            .map(AlertEventDb::from_db)
            .map_err(|err| map_alert_update_error(alert_id, "mark notified", err))
    })
}

pub fn upsert_rule_state(state: &AlertRuleState) -> DbResult<AlertRuleState> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        diesel::insert_into(alert_rule_state::table)
            .values(AlertRuleStateDb::to_db(state))
            .on_conflict((
                alert_rule_state::dsl::rule_key,
                alert_rule_state::dsl::scope_type,
                alert_rule_state::dsl::scope_id,
            ))
            .do_update()
            .set((
                alert_rule_state::dsl::last_evaluated_at
                    .eq(excluded(alert_rule_state::dsl::last_evaluated_at)),
                alert_rule_state::dsl::last_fired_at
                    .eq(excluded(alert_rule_state::dsl::last_fired_at)),
                alert_rule_state::dsl::last_resolved_at
                    .eq(excluded(alert_rule_state::dsl::last_resolved_at)),
                alert_rule_state::dsl::cooldown_until
                    .eq(excluded(alert_rule_state::dsl::cooldown_until)),
            ))
            .returning(AlertRuleStateDb::as_returning())
            .get_result::<AlertRuleStateDb>(conn)
            .map(AlertRuleStateDb::from_db)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to upsert alert rule state {} {}:{}: {}",
                    state.rule_key, state.scope_type, state.scope_id, err
                )))
            })
    })
}

pub fn get_rule_state(
    rule_key: &str,
    scope_type: &str,
    scope_id: &str,
) -> DbResult<Option<AlertRuleState>> {
    let conn = &mut get_connection()?;
    db_execute!(conn, {
        alert_rule_state::table
            .find((rule_key, scope_type, scope_id))
            .select(AlertRuleStateDb::as_select())
            .first::<AlertRuleStateDb>(conn)
            .optional()
            .map(|row| row.map(AlertRuleStateDb::from_db))
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to get alert rule state {} {}:{}: {}",
                    rule_key, scope_type, scope_id, err
                )))
            })
    })
}

fn map_alert_update_error(
    alert_id: i64,
    action: &'static str,
    err: diesel::result::Error,
) -> BaseError {
    if matches!(err, diesel::result::Error::NotFound) {
        BaseError::ParamInvalid(Some(format!("Alert {} not found", alert_id)))
    } else {
        BaseError::DatabaseFatal(Some(format!(
            "Failed to {} alert {}: {}",
            action, alert_id, err
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::TestDbContext;

    #[test]
    fn fire_alert_is_idempotent_and_reopens_resolved_row() {
        let context = TestDbContext::new_sqlite("alert-fire.sqlite");
        context.run_sync(|| {
            let first = fire_alert(&sample_fire_record("warning"), 1_000).unwrap();
            assert_eq!(first.status, ALERT_STATUS_ACTIVE);
            assert_eq!(first.occurrence_count, 1);
            assert_eq!(first.reopened_count, 0);

            let second = fire_alert(&sample_fire_record("critical"), 2_000).unwrap();
            assert_eq!(second.id, first.id);
            assert_eq!(second.severity, "critical");
            assert_eq!(second.occurrence_count, 2);
            assert_eq!(second.reopened_count, 0);
            assert_eq!(second.first_seen_at, 1_000);
            assert_eq!(second.last_seen_at, 2_000);

            let acknowledged =
                acknowledge_alert(second.id, Some("checking upstream".to_string()), 2_500).unwrap();
            assert_eq!(acknowledged.status, ALERT_STATUS_ACTIVE);
            assert_eq!(acknowledged.acknowledged_at, Some(2_500));

            let suppressed =
                suppress_alert(second.id, 10_000, Some("maintenance".to_string()), 2_600).unwrap();
            assert_eq!(suppressed.status, ALERT_STATUS_ACTIVE);
            assert_eq!(suppressed.suppressed_until, Some(10_000));
            let notified = mark_alert_notified(suppressed.id, 2_700).unwrap();
            assert_eq!(notified.last_notification_at, Some(2_700));

            let resolved = resolve_alert(second.id, 3_000).unwrap();
            assert_eq!(resolved.status, ALERT_STATUS_RESOLVED);
            assert_eq!(resolved.resolved_at, Some(3_000));
            assert_eq!(resolved.last_notification_at, Some(2_700));

            let reopened = fire_alert(&sample_fire_record("warning"), 4_000).unwrap();
            assert_eq!(reopened.id, first.id);
            assert_eq!(reopened.status, ALERT_STATUS_ACTIVE);
            assert_eq!(reopened.resolved_at, None);
            assert_eq!(reopened.occurrence_count, 3);
            assert_eq!(reopened.reopened_count, 1);
            assert_eq!(reopened.acknowledged_at, None);
            assert_eq!(reopened.acknowledged_note, None);
            assert_eq!(reopened.suppressed_until, Some(10_000));
            assert_eq!(reopened.last_notification_at, None);
        });
    }

    #[test]
    fn rule_state_upsert_replaces_timestamps() {
        let context = TestDbContext::new_sqlite("alert-rule-state.sqlite");
        context.run_sync(|| {
            let state = AlertRuleState {
                rule_key: "provider_open".to_string(),
                scope_type: "provider".to_string(),
                scope_id: "7".to_string(),
                last_evaluated_at: 1_000,
                last_fired_at: Some(1_000),
                last_resolved_at: None,
                cooldown_until: Some(2_000),
            };
            upsert_rule_state(&state).unwrap();

            let updated = AlertRuleState {
                last_evaluated_at: 3_000,
                last_fired_at: Some(2_500),
                last_resolved_at: Some(2_900),
                cooldown_until: None,
                ..state.clone()
            };
            let saved = upsert_rule_state(&updated).unwrap();
            assert_rule_state_eq(&saved, &updated);
            let loaded = get_rule_state("provider_open", "provider", "7")
                .unwrap()
                .expect("rule state should exist");
            assert_rule_state_eq(&loaded, &updated);
        });
    }

    fn assert_rule_state_eq(left: &AlertRuleState, right: &AlertRuleState) {
        assert_eq!(left.rule_key, right.rule_key);
        assert_eq!(left.scope_type, right.scope_type);
        assert_eq!(left.scope_id, right.scope_id);
        assert_eq!(left.last_evaluated_at, right.last_evaluated_at);
        assert_eq!(left.last_fired_at, right.last_fired_at);
        assert_eq!(left.last_resolved_at, right.last_resolved_at);
        assert_eq!(left.cooldown_until, right.cooldown_until);
    }

    fn sample_fire_record(severity: &str) -> AlertFireRecord {
        AlertFireRecord {
            fingerprint: "provider_open:provider:7".to_string(),
            rule_key: "provider_open".to_string(),
            severity: severity.to_string(),
            scope_type: "provider".to_string(),
            scope_id: "7".to_string(),
            title: "Provider is open".to_string(),
            summary: "Provider circuit is open".to_string(),
            details_json: "{}".to_string(),
            metrics_snapshot_json: Some("{\"request_count\":10}".to_string()),
        }
    }
}
