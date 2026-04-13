use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    routing::{get, post, put},
};
use chrono::Utc;
use cyder_tools::log::warn;
use serde::{Deserialize, Serialize};

use crate::{
    controller::BaseError,
    cost::{
        CostLedger, CostRatingContext, CostRatingResult, CostTemplateSummary, UsageNormalization,
        find_template, list_templates, rate_cost, validate_component_config,
    },
    database::{
        DbResult,
        cost::{
            CostCatalog, CostCatalogVersion, CostComponent, ImportedCostCatalogTemplate,
            NewCostCatalogPayload, NewCostCatalogVersionPayload, NewCostComponentPayload,
            UpdateCostCatalogData, UpdateCostCatalogVersionData, UpdateCostComponentData,
            EnabledVersionResolution, import_cost_catalog_template,
            reconcile_enabled_version_conflicts,
        },
    },
    service::app_state::{AppState, StateRouter, create_state_router},
    utils::HttpResult,
};

#[derive(Debug, Deserialize, Default)]
struct UpdateCostCatalogRequest {
    name: Option<String>,
    description: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct CreateCostCatalogVersionRequest {
    version: String,
    currency: String,
    source: Option<String>,
    effective_from: i64,
    effective_until: Option<i64>,
    is_enabled: bool,
}

#[derive(Debug, Deserialize, Default)]
struct UpdateCostCatalogVersionRequest {
    currency: Option<String>,
    source: Option<Option<String>>,
    effective_from: Option<i64>,
    effective_until: Option<Option<i64>>,
    is_enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct UpdateCostComponentRequest {
    meter_key: Option<String>,
    charge_kind: Option<String>,
    unit_price_nanos: Option<Option<i64>>,
    flat_fee_nanos: Option<Option<i64>>,
    tier_config_json: Option<Option<String>>,
    match_attributes_json: Option<Option<String>>,
    priority: Option<i32>,
    description: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct CostPreviewRequest {
    catalog_version_id: i64,
    normalization: Option<UsageNormalization>,
    ledger: Option<CostLedger>,
    total_input_tokens: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ImportCostTemplateRequest {
    template_key: String,
    catalog_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct CostCatalogListItem {
    catalog: CostCatalog,
    versions: Vec<CostCatalogVersion>,
}

#[derive(Debug, Serialize)]
struct CostCatalogVersionDetail {
    version: CostCatalogVersion,
    components: Vec<CostComponent>,
}

#[derive(Debug, Serialize)]
struct CostPreviewResponse {
    normalization: Option<UsageNormalization>,
    ledger: CostLedger,
    result: CostRatingResult,
}

#[derive(Debug, Serialize)]
struct ImportedCostTemplateResponse {
    template: CostTemplateSummary,
    imported: ImportedCostCatalogTemplate,
}

async fn create_catalog(
    Json(payload): Json<NewCostCatalogPayload>,
) -> DbResult<HttpResult<CostCatalog>> {
    validate_catalog_payload(&payload)?;
    Ok(HttpResult::new(CostCatalog::create(&payload)?))
}

async fn update_catalog(
    Path(id): Path<i64>,
    Json(payload): Json<UpdateCostCatalogRequest>,
) -> DbResult<HttpResult<CostCatalog>> {
    validate_optional_catalog_payload(payload.name.as_deref(), payload.description.as_ref())?;
    let update_data = UpdateCostCatalogData {
        name: payload.name,
        description: payload.description,
    };
    Ok(HttpResult::new(CostCatalog::update(id, &update_data)?))
}

async fn delete_catalog(Path(id): Path<i64>) -> DbResult<HttpResult<()>> {
    CostCatalog::get_by_id(id)?;
    let versions = CostCatalogVersion::list_by_catalog_id(id)?;
    if !versions.is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "Cannot delete a cost catalog that still has versions".to_string(),
        )));
    }

