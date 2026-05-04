use chrono::Utc;
use reqwest::Url;

use crate::controller::BaseError;
use crate::database::notification::{
    NOTIFICATION_CHANNEL_TYPE_WEBHOOK, NewNotificationChannel, NotificationChannel,
    UpdateNotificationChannel, create_channel, delete_channel, get_channel, list_channels,
    update_channel,
};

use super::service::NotificationService;
use super::types::{
    CreateNotificationChannelInput, NotificationChannelResponse, UpdateNotificationChannelInput,
};
use super::webhook::normalize_headers_json;

impl NotificationService {
    pub fn create_channel(
        &self,
        input: CreateNotificationChannelInput,
    ) -> Result<NotificationChannelResponse, BaseError> {
        validate_channel_key(&input.channel_key)?;
        validate_webhook_url(&input.endpoint_url)?;
        let headers_json = normalize_headers_json(input.headers_json)?;
        let cooldown_seconds = normalize_cooldown_seconds(
            input.cooldown_seconds,
            self.default_channel_cooldown_seconds(),
        )?;
        let now_ms = Utc::now().timestamp_millis();
        let channel = create_channel(
            &NewNotificationChannel {
                channel_key: input.channel_key,
                channel_type: NOTIFICATION_CHANNEL_TYPE_WEBHOOK.to_string(),
                name: input.name,
                endpoint_url: input.endpoint_url,
                signing_secret: normalize_secret(input.signing_secret),
                headers_json,
                cooldown_seconds,
                is_enabled: input.is_enabled,
            },
            now_ms,
        )?;
        Ok(redact_channel(channel))
    }

    pub fn update_channel(
        &self,
        channel_id: i64,
        input: UpdateNotificationChannelInput,
    ) -> Result<NotificationChannelResponse, BaseError> {
        if let Some(endpoint_url) = input.endpoint_url.as_deref() {
            validate_webhook_url(endpoint_url)?;
        }
        let signing_secret = if input.clear_signing_secret {
            Some(None)
        } else {
            input
                .signing_secret
                .map(|value| normalize_secret(Some(value)))
        };
        let headers_json = if input.clear_headers {
            Some(None)
        } else {
            input
                .headers_json
                .map(|value| normalize_headers_json(Some(value)))
                .transpose()?
        };
        let cooldown_seconds = input
            .cooldown_seconds
            .map(|value| {
                normalize_cooldown_seconds(Some(value), self.default_channel_cooldown_seconds())
            })
            .transpose()?;
        let channel = update_channel(
            channel_id,
            &UpdateNotificationChannel {
                name: input.name,
                endpoint_url: input.endpoint_url,
                signing_secret,
                headers_json,
                cooldown_seconds,
                is_enabled: input.is_enabled,
            },
            Utc::now().timestamp_millis(),
        )?;
        Ok(redact_channel(channel))
    }

    pub fn delete_channel(
        &self,
        channel_id: i64,
    ) -> Result<NotificationChannelResponse, BaseError> {
        Ok(redact_channel(delete_channel(
            channel_id,
            Utc::now().timestamp_millis(),
        )?))
    }

    pub fn get_channel(&self, channel_id: i64) -> Result<NotificationChannelResponse, BaseError> {
        Ok(redact_channel(get_channel(channel_id)?))
    }

    pub fn get_channel_secret(&self, channel_id: i64) -> Result<NotificationChannel, BaseError> {
        get_channel(channel_id)
    }

    pub fn list_channels(&self) -> Result<Vec<NotificationChannelResponse>, BaseError> {
        Ok(list_channels(false)?
            .into_iter()
            .map(redact_channel)
            .collect())
    }
}

pub fn validate_webhook_url(endpoint_url: &str) -> Result<(), BaseError> {
    let url = Url::parse(endpoint_url).map_err(|_| {
        BaseError::ParamInvalid(Some("endpoint_url must be a valid http(s) URL".to_string()))
    })?;
    match url.scheme() {
        "http" | "https" => Ok(()),
        _ => Err(BaseError::ParamInvalid(Some(
            "endpoint_url must use http or https".to_string(),
        ))),
    }
}

fn validate_channel_key(channel_key: &str) -> Result<(), BaseError> {
    let valid = !channel_key.is_empty()
        && channel_key.len() <= 64
        && channel_key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-');
    if valid {
        Ok(())
    } else {
        Err(BaseError::ParamInvalid(Some(
            "channel_key must be 1-64 ASCII letters, numbers, '_' or '-'".to_string(),
        )))
    }
}

fn normalize_secret(secret: Option<String>) -> Option<String> {
    secret.and_then(|value| {
        let trimmed = value.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn normalize_cooldown_seconds(
    cooldown_seconds: Option<i64>,
    default_cooldown_seconds: u64,
) -> Result<i64, BaseError> {
    let value = cooldown_seconds.unwrap_or(default_cooldown_seconds as i64);
    if (0..=86_400).contains(&value) {
        Ok(value)
    } else {
        Err(BaseError::ParamInvalid(Some(
            "cooldown_seconds must be between 0 and 86400".to_string(),
        )))
    }
}

pub fn redact_channel(channel: NotificationChannel) -> NotificationChannelResponse {
    NotificationChannelResponse {
        id: channel.id,
        channel_key: channel.channel_key,
        channel_type: channel.channel_type,
        name: channel.name,
        endpoint_url: channel.endpoint_url,
        signing_secret_redacted: channel.signing_secret.map(|_| "********".to_string()),
        headers_json: channel.headers_json,
        cooldown_seconds: channel.cooldown_seconds,
        is_enabled: channel.is_enabled,
        last_test_at: channel.last_test_at,
        last_test_success: channel.last_test_success,
        last_test_error: channel.last_test_error,
        created_at: channel.created_at,
        updated_at: channel.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NotificationConfig;

    #[test]
    fn validate_webhook_url_rejects_non_http_schemes() {
        assert!(validate_webhook_url("https://example.com/hook").is_ok());
        assert!(validate_webhook_url("http://localhost:8080/hook").is_ok());
        assert!(validate_webhook_url("ftp://example.com/hook").is_err());
        assert!(validate_webhook_url("not a url").is_err());
    }

    #[test]
    fn channel_response_redacts_secret() {
        let response = redact_channel(NotificationChannel {
            id: 1,
            channel_key: "ops".to_string(),
            channel_type: NOTIFICATION_CHANNEL_TYPE_WEBHOOK.to_string(),
            name: "Ops".to_string(),
            endpoint_url: "https://example.com".to_string(),
            signing_secret: Some("secret".to_string()),
            headers_json: Some(r#"{"X-Ops":"primary"}"#.to_string()),
            cooldown_seconds: 900,
            is_enabled: true,
            last_test_at: None,
            last_test_success: None,
            last_test_error: None,
            deleted_at: None,
            created_at: 1,
            updated_at: 1,
        });

        assert_eq!(
            response.signing_secret_redacted.as_deref(),
            Some("********")
        );
        assert_eq!(
            response.headers_json.as_deref(),
            Some(r#"{"X-Ops":"primary"}"#)
        );
        let service = NotificationService::new(NotificationConfig::default());
        assert!(service.config().enabled);
    }

    #[test]
    fn channel_headers_reject_reserved_names() {
        assert!(
            normalize_headers_json(Some(r#"{"X-Cyder-Event":"override"}"#.to_string())).is_err()
        );
        assert!(
            normalize_headers_json(Some(r#"{"X-Ops":"primary"}"#.to_string()))
                .unwrap()
                .is_some()
        );
    }
}
