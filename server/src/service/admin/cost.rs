use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::Utc;

use crate::controller::BaseError;
use crate::cost::{CostTemplateSummary, find_template, validate_component_config};
use crate::database::cost::{
    CostCatalog, CostCatalogVersion, CostComponent, ImportedCostCatalogTemplate,
    NewCostCatalogPayload, NewCostCatalogVersionPayload, NewCostComponentPayload,
    UpdateCostCatalogData, UpdateCostCatalogVersionData, UpdateCostComponentData,
    import_cost_catalog_template,
};

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{AdminCatalogInvalidation, AdminMutationEffect, AdminMutationRunner};

#[derive(Debug, Clone, Default)]
pub struct DuplicateCostCatalogVersionInput {
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImportCostTemplateInput {
    pub template_key: String,
    pub catalog_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImportedCostTemplateResult {
    pub template: CostTemplateSummary,
    pub imported: ImportedCostCatalogTemplate,
}

pub struct CostAdminService {
    mutation_runner: Arc<AdminMutationRunner>,
}

impl CostAdminService {
    pub(crate) fn new(mutation_runner: Arc<AdminMutationRunner>) -> Self {
        Self { mutation_runner }
    }

    #[cfg(test)]
    pub(crate) fn mutation_runner(&self) -> &Arc<AdminMutationRunner> {
        &self.mutation_runner
    }

    pub async fn create_catalog(
        &self,
        payload: NewCostCatalogPayload,
    ) -> Result<CostCatalog, BaseError> {
        validate_catalog_payload(&payload)?;
        let created = CostCatalog::create(&payload)?;

        self.run_post_commit_effects(vec![AdminMutationEffect::audit(cost_catalog_audit_event(
            "create", &created,
        ))])
        .await;

        Ok(created)
    }

    pub async fn update_catalog(
        &self,
        id: i64,
        payload: UpdateCostCatalogData,
    ) -> Result<CostCatalog, BaseError> {
        validate_optional_catalog_payload(payload.name.as_deref(), payload.description.as_ref())?;
        let updated = CostCatalog::update(id, &payload)?;

        self.run_post_commit_effects(vec![AdminMutationEffect::audit(cost_catalog_audit_event(
            "update", &updated,
        ))])
        .await;

        Ok(updated)
    }

    pub async fn delete_catalog(&self, id: i64) -> Result<(), BaseError> {
        let catalog = CostCatalog::get_by_id(id)?;
        let versions = CostCatalogVersion::list_by_catalog_id(id)?;
        if !versions.is_empty() {
            return Err(BaseError::ParamInvalid(Some(
                "Cannot delete a cost catalog that still has versions".to_string(),
            )));
        }

        CostCatalog::delete(id)?;

        self.run_post_commit_effects(vec![AdminMutationEffect::audit(cost_catalog_audit_event(
            "delete", &catalog,
        ))])
        .await;

        Ok(())
    }

    pub async fn create_catalog_version(
        &self,
        payload: NewCostCatalogVersionPayload,
    ) -> Result<CostCatalogVersion, BaseError> {
        validate_new_catalog_version(&payload)?;

        let write = CostCatalogVersion::create_with_enabled_reconciliation(&payload)?;

        self.run_post_commit_effects(cost_version_effects(
            collect_version_ids(write.version.id, &write.reconciled_versions),
            cost_version_audit_event(
                "create",
                &write.version,
                Some(write.reconciled_versions.len()),
            ),
        ))
        .await;

        Ok(write.version)
    }

    pub async fn delete_version(&self, id: i64) -> Result<(), BaseError> {
        let version = CostCatalogVersion::get_by_id(id)?;
        validate_version_can_delete(&version)?;

        CostCatalogVersion::delete(id)?;

        self.run_post_commit_effects(cost_version_effects(
            vec![id],
            cost_version_audit_event("delete", &version, None),
        ))
        .await;

        Ok(())
    }

    pub async fn enable_version(&self, id: i64) -> Result<CostCatalogVersion, BaseError> {
        let original = CostCatalogVersion::get_by_id(id)?;
        validate_version_can_enable(&original)?;

        let write = CostCatalogVersion::enable_with_conflict_reconciliation(id)?;

        self.run_post_commit_effects(cost_version_effects(
            collect_version_ids(write.version.id, &write.reconciled_versions),
            cost_version_audit_event(
                "enable",
                &write.version,
                Some(write.reconciled_versions.len()),
            ),
        ))
        .await;

        Ok(write.version)
    }

    pub async fn disable_version(&self, id: i64) -> Result<CostCatalogVersion, BaseError> {
        let original = CostCatalogVersion::get_by_id(id)?;
        validate_version_can_disable(&original)?;

        let updated = CostCatalogVersion::update(
            id,
            &UpdateCostCatalogVersionData {
                is_enabled: Some(false),
                ..Default::default()
            },
        )?;

        self.run_post_commit_effects(cost_version_effects(
            vec![id],
            cost_version_audit_event("disable", &updated, None),
        ))
        .await;

        Ok(updated)
    }

    pub async fn archive_version(&self, id: i64) -> Result<CostCatalogVersion, BaseError> {
        let original = CostCatalogVersion::get_by_id(id)?;
        validate_version_can_archive(&original)?;

        let updated = CostCatalogVersion::update(
            id,
            &UpdateCostCatalogVersionData {
                is_archived: Some(true),
                ..Default::default()
            },
        )?;

        self.run_post_commit_effects(cost_version_effects(
            vec![id],
            cost_version_audit_event("archive", &updated, None),
        ))
        .await;

        Ok(updated)
    }

    pub async fn unarchive_version(&self, id: i64) -> Result<CostCatalogVersion, BaseError> {
        let original = CostCatalogVersion::get_by_id(id)?;
        validate_version_can_unarchive(&original)?;

        let updated = CostCatalogVersion::update(
            id,
            &UpdateCostCatalogVersionData {
                is_archived: Some(false),
                is_enabled: Some(false),
                ..Default::default()
            },
        )?;

        self.run_post_commit_effects(cost_version_effects(
            vec![id],
            cost_version_audit_event("unarchive", &updated, None),
        ))
        .await;

        Ok(updated)
    }

    pub async fn duplicate_version(
        &self,
        id: i64,
        payload: DuplicateCostCatalogVersionInput,
    ) -> Result<CostCatalogVersion, BaseError> {
        let requested_version = payload.version.as_deref().map(str::trim);
        if let Some(version) = requested_version
            && version.is_empty()
        {
            return Err(BaseError::ParamInvalid(Some(
                "version cannot be empty when provided".to_string(),
            )));
        }

        let duplicated = CostCatalogVersion::duplicate_as_draft(id, requested_version)?;

        self.run_post_commit_effects(cost_version_effects(
            vec![duplicated.id],
            cost_version_audit_event("duplicate", &duplicated, None),
        ))
        .await;

        Ok(duplicated)
    }

    pub async fn create_component(
        &self,
        payload: NewCostComponentPayload,
    ) -> Result<CostComponent, BaseError> {
        let version = ensure_mutable_version(payload.catalog_version_id)?;
        validate_component_payload(
            &payload.meter_key,
            &payload.charge_kind,
            payload.unit_price_nanos,
            payload.flat_fee_nanos,
            payload.tier_config_json.as_deref(),
            payload.match_attributes_json.as_deref(),
        )?;

        let created = CostComponent::create(&payload)?;

        self.run_post_commit_effects(cost_version_effects(
            vec![version.id],
            cost_component_audit_event("create", &created),
        ))
        .await;

        Ok(created)
    }

    pub async fn update_component(
        &self,
        id: i64,
        payload: UpdateCostComponentData,
    ) -> Result<CostComponent, BaseError> {
        let original = CostComponent::get_by_id(id)?;
        let version = ensure_mutable_version(original.catalog_version_id)?;

        validate_component_payload(
            payload.meter_key.as_deref().unwrap_or(&original.meter_key),
            payload
                .charge_kind
                .as_deref()
                .unwrap_or(&original.charge_kind),
            payload
                .unit_price_nanos
                .unwrap_or(original.unit_price_nanos),
            payload.flat_fee_nanos.unwrap_or(original.flat_fee_nanos),
            payload
                .tier_config_json
                .as_ref()
                .map(|value| value.as_deref())
                .unwrap_or(original.tier_config_json.as_deref()),
            payload
                .match_attributes_json
                .as_ref()
                .map(|value| value.as_deref())
                .unwrap_or(original.match_attributes_json.as_deref()),
        )?;

        let updated = CostComponent::update(id, &payload)?;

        self.run_post_commit_effects(cost_version_effects(
            vec![version.id],
            cost_component_audit_event("update", &updated),
        ))
        .await;

        Ok(updated)
    }

    pub async fn delete_component(&self, id: i64) -> Result<(), BaseError> {
        let component = CostComponent::get_by_id(id)?;
        let version = ensure_mutable_version(component.catalog_version_id)?;

        CostComponent::delete(id)?;

        self.run_post_commit_effects(cost_version_effects(
            vec![version.id],
            cost_component_audit_event("delete", &component),
        ))
        .await;

        Ok(())
    }

    pub async fn import_template(
        &self,
        input: ImportCostTemplateInput,
    ) -> Result<ImportedCostTemplateResult, BaseError> {
        if input.template_key.trim().is_empty() {
            return Err(BaseError::ParamInvalid(Some(
                "template_key cannot be empty".to_string(),
            )));
        }
        if let Some(catalog_name) = input.catalog_name.as_deref()
            && catalog_name.trim().is_empty()
        {
            return Err(BaseError::ParamInvalid(Some(
                "catalog_name cannot be empty when provided".to_string(),
            )));
        }

        let template_key = input.template_key.trim().to_string();
        let template = find_template(&template_key).ok_or_else(|| {
            BaseError::ParamInvalid(Some(format!(
                "Unknown cost template '{}'",
                input.template_key
            )))
        })?;

        let now = Utc::now();
        let import_payload = template.import_payload_at(now, input.catalog_name.as_deref());

        validate_catalog_payload(&NewCostCatalogPayload {
            name: import_payload.catalog_name.clone(),
            description: import_payload.catalog_description.clone(),
        })?;
        validate_catalog_version_request_fields(
            &import_payload.version,
            &import_payload.currency,
            import_payload.source.as_deref(),
            import_payload.effective_from,
            import_payload.effective_until,
        )?;

        if let Some(existing_catalog) = CostCatalog::get_by_name(&import_payload.catalog_name)? {
            validate_catalog_version_uniqueness(existing_catalog.id, &import_payload.version)?;
        }

        for component in &import_payload.components {
            validate_component_payload(
                &component.meter_key,
                &component.charge_kind,
                component.unit_price_nanos,
                component.flat_fee_nanos,
                component.tier_config_json.as_deref(),
                component.match_attributes_json.as_deref(),
            )?;
        }

        let imported = import_cost_catalog_template(&import_payload)?;
        let reconciled_version_count = imported.reconciled_versions.len();

        self.run_post_commit_effects(cost_version_effects(
            collect_version_ids(imported.version.id, &imported.reconciled_versions),
            cost_template_import_audit_event(&template_key, &imported, reconciled_version_count),
        ))
        .await;

        Ok(ImportedCostTemplateResult {
            template: template.summary_at(now),
            imported,
        })
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }
}

fn validate_catalog_payload(payload: &NewCostCatalogPayload) -> Result<(), BaseError> {
    validate_optional_catalog_payload(Some(payload.name.as_str()), Some(&payload.description))
}

fn validate_optional_catalog_payload(
    name: Option<&str>,
    description: Option<&Option<String>>,
) -> Result<(), BaseError> {
    if let Some(name) = name
        && name.trim().is_empty()
    {
        return Err(BaseError::ParamInvalid(Some(
            "catalog name cannot be empty".to_string(),
        )));
    }

    if let Some(Some(description)) = description
        && description.trim().is_empty()
    {
        return Err(BaseError::ParamInvalid(Some(
            "catalog description cannot be empty when provided".to_string(),
        )));
    }

    Ok(())
}

fn validate_new_catalog_version(payload: &NewCostCatalogVersionPayload) -> Result<(), BaseError> {
    validate_catalog_version_request_fields(
        &payload.version,
        &payload.currency,
        payload.source.as_deref(),
        payload.effective_from,
        payload.effective_until,
    )?;
    CostCatalog::get_by_id(payload.catalog_id)?;
    validate_catalog_version_uniqueness(payload.catalog_id, &payload.version)
}

fn validate_catalog_version_request_fields(
    version: &str,
    currency: &str,
    source: Option<&str>,
    effective_from: i64,
    effective_until: Option<i64>,
) -> Result<(), BaseError> {
    if version.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "version cannot be empty".to_string(),
        )));
    }
    if currency.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "currency cannot be empty".to_string(),
        )));
    }
    if let Some(source) = source
        && source.trim().is_empty()
    {
        return Err(BaseError::ParamInvalid(Some(
            "source cannot be empty when provided".to_string(),
        )));
    }
    if let Some(effective_until) = effective_until
        && effective_until <= effective_from
    {
        return Err(BaseError::ParamInvalid(Some(
            "effective_until must be greater than effective_from".to_string(),
        )));
    }

    Ok(())
}