    CostCatalog::delete(id)?;
    Ok(HttpResult::new(()))
}

async fn list_catalogs() -> DbResult<HttpResult<Vec<CostCatalogListItem>>> {
    let catalogs = CostCatalog::list_all()?;
    let versions = CostCatalogVersion::list_all()?;

    let result = catalogs
        .into_iter()
        .map(|catalog| CostCatalogListItem {
            versions: versions
                .iter()
                .filter(|version| version.catalog_id == catalog.id)
                .cloned()
                .collect(),
            catalog,
        })
        .collect();

    Ok(HttpResult::new(result))
}

async fn create_catalog_version(
    State(app_state): State<Arc<AppState>>,
    Path(catalog_id): Path<i64>,
    Json(payload): Json<CreateCostCatalogVersionRequest>,
) -> DbResult<HttpResult<CostCatalogVersion>> {
    validate_new_catalog_version(catalog_id, &payload)?;

    let create_payload = NewCostCatalogVersionPayload {
        catalog_id,
        version: payload.version,
        currency: payload.currency,
        source: payload.source,
        effective_from: payload.effective_from,
        effective_until: payload.effective_until,
        is_enabled: payload.is_enabled,
    };

    let created = CostCatalogVersion::create(&create_payload)?;
    let disabled_versions = if created.is_enabled {
        disable_other_enabled_versions(&created)?
    } else {
        Vec::new()
    };
    for version in disabled_versions {
        invalidate_cost_catalog_version_cache(&app_state, version.id).await;
    }
    invalidate_cost_catalog_version_cache(&app_state, created.id).await;

    Ok(HttpResult::new(created))
}

async fn get_version(Path(id): Path<i64>) -> DbResult<HttpResult<CostCatalogVersionDetail>> {
    let version = CostCatalogVersion::get_by_id(id)?;
    let components = CostComponent::list_by_catalog_version_id(id)?;
    Ok(HttpResult::new(CostCatalogVersionDetail {
        version,
        components,
    }))
}

async fn update_version(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateCostCatalogVersionRequest>,
) -> DbResult<HttpResult<CostCatalogVersion>> {
    let original = CostCatalogVersion::get_by_id(id)?;
    let next_payload = CreateCostCatalogVersionRequest {
        version: original.version.clone(),
        currency: payload
            .currency
            .clone()
            .unwrap_or_else(|| original.currency.clone()),
        source: payload
            .source
            .clone()
            .unwrap_or_else(|| original.source.clone()),
        effective_from: payload.effective_from.unwrap_or(original.effective_from),
        effective_until: payload.effective_until.unwrap_or(original.effective_until),
        is_enabled: payload.is_enabled.unwrap_or(original.is_enabled),
    };

    validate_catalog_version_request_fields(&next_payload)?;

    let updated = CostCatalogVersion::update(
        id,
        &UpdateCostCatalogVersionData {
            currency: payload.currency,
            source: payload.source,
            effective_from: payload.effective_from,
            effective_until: payload.effective_until,
            is_enabled: payload.is_enabled,
        },
    )?;

    let disabled_versions = if updated.is_enabled {
        disable_other_enabled_versions(&updated)?
    } else {
        Vec::new()
    };
    for version in disabled_versions {
        invalidate_cost_catalog_version_cache(&app_state, version.id).await;
    }
    invalidate_cost_catalog_version_cache(&app_state, id).await;
    Ok(HttpResult::new(updated))
}

async fn create_component(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<NewCostComponentPayload>,
) -> DbResult<HttpResult<CostComponent>> {
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
    invalidate_cost_catalog_version_cache(&app_state, version.id).await;
    Ok(HttpResult::new(created))
}

async fn update_component(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateCostComponentRequest>,
) -> DbResult<HttpResult<CostComponent>> {
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

    let update_data = UpdateCostComponentData {
        meter_key: payload.meter_key,
        charge_kind: payload.charge_kind,
        unit_price_nanos: payload.unit_price_nanos,
        flat_fee_nanos: payload.flat_fee_nanos,
        tier_config_json: payload.tier_config_json,
        match_attributes_json: payload.match_attributes_json,
        priority: payload.priority,
        description: payload.description,
    };

    let updated = CostComponent::update(id, &update_data)?;
    invalidate_cost_catalog_version_cache(&app_state, version.id).await;
    Ok(HttpResult::new(updated))
}

async fn delete_component(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> DbResult<HttpResult<()>> {
    let component = CostComponent::get_by_id(id)?;
    let version = ensure_mutable_version(component.catalog_version_id)?;

    CostComponent::delete(id)?;
    invalidate_cost_catalog_version_cache(&app_state, version.id).await;
    Ok(HttpResult::new(()))
}

async fn preview_cost(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<CostPreviewRequest>,
) -> DbResult<HttpResult<CostPreviewResponse>> {
    let version = app_state
        .get_cost_catalog_version_by_id(payload.catalog_version_id)
        .await?
        .ok_or_else(|| {
            BaseError::ParamInvalid(Some(format!(
                "Cost catalog version {} not found",
                payload.catalog_version_id
            )))
        })?;

    let normalization = payload.normalization;
    let (ledger, total_input_tokens) = match (&normalization, payload.ledger) {
        (Some(normalization), None) => (
            CostLedger::from(normalization),
            normalization.total_input_tokens,
        ),
        (None, Some(ledger)) => (ledger, payload.total_input_tokens.unwrap_or(0)),
        (Some(_), Some(_)) => {
            return Err(BaseError::ParamInvalid(Some(
                "preview accepts either normalization or ledger, not both".to_string(),
            )));
        }
        (None, None) => {
            return Err(BaseError::ParamInvalid(Some(
                "preview requires normalization or ledger".to_string(),
            )));
        }
    };

    let mut result = rate_cost(&ledger, &CostRatingContext { total_input_tokens }, &version)?;
    if let Some(normalization) = &normalization {
        result
            .warnings
            .extend(normalization.warnings.iter().cloned());
    }

    Ok(HttpResult::new(CostPreviewResponse {
        normalization,
        ledger,
        result,
    }))
}

async fn list_cost_templates() -> DbResult<HttpResult<Vec<CostTemplateSummary>>> {
    Ok(HttpResult::new(list_templates()))
}

async fn import_cost_template(
    Json(payload): Json<ImportCostTemplateRequest>,
) -> DbResult<HttpResult<ImportedCostTemplateResponse>> {
    if payload.template_key.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "template_key cannot be empty".to_string(),
        )));
    }
    if let Some(catalog_name) = payload.catalog_name.as_deref()
        && catalog_name.trim().is_empty()
    {
        return Err(BaseError::ParamInvalid(Some(
            "catalog_name cannot be empty when provided".to_string(),
        )));
    }

    let template = find_template(payload.template_key.trim()).ok_or_else(|| {
        BaseError::ParamInvalid(Some(format!(
            "Unknown cost template '{}'",
            payload.template_key
        )))
    })?;

    let now = Utc::now();
    let import_payload = template.import_payload_at(now, payload.catalog_name.as_deref());
    validate_catalog_payload(&NewCostCatalogPayload {
        name: import_payload.catalog_name.clone(),
        description: import_payload.catalog_description.clone(),
    })?;
    validate_catalog_version_request_fields(&CreateCostCatalogVersionRequest {
        version: import_payload.version.clone(),
        currency: import_payload.currency.clone(),
        source: import_payload.source.clone(),
        effective_from: import_payload.effective_from,
        effective_until: import_payload.effective_until,
        is_enabled: import_payload.is_enabled,
    })?;

    let target_catalog = CostCatalog::get_by_name(&import_payload.catalog_name)?;
    if let Some(existing_catalog) = target_catalog {
        validate_catalog_version_uniqueness(
            existing_catalog.id,
            &CreateCostCatalogVersionRequest {
                version: import_payload.version.clone(),
                currency: import_payload.currency.clone(),
                source: import_payload.source.clone(),
                effective_from: import_payload.effective_from,
                effective_until: import_payload.effective_until,
                is_enabled: import_payload.is_enabled,
            },
        )?;
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

    Ok(HttpResult::new(ImportedCostTemplateResponse {
        template: template.summary_at(now),
        imported,
    }))
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

fn validate_new_catalog_version(
    catalog_id: i64,
    payload: &CreateCostCatalogVersionRequest,
) -> Result<(), BaseError> {
    validate_catalog_version_request_fields(payload)?;
    CostCatalog::get_by_id(catalog_id)?;
    validate_catalog_version_uniqueness(catalog_id, payload)
}

fn validate_catalog_version_request_fields(
    payload: &CreateCostCatalogVersionRequest,
) -> Result<(), BaseError> {
    if payload.version.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "version cannot be empty".to_string(),
        )));
    }
    if payload.currency.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "currency cannot be empty".to_string(),
        )));
    }
    if let Some(source) = &payload.source
        && source.trim().is_empty()
    {
        return Err(BaseError::ParamInvalid(Some(
            "source cannot be empty when provided".to_string(),
        )));
    }
    if let Some(effective_until) = payload.effective_until
        && effective_until <= payload.effective_from
    {
        return Err(BaseError::ParamInvalid(Some(
            "effective_until must be greater than effective_from".to_string(),
        )));
    }

    Ok(())
}

