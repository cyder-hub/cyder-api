use std::sync::{Arc, Mutex};

use crate::config::AlertsConfig;
use crate::proxy::logging::LogManagerMetricsSnapshot;
use crate::service::app_state::AppState;

use super::types::AlertEvaluationTickResult;

#[derive(Debug, Clone)]
pub struct AlertsService {
    config: AlertsConfig,
    last_log_manager_metrics: Arc<Mutex<Option<LogManagerMetricsSnapshot>>>,
}

impl AlertsService {
    pub fn new(config: AlertsConfig) -> Self {
        Self {
            config,
            last_log_manager_metrics: Arc::new(Mutex::new(None)),
        }
    }

    pub fn config(&self) -> &AlertsConfig {
        &self.config
    }

    pub(crate) fn consume_log_manager_metrics_delta(
        &self,
        current: LogManagerMetricsSnapshot,
    ) -> LogManagerMetricsSnapshot {
        let mut guard = self
            .last_log_manager_metrics
            .lock()
            .expect("alert log manager metrics state should not be poisoned");
        let previous = guard.clone().unwrap_or_default();
        *guard = Some(current.clone());
        super::rules::log_manager_metrics_delta(&current, &previous)
    }

    pub async fn tick_evaluation_worker(
        &self,
        app_state: &Arc<AppState>,
    ) -> AlertEvaluationTickResult {
        if !self.config.enabled {
            return AlertEvaluationTickResult::default();
        }

        match self
            .evaluate_all(app_state, chrono::Utc::now().timestamp_millis())
            .await
        {
            Ok(result) => result,
            Err(err) => {
                crate::error_event!(
                    "alerts.evaluation_worker_failed",
                    error = format!("{err:?}")
                );
                AlertEvaluationTickResult {
                    failed: 1,
                    ..AlertEvaluationTickResult::default()
                }
            }
        }
    }
}