fn validate_catalog_version_uniqueness(catalog_id: i64, version: &str) -> Result<(), BaseError> {
    let existing_versions = CostCatalogVersion::list_by_catalog_id(catalog_id)?;
    if existing_versions
        .iter()
        .any(|existing_version| existing_version.version == version)
    {
        return Err(BaseError::DatabaseDup(Some(format!(
            "Version '{}' already exists for catalog {}",
            version, catalog_id
        ))));
    }

    Ok(())
}

fn ensure_mutable_version(catalog_version_id: i64) -> Result<CostCatalogVersion, BaseError> {
    let version = CostCatalogVersion::get_by_id(catalog_version_id)?;
    validate_version_is_mutable(&version)?;
    Ok(version)
}

fn validate_version_is_mutable(version: &CostCatalogVersion) -> Result<(), BaseError> {
    if version.is_archived {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Cost catalog version {} is archived and cannot be modified",
            version.id
        ))));
    }
    if version.is_frozen() {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Cost catalog version {} has already been used by request logs and is read-only",
            version.id
        ))));
    }
    Ok(())
}

fn validate_version_can_enable(version: &CostCatalogVersion) -> Result<(), BaseError> {
    if version.is_archived {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Cost catalog version {} is archived and cannot be enabled",
            version.id
        ))));
    }
    if version.is_enabled {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Cost catalog version {} is already enabled",
            version.id
        ))));
    }
    Ok(())
}

