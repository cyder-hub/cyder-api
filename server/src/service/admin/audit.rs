use crate::logging::event_message_with_fields;
use cyder_tools::log::info;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminAuditField {
    key: &'static str,
    value: String,
}

impl AdminAuditField {
    pub fn new<T>(key: &'static str, value: T) -> Self
    where
        T: ToString,
    {
        Self {
            key,
            value: value.to_string(),
        }
    }

    pub fn optional<T>(key: &'static str, value: Option<T>) -> Option<Self>
    where
        T: ToString,
    {
        value.map(|value| Self::new(key, value))
    }

    pub fn key(&self) -> &'static str {
        self.key
    }

    pub fn value(&self) -> &str {
        self.value.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminAuditEvent {
    event_name: &'static str,
    fields: Vec<AdminAuditField>,
}

impl AdminAuditEvent {
    pub fn new(event_name: &'static str) -> Self {
        Self {
            event_name,
            fields: Vec::new(),
        }
    }

    pub fn with_fields(
        event_name: &'static str,
        fields: impl IntoIterator<Item = AdminAuditField>,
    ) -> Self {
        Self {
            event_name,
            fields: fields.into_iter().collect(),
        }
    }

    pub fn event_name(&self) -> &'static str {
        self.event_name
    }

    pub fn fields(&self) -> &[AdminAuditField] {
        &self.fields
    }
}

#[derive(Default)]
pub struct AdminAuditLogger;

impl AdminAuditLogger {
    pub fn emit(&self, event: &AdminAuditEvent) {
        let fields = event
            .fields()
            .iter()
            .map(|field| (field.key(), Some(field.value().to_string())))
            .collect::<Vec<_>>();
        info!("{}", event_message_with_fields(event.event_name(), &fields));
    }
}

#[cfg(test)]
mod tests {
    use super::{AdminAuditEvent, AdminAuditField};

    #[test]
    fn audit_event_collects_required_and_optional_fields() {
        let mut fields = vec![
            AdminAuditField::new("provider_id", 7),
            AdminAuditField::new("provider_key", "openai"),
        ];
        fields.extend(AdminAuditField::optional("description", Some("production")));
        fields.extend(AdminAuditField::optional::<&str>("skipped", None));
        let event = AdminAuditEvent::with_fields("manager.provider_created", fields);

        assert_eq!(event.event_name(), "manager.provider_created");
        assert_eq!(event.fields().len(), 3);
        assert_eq!(event.fields()[0].key(), "provider_id");
        assert_eq!(event.fields()[0].value(), "7");
        assert_eq!(event.fields()[2].key(), "description");
        assert_eq!(event.fields()[2].value(), "production");
    }
}