fn validate_catalog_version_uniqueness(
    catalog_id: i64,
    payload: &CreateCostCatalogVersionRequest,
) -> Result<(), BaseError> {
    let existing_versions = CostCatalogVersion::list_by_catalog_id(catalog_id)?;
    if existing_versions
        .iter()
        .any(|version| version.version == payload.version)
    {
        return Err(BaseError::DatabaseDup(Some(format!(
            "Version '{}' already exists for catalog {}",
            payload.version, catalog_id
        ))));
    }

    Ok(())
}

fn disable_other_enabled_versions(
    active_version: &CostCatalogVersion,
) -> Result<Vec<CostCatalogVersion>, BaseError> {
    let versions = CostCatalogVersion::list_by_catalog_id(active_version.catalog_id)?;
    let mut reconciled_versions = Vec::new();

    for resolution in reconcile_enabled_version_conflicts(&versions, active_version) {
        let updated = match resolution {
            EnabledVersionResolution::Disable { version_id } => CostCatalogVersion::update(
                version_id,
                &UpdateCostCatalogVersionData {
                    is_enabled: Some(false),
                    ..Default::default()
                },
            )?,
            EnabledVersionResolution::Truncate {
                version_id,
                effective_until,
            } => CostCatalogVersion::update(
                version_id,
                &UpdateCostCatalogVersionData {
                    effective_until: Some(Some(effective_until)),
                    ..Default::default()
                },
            )?,
        };
        reconciled_versions.push(updated);
    }

    Ok(reconciled_versions)
}