fn validate_version_can_disable(version: &CostCatalogVersion) -> Result<(), BaseError> {
    if version.is_archived {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Cost catalog version {} is archived and cannot be disabled",
            version.id
        ))));
    }
    if !version.is_enabled {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Cost catalog version {} is already disabled",
            version.id
        ))));
    }
    Ok(())
}

fn validate_version_can_archive(version: &CostCatalogVersion) -> Result<(), BaseError> {
    if !version.can_be_archived() {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Cost catalog version {} can only be archived after it is frozen and disabled",
            version.id
        ))));
    }
    Ok(())
}

fn validate_version_can_unarchive(version: &CostCatalogVersion) -> Result<(), BaseError> {
    if !version.is_archived {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Cost catalog version {} is not archived",
            version.id
        ))));
    }
    if !version.is_frozen() {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Cost catalog version {} must remain frozen when unarchived",
            version.id
        ))));
    }
    Ok(())
}

fn validate_version_can_delete(version: &CostCatalogVersion) -> Result<(), BaseError> {
    if version.is_archived {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Cost catalog version {} is archived and cannot be deleted",
            version.id
        ))));
    }
    if version.is_enabled {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Cost catalog version {} is enabled and cannot be deleted",
            version.id
        ))));
    }
    if version.is_frozen() {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Cost catalog version {} has already been used by request logs and cannot be deleted",
            version.id
        ))));
    }
    Ok(())
}

