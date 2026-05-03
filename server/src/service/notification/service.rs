use crate::config::NotificationConfig;

use super::types::NotificationWorkerTickResult;

#[derive(Debug, Clone)]
pub struct NotificationService {
    config: NotificationConfig,
    default_channel_cooldown_seconds: u64,
}

impl NotificationService {
    pub fn new(config: NotificationConfig) -> Self {
        Self::new_with_default_channel_cooldown_seconds(config, 900)
    }

    pub fn new_with_default_channel_cooldown_seconds(
        config: NotificationConfig,
        default_channel_cooldown_seconds: u64,
    ) -> Self {
        Self {
            config,
            default_channel_cooldown_seconds,
        }
    }

    pub fn config(&self) -> &NotificationConfig {
        &self.config
    }

    pub fn default_channel_cooldown_seconds(&self) -> u64 {
        self.default_channel_cooldown_seconds
    }

    pub async fn tick_delivery_worker(
        &self,
        client: &reqwest::Client,
    ) -> NotificationWorkerTickResult {
        match self
            .process_due_deliveries(client, chrono::Utc::now().timestamp_millis(), 100)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                crate::error_event!(
                    "notification.delivery_worker_failed",
                    error = format!("{err:?}")
                );
                NotificationWorkerTickResult {
                    failed: 1,
                    ..NotificationWorkerTickResult::default()
                }
            }
        }
    }
}