fn ensure_mutable_version(catalog_version_id: i64) -> Result<CostCatalogVersion, BaseError> {
    let version = CostCatalogVersion::get_by_id(catalog_version_id)?;
    if version.is_enabled {
        return Err(BaseError::ParamInvalid(Some(format!(
            "Cost catalog version {} is already enabled and cannot be modified",
            catalog_version_id
        ))));
    }
    Ok(version)
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

fn intervals_overlap(
    left_start: i64,
    left_end: Option<i64>,
    right_start: i64,
    right_end: Option<i64>,
) -> bool {
    let left_end = left_end.unwrap_or(i64::MAX);
    let right_end = right_end.unwrap_or(i64::MAX);
    left_start < right_end && right_start < left_end
}

async fn invalidate_cost_catalog_version_cache(app_state: &AppState, version_id: i64) {
    if let Err(err) = app_state.invalidate_cost_catalog_version(version_id).await {
        warn!(
            "Failed to invalidate cost catalog version {} cache: {:?}",
            version_id, err
        );
    }
}

pub fn create_cost_router() -> StateRouter {
    create_state_router().nest(
        "/cost",
        create_state_router()
            .route("/template/list", get(list_cost_templates))
            .route("/template/import", post(import_cost_template))
            .route("/catalog", post(create_catalog))
            .route("/catalog/list", get(list_catalogs))
            .route("/catalog/{id}", put(update_catalog).delete(delete_catalog))
            .route("/catalog/{id}/version", post(create_catalog_version))
            .route("/version/{id}", get(get_version).put(update_version))
            .route("/component", post(create_component))
            .route(
                "/component/{id}",
                put(update_component).delete(delete_component),
            )
            .route("/preview", post(preview_cost)),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        CreateCostCatalogVersionRequest, create_cost_router, intervals_overlap,
        validate_new_catalog_version,
    };

    #[test]
    fn create_cost_router_registers_routes() {
        let _router = create_cost_router();
    }

    #[test]
    fn intervals_overlap_uses_half_open_ranges() {
        assert!(!intervals_overlap(0, Some(100), 100, Some(200)));
        assert!(intervals_overlap(0, Some(101), 100, Some(200)));
    }

    #[test]
    fn version_validation_rejects_empty_fields_before_db_work() {
        let err = validate_new_catalog_version(
            1,
            &CreateCostCatalogVersionRequest {
                version: " ".to_string(),
                currency: "".to_string(),
                source: None,
                effective_from: 100,
                effective_until: None,
                is_enabled: false,
            },
        )
        .expect_err("empty version should fail");

        assert!(matches!(
            err,
            crate::controller::BaseError::ParamInvalid(Some(message))
                if message.contains("version cannot be empty")
        ));
    }
}