fn validate_component_payload(
    meter_key: &str,
    charge_kind: &str,
    unit_price_nanos: Option<i64>,
    flat_fee_nanos: Option<i64>,
    tier_config_json: Option<&str>,
    match_attributes_json: Option<&str>,
) -> Result<(), BaseError> {
    validate_component_config(
        meter_key,
        charge_kind,
        unit_price_nanos,
        flat_fee_nanos,
        tier_config_json,
        match_attributes_json,
    )
}

fn collect_version_ids(version_id: i64, reconciled_versions: &[CostCatalogVersion]) -> Vec<i64> {
    std::iter::once(version_id)
        .chain(reconciled_versions.iter().map(|version| version.id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn cost_version_effects(
    version_ids: Vec<i64>,
    audit_event: AdminAuditEvent,
) -> Vec<AdminMutationEffect> {
    vec![
        AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::CostCatalogVersions {
            ids: version_ids,
        }),
        AdminMutationEffect::audit(audit_event),
    ]
}

fn cost_catalog_audit_event(action: &'static str, catalog: &CostCatalog) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.cost_catalog_created",
        "update" => "manager.cost_catalog_updated",
        "delete" => "manager.cost_catalog_deleted",
        _ => unreachable!("unsupported cost catalog audit action: {action}"),
    };

    AdminAuditEvent::with_fields(
        event_name,
        [
            AdminAuditField::new("action", action),
            AdminAuditField::new("cost_catalog_id", catalog.id),
            AdminAuditField::new("cost_catalog_name", &catalog.name),
        ],
    )
}

fn cost_version_audit_event(
    action: &'static str,
    version: &CostCatalogVersion,
    reconciled_version_count: Option<usize>,
) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.cost_catalog_version_created",
        "delete" => "manager.cost_catalog_version_deleted",
        "enable" => "manager.cost_catalog_version_enabled",
        "disable" => "manager.cost_catalog_version_disabled",
        "archive" => "manager.cost_catalog_version_archived",
        "unarchive" => "manager.cost_catalog_version_unarchived",
        "duplicate" => "manager.cost_catalog_version_duplicated",
        _ => unreachable!("unsupported cost version audit action: {action}"),
    };

    let mut fields = vec![
        AdminAuditField::new("action", action),
        AdminAuditField::new("cost_catalog_version_id", version.id),
        AdminAuditField::new("cost_catalog_id", version.catalog_id),
        AdminAuditField::new("version", &version.version),
        AdminAuditField::new("currency", &version.currency),
        AdminAuditField::new("is_enabled", version.is_enabled),
        AdminAuditField::new("is_archived", version.is_archived),
    ];
    fields.extend(AdminAuditField::optional(
        "reconciled_version_count",
        reconciled_version_count,
    ));
    AdminAuditEvent::with_fields(event_name, fields)
}

fn cost_component_audit_event(action: &'static str, component: &CostComponent) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.cost_component_created",
        "update" => "manager.cost_component_updated",
        "delete" => "manager.cost_component_deleted",
        _ => unreachable!("unsupported cost component audit action: {action}"),
    };

    AdminAuditEvent::with_fields(
        event_name,
        [
            AdminAuditField::new("action", action),
            AdminAuditField::new("cost_component_id", component.id),
            AdminAuditField::new("cost_catalog_version_id", component.catalog_version_id),
            AdminAuditField::new("meter_key", &component.meter_key),
            AdminAuditField::new("charge_kind", &component.charge_kind),
            AdminAuditField::new("priority", component.priority),
        ],
    )
}

fn cost_template_import_audit_event(
    template_key: &str,
    imported: &ImportedCostCatalogTemplate,
    reconciled_version_count: usize,
) -> AdminAuditEvent {
    AdminAuditEvent::with_fields(
        "manager.cost_template_imported",
        [
            AdminAuditField::new("action", "import"),
            AdminAuditField::new("template_key", template_key),
            AdminAuditField::new("cost_catalog_id", imported.catalog.id),
            AdminAuditField::new("cost_catalog_name", &imported.catalog.name),
            AdminAuditField::new("cost_catalog_version_id", imported.version.id),
            AdminAuditField::new("version", &imported.version.version),
            AdminAuditField::new("component_count", imported.components.len()),
            AdminAuditField::new("created_catalog", imported.created_catalog),
            AdminAuditField::new("reconciled_version_count", reconciled_version_count),
        ],
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diesel::connection::SimpleConnection;

    use crate::database::cost::{
        CostCatalog, CostCatalogVersion, NewCostCatalogPayload, NewCostCatalogVersionPayload,
        NewCostComponentPayload, UpdateCostCatalogData, UpdateCostCatalogVersionData,
        UpdateCostComponentData,
    };
    use crate::database::{DbConnection, TestDbContext, get_connection};
    use crate::service::app_state::create_test_app_state;

    use super::{
        CostAdminService, DuplicateCostCatalogVersionInput, ImportCostTemplateInput,
        validate_new_catalog_version, validate_version_can_archive, validate_version_can_delete,
        validate_version_can_disable, validate_version_can_enable, validate_version_can_unarchive,
        validate_version_is_mutable,
    };

    fn seed_catalog(name: &str) -> CostCatalog {
        CostCatalog::create(&NewCostCatalogPayload {
            name: name.to_string(),
            description: Some("seed".to_string()),
        })
        .expect("catalog seed should succeed")
    }

    fn seed_version(
        catalog_id: i64,
        version: &str,
        effective_from: i64,
        effective_until: Option<i64>,
        is_enabled: bool,
    ) -> CostCatalogVersion {
        CostCatalogVersion::create(&NewCostCatalogVersionPayload {
            catalog_id,
            version: version.to_string(),
            currency: "USD".to_string(),
            source: Some("seed".to_string()),
            effective_from,
            effective_until,
            is_enabled,
        })
        .expect("version seed should succeed")
    }

    fn freeze_version(version_id: i64) -> CostCatalogVersion {
        CostCatalogVersion::update(
            version_id,
            &UpdateCostCatalogVersionData {
                first_used_at: Some(Some(111)),
                ..Default::default()
            },
        )
        .expect("freeze should succeed")
    }

    fn install_sqlite_cost_version_reconcile_failure_trigger(version_id: i64) {
        let mut connection = get_connection().expect("test connection should open");
        let DbConnection::Sqlite(conn) = &mut connection else {
            panic!("cost version reconcile rollback test requires sqlite");
        };

        conn.batch_execute(&format!(
            "
            CREATE TRIGGER fail_cost_version_reconcile_{version_id}
            BEFORE UPDATE ON cost_catalog_versions
            WHEN NEW.id = {version_id}
                AND (
                    NEW.effective_until IS NOT OLD.effective_until
                    OR NEW.is_enabled IS NOT OLD.is_enabled
                )
            BEGIN
                SELECT RAISE(ABORT, 'forced cost version reconcile failure');
            END;
            "
        ))
        .expect("cost version reconcile failure trigger should install");
    }

    fn service(app_state: &Arc<crate::service::app_state::AppState>) -> &CostAdminService {
        app_state.admin.cost.as_ref()
    }

    fn version_for_mutability_tests(
        id: i64,
        first_used_at: Option<i64>,
        is_archived: bool,
    ) -> CostCatalogVersion {
        CostCatalogVersion {
            id,
            catalog_id: 1,
            version: format!("v{}", id),
            currency: "USD".to_string(),
            source: None,
            effective_from: 0,
            effective_until: None,
            first_used_at,
            is_archived,
            is_enabled: false,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn validation_rejects_invalid_cost_version_and_mutation_states() {
        let err = validate_new_catalog_version(&NewCostCatalogVersionPayload {
            catalog_id: 1,
            version: " ".to_string(),
            currency: "".to_string(),
            source: None,
            effective_from: 100,
            effective_until: None,
            is_enabled: false,
        })
        .expect_err("empty version should fail");
        assert!(matches!(
            err,
            crate::controller::BaseError::ParamInvalid(Some(message))
                if message.contains("version cannot be empty")
        ));

        let frozen = version_for_mutability_tests(1, Some(100), false);
        let archived = version_for_mutability_tests(2, None, true);
        assert!(validate_version_is_mutable(&frozen).is_err());
        assert!(validate_version_is_mutable(&archived).is_err());

        let mut enabled = version_for_mutability_tests(3, None, false);
        enabled.is_enabled = true;
        assert!(validate_version_can_enable(&archived).is_err());
        assert!(
            validate_version_can_disable(&version_for_mutability_tests(4, None, false)).is_err()
        );
        assert!(validate_version_can_archive(&enabled).is_err());
        assert!(
            validate_version_can_unarchive(&version_for_mutability_tests(5, Some(100), false))
                .is_err()
        );
        assert!(validate_version_can_delete(&enabled).is_err());
    }

    #[tokio::test]
    async fn catalog_lifecycle_and_version_create_refresh_caches() {
        let test_db_context = TestDbContext::new_sqlite("admin-cost-catalog-version.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let created_catalog = service(&app_state)
                    .create_catalog(NewCostCatalogPayload {
                        name: "Google / Gemini".to_string(),
                        description: Some("Gemini pricing".to_string()),
                    })
                    .await
                    .expect("catalog create should succeed");
                let updated_catalog = service(&app_state)
                    .update_catalog(
                        created_catalog.id,
                        UpdateCostCatalogData {
                            name: Some("Google / Gemini Updated".to_string()),
                            description: Some(Some("Updated".to_string())),
                        },
                    )
                    .await
                    .expect("catalog update should succeed");
                assert_eq!(updated_catalog.name, "Google / Gemini Updated");

                let existing = seed_version(created_catalog.id, "2026-04-01", 0, None, true);
                let existing_cached_before = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(existing.id)
                    .await
                    .expect("existing version cache should load")
                    .expect("existing version should exist");
                assert_eq!(existing_cached_before.effective_until, None);

                let created_version = service(&app_state)
                    .create_catalog_version(NewCostCatalogVersionPayload {
                        catalog_id: created_catalog.id,
                        version: "2026-05-01".to_string(),
                        currency: "USD".to_string(),
                        source: Some("manual".to_string()),
                        effective_from: 2_000,
                        effective_until: None,
                        is_enabled: true,
                    })
                    .await
                    .expect("version create should succeed");

                let existing_cached_after = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(existing.id)
                    .await
                    .expect("existing version cache should reload")
                    .expect("existing version should still exist");
                let created_cached = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(created_version.id)
                    .await
                    .expect("created version cache should load")
                    .expect("created version should exist");

                assert_eq!(existing_cached_after.effective_until, Some(2_000));
                assert_eq!(created_cached.id, created_version.id);
                assert!(created_cached.is_enabled);

                service(&app_state)
                    .delete_catalog(created_catalog.id)
                    .await
                    .expect_err("catalog with versions should not delete");
            })
            .await;
    }

    #[tokio::test]
    async fn enable_version_reconciles_conflicts_and_refreshes_all_affected_caches() {
        let test_db_context = TestDbContext::new_sqlite("admin-cost-enable-reconcile-cache.sqlite");

        test_db_context
            .run_async(async {
                let catalog = seed_catalog("Enable Reconcile Cache");
                let existing = seed_version(catalog.id, "2026-04-01", 0, None, true);
                let draft = seed_version(catalog.id, "2026-05-01", 2_000, None, false);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let existing_cached_before = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(existing.id)
                    .await
                    .expect("existing cache should load")
                    .expect("existing version should exist");
                let draft_cached_before = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(draft.id)
                    .await
                    .expect("draft cache should load")
                    .expect("draft version should exist");
                assert_eq!(existing_cached_before.effective_until, None);
                assert!(!draft_cached_before.is_enabled);

                let enabled = service(&app_state)
                    .enable_version(draft.id)
                    .await
                    .expect("enable should succeed");
                assert!(enabled.is_enabled);

                let existing_cached_after = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(existing.id)
                    .await
                    .expect("existing cache should reload")
                    .expect("existing version should still exist");
                let draft_cached_after = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(draft.id)
                    .await
                    .expect("draft cache should reload")
                    .expect("draft version should exist");

                assert_eq!(existing_cached_after.effective_until, Some(2_000));
                assert!(draft_cached_after.is_enabled);
            })
            .await;
    }

    #[tokio::test]
    async fn create_enabled_version_rolls_back_when_conflict_reconcile_fails() {
        let test_db_context =
            TestDbContext::new_sqlite("admin-cost-create-reconcile-rollback.sqlite");

        test_db_context
            .run_async(async {
                let catalog = seed_catalog("Create Reconcile Rollback");
                let existing = seed_version(catalog.id, "2026-04-01", 0, None, true);
                install_sqlite_cost_version_reconcile_failure_trigger(existing.id);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let err = service(&app_state)
                    .create_catalog_version(NewCostCatalogVersionPayload {
                        catalog_id: catalog.id,
                        version: "rollback-create".to_string(),
                        currency: "USD".to_string(),
                        source: Some("manual".to_string()),
                        effective_from: 2_000,
                        effective_until: None,
                        is_enabled: true,
                    })
                    .await
                    .expect_err("create should fail when conflict reconcile fails");
                let message = format!("{err:?}");
                assert!(
                    message.contains("forced cost version reconcile failure"),
                    "unexpected error: {message}"
                );

                let existing_after = CostCatalogVersion::get_by_id(existing.id)
                    .expect("existing version should still load");
                let versions = CostCatalogVersion::list_by_catalog_id(catalog.id)
                    .expect("versions should still list");

                assert!(existing_after.is_enabled);
                assert_eq!(existing_after.effective_until, None);
                assert!(
                    !versions
                        .iter()
                        .any(|version| version.version == "rollback-create")
                );
            })
            .await;
    }

    #[tokio::test]
    async fn enable_version_rolls_back_when_conflict_reconcile_fails() {
        let test_db_context =
            TestDbContext::new_sqlite("admin-cost-enable-reconcile-rollback.sqlite");

        test_db_context
            .run_async(async {
                let catalog = seed_catalog("Enable Reconcile Rollback");
                let existing = seed_version(catalog.id, "2026-04-01", 0, None, true);
                let draft = seed_version(catalog.id, "2026-05-01", 2_000, None, false);
                install_sqlite_cost_version_reconcile_failure_trigger(existing.id);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let err = service(&app_state)
                    .enable_version(draft.id)
                    .await
                    .expect_err("enable should fail when conflict reconcile fails");
                let message = format!("{err:?}");
                assert!(
                    message.contains("forced cost version reconcile failure"),
                    "unexpected error: {message}"
                );

                let existing_after = CostCatalogVersion::get_by_id(existing.id)
                    .expect("existing version should still load");
                let draft_after =
                    CostCatalogVersion::get_by_id(draft.id).expect("draft should still load");

                assert!(existing_after.is_enabled);
                assert_eq!(existing_after.effective_until, None);
                assert!(!draft_after.is_enabled);
            })
            .await;
    }

    #[tokio::test]
    async fn version_actions_and_component_lifecycle_refresh_version_cache() {
        let test_db_context = TestDbContext::new_sqlite("admin-cost-version-actions.sqlite");

        test_db_context
            .run_async(async {
                let catalog = seed_catalog("OpenAI / GPT");
                let draft = seed_version(catalog.id, "draft", 0, None, false);
                let frozen =
                    freeze_version(seed_version(catalog.id, "frozen", 100, None, false).id);
                let deletable = seed_version(catalog.id, "delete-me", 200, None, false);
                let component_version = seed_version(catalog.id, "component", 300, None, false);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let enabled = service(&app_state)
                    .enable_version(draft.id)
                    .await
                    .expect("enable should succeed");
                assert!(enabled.is_enabled);
                let enabled_cached = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(draft.id)
                    .await
                    .expect("enabled cache should load")
                    .expect("enabled version should exist");
                assert!(enabled_cached.is_enabled);

                let disabled = service(&app_state)
                    .disable_version(draft.id)
                    .await
                    .expect("disable should succeed");
                assert!(!disabled.is_enabled);
                let disabled_cached = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(draft.id)
                    .await
                    .expect("disabled cache should load")
                    .expect("disabled version should exist");
                assert!(!disabled_cached.is_enabled);

                let archived = service(&app_state)
                    .archive_version(frozen.id)
                    .await
                    .expect("archive should succeed");
                assert!(archived.is_archived);
                let archived_cached = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(frozen.id)
                    .await
                    .expect("archived cache should load")
                    .expect("archived version should exist");
                assert!(!archived_cached.is_enabled);

                let unarchived = service(&app_state)
                    .unarchive_version(frozen.id)
                    .await
                    .expect("unarchive should succeed");
                assert!(!unarchived.is_archived);
                let unarchived_cached = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(frozen.id)
                    .await
                    .expect("unarchived cache should load")
                    .expect("unarchived version should exist");
                assert!(!unarchived_cached.is_enabled);

                let duplicated = service(&app_state)
                    .duplicate_version(draft.id, DuplicateCostCatalogVersionInput { version: None })
                    .await
                    .expect("duplicate should succeed");
                let duplicated_cached = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(duplicated.id)
                    .await
                    .expect("duplicated cache should load")
                    .expect("duplicated version should exist");
                assert_eq!(duplicated_cached.version, "draft Copy");

                service(&app_state)
                    .delete_version(deletable.id)
                    .await
                    .expect("delete version should succeed");
                let deleted_cached = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(deletable.id)
                    .await
                    .expect("deleted cache lookup should succeed");
                assert!(deleted_cached.is_none());

                let component_cached_before = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(component_version.id)
                    .await
                    .expect("component version cache should load")
                    .expect("component version should exist");
                assert!(component_cached_before.components.is_empty());

                let created_component = service(&app_state)
                    .create_component(NewCostComponentPayload {
                        catalog_version_id: component_version.id,
                        meter_key: "llm.input_text_tokens".to_string(),
                        charge_kind: "per_unit".to_string(),
                        unit_price_nanos: Some(2_500),
                        flat_fee_nanos: None,
                        tier_config_json: None,
                        match_attributes_json: None,
                        priority: 100,
                        description: Some("input".to_string()),
                    })
                    .await
                    .expect("component create should succeed");
                let component_cached_after_create = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(component_version.id)
                    .await
                    .expect("component version cache should reload after create")
                    .expect("component version should exist");
                assert_eq!(component_cached_after_create.components.len(), 1);

                let updated_component = service(&app_state)
                    .update_component(
                        created_component.id,
                        UpdateCostComponentData {
                            meter_key: Some("llm.output_text_tokens".to_string()),
                            description: Some(Some("output".to_string())),
                            ..Default::default()
                        },
                    )
                    .await
                    .expect("component update should succeed");
                assert_eq!(updated_component.meter_key, "llm.output_text_tokens");
                let component_cached_after_update = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(component_version.id)
                    .await
                    .expect("component version cache should reload after update")
                    .expect("component version should exist");
                assert_eq!(
                    component_cached_after_update.components[0].meter_key,
                    "llm.output_text_tokens"
                );

                service(&app_state)
                    .delete_component(created_component.id)
                    .await
                    .expect("component delete should succeed");
                let component_cached_after_delete = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(component_version.id)
                    .await
                    .expect("component version cache should reload after delete")
                    .expect("component version should exist");
                assert!(component_cached_after_delete.components.is_empty());
            })
            .await;
    }

    #[tokio::test]
    async fn template_import_invalidates_new_and_reconciled_versions() {
        let test_db_context = TestDbContext::new_sqlite("admin-cost-template-import.sqlite");

        test_db_context
            .run_async(async {
                let catalog = seed_catalog("Google / Gemini 2.5 Pro");
                let existing = seed_version(catalog.id, "2026-04-01", 0, None, true);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let existing_cached_before = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(existing.id)
                    .await
                    .expect("existing version cache should load")
                    .expect("existing version should exist");
                assert_eq!(existing_cached_before.effective_until, None);

                let imported = service(&app_state)
                    .import_template(ImportCostTemplateInput {
                        template_key: "google.gemini-2.5-pro.text".to_string(),
                        catalog_name: None,
                    })
                    .await
                    .expect("template import should succeed");

                let imported_cached = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(imported.imported.version.id)
                    .await
                    .expect("imported version cache should load")
                    .expect("imported version should exist");
                let existing_cached_after = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(existing.id)
                    .await
                    .expect("existing version cache should reload")
                    .expect("existing version should still exist");

                assert_eq!(imported.template.key, "google.gemini-2.5-pro.text");
                assert!(!imported.imported.components.is_empty());
                assert_eq!(imported.imported.reconciled_versions.len(), 1);
                assert_eq!(
                    existing_cached_after.effective_until,
                    Some(imported.imported.version.effective_from)
                );
                assert_eq!(imported_cached.id, imported.imported.version.id);
                assert_eq!(
                    imported_cached.components.len(),
                    imported.imported.components.len()
                );
            })
            .await;
    }

    #[tokio::test]
    async fn catalog_without_versions_can_be_deleted() {
        let test_db_context = TestDbContext::new_sqlite("admin-cost-catalog-delete.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let catalog = service(&app_state)
                    .create_catalog(NewCostCatalogPayload {
                        name: "Delete Me".to_string(),
                        description: None,
                    })
                    .await
                    .expect("catalog create should succeed");

                service(&app_state)
                    .delete_catalog(catalog.id)
                    .await
                    .expect("catalog delete should succeed");
                assert!(CostCatalog::get_by_id(catalog.id).is_err());
            })
            .await;
    }
}
