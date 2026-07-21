#![allow(dead_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
    sync::Arc,
};

use chrono::Utc;

use crate::{
    controller::BaseError,
    cost::validate_component_config,
    database::{
        api_key::{ApiKey, UpdateApiKeyData, key_last4, key_prefix},
        api_key_acl_rule::NewApiKeyAclRule,
        cost::{
            CostCatalog, CostCatalogVersion, CostComponent, NewCostCatalog, NewCostCatalogVersion,
            NewCostComponent, UpdateCostCatalogData, UpdateCostCatalogVersionData,
            UpdateCostComponentData,
        },
        get_connection,
        model::{Model, NewModel, UpdateModelData},
        provider::{NewProvider, NewProviderApiKey, Provider, UpdateProviderData},
        reasoning_config::{
            ReasoningConfigMode, ReasoningConfigPresetInput, ReasoningConfigScope,
            ReasoningConfigWithPresets, ReasoningPatchFamily, ReasoningPreset,
        },
        request_patch::{
            CreateRequestPatchPayload, NewRequestPatchRule, RequestPatchImportValidation,
            validate_request_patch_import_payload,
        },
    },
    schema::enum_def::{RequestPatchPlacement, RuleScope},
    service::portable_config::{
        digest::canonical_json_digest,
        file_crypto::{PortableFileEncodeOptions, decode_portable_file, encode_portable_file},
        preview::{
            blocked_file_preview, blocked_item, blocked_item_with_target, excluded_data_types,
        },
        registry::{
            PortableModuleRegistryItem, PortableModuleRegistryResponse,
            PortableSubrangeRegistryItem, import_module_registry, module_registry,
            registry_response,
        },
        schema::{
            ConflictStrategy, FileProtectionMode, PORTABLE_MODULE_VERSION_V1,
            PORTABLE_SCHEMA_VERSION, ParsedPortableBundle, PortableApiKeyAclRuleItem,
            PortableApiKeyItem, PortableApiKeyPreviewItem, PortableApplyModuleResult,
            PortableApplyModuleStatus, PortableApplyRequest, PortableApplyResult,
            PortableBlockedItem, PortableBundle, PortableBundleModule, PortableCostBindingItem,
            PortableCostCatalogItem, PortableCostCatalogItems, PortableCostCatalogVersionItem,
            PortableCostComponentItem, PortableDangerousPatchConfirmation,
            PortableDependencyStatus, PortableExportRequest, PortableExportResponse,
            PortableFileProtectionStatus, PortableImportPreviewRequest, PortableModelRef,
            PortableModuleId, PortableModuleSelection, PortableModuleSummary,
            PortablePreviewModule, PortablePreviewResponse, PortableProviderApiKeyItem,
            PortableProviderItem, PortableProviderModelItem, PortableProviderOwnerRef,
            PortableProviderProfileItems, PortableProviderReasoningConfigItem,
            PortableProviderRequestPatchItem, PortableReasoningConfigPresetItem,
            PortableReferenceStatus, PortableSubrangeId, parse_portable_bundle_str,
        },
    },
    service::secret_encryption::{
        EncryptedSecret, SecretDomain, SecretEncryptionService, SensitiveSecret,
    },
    utils::ID_GENERATOR,
};

use self::repository::{
    api_key as api_key_repository, cost as cost_repository, provider as provider_repository,
    reasoning_config as reasoning_config_repository, request_patch as request_patch_repository,
};
use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::AdminMutationRunner;
use super::mutation::{AdminCatalogInvalidation, AdminModelCacheName, AdminMutationEffect};

pub(crate) mod repository;

pub struct PortableConfigAdminService {
    mutation_runner: Arc<AdminMutationRunner>,
    secret_encryption: Arc<SecretEncryptionService>,
}

impl PortableConfigAdminService {
    pub(crate) fn new(
        mutation_runner: Arc<AdminMutationRunner>,
        secret_encryption: Arc<SecretEncryptionService>,
    ) -> Self {
        Self {
            mutation_runner,
            secret_encryption,
        }
    }

    pub fn module_registry(&self) -> PortableModuleRegistryResponse {
        registry_response()
    }

    pub async fn export_config(
        &self,
        request: PortableExportRequest,
    ) -> Result<PortableExportResponse, BaseError> {
        let selection = NormalizedExportSelection::from_request(&request)?;
        let exported_at = Utc::now().timestamp_millis();
        let mut conn = get_connection()?;
        let bundle = repository::with_transaction(&mut conn, |tx| {
            build_export_bundle(tx, &selection, exported_at)
        })?;
        let plaintext = serde_json::to_string_pretty(&bundle).map_err(|err| {
            BaseError::InternalServerError(Some(format!(
                "failed to serialize portable export bundle: {err}"
            )))
        })?;
        let bundle_digest = canonical_json_digest(&bundle).map_err(|err| {
            BaseError::InternalServerError(Some(format!(
                "failed to calculate portable export digest: {err}"
            )))
        })?;
        let encoded = encode_portable_file(
            &plaintext,
            PortableFileEncodeOptions {
                mode: request.file_protection,
                password: request.password,
                auto_generate_password: request.auto_generate_password,
            },
        )
        .map_err(|err| BaseError::ParamInvalid(Some(err.to_string())))?;
        let response = PortableExportResponse {
            filename: format!("cyder-portable-{exported_at}.cyd"),
            content: encoded.content,
            file_protection: encoded.file_protection,
            generated_password: encoded.generated_password,
            bundle_digest,
        };

        self.run_post_commit_effects(vec![AdminMutationEffect::audit(
            portable_export_audit_event(
                &response.bundle_digest,
                response.file_protection,
                &bundle.modules,
            ),
        )])
        .await;

        Ok(response)
    }

    pub async fn preview_import(
        &self,
        request: PortableImportPreviewRequest,
    ) -> Result<PortablePreviewResponse, BaseError> {
        let decoded = match decode_portable_file(&request.content, request.password.as_deref()) {
            Ok(decoded) => decoded,
            Err(err) => {
                if request.content.trim_start().starts_with(
                    crate::service::portable_config::file_crypto::PORTABLE_BACKUP_HEADER,
                ) {
                    return Ok(blocked_file_preview(&request.content, &err));
                }
                return Err(BaseError::ParamInvalid(Some(err.to_string())));
            }
        };
        let parsed = parse_portable_bundle_str(&decoded.plaintext)
            .map_err(|err| BaseError::ParamInvalid(Some(err.to_string())))?;
        let bundle_digest = canonical_json_digest(&parsed.bundle).map_err(|err| {
            BaseError::InternalServerError(Some(format!(
                "failed to calculate portable import digest: {err}"
            )))
        })?;
        let mut conn = get_connection()?;

        repository::with_transaction(&mut conn, |tx| {
            build_import_preview(tx, parsed, decoded.file_protection, bundle_digest)
        })
    }

    pub async fn apply_import(
        &self,
        request: PortableApplyRequest,
    ) -> Result<PortableApplyResult, BaseError> {
        if request.reason.trim().is_empty() {
            return Err(BaseError::ParamInvalid(Some(
                "portable import reason must not be empty".to_string(),
            )));
        }
        let decoded = decode_portable_file(&request.content, request.password.as_deref())
            .map_err(|err| BaseError::ParamInvalid(Some(err.to_string())))?;
        let parsed = parse_portable_bundle_str(&decoded.plaintext)
            .map_err(|err| BaseError::ParamInvalid(Some(err.to_string())))?;
        let bundle_digest = canonical_json_digest(&parsed.bundle).map_err(|err| {
            BaseError::InternalServerError(Some(format!(
                "failed to calculate portable import digest: {err}"
            )))
        })?;
        if bundle_digest != request.bundle_digest {
            return Err(BaseError::ParamInvalid(Some(format!(
                "portable import digest mismatch: expected `{}`, actual `{}`",
                request.bundle_digest, bundle_digest
            ))));
        }

        let selection = NormalizedApplySelection::from_request(&request, &parsed.bundle)?;
        let conflict_strategy = request.conflict_strategy;
        let now = Utc::now().timestamp_millis();
        let mut conn = get_connection()?;
        let applied = repository::with_transaction(&mut conn, |tx| {
            apply_import_bundle(
                tx,
                &parsed.bundle,
                &selection,
                conflict_strategy,
                &bundle_digest,
                request.reason.trim(),
                &request.dangerous_patch_confirmations,
                now,
                &self.secret_encryption,
            )
        })?;

        self.run_post_commit_effects(applied.effects).await;

        Ok(PortableApplyResult {
            bundle_digest,
            conflict_strategy,
            modules: applied.modules,
            summary: applied.summary,
        })
    }

    #[cfg(test)]
    pub(crate) fn mutation_runner(&self) -> &Arc<AdminMutationRunner> {
        &self.mutation_runner
    }

    #[cfg(test)]
    pub(crate) fn secret_encryption(&self) -> &Arc<SecretEncryptionService> {
        &self.secret_encryption
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }
}

#[derive(Debug, Clone)]
struct NormalizedExportSelection {
    modules: BTreeMap<PortableModuleId, Vec<PortableSubrangeId>>,
}

impl NormalizedExportSelection {
    fn from_request(request: &PortableExportRequest) -> Result<Self, BaseError> {
        let selections = if request.selected_modules.is_empty() {
            default_export_selections()
        } else {
            request.selected_modules.clone()
        };
        let registry = export_registry_by_module_id();
        let mut modules = BTreeMap::new();
        let mut seen = BTreeSet::new();

        for selection in selections {
            let module_key = selection.module_id.as_str().to_string();
            if !seen.insert(module_key.clone()) {
                return Err(BaseError::ParamInvalid(Some(format!(
                    "portable export module `{module_key}` is selected more than once"
                ))));
            }

            let registry_item = registry.get(&selection.module_id).ok_or_else(|| {
                BaseError::ParamInvalid(Some(format!(
                    "portable export module `{}` is not supported",
                    selection.module_id
                )))
            })?;
            if registry_item.deferred {
                return Err(BaseError::ParamInvalid(Some(format!(
                    "portable export module `{}` is deferred and cannot be exported yet",
                    selection.module_id
                ))));
            }

            match selection.module_id {
                PortableModuleId::ProviderProfile
                | PortableModuleId::CostCatalogs
                | PortableModuleId::CostBindings => {
                    let subranges = normalize_subranges(registry_item, &selection)?;
                    modules.insert(selection.module_id, subranges);
                }
                PortableModuleId::ApiKeys | PortableModuleId::Unknown(_) => {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "portable export module `{}` is not supported",
                        selection.module_id
                    ))));
                }
            }
        }

        if modules.contains_key(&PortableModuleId::CostBindings)
            && (!modules.contains_key(&PortableModuleId::ProviderProfile)
                || !modules.contains_key(&PortableModuleId::CostCatalogs))
        {
            return Err(BaseError::ParamInvalid(Some(
                "portable export module `cost_bindings` requires `provider_profile` and `cost_catalogs`"
                    .to_string(),
            )));
        }

        Ok(Self { modules })
    }

    fn subranges(&self, module_id: &PortableModuleId) -> Option<&[PortableSubrangeId]> {
        self.modules.get(module_id).map(Vec::as_slice)
    }
}

#[derive(Debug, Clone)]
struct NormalizedApplySelection {
    modules: BTreeMap<PortableModuleId, Vec<PortableSubrangeId>>,
}

impl NormalizedApplySelection {
    fn from_request(
        request: &PortableApplyRequest,
        bundle: &PortableBundle,
    ) -> Result<Self, BaseError> {
        let bundle_modules = bundle
            .modules
            .iter()
            .map(|module| (module.module_id.clone(), module))
            .collect::<BTreeMap<_, _>>();
        let registry = import_registry_by_module_id();
        let selections = if request.selected_modules.is_empty() {
            bundle
                .modules
                .iter()
                .filter(|module| {
                    matches!(
                        module.module_id,
                        PortableModuleId::ProviderProfile | PortableModuleId::ApiKeys
                    )
                })
                .map(|module| PortableModuleSelection {
                    module_id: module.module_id.clone(),
                    subranges: module.subranges.clone(),
                })
                .collect::<Vec<_>>()
        } else {
            request.selected_modules.clone()
        };
        let mut modules = BTreeMap::new();
        let mut seen = BTreeSet::new();

        for selection in selections {
            let module_key = selection.module_id.as_str().to_string();
            if !seen.insert(module_key.clone()) {
                return Err(BaseError::ParamInvalid(Some(format!(
                    "portable import module `{module_key}` is selected more than once"
                ))));
            }
            let bundle_module = bundle_modules.get(&selection.module_id).ok_or_else(|| {
                BaseError::ParamInvalid(Some(format!(
                    "portable import module `{}` is not present in the bundle",
                    selection.module_id
                )))
            })?;
            let registry_item = registry.get(&selection.module_id).ok_or_else(|| {
                BaseError::ParamInvalid(Some(format!(
                    "portable import module `{}` is not supported",
                    selection.module_id
                )))
            })?;
            if registry_item.deferred {
                return Err(BaseError::ParamInvalid(Some(format!(
                    "portable import module `{}` is deferred and cannot be applied yet",
                    selection.module_id
                ))));
            }

            match selection.module_id {
                PortableModuleId::ProviderProfile
                | PortableModuleId::ApiKeys
                | PortableModuleId::CostCatalogs
                | PortableModuleId::CostBindings => {
                    let subranges =
                        normalize_apply_subranges(registry_item, &selection, bundle_module)?;
                    modules.insert(selection.module_id, subranges);
                }
                PortableModuleId::Unknown(_) => {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "portable import module `{}` is not supported",
                        selection.module_id
                    ))));
                }
            }
        }

        Ok(Self { modules })
    }

    fn contains(&self, module_id: &PortableModuleId) -> bool {
        self.modules.contains_key(module_id)
    }

    fn subranges(&self, module_id: &PortableModuleId) -> Option<&[PortableSubrangeId]> {
        self.modules.get(module_id).map(Vec::as_slice)
    }

    fn subrange_selected(
        &self,
        module_id: &PortableModuleId,
        subrange_id: &PortableSubrangeId,
    ) -> bool {
        self.subranges(module_id)
            .is_some_and(|subranges| subranges.contains(subrange_id))
    }
}

fn build_export_bundle(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    selection: &NormalizedExportSelection,
    exported_at: i64,
) -> Result<PortableBundle, BaseError> {
    let mut modules = Vec::new();

    if let Some(subranges) = selection.subranges(&PortableModuleId::CostCatalogs) {
        modules.push(export_cost_catalogs(conn, subranges)?);
    }
    if let Some(subranges) = selection.subranges(&PortableModuleId::ProviderProfile) {
        modules.push(export_provider_profile(conn, subranges)?);
    }
    if let Some(subranges) = selection.subranges(&PortableModuleId::CostBindings) {
        modules.push(export_cost_bindings(conn, selection, subranges, &modules)?);
    }
    Ok(PortableBundle {
        schema_version: PORTABLE_SCHEMA_VERSION.to_string(),
        exported_at,
        cyder_version: env!("CARGO_PKG_VERSION").to_string(),
        modules,
    })
}

fn build_import_preview(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    parsed: ParsedPortableBundle,
    file_protection: PortableFileProtectionStatus,
    bundle_digest: String,
) -> Result<PortablePreviewResponse, BaseError> {
    let registry = import_registry_by_module_id();
    let bundle_refs = collect_bundle_refs(&parsed.bundle);
    let mut preview_modules = Vec::with_capacity(parsed.bundle.modules.len());

    for (module_index, module) in parsed.bundle.modules.iter().enumerate() {
        let preview = match registry.get(&module.module_id) {
            Some(registry_item) => match module.module_id {
                PortableModuleId::ProviderProfile => {
                    preview_provider_profile_module(conn, module_index, module, registry_item, &[])?
                }
                PortableModuleId::ApiKeys => preview_api_keys_module(
                    conn,
                    module_index,
                    module,
                    registry_item,
                    &bundle_refs,
                )?,
                PortableModuleId::CostCatalogs => {
                    preview_cost_catalogs_module(conn, module_index, module, registry_item)?
                }
                PortableModuleId::CostBindings => preview_cost_bindings_module(
                    conn,
                    module_index,
                    module,
                    registry_item,
                    &bundle_refs,
                )?,
                PortableModuleId::Unknown(_) => preview_unsupported_module(module),
            },
            None => preview_unsupported_module(module),
        };
        preview_modules.push(preview);
    }

    let default_selected_modules = preview_modules
        .iter()
        .filter(|module| {
            module.selected_by_default
                && module.supported
                && module.available
                && module
                    .blocking_issues
                    .iter()
                    .all(|issue| issue.code != "unsupported_module_version")
        })
        .map(|module| module.module_id.clone())
        .collect();

    Ok(PortablePreviewResponse {
        schema_version: parsed.bundle.schema_version,
        exported_at: parsed.bundle.exported_at,
        cyder_version: parsed.bundle.cyder_version,
        bundle_digest,
        file_protection,
        modules: preview_modules,
        default_selected_modules,
        unsupported_modules: parsed.unsupported_modules,
        blocking_issues: Vec::new(),
        excluded_data_types: excluded_data_types(),
    })
}

#[derive(Debug, Default)]
struct PreviewBundleRefs {
    providers: BTreeSet<String>,
    models: BTreeSet<(String, String)>,
    cost_catalogs: BTreeSet<String>,
}

fn collect_bundle_refs(bundle: &PortableBundle) -> PreviewBundleRefs {
    let mut refs = PreviewBundleRefs::default();

    for module in &bundle.modules {
        match module.module_id {
            PortableModuleId::ProviderProfile => {
                let Ok(items) =
                    serde_json::from_value::<PortableProviderProfileItems>(module.items.clone())
                else {
                    continue;
                };
                let include_models = module
                    .subranges
                    .contains(&PortableSubrangeId::ProviderModels);
                for provider in items.providers {
                    refs.providers.insert(provider.provider_key.clone());
                    if include_models {
                        for model in provider.models {
                            refs.models
                                .insert((model.provider_ref.clone(), model.model_name.clone()));
                        }
                    }
                }
            }
            PortableModuleId::CostCatalogs => {
                let Ok(items) =
                    serde_json::from_value::<PortableCostCatalogItems>(module.items.clone())
                else {
                    continue;
                };
                for catalog in items.catalogs {
                    refs.cost_catalogs.insert(catalog.name);
                }
            }
            _ => {}
        }
    }

    refs
}

fn collect_provider_profile_item_refs(
    items: &PortableProviderProfileItems,
    include_models: bool,
) -> PreviewBundleRefs {
    let mut refs = PreviewBundleRefs::default();
    for provider in &items.providers {
        refs.providers.insert(provider.provider_key.clone());
        if include_models {
            for model in &provider.models {
                refs.models
                    .insert((model.provider_ref.clone(), model.model_name.clone()));
            }
        }
    }
    refs
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum PortableOwnerKey {
    Provider(String),
    Model {
        provider_key: String,
        model_name: String,
    },
}

impl PortableOwnerKey {
    fn label(&self) -> String {
        match self {
            Self::Provider(provider_key) => format!("provider `{provider_key}`"),
            Self::Model {
                provider_key,
                model_name,
            } => format!("model `{provider_key}/{model_name}`"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderProfileOwnerResolution {
    NewInBundle,
    ExistingInTarget { owner_id: i64 },
    Missing,
}

fn portable_owner_key(owner: &PortableProviderOwnerRef) -> Option<PortableOwnerKey> {
    match owner.scope.clone() {
        RuleScope::Provider => owner
            .provider_ref
            .as_ref()
            .map(|provider_ref| PortableOwnerKey::Provider(provider_ref.clone())),
        RuleScope::Model => owner
            .model_ref
            .as_ref()
            .map(|model_ref| PortableOwnerKey::Model {
                provider_key: model_ref.provider_key.clone(),
                model_name: model_ref.model_name.clone(),
            }),
    }
}

fn owner_ref_path_message(owner: &PortableProviderOwnerRef) -> &'static str {
    match owner.scope.clone() {
        RuleScope::Provider => "provider-scoped item requires provider_ref",
        RuleScope::Model => "model-scoped item requires model_ref",
    }
}

fn preview_deferred_or_invalid_module(
    module_index: usize,
    module: &PortableBundleModule,
    registry_item: &PortableModuleRegistryItem,
) -> PortablePreviewModule {
    if module.module_version != registry_item.module_version {
        return module_with_single_blocking_issue(
            module_index,
            module,
            registry_item,
            "unsupported_module_version",
            format!(
                "portable module `{}` version {} is not supported",
                module.module_id, module.module_version
            ),
        );
    }
    if registry_item.deferred {
        return module_with_single_blocking_issue(
            module_index,
            module,
            registry_item,
            "module_deferred",
            registry_item
                .deferred_reason
                .clone()
                .unwrap_or_else(|| "portable module is deferred".to_string()),
        );
    }

    preview_module(
        module,
        registry_item,
        Vec::new(),
        PortableModuleSummary::default(),
        Vec::new(),
        Vec::new(),
    )
}

fn module_with_single_blocking_issue(
    module_index: usize,
    module: &PortableBundleModule,
    registry_item: &PortableModuleRegistryItem,
    code: &str,
    message: String,
) -> PortablePreviewModule {
    let blocked = module.summary.total.max(1);
    preview_module(
        module,
        registry_item,
        Vec::new(),
        PortableModuleSummary {
            total: blocked,
            blocked,
            ..PortableModuleSummary::default()
        },
        Vec::new(),
        vec![blocked_item(
            code,
            message,
            format!("$.modules[{module_index}]"),
            Some(module.module_id.clone()),
            None,
        )],
    )
}

fn preview_provider_profile_module(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module_index: usize,
    module: &PortableBundleModule,
    registry_item: &PortableModuleRegistryItem,
    dangerous_patch_confirmations: &[PortableDangerousPatchConfirmation],
) -> Result<PortablePreviewModule, BaseError> {
    if registry_item.deferred || module.module_version != registry_item.module_version {
        return Ok(preview_deferred_or_invalid_module(
            module_index,
            module,
            registry_item,
        ));
    }

    let items = match serde_json::from_value::<PortableProviderProfileItems>(module.items.clone()) {
        Ok(items) => items,
        Err(err) => {
            return Ok(module_with_single_blocking_issue(
                module_index,
                module,
                registry_item,
                "invalid_module_items",
                format!("provider_profile items are invalid: {err}"),
            ));
        }
    };

    let mut summary = PortableModuleSummary::default();
    let mut blocking_issues = Vec::new();
    let mut warnings = Vec::new();
    let mut seen_provider_refs = BTreeSet::new();
    let include_models = module
        .subranges
        .contains(&PortableSubrangeId::ProviderModels);
    let include_request_patches = module
        .subranges
        .contains(&PortableSubrangeId::ProviderRequestPatches);
    let include_reasoning_config = module
        .subranges
        .contains(&PortableSubrangeId::ProviderReasoningConfig);
    let bundle_refs = collect_provider_profile_item_refs(&items, include_models);

    for (provider_index, provider) in items.providers.iter().enumerate() {
        summary.total += 1;
        let provider_path = format!("$.modules[{module_index}].items.providers[{provider_index}]");
        if provider.provider_key.trim().is_empty() {
            add_blocked_issue(
                &mut summary,
                &mut blocking_issues,
                "missing_dependency",
                "provider_key must not be empty",
                provider_path,
                PortableModuleId::ProviderProfile,
                PortableSubrangeId::ProviderCore,
            );
            continue;
        }
        if !seen_provider_refs.insert(provider.provider_key.clone()) {
            add_blocked_issue(
                &mut summary,
                &mut blocking_issues,
                "duplicate_natural_ref",
                "provider_profile contains the same provider_key more than once",
                provider_path,
                PortableModuleId::ProviderProfile,
                PortableSubrangeId::ProviderCore,
            );
            continue;
        }

        let target_provider =
            provider_repository::find_active_provider_by_key(conn, &provider.provider_key)?;
        match target_provider.as_ref() {
            Some(existing) if provider_core_matches(existing, provider) => summary.skip += 1,
            Some(_) => {
                summary.conflict += 1;
                blocking_issues.push(blocked_item(
                    "conflict",
                    format!(
                        "target provider `{}` already exists with different fields",
                        provider.provider_key
                    ),
                    provider_path.clone(),
                    Some(PortableModuleId::ProviderProfile),
                    Some(PortableSubrangeId::ProviderCore),
                ));
            }
            None => summary.create += 1,
        }

        preview_provider_keys(
            conn,
            module_index,
            provider_index,
            provider,
            target_provider.as_ref(),
            &mut summary,
            &mut blocking_issues,
        )?;
        if include_models {
            preview_provider_models(
                conn,
                module_index,
                provider_index,
                provider,
                target_provider.as_ref(),
                &mut summary,
                &mut blocking_issues,
            )?;
        }
    }

    if include_request_patches {
        preview_provider_request_patches(
            conn,
            module_index,
            &items.request_patches,
            &bundle_refs,
            dangerous_patch_confirmations,
            &mut summary,
            &mut blocking_issues,
        )?;
    }
    if include_reasoning_config {
        preview_provider_reasoning_configs(
            conn,
            module_index,
            &items.reasoning_configs,
            &bundle_refs,
            &mut summary,
            &mut blocking_issues,
        )?;
    }

    if module
        .subranges
        .iter()
        .any(|subrange| matches!(subrange, PortableSubrangeId::Unknown(_)))
    {
        warnings.push(
            "provider_profile contains unknown subranges; known subranges were previewed"
                .to_string(),
        );
    }

    Ok(preview_module(
        module,
        registry_item,
        Vec::new(),
        summary,
        warnings,
        blocking_issues,
    ))
}

fn preview_provider_keys(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module_index: usize,
    provider_index: usize,
    provider: &PortableProviderItem,
    target_provider: Option<&Provider>,
    summary: &mut PortableModuleSummary,
    blocking_issues: &mut Vec<crate::service::portable_config::schema::PortableBlockedItem>,
) -> Result<(), BaseError> {
    let mut seen_raw_keys = BTreeSet::new();
    for (key_index, key) in provider.keys.iter().enumerate() {
        summary.total += 1;
        let path = format!(
            "$.modules[{module_index}].items.providers[{provider_index}].keys[{key_index}]"
        );
        if key.api_key.trim().is_empty() {
            add_blocked_issue(
                summary,
                blocking_issues,
                "missing_dependency",
                "provider api_key must not be empty",
                path,
                PortableModuleId::ProviderProfile,
                PortableSubrangeId::ProviderKeys,
            );
            continue;
        }
        if !seen_raw_keys.insert(key.api_key.clone()) {
            summary.skip += 1;
            continue;
        }

        match target_provider {
            Some(existing_provider) => {
                if provider_repository::find_provider_api_key_by_raw_key(
                    conn,
                    existing_provider.id,
                    &key.api_key,
                )?
                .is_some()
                {
                    summary.skip += 1;
                } else {
                    summary.create += 1;
                }
            }
            None => summary.create += 1,
        }
    }
    Ok(())
}

fn preview_provider_models(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module_index: usize,
    provider_index: usize,
    provider: &PortableProviderItem,
    target_provider: Option<&Provider>,
    summary: &mut PortableModuleSummary,
    blocking_issues: &mut Vec<crate::service::portable_config::schema::PortableBlockedItem>,
) -> Result<(), BaseError> {
    let mut seen_models = BTreeSet::new();
    for (model_index, model) in provider.models.iter().enumerate() {
        summary.total += 1;
        let path = format!(
            "$.modules[{module_index}].items.providers[{provider_index}].models[{model_index}]"
        );
        if model.provider_ref != provider.provider_key {
            add_blocked_issue(
                summary,
                blocking_issues,
                "missing_dependency",
                "model provider_ref does not match its provider item",
                path,
                PortableModuleId::ProviderProfile,
                PortableSubrangeId::ProviderModels,
            );
            continue;
        }
        if model.model_name.trim().is_empty() {
            add_blocked_issue(
                summary,
                blocking_issues,
                "missing_dependency",
                "model_name must not be empty",
                path,
                PortableModuleId::ProviderProfile,
                PortableSubrangeId::ProviderModels,
            );
            continue;
        }
        if !seen_models.insert(model.model_name.clone()) {
            add_blocked_issue(
                summary,
                blocking_issues,
                "duplicate_natural_ref",
                "provider_profile contains the same model_name more than once for a provider",
                path,
                PortableModuleId::ProviderProfile,
                PortableSubrangeId::ProviderModels,
            );
            continue;
        }

        match target_provider {
            Some(existing_provider) => {
                match provider_repository::find_active_model_for_provider(
                    conn,
                    existing_provider.id,
                    &model.model_name,
                )? {
                    Some(existing_model) if model_core_matches(&existing_model, model) => {
                        summary.skip += 1
                    }
                    Some(_) => {
                        summary.conflict += 1;
                        blocking_issues.push(blocked_item(
                            "conflict",
                            format!(
                                "target model `{}/{}` already exists with different fields",
                                provider.provider_key, model.model_name
                            ),
                            path,
                            Some(PortableModuleId::ProviderProfile),
                            Some(PortableSubrangeId::ProviderModels),
                        ));
                    }
                    None => summary.create += 1,
                }
            }
            None => summary.create += 1,
        }
    }
    Ok(())
}

fn preview_provider_request_patches(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module_index: usize,
    request_patches: &[PortableProviderRequestPatchItem],
    bundle_refs: &PreviewBundleRefs,
    dangerous_patch_confirmations: &[PortableDangerousPatchConfirmation],
    summary: &mut PortableModuleSummary,
    blocking_issues: &mut Vec<crate::service::portable_config::schema::PortableBlockedItem>,
) -> Result<(), BaseError> {
    let mut seen_identities = BTreeSet::new();
    let mut body_targets_by_owner = BTreeMap::<PortableOwnerKey, Vec<String>>::new();

    for (patch_index, patch) in request_patches.iter().enumerate() {
        summary.total += 1;
        let path = format!("$.modules[{module_index}].items.request_patches[{patch_index}]");
        let Some(owner_key) = portable_owner_key(&patch.owner) else {
            add_blocked_issue(
                summary,
                blocking_issues,
                "missing_dependency",
                owner_ref_path_message(&patch.owner),
                path,
                PortableModuleId::ProviderProfile,
                PortableSubrangeId::ProviderRequestPatches,
            );
            continue;
        };

        match resolve_provider_profile_owner(conn, bundle_refs, &owner_key)? {
            ProviderProfileOwnerResolution::ExistingInTarget { .. } => {
                summary.skip += 1;
                continue;
            }
            ProviderProfileOwnerResolution::Missing => {
                add_blocked_issue(
                    summary,
                    blocking_issues,
                    "missing_dependency",
                    format!(
                        "request patch owner {} is not in the bundle or target environment",
                        owner_key.label()
                    ),
                    path,
                    PortableModuleId::ProviderProfile,
                    PortableSubrangeId::ProviderRequestPatches,
                );
                continue;
            }
            ProviderProfileOwnerResolution::NewInBundle => {}
        }

        let validation = match validate_request_patch_item_with_confirmations(
            patch,
            &path,
            dangerous_patch_confirmations,
        ) {
            Ok(validation) => validation,
            Err(err) => {
                add_blocked_issue(
                    summary,
                    blocking_issues,
                    "invalid_request_patch",
                    format!("{err:?}"),
                    path,
                    PortableModuleId::ProviderProfile,
                    PortableSubrangeId::ProviderRequestPatches,
                );
                continue;
            }
        };

        if let Some(confirmation) = validation.confirmation.as_ref() {
            summary.blocked += 1;
            blocking_issues.push(blocked_item_with_target(
                "dangerous_request_patch_confirmation_required",
                confirmation.reason.clone(),
                path,
                validation.target.clone(),
                Some(PortableModuleId::ProviderProfile),
                Some(PortableSubrangeId::ProviderRequestPatches),
            ));
            continue;
        }

        if validation.is_enabled {
            let identity = (
                owner_key.clone(),
                request_patch_placement_key(validation.placement),
                validation.target.clone(),
            );
            if !seen_identities.insert(identity) {
                add_blocked_issue(
                    summary,
                    blocking_issues,
                    "duplicate_natural_ref",
                    format!(
                        "{} already has an imported {:?} request patch for target `{}`",
                        owner_key.label(),
                        validation.placement,
                        validation.target
                    ),
                    path,
                    PortableModuleId::ProviderProfile,
                    PortableSubrangeId::ProviderRequestPatches,
                );
                continue;
            }

            if validation.placement == RequestPatchPlacement::Body {
                let body_targets = body_targets_by_owner.entry(owner_key.clone()).or_default();
                if let Some(conflict_target) = body_targets.iter().find(|existing| {
                    request_patch_body_targets_overlap(existing, &validation.target)
                }) {
                    add_blocked_issue(
                        summary,
                        blocking_issues,
                        "request_patch_body_target_conflict",
                        format!(
                            "{} BODY target `{}` conflicts with imported BODY target `{}`",
                            owner_key.label(),
                            validation.target,
                            conflict_target
                        ),
                        path,
                        PortableModuleId::ProviderProfile,
                        PortableSubrangeId::ProviderRequestPatches,
                    );
                    continue;
                }
                body_targets.push(validation.target.clone());
            }
        }

        summary.create += 1;
    }

    Ok(())
}

fn preview_provider_reasoning_configs(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module_index: usize,
    reasoning_configs: &[PortableProviderReasoningConfigItem],
    bundle_refs: &PreviewBundleRefs,
    summary: &mut PortableModuleSummary,
    blocking_issues: &mut Vec<crate::service::portable_config::schema::PortableBlockedItem>,
) -> Result<(), BaseError> {
    let mut seen_owners = BTreeSet::new();

    for (config_index, config) in reasoning_configs.iter().enumerate() {
        summary.total += 1;
        let path = format!("$.modules[{module_index}].items.reasoning_configs[{config_index}]");
        let Some(owner_key) = portable_owner_key(&config.owner) else {
            add_blocked_issue(
                summary,
                blocking_issues,
                "missing_dependency",
                owner_ref_path_message(&config.owner),
                path,
                PortableModuleId::ProviderProfile,
                PortableSubrangeId::ProviderReasoningConfig,
            );
            continue;
        };
        if !seen_owners.insert(owner_key.clone()) {
            add_blocked_issue(
                summary,
                blocking_issues,
                "duplicate_natural_ref",
                format!(
                    "provider_profile contains more than one reasoning config for {}",
                    owner_key.label()
                ),
                path,
                PortableModuleId::ProviderProfile,
                PortableSubrangeId::ProviderReasoningConfig,
            );
            continue;
        }

        let resolution = resolve_provider_profile_owner(conn, bundle_refs, &owner_key)?;
        let existing_config = match resolution {
            ProviderProfileOwnerResolution::Missing => {
                add_blocked_issue(
                    summary,
                    blocking_issues,
                    "missing_dependency",
                    format!(
                        "reasoning config owner {} is not in the bundle or target environment",
                        owner_key.label()
                    ),
                    path,
                    PortableModuleId::ProviderProfile,
                    PortableSubrangeId::ProviderReasoningConfig,
                );
                continue;
            }
            ProviderProfileOwnerResolution::NewInBundle => None,
            ProviderProfileOwnerResolution::ExistingInTarget { owner_id } => match &owner_key {
                PortableOwnerKey::Provider(_) => {
                    reasoning_config_repository::get_active_provider_reasoning_config(
                        conn, owner_id,
                    )?
                }
                PortableOwnerKey::Model { .. } => {
                    reasoning_config_repository::get_active_model_reasoning_config(conn, owner_id)?
                }
            },
        };

        if let Err(err) = validate_reasoning_config_item(config) {
            add_blocked_issue(
                summary,
                blocking_issues,
                "invalid_reasoning_config",
                format!("{err:?}"),
                path,
                PortableModuleId::ProviderProfile,
                PortableSubrangeId::ProviderReasoningConfig,
            );
            continue;
        }

        match existing_config.as_ref() {
            Some(existing) if reasoning_config_matches(existing, config) => summary.skip += 1,
            Some(_) => {
                summary.conflict += 1;
                blocking_issues.push(blocked_item(
                    "conflict",
                    format!(
                        "target {} already has a different reasoning config",
                        owner_key.label()
                    ),
                    path,
                    Some(PortableModuleId::ProviderProfile),
                    Some(PortableSubrangeId::ProviderReasoningConfig),
                ));
            }
            None => summary.create += 1,
        }
    }

    Ok(())
}

fn resolve_provider_profile_owner(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    bundle_refs: &PreviewBundleRefs,
    owner_key: &PortableOwnerKey,
) -> Result<ProviderProfileOwnerResolution, BaseError> {
    match owner_key {
        PortableOwnerKey::Provider(provider_key) => {
            if let Some(provider) =
                provider_repository::find_active_provider_by_key(conn, provider_key)?
            {
                return Ok(ProviderProfileOwnerResolution::ExistingInTarget {
                    owner_id: provider.id,
                });
            }
            if bundle_refs.providers.contains(provider_key) {
                return Ok(ProviderProfileOwnerResolution::NewInBundle);
            }
            Ok(ProviderProfileOwnerResolution::Missing)
        }
        PortableOwnerKey::Model {
            provider_key,
            model_name,
        } => {
            if let Some(model) =
                provider_repository::find_active_model_by_ref(conn, provider_key, model_name)?
            {
                return Ok(ProviderProfileOwnerResolution::ExistingInTarget { owner_id: model.id });
            }
            if bundle_refs
                .models
                .contains(&(provider_key.clone(), model_name.clone()))
            {
                return Ok(ProviderProfileOwnerResolution::NewInBundle);
            }
            Ok(ProviderProfileOwnerResolution::Missing)
        }
    }
}

fn request_patch_payload(
    item: &PortableProviderRequestPatchItem,
    confirm_dangerous_target: bool,
) -> CreateRequestPatchPayload {
    CreateRequestPatchPayload {
        placement: item.placement,
        target: item.target.clone(),
        operation: item.operation,
        value_json: item.value_json.clone().map(Some),
        description: item.description.clone(),
        is_enabled: Some(item.is_enabled),
        confirm_dangerous_target: Some(confirm_dangerous_target),
    }
}

fn validate_request_patch_item_with_confirmations(
    item: &PortableProviderRequestPatchItem,
    path: &str,
    dangerous_patch_confirmations: &[PortableDangerousPatchConfirmation],
) -> Result<RequestPatchImportValidation, BaseError> {
    let validation = validate_request_patch_import_payload(&request_patch_payload(item, false))?;
    if validation.confirmation.is_some()
        && dangerous_patch_confirmed(dangerous_patch_confirmations, path, &validation.target)
    {
        return validate_request_patch_import_payload(&request_patch_payload(item, true));
    }
    Ok(validation)
}

fn dangerous_patch_confirmed(
    confirmations: &[PortableDangerousPatchConfirmation],
    path: &str,
    target: &str,
) -> bool {
    confirmations.iter().any(|confirmation| {
        confirmation.confirmed && confirmation.path == path && confirmation.target == target
    })
}

fn request_patch_placement_key(placement: RequestPatchPlacement) -> &'static str {
    match placement {
        RequestPatchPlacement::Header => "HEADER",
        RequestPatchPlacement::Query => "QUERY",
        RequestPatchPlacement::Body => "BODY",
    }
}

fn request_patch_body_targets_overlap(left: &str, right: &str) -> bool {
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn validate_reasoning_config_item(
    item: &PortableProviderReasoningConfigItem,
) -> Result<(), BaseError> {
    let scope = reasoning_owner_scope(&item.owner)?;
    if matches!(scope, ReasoningConfigScope::Provider)
        && matches!(item.mode, ReasoningConfigMode::Disabled)
    {
        return Err(BaseError::ParamInvalid(Some(
            "provider reasoning config does not support disabled mode".to_string(),
        )));
    }

    match item.mode {
        ReasoningConfigMode::Custom => {
            let family_key = item.family_key.as_deref().ok_or_else(|| {
                BaseError::ParamInvalid(Some(
                    "custom reasoning config requires family_key".to_string(),
                ))
            })?;
            let family = ReasoningPatchFamily::from_str(family_key)
                .map_err(|err| BaseError::ParamInvalid(Some(err.to_string())))?;
            let mut seen = BTreeSet::new();
            for preset in &item.presets {
                let preset_key = ReasoningPreset::from_str(&preset.preset_key)
                    .map_err(|err| BaseError::ParamInvalid(Some(err.to_string())))?;
                if !seen.insert(preset_key.as_key().to_string()) {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "duplicate reasoning preset '{}'",
                        preset_key.as_key()
                    ))));
                }
                if let Some(reason) = family.unsupported_preset_reason(preset_key) {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "reasoning family '{}' does not support preset '{}': {}",
                        family.as_key(),
                        preset_key.as_key(),
                        reason
                    ))));
                }
            }
            Ok(())
        }
        ReasoningConfigMode::Disabled => {
            if item.family_key.is_some() {
                return Err(BaseError::ParamInvalid(Some(
                    "disabled reasoning config must not include family_key".to_string(),
                )));
            }
            if !item.presets.is_empty() {
                return Err(BaseError::ParamInvalid(Some(
                    "disabled reasoning config must not include preset rows".to_string(),
                )));
            }
            Ok(())
        }
    }
}

fn reasoning_owner_scope(
    owner: &PortableProviderOwnerRef,
) -> Result<ReasoningConfigScope, BaseError> {
    match owner.scope.clone() {
        RuleScope::Provider => Ok(ReasoningConfigScope::Provider),
        RuleScope::Model => Ok(ReasoningConfigScope::Model),
    }
}

fn reasoning_config_matches(
    existing: &ReasoningConfigWithPresets,
    item: &PortableProviderReasoningConfigItem,
) -> bool {
    if existing.mode != item.mode {
        return false;
    }
    let existing_family = existing.family.map(|family| family.as_key().to_string());
    if existing_family != normalized_reasoning_family_key(item.family_key.as_deref()) {
        return false;
    }

    let existing_presets = existing
        .presets
        .iter()
        .map(|preset| {
            (
                preset.preset_key.as_key().to_string(),
                preset.preset.expose_in_models,
                preset.preset.is_enabled,
            )
        })
        .collect::<BTreeSet<_>>();
    let incoming_presets = item
        .presets
        .iter()
        .filter_map(|preset| {
            ReasoningPreset::from_str(&preset.preset_key)
                .ok()
                .map(|preset_key| {
                    (
                        preset_key.as_key().to_string(),
                        preset.expose_in_models,
                        preset.is_enabled,
                    )
                })
        })
        .collect::<BTreeSet<_>>();
    existing_presets == incoming_presets
}

fn normalized_reasoning_family_key(family_key: Option<&str>) -> Option<String> {
    family_key.and_then(|value| {
        ReasoningPatchFamily::from_str(value)
            .ok()
            .map(|family| family.as_key().to_string())
    })
}

fn preview_api_keys_module(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module_index: usize,
    module: &PortableBundleModule,
    registry_item: &PortableModuleRegistryItem,
    bundle_refs: &PreviewBundleRefs,
) -> Result<PortablePreviewModule, BaseError> {
    if registry_item.deferred || module.module_version != registry_item.module_version {
        return Ok(preview_deferred_or_invalid_module(
            module_index,
            module,
            registry_item,
        ));
    }

    let items = match serde_json::from_value::<Vec<PortableApiKeyItem>>(module.items.clone()) {
        Ok(items) => items,
        Err(err) => {
            return Ok(module_with_single_blocking_issue(
                module_index,
                module,
                registry_item,
                "invalid_module_items",
                format!("api_keys items are invalid: {err}"),
            ));
        }
    };

    let mut summary = PortableModuleSummary::default();
    let mut blocking_issues = Vec::new();
    let mut warnings = Vec::new();
    let mut seen_raw_keys = BTreeSet::new();
    let mut safe_items = Vec::with_capacity(items.len());
    let mut provider_dependency_missing_count = 0_u64;
    let mut has_provider_dependency = false;
    let include_acl = module.subranges.contains(&PortableSubrangeId::ApiKeyAcl);

    for (api_key_index, api_key) in items.iter().enumerate() {
        summary.total += 1;
        let api_key_path = format!("$.modules[{module_index}].items[{api_key_index}]");
        if api_key.api_key.trim().is_empty() {
            safe_items.push(api_key_preview_item(api_key, "blocked"));
            add_blocked_issue(
                &mut summary,
                &mut blocking_issues,
                "missing_dependency",
                "api_key must not be empty",
                api_key_path.clone(),
                PortableModuleId::ApiKeys,
                PortableSubrangeId::ApiKeyCore,
            );
            continue;
        }
        if !seen_raw_keys.insert(api_key.api_key.clone()) {
            safe_items.push(api_key_preview_item(api_key, "blocked"));
            add_blocked_issue(
                &mut summary,
                &mut blocking_issues,
                "duplicate_natural_ref",
                "api_keys contains the same raw API key more than once",
                api_key_path.clone(),
                PortableModuleId::ApiKeys,
                PortableSubrangeId::ApiKeyCore,
            );
            continue;
        }

        match api_key_repository::find_active_api_key_by_raw_key(conn, &api_key.api_key)? {
            Some(existing) if api_key_core_matches(&existing, api_key) => {
                safe_items.push(api_key_preview_item(api_key, "skip"));
                summary.skip += 1;
                preview_skip_existing_api_key_children(
                    api_key,
                    include_acl,
                    &mut summary,
                    &mut warnings,
                );
                continue;
            }
            Some(_) => {
                safe_items.push(api_key_preview_item(api_key, "conflict"));
                summary.conflict += 1;
                blocking_issues.push(blocked_item(
                    "conflict",
                    "target API key already exists with different governance fields",
                    api_key_path.clone(),
                    Some(PortableModuleId::ApiKeys),
                    Some(PortableSubrangeId::ApiKeyCore),
                ));
                preview_skip_existing_api_key_children(
                    api_key,
                    include_acl,
                    &mut summary,
                    &mut warnings,
                );
                continue;
            }
            None => {
                safe_items.push(api_key_preview_item(api_key, "create"));
                summary.create += 1;
            }
        }

        if include_acl {
            preview_api_key_acl_rules(
                conn,
                module_index,
                api_key_index,
                api_key,
                bundle_refs,
                &mut summary,
                &mut blocking_issues,
                &mut provider_dependency_missing_count,
                &mut has_provider_dependency,
            )?;
        }
    }

    if module
        .subranges
        .iter()
        .any(|subrange| matches!(subrange, PortableSubrangeId::Unknown(_)))
    {
        warnings.push(
            "api_keys contains unknown subranges; known subranges were previewed".to_string(),
        );
    }

    let dependencies = if has_provider_dependency {
        vec![PortableDependencyStatus {
            module_id: PortableModuleId::ProviderProfile,
            status: if provider_dependency_missing_count > 0 {
                PortableReferenceStatus::MissingDependency
            } else if !bundle_refs.providers.is_empty() || !bundle_refs.models.is_empty() {
                PortableReferenceStatus::ResolvedInBundle
            } else {
                PortableReferenceStatus::ResolvedInTarget
            },
            message: if provider_dependency_missing_count > 0 {
                Some(format!(
                    "{provider_dependency_missing_count} provider/model references are missing"
                ))
            } else {
                None
            },
        }]
    } else {
        Vec::new()
    };

    let mut preview = preview_module(
        module,
        registry_item,
        dependencies,
        summary,
        warnings,
        blocking_issues,
    );
    preview.api_key_items = safe_items;
    Ok(preview)
}

fn api_key_preview_item(api_key: &PortableApiKeyItem, outcome: &str) -> PortableApiKeyPreviewItem {
    let (key_prefix, key_last4) = if api_key.api_key.chars().count() >= 20 {
        (key_prefix(&api_key.api_key), key_last4(&api_key.api_key))
    } else {
        ("<masked>".to_string(), String::new())
    };
    PortableApiKeyPreviewItem {
        name: api_key.name.clone(),
        key_prefix,
        key_last4,
        acl_rule_count: api_key.acl_rules.len() as u64,
        outcome: outcome.to_string(),
    }
}

fn preview_skip_existing_api_key_children(
    api_key: &PortableApiKeyItem,
    include_acl: bool,
    summary: &mut PortableModuleSummary,
    warnings: &mut Vec<String>,
) {
    let skipped = skip_api_key_children(api_key, include_acl, summary);
    if skipped > 0 {
        warnings.push(format!(
            "API key `{}` already exists; skipped {} child ACL rows because existing API key child governance is metadata-only",
            api_key.name, skipped
        ));
    }
}

fn preview_api_key_acl_rules(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module_index: usize,
    api_key_index: usize,
    api_key: &PortableApiKeyItem,
    bundle_refs: &PreviewBundleRefs,
    summary: &mut PortableModuleSummary,
    blocking_issues: &mut Vec<crate::service::portable_config::schema::PortableBlockedItem>,
    provider_dependency_missing_count: &mut u64,
    has_provider_dependency: &mut bool,
) -> Result<(), BaseError> {
    for (rule_index, rule) in api_key.acl_rules.iter().enumerate() {
        summary.total += 1;
        *has_provider_dependency = true;
        let path =
            format!("$.modules[{module_index}].items[{api_key_index}].acl_rules[{rule_index}]");
        let status = match rule.scope {
            RuleScope::Provider => {
                let Some(provider_ref) = rule.provider_ref.as_deref() else {
                    add_acl_missing_dependency(summary, blocking_issues, path);
                    *provider_dependency_missing_count += 1;
                    continue;
                };
                resolve_provider_ref(conn, bundle_refs, provider_ref)?
            }
            RuleScope::Model => {
                let Some(model_ref) = rule.model_ref.as_ref() else {
                    add_acl_missing_dependency(summary, blocking_issues, path);
                    *provider_dependency_missing_count += 1;
                    continue;
                };
                resolve_model_ref(
                    conn,
                    bundle_refs,
                    &model_ref.provider_key,
                    &model_ref.model_name,
                )?
            }
        };

        if status == PortableReferenceStatus::MissingDependency {
            add_acl_missing_dependency(summary, blocking_issues, path);
            *provider_dependency_missing_count += 1;
        } else {
            summary.create += 1;
        }
    }
    Ok(())
}

fn add_acl_missing_dependency(
    summary: &mut PortableModuleSummary,
    blocking_issues: &mut Vec<crate::service::portable_config::schema::PortableBlockedItem>,
    path: String,
) {
    add_blocked_issue(
        summary,
        blocking_issues,
        "missing_dependency",
        "ACL rule references a provider/model that is not in the bundle or target environment",
        path,
        PortableModuleId::ApiKeys,
        PortableSubrangeId::ApiKeyAcl,
    );
}

fn preview_cost_catalogs_module(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module_index: usize,
    module: &PortableBundleModule,
    registry_item: &PortableModuleRegistryItem,
) -> Result<PortablePreviewModule, BaseError> {
    if registry_item.deferred || module.module_version != registry_item.module_version {
        return Ok(preview_deferred_or_invalid_module(
            module_index,
            module,
            registry_item,
        ));
    }

    let items = match serde_json::from_value::<PortableCostCatalogItems>(module.items.clone()) {
        Ok(items) => items,
        Err(err) => {
            return Ok(module_with_single_blocking_issue(
                module_index,
                module,
                registry_item,
                "invalid_module_items",
                format!("cost_catalogs items are invalid: {err}"),
            ));
        }
    };

    let mut summary = PortableModuleSummary::default();
    let mut blocking_issues = Vec::new();
    let mut warnings = Vec::new();
    let mut seen_catalogs = BTreeSet::new();
    let include_versions = module
        .subranges
        .contains(&PortableSubrangeId::CostCatalogVersions);
    let include_components = module
        .subranges
        .contains(&PortableSubrangeId::CostComponents);

    for (catalog_index, catalog) in items.catalogs.iter().enumerate() {
        summary.total += 1;
        let catalog_path = format!("$.modules[{module_index}].items.catalogs[{catalog_index}]");
        if catalog.name.trim().is_empty() {
            add_blocked_issue(
                &mut summary,
                &mut blocking_issues,
                "missing_dependency",
                "cost catalog name must not be empty",
                catalog_path,
                PortableModuleId::CostCatalogs,
                PortableSubrangeId::CostCatalogCore,
            );
            continue;
        }
        if !seen_catalogs.insert(catalog.name.clone()) {
            add_blocked_issue(
                &mut summary,
                &mut blocking_issues,
                "duplicate_natural_ref",
                "cost_catalogs contains the same catalog name more than once",
                catalog_path,
                PortableModuleId::CostCatalogs,
                PortableSubrangeId::CostCatalogCore,
            );
            continue;
        }

        let target_catalog =
            cost_repository::find_active_cost_catalog_by_name(conn, &catalog.name)?;
        match target_catalog.as_ref() {
            Some(existing) if cost_catalog_core_matches(existing, catalog) => summary.skip += 1,
            Some(_) => {
                summary.conflict += 1;
                blocking_issues.push(blocked_item(
                    "conflict",
                    format!(
                        "target cost catalog `{}` already exists with different fields",
                        catalog.name
                    ),
                    catalog_path.clone(),
                    Some(PortableModuleId::CostCatalogs),
                    Some(PortableSubrangeId::CostCatalogCore),
                ));
            }
            None => summary.create += 1,
        }

        if include_versions {
            preview_cost_catalog_versions(
                conn,
                module_index,
                catalog_index,
                catalog,
                target_catalog.as_ref(),
                include_components,
                &mut summary,
                &mut blocking_issues,
            )?;
        }
    }

    if module
        .subranges
        .iter()
        .any(|subrange| matches!(subrange, PortableSubrangeId::Unknown(_)))
    {
        warnings.push(
            "cost_catalogs contains unknown subranges; known subranges were previewed".to_string(),
        );
    }

    Ok(preview_module(
        module,
        registry_item,
        Vec::new(),
        summary,
        warnings,
        blocking_issues,
    ))
}

fn preview_cost_catalog_versions(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module_index: usize,
    catalog_index: usize,
    catalog: &PortableCostCatalogItem,
    target_catalog: Option<&CostCatalog>,
    include_components: bool,
    summary: &mut PortableModuleSummary,
    blocking_issues: &mut Vec<PortableBlockedItem>,
) -> Result<(), BaseError> {
    let mut seen_versions = BTreeSet::new();
    for (version_index, version) in catalog.versions.iter().enumerate() {
        summary.total += 1;
        let path = format!(
            "$.modules[{module_index}].items.catalogs[{catalog_index}].versions[{version_index}]"
        );
        if let Err(message) = validate_cost_catalog_version_item(version, &catalog.name) {
            add_blocked_issue(
                summary,
                blocking_issues,
                "invalid_cost_catalog_version",
                message,
                path,
                PortableModuleId::CostCatalogs,
                PortableSubrangeId::CostCatalogVersions,
            );
            continue;
        }
        if !seen_versions.insert(version.version.clone()) {
            add_blocked_issue(
                summary,
                blocking_issues,
                "duplicate_natural_ref",
                "cost_catalogs contains the same version more than once for a catalog",
                path,
                PortableModuleId::CostCatalogs,
                PortableSubrangeId::CostCatalogVersions,
            );
            continue;
        }

        let target_version = match target_catalog {
            Some(target_catalog) => {
                cost_repository::find_cost_catalog_version_by_catalog_and_version(
                    conn,
                    target_catalog.id,
                    &version.version,
                )?
            }
            None => None,
        };
        match target_version.as_ref() {
            Some(existing) if cost_catalog_version_matches(existing, version) => summary.skip += 1,
            Some(_) => {
                summary.conflict += 1;
                blocking_issues.push(blocked_item(
                    "conflict",
                    format!(
                        "target cost catalog version `{}/{}` already exists with different fields",
                        catalog.name, version.version
                    ),
                    path.clone(),
                    Some(PortableModuleId::CostCatalogs),
                    Some(PortableSubrangeId::CostCatalogVersions),
                ));
            }
            None => summary.create += 1,
        }

        if include_components {
            preview_cost_components(
                conn,
                module_index,
                catalog_index,
                version_index,
                version,
                target_version.as_ref(),
                summary,
                blocking_issues,
            )?;
        }
    }
    Ok(())
}

fn preview_cost_components(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module_index: usize,
    catalog_index: usize,
    version_index: usize,
    version: &PortableCostCatalogVersionItem,
    target_version: Option<&CostCatalogVersion>,
    summary: &mut PortableModuleSummary,
    blocking_issues: &mut Vec<PortableBlockedItem>,
) -> Result<(), BaseError> {
    let existing_components = match target_version {
        Some(target_version) => {
            cost_repository::list_cost_components_for_export(conn, target_version.id)?
        }
        None => Vec::new(),
    };
    let mut seen_components = BTreeSet::new();

    for (component_index, component) in version.components.iter().enumerate() {
        summary.total += 1;
        let path = format!(
            "$.modules[{module_index}].items.catalogs[{catalog_index}].versions[{version_index}].components[{component_index}]"
        );
        if let Err(message) = validate_cost_component_item(component) {
            add_blocked_issue(
                summary,
                blocking_issues,
                "invalid_cost_component",
                message,
                path,
                PortableModuleId::CostCatalogs,
                PortableSubrangeId::CostComponents,
            );
            continue;
        }
        let key = portable_cost_component_key(component);
        if !seen_components.insert(key.clone()) {
            add_blocked_issue(
                summary,
                blocking_issues,
                "duplicate_natural_ref",
                "cost_catalogs contains duplicate components for a version",
                path,
                PortableModuleId::CostCatalogs,
                PortableSubrangeId::CostComponents,
            );
            continue;
        }

        match existing_components
            .iter()
            .find(|existing| cost_component_key(existing) == key)
        {
            Some(existing) if cost_component_matches(existing, component) => summary.skip += 1,
            Some(_) => {
                summary.conflict += 1;
                blocking_issues.push(blocked_item(
                    "conflict",
                    format!(
                        "target cost component `{}` already exists with different fields",
                        component.meter_key
                    ),
                    path,
                    Some(PortableModuleId::CostCatalogs),
                    Some(PortableSubrangeId::CostComponents),
                ));
            }
            None => summary.create += 1,
        }
    }
    Ok(())
}

fn preview_cost_bindings_module(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module_index: usize,
    module: &PortableBundleModule,
    registry_item: &PortableModuleRegistryItem,
    bundle_refs: &PreviewBundleRefs,
) -> Result<PortablePreviewModule, BaseError> {
    if registry_item.deferred || module.module_version != registry_item.module_version {
        return Ok(preview_deferred_or_invalid_module(
            module_index,
            module,
            registry_item,
        ));
    }

    let items = match serde_json::from_value::<Vec<PortableCostBindingItem>>(module.items.clone()) {
        Ok(items) => items,
        Err(err) => {
            return Ok(module_with_single_blocking_issue(
                module_index,
                module,
                registry_item,
                "invalid_module_items",
                format!("cost_bindings items are invalid: {err}"),
            ));
        }
    };

    let mut summary = PortableModuleSummary::default();
    let mut blocking_issues = Vec::new();
    let mut seen_bindings = BTreeSet::new();
    let mut missing_model_count = 0_u64;
    let mut missing_catalog_count = 0_u64;

    for (binding_index, binding) in items.iter().enumerate() {
        summary.total += 1;
        let path = format!("$.modules[{module_index}].items[{binding_index}]");
        if binding.target_kind != "model" {
            add_blocked_issue(
                &mut summary,
                &mut blocking_issues,
                "unsupported_cost_binding_target",
                "cost binding target_kind must be `model` in this version",
                path,
                PortableModuleId::CostBindings,
                PortableSubrangeId::CostModelBindings,
            );
            continue;
        }
        let Some(model_ref) = binding.model_ref.as_ref() else {
            add_blocked_issue(
                &mut summary,
                &mut blocking_issues,
                "missing_dependency",
                "model cost binding requires model_ref",
                path,
                PortableModuleId::CostBindings,
                PortableSubrangeId::CostModelBindings,
            );
            missing_model_count += 1;
            continue;
        };
        if binding.cost_catalog_ref.trim().is_empty() {
            add_blocked_issue(
                &mut summary,
                &mut blocking_issues,
                "missing_dependency",
                "model cost binding requires cost_catalog_ref",
                path,
                PortableModuleId::CostBindings,
                PortableSubrangeId::CostModelBindings,
            );
            missing_catalog_count += 1;
            continue;
        }
        let binding_key = (model_ref.provider_key.clone(), model_ref.model_name.clone());
        if !seen_bindings.insert(binding_key.clone()) {
            add_blocked_issue(
                &mut summary,
                &mut blocking_issues,
                "duplicate_natural_ref",
                "cost_bindings contains more than one binding for the same model",
                path,
                PortableModuleId::CostBindings,
                PortableSubrangeId::CostModelBindings,
            );
            continue;
        }

        let model_status = resolve_model_ref(
            conn,
            bundle_refs,
            &model_ref.provider_key,
            &model_ref.model_name,
        )?;
        let catalog_status =
            resolve_cost_catalog_ref(conn, bundle_refs, &binding.cost_catalog_ref)?;
        if model_status == PortableReferenceStatus::MissingDependency {
            missing_model_count += 1;
            add_blocked_issue(
                &mut summary,
                &mut blocking_issues,
                "missing_dependency",
                "cost binding model is not in the bundle or target environment",
                path.clone(),
                PortableModuleId::CostBindings,
                PortableSubrangeId::CostModelBindings,
            );
            continue;
        }
        if catalog_status == PortableReferenceStatus::MissingDependency {
            missing_catalog_count += 1;
            add_blocked_issue(
                &mut summary,
                &mut blocking_issues,
                "missing_dependency",
                format!(
                    "cost binding catalog `{}` is not in the bundle or target environment",
                    binding.cost_catalog_ref
                ),
                path.clone(),
                PortableModuleId::CostBindings,
                PortableSubrangeId::CostModelBindings,
            );
            continue;
        }

        match provider_repository::find_active_model_by_ref(
            conn,
            &model_ref.provider_key,
            &model_ref.model_name,
        )? {
            Some(existing_model) => {
                let target_catalog = cost_repository::find_active_cost_catalog_by_name(
                    conn,
                    &binding.cost_catalog_ref,
                )?;
                if target_catalog
                    .as_ref()
                    .is_some_and(|target| existing_model.cost_catalog_id == Some(target.id))
                {
                    summary.skip += 1;
                } else if existing_model.cost_catalog_id.is_some() {
                    summary.conflict += 1;
                    blocking_issues.push(blocked_item(
                        "conflict",
                        format!(
                            "target model `{}/{}` already has a different cost catalog binding",
                            model_ref.provider_key, model_ref.model_name
                        ),
                        path,
                        Some(PortableModuleId::CostBindings),
                        Some(PortableSubrangeId::CostModelBindings),
                    ));
                } else {
                    summary.update += 1;
                }
            }
            None => summary.update += 1,
        }
    }

    let mut dependencies = Vec::new();
    dependencies.push(PortableDependencyStatus {
        module_id: PortableModuleId::ProviderProfile,
        status: if missing_model_count > 0 {
            PortableReferenceStatus::MissingDependency
        } else if !bundle_refs.models.is_empty() {
            PortableReferenceStatus::ResolvedInBundle
        } else {
            PortableReferenceStatus::ResolvedInTarget
        },
        message: if missing_model_count > 0 {
            Some(format!(
                "{missing_model_count} model references are missing"
            ))
        } else {
            None
        },
    });
    dependencies.push(PortableDependencyStatus {
        module_id: PortableModuleId::CostCatalogs,
        status: if missing_catalog_count > 0 {
            PortableReferenceStatus::MissingDependency
        } else if !bundle_refs.cost_catalogs.is_empty() {
            PortableReferenceStatus::ResolvedInBundle
        } else {
            PortableReferenceStatus::ResolvedInTarget
        },
        message: if missing_catalog_count > 0 {
            Some(format!(
                "{missing_catalog_count} cost catalog references are missing"
            ))
        } else {
            None
        },
    });

    Ok(preview_module(
        module,
        registry_item,
        dependencies,
        summary,
        Vec::new(),
        blocking_issues,
    ))
}

fn resolve_provider_ref(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    bundle_refs: &PreviewBundleRefs,
    provider_ref: &str,
) -> Result<PortableReferenceStatus, BaseError> {
    if bundle_refs.providers.contains(provider_ref) {
        return Ok(PortableReferenceStatus::ResolvedInBundle);
    }
    if provider_repository::find_active_provider_by_key(conn, provider_ref)?.is_some() {
        return Ok(PortableReferenceStatus::ResolvedInTarget);
    }
    Ok(PortableReferenceStatus::MissingDependency)
}

fn resolve_model_ref(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    bundle_refs: &PreviewBundleRefs,
    provider_ref: &str,
    model_name: &str,
) -> Result<PortableReferenceStatus, BaseError> {
    if bundle_refs
        .models
        .contains(&(provider_ref.to_string(), model_name.to_string()))
    {
        return Ok(PortableReferenceStatus::ResolvedInBundle);
    }
    if provider_repository::find_active_model_by_ref(conn, provider_ref, model_name)?.is_some() {
        return Ok(PortableReferenceStatus::ResolvedInTarget);
    }
    Ok(PortableReferenceStatus::MissingDependency)
}

fn resolve_cost_catalog_ref(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    bundle_refs: &PreviewBundleRefs,
    cost_catalog_ref: &str,
) -> Result<PortableReferenceStatus, BaseError> {
    if bundle_refs.cost_catalogs.contains(cost_catalog_ref) {
        return Ok(PortableReferenceStatus::ResolvedInBundle);
    }
    if cost_repository::find_active_cost_catalog_by_name(conn, cost_catalog_ref)?.is_some() {
        return Ok(PortableReferenceStatus::ResolvedInTarget);
    }
    Ok(PortableReferenceStatus::MissingDependency)
}

fn preview_unsupported_module(module: &PortableBundleModule) -> PortablePreviewModule {
    PortablePreviewModule {
        module_id: module.module_id.clone(),
        module_version: module.module_version,
        label: module.module_id.as_str().to_string(),
        supported: false,
        available: false,
        selected_by_default: false,
        contains_secrets: false,
        deferred: false,
        dependencies: Vec::new(),
        subranges: module.subranges.clone(),
        summary: PortableModuleSummary {
            total: module.summary.total,
            ..PortableModuleSummary::default()
        },
        warnings: vec![format!(
            "portable module `{}` is not supported by this version",
            module.module_id
        )],
        blocking_issues: Vec::new(),
        api_key_items: Vec::new(),
    }
}

fn preview_module(
    module: &PortableBundleModule,
    registry_item: &PortableModuleRegistryItem,
    dependencies: Vec<PortableDependencyStatus>,
    summary: PortableModuleSummary,
    warnings: Vec<String>,
    blocking_issues: Vec<crate::service::portable_config::schema::PortableBlockedItem>,
) -> PortablePreviewModule {
    PortablePreviewModule {
        module_id: module.module_id.clone(),
        module_version: module.module_version,
        label: registry_item.label.clone(),
        supported: true,
        available: !registry_item.deferred,
        selected_by_default: registry_item.default_selected && !registry_item.deferred,
        contains_secrets: registry_item.contains_secrets,
        deferred: registry_item.deferred,
        dependencies,
        subranges: module.subranges.clone(),
        summary,
        warnings,
        blocking_issues,
        api_key_items: Vec::new(),
    }
}

fn add_blocked_issue(
    summary: &mut PortableModuleSummary,
    blocking_issues: &mut Vec<crate::service::portable_config::schema::PortableBlockedItem>,
    code: &str,
    message: impl Into<String>,
    path: String,
    module_id: PortableModuleId,
    subrange_id: PortableSubrangeId,
) {
    summary.blocked += 1;
    blocking_issues.push(blocked_item(
        code,
        message,
        path,
        Some(module_id),
        Some(subrange_id),
    ));
}

fn provider_core_matches(existing: &Provider, item: &PortableProviderItem) -> bool {
    existing.name == item.name
        && existing.endpoint == item.endpoint
        && existing.use_proxy == item.use_proxy
        && existing.is_enabled == item.is_enabled
        && existing.provider_type == item.provider_type
        && existing.provider_api_key_mode == item.provider_api_key_mode
}

fn model_core_matches(existing: &Model, item: &PortableProviderModelItem) -> bool {
    existing.real_model_name == item.real_model_name
        && existing.supports_streaming == item.supports_streaming
        && existing.supports_tools == item.supports_tools
        && existing.supports_reasoning == item.supports_reasoning
        && existing.supports_image_input == item.supports_image_input
        && existing.supports_embeddings == item.supports_embeddings
        && existing.supports_rerank == item.supports_rerank
        && existing.is_enabled == item.is_enabled
}

fn api_key_core_matches(existing: &ApiKey, item: &PortableApiKeyItem) -> bool {
    existing.name == item.name
        && existing.description == item.description
        && existing.default_action == item.default_action
        && existing.is_enabled == item.is_enabled
        && existing.expires_at == item.expires_at
        && existing.rate_limit_rpm == item.rate_limit_rpm
        && existing.max_concurrent_requests == item.max_concurrent_requests
        && existing.quota_daily_requests == item.quota_daily_requests
        && existing.quota_daily_tokens == item.quota_daily_tokens
        && existing.quota_monthly_tokens == item.quota_monthly_tokens
        && existing.budget_daily_nanos == item.budget_daily_nanos
        && existing.budget_daily_currency == item.budget_daily_currency
        && existing.budget_monthly_nanos == item.budget_monthly_nanos
        && existing.budget_monthly_currency == item.budget_monthly_currency
}

fn cost_catalog_core_matches(existing: &CostCatalog, item: &PortableCostCatalogItem) -> bool {
    existing.name == item.name && existing.description == item.description
}

fn cost_catalog_version_matches(
    existing: &CostCatalogVersion,
    item: &PortableCostCatalogVersionItem,
) -> bool {
    existing.version == item.version
        && existing.currency == item.currency
        && existing.source == item.source
        && existing.effective_from == item.effective_from
        && existing.effective_until == item.effective_until
        && existing.is_enabled == item.is_enabled
        && existing.is_archived == item.is_archived
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CostComponentIdentity {
    meter_key: String,
    charge_kind: String,
    priority: i32,
    tier_config_json: Option<String>,
    match_attributes_json: Option<String>,
}

fn portable_cost_component_key(item: &PortableCostComponentItem) -> CostComponentIdentity {
    CostComponentIdentity {
        meter_key: item.meter_key.clone(),
        charge_kind: item.charge_kind.clone(),
        priority: item.priority,
        tier_config_json: canonical_optional_json(item.tier_config_json.as_ref()),
        match_attributes_json: canonical_optional_json(item.match_attributes_json.as_ref()),
    }
}

fn cost_component_key(item: &CostComponent) -> CostComponentIdentity {
    CostComponentIdentity {
        meter_key: item.meter_key.clone(),
        charge_kind: item.charge_kind.clone(),
        priority: item.priority,
        tier_config_json: canonical_json_str(item.tier_config_json.as_deref()),
        match_attributes_json: canonical_json_str(item.match_attributes_json.as_deref()),
    }
}

fn cost_component_matches(existing: &CostComponent, item: &PortableCostComponentItem) -> bool {
    cost_component_key(existing) == portable_cost_component_key(item)
        && existing.unit_price_nanos == item.unit_price_nanos
        && existing.flat_fee_nanos == item.flat_fee_nanos
        && existing.description == item.description
}

fn validate_cost_catalog_version_item(
    item: &PortableCostCatalogVersionItem,
    expected_catalog_ref: &str,
) -> Result<(), String> {
    if item.catalog_ref != expected_catalog_ref {
        return Err("cost catalog version catalog_ref does not match its catalog item".to_string());
    }
    if item.version.trim().is_empty() {
        return Err("cost catalog version must not be empty".to_string());
    }
    if item.currency.len() != 3 {
        return Err("cost catalog version currency must be a 3-letter code".to_string());
    }
    if let Some(effective_until) = item.effective_until
        && effective_until <= item.effective_from
    {
        return Err(
            "cost catalog version effective_until must be after effective_from".to_string(),
        );
    }
    Ok(())
}

fn validate_cost_component_item(item: &PortableCostComponentItem) -> Result<(), String> {
    let tier_config_json = optional_json_to_string(item.tier_config_json.as_ref())
        .map_err(|err| format!("tier_config_json is invalid: {err:?}"))?;
    let match_attributes_json = optional_json_to_string(item.match_attributes_json.as_ref())
        .map_err(|err| format!("match_attributes_json is invalid: {err:?}"))?;
    validate_component_config(
        &item.meter_key,
        &item.charge_kind,
        item.unit_price_nanos,
        item.flat_fee_nanos,
        tier_config_json.as_deref(),
        match_attributes_json.as_deref(),
    )
    .map_err(|err| format!("{err:?}"))
}

fn canonical_optional_json(value: Option<&serde_json::Value>) -> Option<String> {
    value.and_then(|value| serde_json::to_string(value).ok())
}

fn canonical_json_str(value: Option<&str>) -> Option<String> {
    value.and_then(|raw| {
        serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .and_then(|value| serde_json::to_string(&value).ok())
    })
}

fn optional_json_to_string(value: Option<&serde_json::Value>) -> Result<Option<String>, BaseError> {
    value.map(serde_json::to_string).transpose().map_err(|err| {
        BaseError::ParamInvalid(Some(format!(
            "failed to serialize portable cost component JSON: {err}"
        )))
    })
}

#[derive(Debug, Default)]
struct AppliedImport {
    modules: Vec<PortableApplyModuleResult>,
    summary: PortableModuleSummary,
    effects: Vec<AdminMutationEffect>,
}

#[derive(Debug, Default)]
struct ApplyImportContext {
    provider_ids: BTreeMap<String, i64>,
    model_ids: BTreeMap<(String, String), i64>,
    created_provider_refs: BTreeSet<String>,
    created_model_refs: BTreeSet<(String, String)>,
    cost_catalog_ids: BTreeMap<String, i64>,
    effects: Vec<AdminMutationEffect>,
}

impl ApplyImportContext {
    fn remember_provider(&mut self, provider_key: impl Into<String>, provider_id: i64) {
        self.provider_ids.insert(provider_key.into(), provider_id);
    }

    fn remember_created_provider(&mut self, provider_key: impl Into<String>, provider_id: i64) {
        let provider_key = provider_key.into();
        self.provider_ids.insert(provider_key.clone(), provider_id);
        self.created_provider_refs.insert(provider_key);
    }

    fn remember_model(
        &mut self,
        provider_key: impl Into<String>,
        model_name: impl Into<String>,
        model_id: i64,
    ) {
        self.model_ids
            .insert((provider_key.into(), model_name.into()), model_id);
    }

    fn remember_created_model(
        &mut self,
        provider_key: impl Into<String>,
        model_name: impl Into<String>,
        model_id: i64,
    ) {
        let key = (provider_key.into(), model_name.into());
        self.model_ids.insert(key.clone(), model_id);
        self.created_model_refs.insert(key);
    }

    fn remember_cost_catalog(&mut self, name: impl Into<String>, cost_catalog_id: i64) {
        self.cost_catalog_ids.insert(name.into(), cost_catalog_id);
    }
}

fn apply_import_bundle(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    bundle: &PortableBundle,
    selection: &NormalizedApplySelection,
    conflict_strategy: ConflictStrategy,
    bundle_digest: &str,
    reason: &str,
    dangerous_patch_confirmations: &[PortableDangerousPatchConfirmation],
    now: i64,
    secret_encryption: &SecretEncryptionService,
) -> Result<AppliedImport, BaseError> {
    // Keep DB preflight and mutations in one transaction so conflict reads and writes
    // observe the same target state during apply.
    let previews = build_apply_previews(conn, bundle, selection, dangerous_patch_confirmations)?;
    let blocking_results = apply_blocking_results(&previews, conflict_strategy);
    if !blocking_results.is_empty() {
        return Ok(AppliedImport {
            summary: summarize_apply_modules(&blocking_results),
            modules: blocking_results,
            effects: Vec::new(),
        });
    }

    let mut context = ApplyImportContext::default();
    let mut modules = Vec::new();

    if selection.contains(&PortableModuleId::CostCatalogs)
        && let Some(module) = find_bundle_module(bundle, &PortableModuleId::CostCatalogs)
    {
        modules.push(apply_cost_catalogs_module(
            conn,
            module,
            selection,
            conflict_strategy,
            now,
            &mut context,
        )?);
    }
    if selection.contains(&PortableModuleId::ProviderProfile)
        && let Some(module) = find_bundle_module(bundle, &PortableModuleId::ProviderProfile)
    {
        let module_index =
            bundle_module_index(bundle, &PortableModuleId::ProviderProfile).unwrap_or(0);
        modules.push(apply_provider_profile_module(
            conn,
            module_index,
            module,
            selection,
            conflict_strategy,
            now,
            dangerous_patch_confirmations,
            &mut context,
        )?);
    }
    if selection.contains(&PortableModuleId::CostBindings)
        && let Some(module) = find_bundle_module(bundle, &PortableModuleId::CostBindings)
    {
        modules.push(apply_cost_bindings_module(
            conn,
            module,
            conflict_strategy,
            now,
            &mut context,
        )?);
    }
    if selection.contains(&PortableModuleId::ApiKeys)
        && let Some(module) = find_bundle_module(bundle, &PortableModuleId::ApiKeys)
    {
        modules.push(apply_api_keys_module(
            conn,
            module,
            selection,
            conflict_strategy,
            now,
            &mut context,
            secret_encryption,
        )?);
    }

    let summary = summarize_apply_modules(&modules);
    context
        .effects
        .push(AdminMutationEffect::audit(portable_import_audit_event(
            conflict_strategy,
            bundle_digest,
            reason,
            &summary,
            selection,
        )));

    Ok(AppliedImport {
        modules,
        summary,
        effects: context.effects,
    })
}

fn build_apply_previews(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    bundle: &PortableBundle,
    selection: &NormalizedApplySelection,
    dangerous_patch_confirmations: &[PortableDangerousPatchConfirmation],
) -> Result<Vec<PortablePreviewModule>, BaseError> {
    let registry = import_registry_by_module_id();
    let bundle_refs = collect_selected_bundle_refs(bundle, selection);
    let mut previews = Vec::new();

    for module_id in [
        PortableModuleId::CostCatalogs,
        PortableModuleId::ProviderProfile,
        PortableModuleId::CostBindings,
        PortableModuleId::ApiKeys,
    ] {
        if !selection.contains(&module_id) {
            continue;
        }
        let Some(module) = find_bundle_module(bundle, &module_id) else {
            continue;
        };
        let Some(registry_item) = registry.get(&module.module_id) else {
            previews.push(preview_unsupported_module(module));
            continue;
        };
        let module_index = bundle_module_index(bundle, &module.module_id).unwrap_or(0);
        let preview_module_input = selected_module_view(module, selection);
        previews.push(match &preview_module_input.module_id {
            PortableModuleId::ProviderProfile => preview_provider_profile_module(
                conn,
                module_index,
                &preview_module_input,
                registry_item,
                dangerous_patch_confirmations,
            )?,
            PortableModuleId::ApiKeys => preview_api_keys_module(
                conn,
                module_index,
                &preview_module_input,
                registry_item,
                &bundle_refs,
            )?,
            PortableModuleId::CostCatalogs => preview_cost_catalogs_module(
                conn,
                module_index,
                &preview_module_input,
                registry_item,
            )?,
            PortableModuleId::CostBindings => preview_cost_bindings_module(
                conn,
                module_index,
                &preview_module_input,
                registry_item,
                &bundle_refs,
            )?,
            _ => preview_deferred_or_invalid_module(
                module_index,
                &preview_module_input,
                registry_item,
            ),
        });
    }

    Ok(previews)
}

fn collect_selected_bundle_refs(
    bundle: &PortableBundle,
    selection: &NormalizedApplySelection,
) -> PreviewBundleRefs {
    let mut filtered = PortableBundle {
        schema_version: bundle.schema_version.clone(),
        exported_at: bundle.exported_at,
        cyder_version: bundle.cyder_version.clone(),
        modules: bundle
            .modules
            .iter()
            .filter(|module| selection.contains(&module.module_id))
            .map(|module| {
                let mut selected = module.clone();
                if let Some(subranges) = selection.subranges(&module.module_id) {
                    selected.subranges = subranges.to_vec();
                }
                selected
            })
            .collect(),
    };
    filtered
        .modules
        .sort_by(|left, right| left.module_id.cmp(&right.module_id));
    collect_bundle_refs(&filtered)
}

fn selected_module_view(
    module: &PortableBundleModule,
    selection: &NormalizedApplySelection,
) -> PortableBundleModule {
    let mut selected = module.clone();
    if let Some(subranges) = selection.subranges(&module.module_id) {
        selected.subranges = subranges.to_vec();
    }
    selected
}

fn apply_blocking_results(
    previews: &[PortablePreviewModule],
    conflict_strategy: ConflictStrategy,
) -> Vec<PortableApplyModuleResult> {
    let mut results = Vec::new();

    for preview in previews {
        let blocking_issues = preview
            .blocking_issues
            .iter()
            .filter(|issue| !is_ignorable_apply_issue(issue, conflict_strategy))
            .cloned()
            .collect::<Vec<_>>();
        if blocking_issues.is_empty() {
            continue;
        }
        results.push(PortableApplyModuleResult {
            module_id: preview.module_id.clone(),
            status: PortableApplyModuleStatus::Blocked,
            summary: preview.summary.clone(),
            messages: vec![
                "portable import apply was not started because preflight found blocking issues"
                    .to_string(),
            ],
            blocking_issues,
        });
    }

    results
}

fn is_ignorable_apply_issue(
    issue: &PortableBlockedItem,
    conflict_strategy: ConflictStrategy,
) -> bool {
    if issue.code == "conflict" && conflict_strategy != ConflictStrategy::FailOnConflict {
        return true;
    }
    false
}

fn summarize_apply_modules(modules: &[PortableApplyModuleResult]) -> PortableModuleSummary {
    modules
        .iter()
        .fold(PortableModuleSummary::default(), |mut acc, module| {
            acc.total += module.summary.total;
            acc.create += module.summary.create;
            acc.update += module.summary.update;
            acc.skip += module.summary.skip;
            acc.blocked += module.summary.blocked;
            acc.conflict += module.summary.conflict;
            acc
        })
}

fn find_bundle_module<'a>(
    bundle: &'a PortableBundle,
    module_id: &PortableModuleId,
) -> Option<&'a PortableBundleModule> {
    bundle
        .modules
        .iter()
        .find(|module| &module.module_id == module_id)
}

fn bundle_module_index(bundle: &PortableBundle, module_id: &PortableModuleId) -> Option<usize> {
    bundle
        .modules
        .iter()
        .position(|module| &module.module_id == module_id)
}

fn apply_provider_profile_module(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module_index: usize,
    module: &PortableBundleModule,
    selection: &NormalizedApplySelection,
    conflict_strategy: ConflictStrategy,
    now: i64,
    dangerous_patch_confirmations: &[PortableDangerousPatchConfirmation],
    context: &mut ApplyImportContext,
) -> Result<PortableApplyModuleResult, BaseError> {
    let items = serde_json::from_value::<PortableProviderProfileItems>(module.items.clone())
        .map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "provider_profile items are invalid during apply: {err}"
            )))
        })?;
    let include_keys = selection.subrange_selected(
        &PortableModuleId::ProviderProfile,
        &PortableSubrangeId::ProviderKeys,
    );
    let include_models = selection.subrange_selected(
        &PortableModuleId::ProviderProfile,
        &PortableSubrangeId::ProviderModels,
    );
    let include_request_patches = selection.subrange_selected(
        &PortableModuleId::ProviderProfile,
        &PortableSubrangeId::ProviderRequestPatches,
    );
    let include_reasoning_config = selection.subrange_selected(
        &PortableModuleId::ProviderProfile,
        &PortableSubrangeId::ProviderReasoningConfig,
    );
    let mut summary = PortableModuleSummary::default();
    let mut messages = Vec::new();

    for provider in &items.providers {
        summary.total += 1;
        let provider_id =
            match provider_repository::find_active_provider_by_key(conn, &provider.provider_key)? {
                Some(existing) if provider_core_matches(&existing, provider) => {
                    summary.skip += 1;
                    existing.id
                }
                Some(existing) => match conflict_strategy {
                    ConflictStrategy::FailOnConflict => {
                        return Err(BaseError::ParamInvalid(Some(format!(
                            "provider `{}` conflicts with target environment",
                            provider.provider_key
                        ))));
                    }
                    ConflictStrategy::SkipExisting => {
                        summary.skip += 1;
                        messages.push(format!(
                            "skipped existing provider `{}` because fields differ",
                            provider.provider_key
                        ));
                        existing.id
                    }
                    ConflictStrategy::OverwriteExisting => {
                        let updated = provider_repository::update_provider(
                            conn,
                            existing.id,
                            &provider_update_data(provider),
                            now,
                        )?;
                        summary.update += 1;
                        context
                            .effects
                            .push(AdminMutationEffect::catalog_invalidation(
                                AdminCatalogInvalidation::Provider {
                                    id: updated.id,
                                    key: Some(updated.provider_key.clone()),
                                },
                            ));
                        updated.id
                    }
                },
                None => {
                    let created = provider_repository::insert_provider(
                        conn,
                        &NewProvider {
                            id: ID_GENERATOR.generate_id(),
                            provider_key: provider.provider_key.clone(),
                            name: provider.name.clone(),
                            endpoint: provider.endpoint.clone(),
                            use_proxy: provider.use_proxy,
                            is_enabled: provider.is_enabled,
                            created_at: now,
                            updated_at: now,
                            provider_type: provider.provider_type.clone(),
                            provider_api_key_mode: provider.provider_api_key_mode.clone(),
                        },
                    )?;
                    summary.create += 1;
                    context
                        .effects
                        .push(AdminMutationEffect::catalog_invalidation(
                            AdminCatalogInvalidation::Provider {
                                id: created.id,
                                key: Some(created.provider_key.clone()),
                            },
                        ));
                    context
                        .effects
                        .push(AdminMutationEffect::catalog_invalidation(
                            AdminCatalogInvalidation::ModelsCatalog,
                        ));
                    context.remember_created_provider(provider.provider_key.clone(), created.id);
                    created.id
                }
            };
        if !context
            .created_provider_refs
            .contains(&provider.provider_key)
        {
            context.remember_provider(provider.provider_key.clone(), provider_id);
        }

        if include_keys {
            apply_provider_keys(conn, provider, provider_id, now, &mut summary, context)?;
        }
        if include_models {
            apply_provider_models(
                conn,
                provider,
                provider_id,
                conflict_strategy,
                now,
                &mut summary,
                context,
                &mut messages,
            )?;
        }
    }

    if include_request_patches {
        apply_provider_request_patches(
            conn,
            module_index,
            &items.request_patches,
            dangerous_patch_confirmations,
            now,
            &mut summary,
            context,
            &mut messages,
        )?;
    }
    if include_reasoning_config {
        apply_provider_reasoning_configs(
            conn,
            &items.reasoning_configs,
            conflict_strategy,
            now,
            &mut summary,
            context,
            &mut messages,
        )?;
    }

    Ok(PortableApplyModuleResult {
        module_id: PortableModuleId::ProviderProfile,
        status: apply_status_from_summary(&summary),
        summary,
        messages,
        blocking_issues: Vec::new(),
    })
}

fn apply_provider_keys(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    provider: &PortableProviderItem,
    provider_id: i64,
    now: i64,
    summary: &mut PortableModuleSummary,
    context: &mut ApplyImportContext,
) -> Result<(), BaseError> {
    let mut saw_key_change = false;
    let mut seen_keys = BTreeSet::new();

    for key in &provider.keys {
        summary.total += 1;
        if !seen_keys.insert(key.api_key.clone()) {
            summary.skip += 1;
            continue;
        }
        match provider_repository::insert_provider_api_key_if_missing_by_raw_key(
            conn,
            &NewProviderApiKey {
                id: ID_GENERATOR.generate_id(),
                provider_id,
                api_key: key.api_key.clone(),
                description: key.description.clone(),
                is_enabled: key.is_enabled,
                created_at: now,
                updated_at: now,
            },
        )? {
            provider_repository::ProviderApiKeyImportOutcome::Created(_) => {
                summary.create += 1;
                saw_key_change = true;
            }
            provider_repository::ProviderApiKeyImportOutcome::Existing(_) => {
                summary.skip += 1;
            }
        }
    }

    if saw_key_change {
        context
            .effects
            .push(AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::ProviderApiKeys { provider_id },
            ));
    }

    Ok(())
}

fn apply_provider_models(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    provider: &PortableProviderItem,
    provider_id: i64,
    conflict_strategy: ConflictStrategy,
    now: i64,
    summary: &mut PortableModuleSummary,
    context: &mut ApplyImportContext,
    messages: &mut Vec<String>,
) -> Result<(), BaseError> {
    let mut seen_models = BTreeSet::new();

    for model in &provider.models {
        summary.total += 1;
        if !seen_models.insert(model.model_name.clone()) {
            summary.skip += 1;
            continue;
        }
        let model_id = match provider_repository::find_active_model_for_provider(
            conn,
            provider_id,
            &model.model_name,
        )? {
            Some(existing) if model_core_matches(&existing, model) => {
                summary.skip += 1;
                existing.id
            }
            Some(existing) => match conflict_strategy {
                ConflictStrategy::FailOnConflict => {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "model `{}/{}` conflicts with target environment",
                        provider.provider_key, model.model_name
                    ))));
                }
                ConflictStrategy::SkipExisting => {
                    summary.skip += 1;
                    messages.push(format!(
                        "skipped existing model `{}/{}` because fields differ",
                        provider.provider_key, model.model_name
                    ));
                    existing.id
                }
                ConflictStrategy::OverwriteExisting => {
                    let updated = provider_repository::update_model(
                        conn,
                        existing.id,
                        &model_update_data(model),
                        now,
                    )?;
                    summary.update += 1;
                    context
                        .effects
                        .push(AdminMutationEffect::catalog_invalidation(
                            AdminCatalogInvalidation::Model {
                                id: updated.id,
                                name: Some(AdminModelCacheName::new(
                                    provider.provider_key.clone(),
                                    updated.model_name.clone(),
                                )),
                                previous_name: None,
                            },
                        ));
                    updated.id
                }
            },
            None => {
                let created = provider_repository::insert_model(
                    conn,
                    &NewModel {
                        id: ID_GENERATOR.generate_id(),
                        provider_id,
                        model_name: model.model_name.clone(),
                        real_model_name: model.real_model_name.clone(),
                        supports_streaming: model.supports_streaming,
                        supports_tools: model.supports_tools,
                        supports_reasoning: model.supports_reasoning,
                        supports_image_input: model.supports_image_input,
                        supports_embeddings: model.supports_embeddings,
                        supports_rerank: model.supports_rerank,
                        is_enabled: model.is_enabled,
                        created_at: now,
                        updated_at: now,
                    },
                )?;
                summary.create += 1;
                context
                    .effects
                    .push(AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::ModelsCatalog,
                    ));
                context
                    .effects
                    .push(AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::Model {
                            id: created.id,
                            name: Some(AdminModelCacheName::new(
                                provider.provider_key.clone(),
                                created.model_name.clone(),
                            )),
                            previous_name: None,
                        },
                    ));
                context.remember_created_model(
                    provider.provider_key.clone(),
                    model.model_name.clone(),
                    created.id,
                );
                created.id
            }
        };
        if !context
            .created_model_refs
            .contains(&(provider.provider_key.clone(), model.model_name.clone()))
        {
            context.remember_model(
                provider.provider_key.clone(),
                model.model_name.clone(),
                model_id,
            );
        }
    }

    Ok(())
}

fn apply_provider_request_patches(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module_index: usize,
    request_patches: &[PortableProviderRequestPatchItem],
    dangerous_patch_confirmations: &[PortableDangerousPatchConfirmation],
    now: i64,
    summary: &mut PortableModuleSummary,
    context: &mut ApplyImportContext,
    messages: &mut Vec<String>,
) -> Result<(), BaseError> {
    let mut invalidated_providers = BTreeSet::new();
    let mut invalidated_models = BTreeSet::new();

    for (patch_index, patch) in request_patches.iter().enumerate() {
        summary.total += 1;
        let path = format!("$.modules[{module_index}].items.request_patches[{patch_index}]");
        let Some(owner_key) = portable_owner_key(&patch.owner) else {
            return Err(BaseError::ParamInvalid(Some(
                owner_ref_path_message(&patch.owner).to_string(),
            )));
        };
        let Some(owner_target) = resolve_created_request_patch_owner(conn, context, &owner_key)?
        else {
            summary.skip += 1;
            messages.push(format!(
                "skipped request patch for {} because the owner already exists or was not created by this import",
                owner_key.label()
            ));
            continue;
        };

        let validation = validate_request_patch_item_with_confirmations(
            patch,
            &path,
            dangerous_patch_confirmations,
        )?;
        if let Some(confirmation) = validation.confirmation {
            return Err(BaseError::ParamInvalid(Some(format!(
                "request patch target `{}` requires confirmation: {}",
                confirmation.target, confirmation.reason
            ))));
        }
        let (provider_id, model_id) = match owner_target {
            RequestPatchImportOwner::Provider(provider_id) => (Some(provider_id), None),
            RequestPatchImportOwner::Model(model_id) => (None, Some(model_id)),
        };
        let inserted = request_patch_repository::insert_request_patch_rule(
            conn,
            &NewRequestPatchRule {
                id: ID_GENERATOR.generate_id(),
                provider_id,
                model_id,
                placement: validation.placement,
                target: validation.target,
                operation: validation.operation,
                value_json: validation.value_json,
                description: validation.description,
                is_enabled: validation.is_enabled,
                created_at: now,
                updated_at: now,
            },
        )?;
        summary.create += 1;
        if let Some(provider_id) = inserted.provider_id {
            invalidated_providers.insert(provider_id);
        }
        if let Some(model_id) = inserted.model_id {
            invalidated_models.insert(model_id);
        }
    }

    for provider_id in invalidated_providers {
        context
            .effects
            .push(AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::ProviderRequestPatchRules { provider_id },
            ));
    }
    for model_id in invalidated_models {
        context
            .effects
            .push(AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::ModelRequestPatchRules { model_id },
            ));
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum RequestPatchImportOwner {
    Provider(i64),
    Model(i64),
}

fn resolve_created_request_patch_owner(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    context: &ApplyImportContext,
    owner_key: &PortableOwnerKey,
) -> Result<Option<RequestPatchImportOwner>, BaseError> {
    match owner_key {
        PortableOwnerKey::Provider(provider_key) => {
            if context.created_provider_refs.contains(provider_key) {
                let provider_id = resolve_provider_id(conn, context, provider_key)?.ok_or_else(|| {
                    BaseError::ParamInvalid(Some(format!(
                        "imported provider `{provider_key}` disappeared before request patch import"
                    )))
                })?;
                return Ok(Some(RequestPatchImportOwner::Provider(provider_id)));
            }
            Ok(None)
        }
        PortableOwnerKey::Model {
            provider_key,
            model_name,
        } => {
            if context
                .created_model_refs
                .contains(&(provider_key.clone(), model_name.clone()))
            {
                let model_id = resolve_model_id(conn, context, provider_key, model_name)?
                    .ok_or_else(|| {
                        BaseError::ParamInvalid(Some(format!(
                            "imported model `{provider_key}/{model_name}` disappeared before request patch import"
                        )))
                    })?;
                return Ok(Some(RequestPatchImportOwner::Model(model_id)));
            }
            Ok(None)
        }
    }
}

fn apply_provider_reasoning_configs(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    reasoning_configs: &[PortableProviderReasoningConfigItem],
    conflict_strategy: ConflictStrategy,
    now: i64,
    summary: &mut PortableModuleSummary,
    context: &mut ApplyImportContext,
    messages: &mut Vec<String>,
) -> Result<(), BaseError> {
    let mut invalidated_providers = BTreeSet::new();
    let mut invalidated_models = BTreeSet::new();

    for config in reasoning_configs {
        summary.total += 1;
        validate_reasoning_config_item(config)?;
        let Some(owner_key) = portable_owner_key(&config.owner) else {
            return Err(BaseError::ParamInvalid(Some(
                owner_ref_path_message(&config.owner).to_string(),
            )));
        };
        let owner_target = resolve_reasoning_owner_id(conn, context, &owner_key)?;
        let existing = match owner_target {
            ReasoningImportOwner::Provider(provider_id) => {
                reasoning_config_repository::get_active_provider_reasoning_config(
                    conn,
                    provider_id,
                )?
            }
            ReasoningImportOwner::Model(model_id) => {
                reasoning_config_repository::get_active_model_reasoning_config(conn, model_id)?
            }
        };
        if let Some(existing) = existing.as_ref() {
            if reasoning_config_matches(existing, config) {
                summary.skip += 1;
                continue;
            }
            match conflict_strategy {
                ConflictStrategy::FailOnConflict => {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "reasoning config for {} conflicts with target environment",
                        owner_key.label()
                    ))));
                }
                ConflictStrategy::SkipExisting => {
                    summary.skip += 1;
                    messages.push(format!(
                        "skipped existing reasoning config for {} because fields differ",
                        owner_key.label()
                    ));
                    continue;
                }
                ConflictStrategy::OverwriteExisting => {}
            }
        }

        let input = reasoning_config_import_input(config, owner_target, now)?;
        let saved = reasoning_config_repository::upsert_reasoning_config(conn, &input)?;
        if existing.is_some() {
            summary.update += 1;
        } else {
            summary.create += 1;
        }
        match saved.scope {
            ReasoningConfigScope::Provider => {
                if let Some(provider_id) = saved.config.provider_id {
                    invalidated_providers.insert(provider_id);
                }
            }
            ReasoningConfigScope::Model => {
                if let Some(model_id) = saved.config.model_id {
                    invalidated_models.insert(model_id);
                }
            }
        }
    }

    for provider_id in invalidated_providers {
        context
            .effects
            .push(AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::ReasoningProviderConfig { provider_id },
            ));
    }
    for model_id in invalidated_models {
        context
            .effects
            .push(AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::ReasoningModelConfig { model_id },
            ));
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum ReasoningImportOwner {
    Provider(i64),
    Model(i64),
}

fn resolve_reasoning_owner_id(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    context: &ApplyImportContext,
    owner_key: &PortableOwnerKey,
) -> Result<ReasoningImportOwner, BaseError> {
    match owner_key {
        PortableOwnerKey::Provider(provider_key) => {
            let provider_id = resolve_provider_id(conn, context, provider_key)?.ok_or_else(|| {
                BaseError::ParamInvalid(Some(format!(
                    "provider `{provider_key}` referenced by reasoning config was not imported or found"
                )))
            })?;
            Ok(ReasoningImportOwner::Provider(provider_id))
        }
        PortableOwnerKey::Model {
            provider_key,
            model_name,
        } => {
            let model_id = resolve_model_id(conn, context, provider_key, model_name)?
                .ok_or_else(|| {
                    BaseError::ParamInvalid(Some(format!(
                        "model `{provider_key}/{model_name}` referenced by reasoning config was not imported or found"
                    )))
                })?;
            Ok(ReasoningImportOwner::Model(model_id))
        }
    }
}

fn reasoning_config_import_input(
    item: &PortableProviderReasoningConfigItem,
    owner: ReasoningImportOwner,
    now: i64,
) -> Result<reasoning_config_repository::ReasoningConfigImportInput, BaseError> {
    let (scope, owner_id) = match owner {
        ReasoningImportOwner::Provider(provider_id) => {
            (ReasoningConfigScope::Provider, provider_id)
        }
        ReasoningImportOwner::Model(model_id) => (ReasoningConfigScope::Model, model_id),
    };
    Ok(reasoning_config_repository::ReasoningConfigImportInput {
        scope,
        owner_id,
        mode: item.mode,
        family_key: item.family_key.clone(),
        presets: reasoning_preset_inputs(&item.presets)?,
        now,
    })
}

fn reasoning_preset_inputs(
    presets: &[PortableReasoningConfigPresetItem],
) -> Result<Vec<ReasoningConfigPresetInput>, BaseError> {
    presets
        .iter()
        .map(|preset| {
            let preset_key = ReasoningPreset::from_str(&preset.preset_key)
                .map_err(|err| BaseError::ParamInvalid(Some(err.to_string())))?;
            Ok(ReasoningConfigPresetInput {
                preset_key: preset_key.as_key().to_string(),
                expose_in_models: preset.expose_in_models,
                is_enabled: preset.is_enabled,
            })
        })
        .collect()
}

fn apply_cost_catalogs_module(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module: &PortableBundleModule,
    selection: &NormalizedApplySelection,
    conflict_strategy: ConflictStrategy,
    now: i64,
    context: &mut ApplyImportContext,
) -> Result<PortableApplyModuleResult, BaseError> {
    let items = serde_json::from_value::<PortableCostCatalogItems>(module.items.clone()).map_err(
        |err| {
            BaseError::ParamInvalid(Some(format!(
                "cost_catalogs items are invalid during apply: {err}"
            )))
        },
    )?;
    let include_versions = selection.subrange_selected(
        &PortableModuleId::CostCatalogs,
        &PortableSubrangeId::CostCatalogVersions,
    );
    let include_components = selection.subrange_selected(
        &PortableModuleId::CostCatalogs,
        &PortableSubrangeId::CostComponents,
    );
    let mut summary = PortableModuleSummary::default();
    let mut messages = Vec::new();
    let mut invalidated_version_ids = BTreeSet::new();
    let mut seen_catalogs = BTreeSet::new();

    for catalog in &items.catalogs {
        summary.total += 1;
        if catalog.name.trim().is_empty() {
            return Err(BaseError::ParamInvalid(Some(
                "cost catalog name must not be empty".to_string(),
            )));
        }
        if !seen_catalogs.insert(catalog.name.clone()) {
            summary.skip += 1;
            continue;
        }

        let catalog_id =
            match cost_repository::find_active_cost_catalog_by_name(conn, &catalog.name)? {
                Some(existing) if cost_catalog_core_matches(&existing, catalog) => {
                    summary.skip += 1;
                    existing.id
                }
                Some(existing) => match conflict_strategy {
                    ConflictStrategy::FailOnConflict => {
                        return Err(BaseError::ParamInvalid(Some(format!(
                            "cost catalog `{}` conflicts with target environment",
                            catalog.name
                        ))));
                    }
                    ConflictStrategy::SkipExisting => {
                        summary.skip += 1;
                        messages.push(format!(
                            "skipped existing cost catalog `{}` because fields differ",
                            catalog.name
                        ));
                        existing.id
                    }
                    ConflictStrategy::OverwriteExisting => {
                        let updated = cost_repository::update_cost_catalog(
                            conn,
                            existing.id,
                            &UpdateCostCatalogData {
                                name: None,
                                description: Some(catalog.description.clone()),
                            },
                            now,
                        )?;
                        summary.update += 1;
                        updated.id
                    }
                },
                None => {
                    let created = cost_repository::insert_cost_catalog(
                        conn,
                        &NewCostCatalog {
                            id: ID_GENERATOR.generate_id(),
                            name: catalog.name.clone(),
                            description: catalog.description.clone(),
                            created_at: now,
                            updated_at: now,
                        },
                    )?;
                    summary.create += 1;
                    created.id
                }
            };
        context.remember_cost_catalog(catalog.name.clone(), catalog_id);

        if include_versions {
            apply_cost_catalog_versions(
                conn,
                catalog,
                catalog_id,
                include_components,
                conflict_strategy,
                now,
                &mut summary,
                &mut messages,
                &mut invalidated_version_ids,
            )?;
        }
    }

    if !invalidated_version_ids.is_empty() {
        context
            .effects
            .push(AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::CostCatalogVersions {
                    ids: invalidated_version_ids.into_iter().collect(),
                },
            ));
    }

    Ok(PortableApplyModuleResult {
        module_id: PortableModuleId::CostCatalogs,
        status: apply_status_from_summary(&summary),
        summary,
        messages,
        blocking_issues: Vec::new(),
    })
}

fn apply_cost_catalog_versions(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    catalog: &PortableCostCatalogItem,
    catalog_id: i64,
    include_components: bool,
    conflict_strategy: ConflictStrategy,
    now: i64,
    summary: &mut PortableModuleSummary,
    messages: &mut Vec<String>,
    invalidated_version_ids: &mut BTreeSet<i64>,
) -> Result<(), BaseError> {
    let mut seen_versions = BTreeSet::new();
    for version in &catalog.versions {
        summary.total += 1;
        if let Err(message) = validate_cost_catalog_version_item(version, &catalog.name) {
            return Err(BaseError::ParamInvalid(Some(message)));
        }
        if !seen_versions.insert(version.version.clone()) {
            summary.skip += 1;
            continue;
        }

        let version_id = match cost_repository::find_cost_catalog_version_by_catalog_and_version(
            conn,
            catalog_id,
            &version.version,
        )? {
            Some(existing) if cost_catalog_version_matches(&existing, version) => {
                summary.skip += 1;
                existing.id
            }
            Some(existing) => match conflict_strategy {
                ConflictStrategy::FailOnConflict => {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "cost catalog version `{}/{}` conflicts with target environment",
                        catalog.name, version.version
                    ))));
                }
                ConflictStrategy::SkipExisting => {
                    summary.skip += 1;
                    messages.push(format!(
                        "skipped existing cost catalog version `{}/{}` because fields differ",
                        catalog.name, version.version
                    ));
                    existing.id
                }
                ConflictStrategy::OverwriteExisting => {
                    let updated = cost_repository::update_cost_catalog_version(
                        conn,
                        existing.id,
                        &UpdateCostCatalogVersionData {
                            currency: Some(version.currency.clone()),
                            source: Some(version.source.clone()),
                            effective_from: Some(version.effective_from),
                            effective_until: Some(version.effective_until),
                            first_used_at: None,
                            is_archived: Some(version.is_archived),
                            is_enabled: Some(version.is_enabled),
                        },
                        now,
                    )?;
                    summary.update += 1;
                    invalidated_version_ids.insert(updated.id);
                    updated.id
                }
            },
            None => {
                let created = cost_repository::insert_cost_catalog_version(
                    conn,
                    &NewCostCatalogVersion {
                        id: ID_GENERATOR.generate_id(),
                        catalog_id,
                        version: version.version.clone(),
                        currency: version.currency.clone(),
                        source: version.source.clone(),
                        effective_from: version.effective_from,
                        effective_until: version.effective_until,
                        first_used_at: None,
                        is_archived: version.is_archived,
                        is_enabled: version.is_enabled,
                        created_at: now,
                        updated_at: now,
                    },
                )?;
                summary.create += 1;
                invalidated_version_ids.insert(created.id);
                created.id
            }
        };

        if include_components {
            apply_cost_components(
                conn,
                version,
                version_id,
                conflict_strategy,
                now,
                summary,
                messages,
                invalidated_version_ids,
            )?;
        }
    }
    Ok(())
}

fn apply_cost_components(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    version: &PortableCostCatalogVersionItem,
    version_id: i64,
    conflict_strategy: ConflictStrategy,
    now: i64,
    summary: &mut PortableModuleSummary,
    messages: &mut Vec<String>,
    invalidated_version_ids: &mut BTreeSet<i64>,
) -> Result<(), BaseError> {
    let existing_components = cost_repository::list_cost_components_for_export(conn, version_id)?;
    let mut seen_components = BTreeSet::new();
    for component in &version.components {
        summary.total += 1;
        if let Err(message) = validate_cost_component_item(component) {
            return Err(BaseError::ParamInvalid(Some(message)));
        }
        let key = portable_cost_component_key(component);
        if !seen_components.insert(key.clone()) {
            summary.skip += 1;
            continue;
        }
        let existing = existing_components
            .iter()
            .find(|existing| cost_component_key(existing) == key);
        match existing {
            Some(existing) if cost_component_matches(existing, component) => {
                summary.skip += 1;
            }
            Some(existing) => match conflict_strategy {
                ConflictStrategy::FailOnConflict => {
                    return Err(BaseError::ParamInvalid(Some(format!(
                        "cost component `{}` conflicts with target environment",
                        component.meter_key
                    ))));
                }
                ConflictStrategy::SkipExisting => {
                    summary.skip += 1;
                    messages.push(format!(
                        "skipped existing cost component `{}` because fields differ",
                        component.meter_key
                    ));
                }
                ConflictStrategy::OverwriteExisting => {
                    let updated = cost_repository::update_cost_component(
                        conn,
                        existing.id,
                        &cost_component_update_data(component)?,
                        now,
                    )?;
                    summary.update += 1;
                    invalidated_version_ids.insert(updated.catalog_version_id);
                }
            },
            None => {
                let inserted = cost_repository::insert_cost_component(
                    conn,
                    &NewCostComponent {
                        id: ID_GENERATOR.generate_id(),
                        catalog_version_id: version_id,
                        meter_key: component.meter_key.clone(),
                        charge_kind: component.charge_kind.clone(),
                        unit_price_nanos: component.unit_price_nanos,
                        flat_fee_nanos: component.flat_fee_nanos,
                        tier_config_json: optional_json_to_string(
                            component.tier_config_json.as_ref(),
                        )?,
                        match_attributes_json: optional_json_to_string(
                            component.match_attributes_json.as_ref(),
                        )?,
                        priority: component.priority,
                        description: component.description.clone(),
                        created_at: now,
                        updated_at: now,
                    },
                )?;
                summary.create += 1;
                invalidated_version_ids.insert(inserted.catalog_version_id);
            }
        }
    }
    Ok(())
}

fn cost_component_update_data(
    component: &PortableCostComponentItem,
) -> Result<UpdateCostComponentData, BaseError> {
    Ok(UpdateCostComponentData {
        meter_key: Some(component.meter_key.clone()),
        charge_kind: Some(component.charge_kind.clone()),
        unit_price_nanos: Some(component.unit_price_nanos),
        flat_fee_nanos: Some(component.flat_fee_nanos),
        tier_config_json: Some(optional_json_to_string(
            component.tier_config_json.as_ref(),
        )?),
        match_attributes_json: Some(optional_json_to_string(
            component.match_attributes_json.as_ref(),
        )?),
        priority: Some(component.priority),
        description: Some(component.description.clone()),
    })
}

fn apply_cost_bindings_module(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module: &PortableBundleModule,
    conflict_strategy: ConflictStrategy,
    now: i64,
    context: &mut ApplyImportContext,
) -> Result<PortableApplyModuleResult, BaseError> {
    let items = serde_json::from_value::<Vec<PortableCostBindingItem>>(module.items.clone())
        .map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "cost_bindings items are invalid during apply: {err}"
            )))
        })?;
    let mut summary = PortableModuleSummary::default();
    let mut messages = Vec::new();
    let mut invalidated_version_ids = BTreeSet::new();
    let mut seen_bindings = BTreeSet::new();

    for binding in &items {
        summary.total += 1;
        if binding.target_kind != "model" {
            return Err(BaseError::ParamInvalid(Some(
                "cost binding target_kind must be `model` in this version".to_string(),
            )));
        }
        let model_ref = binding.model_ref.as_ref().ok_or_else(|| {
            BaseError::ParamInvalid(Some("model cost binding requires model_ref".to_string()))
        })?;
        if !seen_bindings.insert((model_ref.provider_key.clone(), model_ref.model_name.clone())) {
            summary.skip += 1;
            continue;
        }
        let target_catalog_id = resolve_cost_catalog_id(conn, context, &binding.cost_catalog_ref)?
            .ok_or_else(|| {
                BaseError::ParamInvalid(Some(format!(
                    "cost catalog `{}` referenced by cost binding was not imported or found",
                    binding.cost_catalog_ref
                )))
            })?;
        let existing_model = provider_repository::find_active_model_by_ref(
            conn,
            &model_ref.provider_key,
            &model_ref.model_name,
        )?
        .ok_or_else(|| {
            BaseError::ParamInvalid(Some(format!(
                "model `{}/{}` referenced by cost binding was not imported or found",
                model_ref.provider_key, model_ref.model_name
            )))
        })?;

        if existing_model.cost_catalog_id == Some(target_catalog_id) {
            summary.skip += 1;
            continue;
        }
        if existing_model.cost_catalog_id.is_some()
            && conflict_strategy == ConflictStrategy::FailOnConflict
        {
            return Err(BaseError::ParamInvalid(Some(format!(
                "model `{}/{}` already has a different cost catalog binding",
                model_ref.provider_key, model_ref.model_name
            ))));
        }
        if existing_model.cost_catalog_id.is_some()
            && conflict_strategy == ConflictStrategy::SkipExisting
        {
            summary.skip += 1;
            messages.push(format!(
                "skipped existing cost binding for model `{}/{}`",
                model_ref.provider_key, model_ref.model_name
            ));
            continue;
        }

        let updated = cost_repository::update_model_cost_catalog(
            conn,
            existing_model.id,
            Some(target_catalog_id),
            now,
        )?;
        summary.update += 1;
        context.remember_model(
            model_ref.provider_key.clone(),
            model_ref.model_name.clone(),
            updated.id,
        );
        context
            .effects
            .push(AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::ModelsCatalog,
            ));
        context
            .effects
            .push(AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::Model {
                    id: updated.id,
                    name: Some(AdminModelCacheName::new(
                        model_ref.provider_key.clone(),
                        updated.model_name.clone(),
                    )),
                    previous_name: None,
                },
            ));
        if let Some(old_catalog_id) = existing_model.cost_catalog_id {
            invalidated_version_ids.extend(cost_repository::list_cost_catalog_version_ids(
                conn,
                old_catalog_id,
            )?);
        }
        invalidated_version_ids.extend(cost_repository::list_cost_catalog_version_ids(
            conn,
            target_catalog_id,
        )?);
    }

    if !invalidated_version_ids.is_empty() {
        context
            .effects
            .push(AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::CostCatalogVersions {
                    ids: invalidated_version_ids.into_iter().collect(),
                },
            ));
    }

    Ok(PortableApplyModuleResult {
        module_id: PortableModuleId::CostBindings,
        status: apply_status_from_summary(&summary),
        summary,
        messages,
        blocking_issues: Vec::new(),
    })
}

fn apply_api_keys_module(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    module: &PortableBundleModule,
    selection: &NormalizedApplySelection,
    conflict_strategy: ConflictStrategy,
    now: i64,
    context: &mut ApplyImportContext,
    secret_encryption: &SecretEncryptionService,
) -> Result<PortableApplyModuleResult, BaseError> {
    let items =
        serde_json::from_value::<Vec<PortableApiKeyItem>>(module.items.clone()).map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "api_keys items are invalid during apply: {err}"
            )))
        })?;
    let include_acl =
        selection.subrange_selected(&PortableModuleId::ApiKeys, &PortableSubrangeId::ApiKeyAcl);
    let mut summary = PortableModuleSummary::default();
    let mut messages = Vec::new();
    let mut seen_raw_keys = BTreeSet::new();

    for api_key in &items {
        summary.total += 1;
        if !seen_raw_keys.insert(api_key.api_key.clone()) {
            summary.skip += 1;
            continue;
        }
        let existing = api_key_repository::find_active_api_key_by_raw_key(conn, &api_key.api_key)?;
        let api_key_id = match existing {
            Some(existing) if api_key_core_matches(&existing, api_key) => {
                store_recoverable_import_secret(
                    conn,
                    secret_encryption,
                    existing.id,
                    &api_key.api_key,
                )?;
                summary.skip += 1;
                if include_acl {
                    let skipped = skip_api_key_children(api_key, include_acl, &mut summary);
                    if skipped > 0 {
                        messages.push(format!(
                            "skipped {} child ACL rows for existing API key `{}` because existing API key child governance is metadata-only",
                            skipped, api_key.name
                        ));
                    }
                }
                continue;
            }
            Some(existing) => match conflict_strategy {
                ConflictStrategy::FailOnConflict => {
                    return Err(BaseError::ParamInvalid(Some(
                        "API key conflicts with target environment".to_string(),
                    )));
                }
                ConflictStrategy::SkipExisting => {
                    store_recoverable_import_secret(
                        conn,
                        secret_encryption,
                        existing.id,
                        &api_key.api_key,
                    )?;
                    summary.skip += 1;
                    let skipped_children = if include_acl {
                        skip_api_key_children(api_key, include_acl, &mut summary)
                    } else {
                        0
                    };
                    let child_message = if skipped_children > 0 {
                        format!(
                            "; skipped {skipped_children} child ACL rows because existing API key child governance is metadata-only"
                        )
                    } else {
                        String::new()
                    };
                    messages.push(format!(
                        "skipped existing API key `{}` because governance fields differ{}",
                        api_key.name, child_message
                    ));
                    continue;
                }
                ConflictStrategy::OverwriteExisting => {
                    let updated = api_key_repository::update_api_key_metadata(
                        conn,
                        existing.id,
                        &api_key_update_data(api_key),
                        now,
                    )?;
                    store_recoverable_import_secret(
                        conn,
                        secret_encryption,
                        updated.id,
                        &api_key.api_key,
                    )?;
                    summary.update += 1;
                    context
                        .effects
                        .push(AdminMutationEffect::catalog_invalidation(
                            AdminCatalogInvalidation::ApiKeyId { id: updated.id },
                        ));
                    context
                        .effects
                        .push(AdminMutationEffect::catalog_invalidation(
                            AdminCatalogInvalidation::ApiKeyHash {
                                api_key_hash: updated.api_key_hash.clone(),
                            },
                        ));
                    let skipped_children = if include_acl {
                        skip_api_key_children(api_key, include_acl, &mut summary)
                    } else {
                        0
                    };
                    if skipped_children > 0 {
                        messages.push(format!(
                            "updated API key `{}` metadata; skipped {} child ACL rows because existing API key child governance is metadata-only",
                            api_key.name, skipped_children
                        ));
                    } else {
                        messages.push(format!("updated API key `{}` metadata", api_key.name));
                    }
                    continue;
                }
            },
            None => {
                let id = ID_GENERATOR.generate_id();
                let encrypted_secret =
                    imported_secret_for_mode(secret_encryption, id, &api_key.api_key)?;
                let created = api_key_repository::insert_raw_api_key(
                    conn,
                    &api_key_repository::RawApiKeyImportInput {
                        id,
                        raw_api_key: api_key.api_key.clone(),
                        encrypted_secret,
                        name: api_key.name.clone(),
                        description: api_key.description.clone(),
                        default_action: api_key.default_action.clone(),
                        is_enabled: api_key.is_enabled,
                        expires_at: api_key.expires_at,
                        rate_limit_rpm: api_key.rate_limit_rpm,
                        max_concurrent_requests: api_key.max_concurrent_requests,
                        quota_daily_requests: api_key.quota_daily_requests,
                        quota_daily_tokens: api_key.quota_daily_tokens,
                        quota_monthly_tokens: api_key.quota_monthly_tokens,
                        budget_daily_nanos: api_key.budget_daily_nanos,
                        budget_daily_currency: api_key.budget_daily_currency.clone(),
                        budget_monthly_nanos: api_key.budget_monthly_nanos,
                        budget_monthly_currency: api_key.budget_monthly_currency.clone(),
                        now,
                    },
                )?;
                summary.create += 1;
                context
                    .effects
                    .push(AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::ApiKeyId { id: created.id },
                    ));
                context
                    .effects
                    .push(AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::ApiKeyHash {
                            api_key_hash: created.api_key_hash.clone(),
                        },
                    ));
                created.id
            }
        };

        if include_acl {
            apply_api_key_acl_rules(conn, api_key_id, api_key, now, &mut summary, context)?;
        }
    }

    Ok(PortableApplyModuleResult {
        module_id: PortableModuleId::ApiKeys,
        status: apply_status_from_summary(&summary),
        summary,
        messages,
        blocking_issues: Vec::new(),
    })
}

fn imported_secret_for_mode(
    secret_encryption: &SecretEncryptionService,
    api_key_id: i64,
    raw_api_key: &str,
) -> Result<Option<EncryptedSecret>, BaseError> {
    if secret_encryption.downstream_mode() == crate::config::DownstreamSecretMode::OneTime {
        return Ok(None);
    }
    secret_encryption
        .encrypt_current(
            SecretDomain::DownstreamApiKey(api_key_id),
            &SensitiveSecret::new(raw_api_key.to_string()),
        )
        .map(Some)
        .map_err(|_| {
            BaseError::InternalServerError(Some(
                "failed to protect imported api key secret".to_string(),
            ))
        })
}

fn store_recoverable_import_secret(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    secret_encryption: &SecretEncryptionService,
    api_key_id: i64,
    raw_api_key: &str,
) -> Result<(), BaseError> {
    if let Some(encrypted) = imported_secret_for_mode(secret_encryption, api_key_id, raw_api_key)? {
        api_key_repository::update_api_key_secret(conn, api_key_id, &encrypted)?;
    }
    Ok(())
}

fn skip_api_key_children(
    api_key: &PortableApiKeyItem,
    include_acl: bool,
    summary: &mut PortableModuleSummary,
) -> u64 {
    let mut skipped = 0;
    if include_acl {
        let count = api_key.acl_rules.len() as u64;
        summary.total += count;
        summary.skip += count;
        skipped += count;
    }
    skipped
}

fn apply_api_key_acl_rules(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    api_key_id: i64,
    api_key: &PortableApiKeyItem,
    now: i64,
    summary: &mut PortableModuleSummary,
    context: &ApplyImportContext,
) -> Result<(), BaseError> {
    for rule in &api_key.acl_rules {
        summary.total += 1;
        let (provider_id, model_id) = resolve_acl_target(conn, context, rule)?;
        api_key_repository::insert_api_key_acl_rule(
            conn,
            &NewApiKeyAclRule {
                id: ID_GENERATOR.generate_id(),
                api_key_id,
                effect: rule.effect.clone(),
                scope: rule.scope.clone(),
                provider_id,
                model_id,
                priority: rule.priority,
                is_enabled: rule.is_enabled,
                description: rule.description.clone(),
                created_at: now,
                updated_at: now,
                deleted_at: None,
            },
        )?;
        summary.create += 1;
    }

    Ok(())
}

fn resolve_acl_target(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    context: &ApplyImportContext,
    rule: &PortableApiKeyAclRuleItem,
) -> Result<(Option<i64>, Option<i64>), BaseError> {
    match rule.scope {
        RuleScope::Provider => {
            let provider_ref = rule.provider_ref.as_deref().ok_or_else(|| {
                BaseError::ParamInvalid(Some(
                    "provider-scoped ACL rule requires provider_ref".to_string(),
                ))
            })?;
            let provider_id =
                resolve_provider_id(conn, context, provider_ref)?.ok_or_else(|| {
                    BaseError::ParamInvalid(Some(format!(
                        "provider `{provider_ref}` referenced by ACL rule was not imported or found"
                    )))
                })?;
            Ok((Some(provider_id), None))
        }
        RuleScope::Model => {
            let model_ref = rule.model_ref.as_ref().ok_or_else(|| {
                BaseError::ParamInvalid(Some(
                    "model-scoped ACL rule requires model_ref".to_string(),
                ))
            })?;
            let model_id = resolve_model_id(
                conn,
                context,
                &model_ref.provider_key,
                &model_ref.model_name,
            )?
            .ok_or_else(|| {
                BaseError::ParamInvalid(Some(format!(
                    "model `{}/{}` referenced by ACL rule was not imported or found",
                    model_ref.provider_key, model_ref.model_name
                )))
            })?;
            let provider_id = resolve_provider_id(conn, context, &model_ref.provider_key)?;
            Ok((provider_id, Some(model_id)))
        }
    }
}

fn resolve_provider_id(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    context: &ApplyImportContext,
    provider_key: &str,
) -> Result<Option<i64>, BaseError> {
    if let Some(provider_id) = context.provider_ids.get(provider_key) {
        return Ok(Some(*provider_id));
    }
    Ok(provider_repository::find_active_provider_by_key(conn, provider_key)?.map(|row| row.id))
}

fn resolve_model_id(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    context: &ApplyImportContext,
    provider_key: &str,
    model_name: &str,
) -> Result<Option<i64>, BaseError> {
    if let Some(model_id) = context
        .model_ids
        .get(&(provider_key.to_string(), model_name.to_string()))
    {
        return Ok(Some(*model_id));
    }
    Ok(
        provider_repository::find_active_model_by_ref(conn, provider_key, model_name)?
            .map(|row| row.id),
    )
}

fn resolve_cost_catalog_id(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    context: &ApplyImportContext,
    cost_catalog_ref: &str,
) -> Result<Option<i64>, BaseError> {
    if let Some(cost_catalog_id) = context.cost_catalog_ids.get(cost_catalog_ref) {
        return Ok(Some(*cost_catalog_id));
    }
    Ok(
        cost_repository::find_active_cost_catalog_by_name(conn, cost_catalog_ref)?
            .map(|row| row.id),
    )
}

fn apply_status_from_summary(summary: &PortableModuleSummary) -> PortableApplyModuleStatus {
    if summary.blocked > 0 || summary.conflict > 0 {
        PortableApplyModuleStatus::Blocked
    } else if summary.create > 0 || summary.update > 0 {
        PortableApplyModuleStatus::Applied
    } else {
        PortableApplyModuleStatus::Skipped
    }
}

fn provider_update_data(item: &PortableProviderItem) -> UpdateProviderData {
    UpdateProviderData {
        provider_key: None,
        name: Some(item.name.clone()),
        endpoint: Some(item.endpoint.clone()),
        use_proxy: Some(item.use_proxy),
        is_enabled: Some(item.is_enabled),
        provider_type: Some(item.provider_type.clone()),
        provider_api_key_mode: Some(item.provider_api_key_mode.clone()),
    }
}

fn model_update_data(item: &PortableProviderModelItem) -> UpdateModelData {
    UpdateModelData {
        model_name: None,
        real_model_name: Some(item.real_model_name.clone()),
        is_enabled: Some(item.is_enabled),
        cost_catalog_id: None,
        supports_streaming: Some(item.supports_streaming),
        supports_tools: Some(item.supports_tools),
        supports_reasoning: Some(item.supports_reasoning),
        supports_image_input: Some(item.supports_image_input),
        supports_embeddings: Some(item.supports_embeddings),
        supports_rerank: Some(item.supports_rerank),
    }
}

fn api_key_update_data(item: &PortableApiKeyItem) -> UpdateApiKeyData {
    UpdateApiKeyData {
        name: Some(item.name.clone()),
        description: Some(item.description.clone()),
        default_action: Some(item.default_action.clone()),
        is_enabled: Some(item.is_enabled),
        expires_at: Some(item.expires_at),
        rate_limit_rpm: Some(item.rate_limit_rpm),
        max_concurrent_requests: Some(item.max_concurrent_requests),
        quota_daily_requests: Some(item.quota_daily_requests),
        quota_daily_tokens: Some(item.quota_daily_tokens),
        quota_monthly_tokens: Some(item.quota_monthly_tokens),
        budget_daily_nanos: Some(item.budget_daily_nanos),
        budget_daily_currency: Some(item.budget_daily_currency.clone()),
        budget_monthly_nanos: Some(item.budget_monthly_nanos),
        budget_monthly_currency: Some(item.budget_monthly_currency.clone()),
    }
}

fn portable_import_audit_event(
    conflict_strategy: ConflictStrategy,
    bundle_digest: &str,
    reason: &str,
    summary: &PortableModuleSummary,
    selection: &NormalizedApplySelection,
) -> AdminAuditEvent {
    let selected_modules = selection
        .modules
        .keys()
        .map(PortableModuleId::as_str)
        .collect::<Vec<_>>()
        .join(",");
    AdminAuditEvent::with_fields(
        "manager.portable_config_imported",
        [
            AdminAuditField::new("action", "import"),
            AdminAuditField::new("bundle_digest", bundle_digest),
            AdminAuditField::new("conflict_strategy", format!("{conflict_strategy:?}")),
            AdminAuditField::new("reason", reason),
            AdminAuditField::new("selected_modules", selected_modules),
            AdminAuditField::new("total", summary.total),
            AdminAuditField::new("created", summary.create),
            AdminAuditField::new("updated", summary.update),
            AdminAuditField::new("skipped", summary.skip),
            AdminAuditField::new("blocked", summary.blocked),
        ],
    )
}

fn portable_export_audit_event(
    bundle_digest: &str,
    file_protection: FileProtectionMode,
    modules: &[PortableBundleModule],
) -> AdminAuditEvent {
    let selected_modules = modules
        .iter()
        .map(|module| module.module_id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let summary = summarize_export_modules(modules);
    let module_summaries = export_module_summaries_json(modules);

    AdminAuditEvent::with_fields(
        "manager.portable_config_exported",
        [
            AdminAuditField::new("action", "export"),
            AdminAuditField::new("bundle_digest", bundle_digest),
            AdminAuditField::new("selected_modules", selected_modules),
            AdminAuditField::new(
                "file_protection",
                file_protection_mode_as_str(file_protection),
            ),
            AdminAuditField::new("total", summary.total),
            AdminAuditField::new("created", summary.create),
            AdminAuditField::new("updated", summary.update),
            AdminAuditField::new("skipped", summary.skip),
            AdminAuditField::new("blocked", summary.blocked),
            AdminAuditField::new("conflicts", summary.conflict),
            AdminAuditField::new("module_summaries", module_summaries),
        ],
    )
}

fn summarize_export_modules(modules: &[PortableBundleModule]) -> PortableModuleSummary {
    modules
        .iter()
        .fold(PortableModuleSummary::default(), |mut acc, module| {
            acc.total += module.summary.total;
            acc.create += module.summary.create;
            acc.update += module.summary.update;
            acc.skip += module.summary.skip;
            acc.blocked += module.summary.blocked;
            acc.conflict += module.summary.conflict;
            acc
        })
}

fn export_module_summaries_json(modules: &[PortableBundleModule]) -> String {
    let summaries = modules
        .iter()
        .map(|module| {
            serde_json::json!({
                "module_id": module.module_id.as_str(),
                "summary": {
                    "total": module.summary.total,
                    "create": module.summary.create,
                    "update": module.summary.update,
                    "skip": module.summary.skip,
                    "blocked": module.summary.blocked,
                    "conflict": module.summary.conflict,
                },
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&summaries).unwrap_or_else(|_| "[]".to_string())
}

fn file_protection_mode_as_str(mode: FileProtectionMode) -> &'static str {
    match mode {
        FileProtectionMode::Plaintext => "plaintext",
        FileProtectionMode::PasswordEncrypted => "password_encrypted",
    }
}

fn export_provider_profile(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    subranges: &[PortableSubrangeId],
) -> Result<PortableBundleModule, BaseError> {
    let include_models = subranges.contains(&PortableSubrangeId::ProviderModels);
    let include_request_patches = subranges.contains(&PortableSubrangeId::ProviderRequestPatches);
    let include_reasoning_config = subranges.contains(&PortableSubrangeId::ProviderReasoningConfig);
    let mut providers = Vec::new();
    let mut request_patches = Vec::new();
    let mut reasoning_configs = Vec::new();

    for provider in provider_repository::list_providers_for_export(conn)? {
        let keys = provider_repository::list_provider_api_keys_for_export(conn, provider.id)?
            .into_iter()
            .map(|key| PortableProviderApiKeyItem {
                description: key.description,
                is_enabled: key.is_enabled,
                api_key: key.api_key,
            })
            .collect::<Vec<_>>();
        let models_for_provider =
            if include_models || include_request_patches || include_reasoning_config {
                provider_repository::list_models_for_provider_export(conn, provider.id)?
            } else {
                Vec::new()
            };
        let models = if include_models {
            models_for_provider
                .iter()
                .map(|model| PortableProviderModelItem {
                    provider_ref: provider.provider_key.clone(),
                    model_name: model.model_name.clone(),
                    real_model_name: model.real_model_name.clone(),
                    supports_streaming: model.supports_streaming,
                    supports_tools: model.supports_tools,
                    supports_reasoning: model.supports_reasoning,
                    supports_image_input: model.supports_image_input,
                    supports_embeddings: model.supports_embeddings,
                    supports_rerank: model.supports_rerank,
                    is_enabled: model.is_enabled,
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        if include_request_patches {
            export_request_patches_for_provider(
                conn,
                &provider.provider_key,
                provider.id,
                &models_for_provider,
                &mut request_patches,
            )?;
        }
        if include_reasoning_config {
            export_reasoning_configs_for_provider(
                conn,
                &provider.provider_key,
                provider.id,
                &models_for_provider,
                &mut reasoning_configs,
            )?;
        }

        providers.push(PortableProviderItem {
            provider_key: provider.provider_key,
            name: provider.name,
            endpoint: provider.endpoint,
            use_proxy: provider.use_proxy,
            is_enabled: provider.is_enabled,
            provider_type: provider.provider_type,
            provider_api_key_mode: provider.provider_api_key_mode,
            keys,
            models,
        });
    }
    let total = providers
        .iter()
        .map(|provider| 1 + provider.keys.len() as u64 + provider.models.len() as u64)
        .sum::<u64>()
        + request_patches.len() as u64
        + reasoning_configs.len() as u64;
    let items = PortableProviderProfileItems {
        providers,
        request_patches,
        reasoning_configs,
    };

    Ok(PortableBundleModule {
        module_id: PortableModuleId::ProviderProfile,
        module_version: PORTABLE_MODULE_VERSION_V1,
        subranges: subranges.to_vec(),
        summary: PortableModuleSummary {
            total,
            ..PortableModuleSummary::default()
        },
        items: serde_json::to_value(items).map_err(|err| {
            BaseError::InternalServerError(Some(format!(
                "failed to serialize provider portable export items: {err}"
            )))
        })?,
    })
}

fn export_request_patches_for_provider(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    provider_key: &str,
    provider_id: i64,
    models: &[Model],
    output: &mut Vec<PortableProviderRequestPatchItem>,
) -> Result<(), BaseError> {
    for rule in
        request_patch_repository::list_provider_request_patch_rules_for_export(conn, provider_id)?
    {
        output.push(portable_request_patch_item(
            portable_provider_owner(provider_key),
            rule,
        )?);
    }
    for model in models {
        for rule in
            request_patch_repository::list_model_request_patch_rules_for_export(conn, model.id)?
        {
            output.push(portable_request_patch_item(
                portable_model_owner(provider_key, &model.model_name),
                rule,
            )?);
        }
    }
    Ok(())
}

fn portable_request_patch_item(
    owner: PortableProviderOwnerRef,
    rule: crate::database::request_patch::RequestPatchRule,
) -> Result<PortableProviderRequestPatchItem, BaseError> {
    let value_json = rule
        .value_json
        .as_deref()
        .map(|raw| {
            serde_json::from_str(raw).map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "request patch value_json contains invalid JSON: {err}"
                )))
            })
        })
        .transpose()?;

    Ok(PortableProviderRequestPatchItem {
        owner,
        placement: rule.placement,
        target: rule.target,
        operation: rule.operation,
        value_json,
        description: rule.description,
        is_enabled: rule.is_enabled,
    })
}

fn export_reasoning_configs_for_provider(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    provider_key: &str,
    provider_id: i64,
    models: &[Model],
    output: &mut Vec<PortableProviderReasoningConfigItem>,
) -> Result<(), BaseError> {
    if let Some(config) =
        reasoning_config_repository::get_active_provider_reasoning_config(conn, provider_id)?
    {
        output.push(portable_reasoning_config_item(
            portable_provider_owner(provider_key),
            &config,
        ));
    }
    for model in models {
        if let Some(config) =
            reasoning_config_repository::get_active_model_reasoning_config(conn, model.id)?
        {
            output.push(portable_reasoning_config_item(
                portable_model_owner(provider_key, &model.model_name),
                &config,
            ));
        }
    }
    Ok(())
}

fn portable_reasoning_config_item(
    owner: PortableProviderOwnerRef,
    config: &ReasoningConfigWithPresets,
) -> PortableProviderReasoningConfigItem {
    PortableProviderReasoningConfigItem {
        owner,
        mode: config.mode,
        family_key: config.family.map(|family| family.as_key().to_string()),
        presets: config
            .presets
            .iter()
            .map(|preset| PortableReasoningConfigPresetItem {
                preset_key: preset.preset_key.as_key().to_string(),
                expose_in_models: preset.preset.expose_in_models,
                is_enabled: preset.preset.is_enabled,
            })
            .collect(),
    }
}

fn portable_provider_owner(provider_key: &str) -> PortableProviderOwnerRef {
    PortableProviderOwnerRef {
        scope: RuleScope::Provider,
        provider_ref: Some(provider_key.to_string()),
        model_ref: None,
    }
}

fn portable_model_owner(provider_key: &str, model_name: &str) -> PortableProviderOwnerRef {
    PortableProviderOwnerRef {
        scope: RuleScope::Model,
        provider_ref: None,
        model_ref: Some(PortableModelRef {
            provider_key: provider_key.to_string(),
            model_name: model_name.to_string(),
        }),
    }
}

fn export_cost_catalogs(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    subranges: &[PortableSubrangeId],
) -> Result<PortableBundleModule, BaseError> {
    let include_versions = subranges.contains(&PortableSubrangeId::CostCatalogVersions);
    let include_components = subranges.contains(&PortableSubrangeId::CostComponents);
    let mut catalogs = Vec::new();

    for catalog in cost_repository::list_cost_catalogs_for_export(conn)? {
        let versions = if include_versions {
            cost_repository::list_cost_catalog_versions_for_export(conn, catalog.id)?
                .into_iter()
                .map(|version| {
                    let components = if include_components {
                        cost_repository::list_cost_components_for_export(conn, version.id)?
                            .into_iter()
                            .map(portable_cost_component_item)
                            .collect::<Result<Vec<_>, _>>()?
                    } else {
                        Vec::new()
                    };
                    Ok::<PortableCostCatalogVersionItem, BaseError>(
                        PortableCostCatalogVersionItem {
                            catalog_ref: catalog.name.clone(),
                            version: version.version,
                            currency: version.currency,
                            source: version.source,
                            effective_from: version.effective_from,
                            effective_until: version.effective_until,
                            is_enabled: version.is_enabled,
                            is_archived: version.is_archived,
                            components,
                        },
                    )
                })
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        catalogs.push(PortableCostCatalogItem {
            name: catalog.name,
            description: catalog.description,
            versions,
        });
    }

    let total = catalogs
        .iter()
        .map(|catalog| {
            1 + catalog
                .versions
                .iter()
                .map(|version| 1 + version.components.len() as u64)
                .sum::<u64>()
        })
        .sum();
    let items = PortableCostCatalogItems { catalogs };

    Ok(PortableBundleModule {
        module_id: PortableModuleId::CostCatalogs,
        module_version: PORTABLE_MODULE_VERSION_V1,
        subranges: subranges.to_vec(),
        summary: PortableModuleSummary {
            total,
            ..PortableModuleSummary::default()
        },
        items: serde_json::to_value(items).map_err(|err| {
            BaseError::InternalServerError(Some(format!(
                "failed to serialize cost catalog portable export items: {err}"
            )))
        })?,
    })
}

fn portable_cost_component_item(
    component: CostComponent,
) -> Result<PortableCostComponentItem, BaseError> {
    Ok(PortableCostComponentItem {
        meter_key: component.meter_key,
        charge_kind: component.charge_kind,
        unit_price_nanos: component.unit_price_nanos,
        flat_fee_nanos: component.flat_fee_nanos,
        tier_config_json: parse_optional_json_string(component.tier_config_json)?,
        match_attributes_json: parse_optional_json_string(component.match_attributes_json)?,
        priority: component.priority,
        description: component.description,
    })
}

fn parse_optional_json_string(
    value: Option<String>,
) -> Result<Option<serde_json::Value>, BaseError> {
    value
        .map(|raw| {
            serde_json::from_str::<serde_json::Value>(&raw).map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "cost component JSON field contains invalid JSON: {err}"
                )))
            })
        })
        .transpose()
}

fn export_cost_bindings(
    conn: &mut repository::PortableRepositoryConnection<'_>,
    _selection: &NormalizedExportSelection,
    subranges: &[PortableSubrangeId],
    modules: &[PortableBundleModule],
) -> Result<PortableBundleModule, BaseError> {
    let included_models = exported_provider_model_refs(modules)?;
    let included_catalogs = exported_cost_catalog_refs(modules)?;
    let items = cost_repository::list_model_cost_bindings_for_export(conn)?
        .into_iter()
        .filter(|binding| {
            included_models.contains(&(
                binding.provider_key.clone(),
                binding.model.model_name.clone(),
            )) && included_catalogs.contains(&binding.cost_catalog.name)
        })
        .map(|binding| PortableCostBindingItem {
            target_kind: "model".to_string(),
            model_ref: Some(PortableModelRef {
                provider_key: binding.provider_key,
                model_name: binding.model.model_name,
            }),
            provider_ref: None,
            cost_catalog_ref: binding.cost_catalog.name,
        })
        .collect::<Vec<_>>();
    let total = items.len() as u64;

    Ok(PortableBundleModule {
        module_id: PortableModuleId::CostBindings,
        module_version: PORTABLE_MODULE_VERSION_V1,
        subranges: subranges.to_vec(),
        summary: PortableModuleSummary {
            total,
            ..PortableModuleSummary::default()
        },
        items: serde_json::to_value(items).map_err(|err| {
            BaseError::InternalServerError(Some(format!(
                "failed to serialize cost binding portable export items: {err}"
            )))
        })?,
    })
}

fn exported_provider_model_refs(
    modules: &[PortableBundleModule],
) -> Result<BTreeSet<(String, String)>, BaseError> {
    let mut refs = BTreeSet::new();
    let Some(module) = modules
        .iter()
        .find(|module| module.module_id == PortableModuleId::ProviderProfile)
    else {
        return Ok(refs);
    };
    if !module
        .subranges
        .contains(&PortableSubrangeId::ProviderModels)
    {
        return Ok(refs);
    }
    let items = serde_json::from_value::<PortableProviderProfileItems>(module.items.clone())
        .map_err(|err| {
            BaseError::InternalServerError(Some(format!(
                "failed to read provider profile export items for cost bindings: {err}"
            )))
        })?;
    for provider in items.providers {
        for model in provider.models {
            refs.insert((model.provider_ref, model.model_name));
        }
    }
    Ok(refs)
}

fn exported_cost_catalog_refs(
    modules: &[PortableBundleModule],
) -> Result<BTreeSet<String>, BaseError> {
    let mut refs = BTreeSet::new();
    let Some(module) = modules
        .iter()
        .find(|module| module.module_id == PortableModuleId::CostCatalogs)
    else {
        return Ok(refs);
    };
    let items = serde_json::from_value::<PortableCostCatalogItems>(module.items.clone()).map_err(
        |err| {
            BaseError::InternalServerError(Some(format!(
                "failed to read cost catalog export items for cost bindings: {err}"
            )))
        },
    )?;
    refs.extend(items.catalogs.into_iter().map(|catalog| catalog.name));
    Ok(refs)
}

fn default_export_selections() -> Vec<PortableModuleSelection> {
    module_registry()
        .into_iter()
        .filter(|module| module.default_selected && !module.deferred)
        .map(|module| PortableModuleSelection {
            module_id: module.module_id,
            subranges: module
                .subranges
                .into_iter()
                .filter(|subrange| subrange.default_selected && !subrange.deferred)
                .map(|subrange| subrange.subrange_id)
                .collect(),
        })
        .collect()
}

fn export_registry_by_module_id() -> BTreeMap<PortableModuleId, PortableModuleRegistryItem> {
    module_registry()
        .into_iter()
        .map(|module| (module.module_id.clone(), module))
        .collect()
}

fn import_registry_by_module_id() -> BTreeMap<PortableModuleId, PortableModuleRegistryItem> {
    import_module_registry()
        .into_iter()
        .map(|module| (module.module_id.clone(), module))
        .collect()
}

fn normalize_subranges(
    registry_item: &PortableModuleRegistryItem,
    selection: &PortableModuleSelection,
) -> Result<Vec<PortableSubrangeId>, BaseError> {
    let subrange_registry = registry_item
        .subranges
        .iter()
        .map(|subrange| (subrange.subrange_id.clone(), subrange))
        .collect::<BTreeMap<_, _>>();
    let selected = if selection.subranges.is_empty() {
        registry_item
            .subranges
            .iter()
            .filter(|subrange| subrange.default_selected && !subrange.deferred)
            .map(|subrange| subrange.subrange_id.clone())
            .collect::<Vec<_>>()
    } else {
        selection.subranges.clone()
    };
    let mut subrange_ids = BTreeSet::new();

    for subrange_id in selected {
        let subrange_item = subrange_registry.get(&subrange_id).ok_or_else(|| {
            BaseError::ParamInvalid(Some(format!(
                "portable export subrange `{}` is not valid for module `{}`",
                subrange_id, selection.module_id
            )))
        })?;
        if subrange_item.deferred {
            return Err(BaseError::ParamInvalid(Some(format!(
                "portable export subrange `{}` is deferred and cannot be exported yet",
                subrange_id
            ))));
        }
        subrange_ids.insert(subrange_id);
    }

    for required in registry_item
        .subranges
        .iter()
        .filter(|subrange| subrange.required && !subrange.deferred)
    {
        subrange_ids.insert(required.subrange_id.clone());
    }

    Ok(ordered_subranges(&registry_item.subranges, &subrange_ids))
}

fn normalize_apply_subranges(
    registry_item: &PortableModuleRegistryItem,
    selection: &PortableModuleSelection,
    bundle_module: &PortableBundleModule,
) -> Result<Vec<PortableSubrangeId>, BaseError> {
    let subrange_registry = registry_item
        .subranges
        .iter()
        .map(|subrange| (subrange.subrange_id.clone(), subrange))
        .collect::<BTreeMap<_, _>>();
    let selected = if selection.subranges.is_empty() {
        bundle_module.subranges.clone()
    } else {
        selection.subranges.clone()
    };
    let mut subrange_ids = BTreeSet::new();

    for subrange_id in selected {
        let Some(subrange_item) = subrange_registry.get(&subrange_id) else {
            if matches!(subrange_id, PortableSubrangeId::Unknown(_)) {
                continue;
            }
            return Err(BaseError::ParamInvalid(Some(format!(
                "portable import subrange `{}` is not valid for module `{}`",
                subrange_id, selection.module_id
            ))));
        };
        if subrange_item.deferred {
            return Err(BaseError::ParamInvalid(Some(format!(
                "portable import subrange `{}` is deferred and cannot be applied yet",
                subrange_id
            ))));
        }
        subrange_ids.insert(subrange_id);
    }

    for required in registry_item
        .subranges
        .iter()
        .filter(|subrange| subrange.required && !subrange.deferred)
    {
        subrange_ids.insert(required.subrange_id.clone());
    }

    Ok(ordered_subranges(&registry_item.subranges, &subrange_ids))
}

fn ordered_subranges(
    registry: &[PortableSubrangeRegistryItem],
    selected: &BTreeSet<PortableSubrangeId>,
) -> Vec<PortableSubrangeId> {
    registry
        .iter()
        .filter(|subrange| selected.contains(&subrange.subrange_id))
        .map(|subrange| subrange.subrange_id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diesel::{RunQueryDsl, connection::SimpleConnection, prelude::*};

    use crate::config::SecretEncryptionConfig;
    use crate::database::api_key::{ApiKey, hash_api_key};
    use crate::database::{DbConnection, TestDbContext, get_connection};
    use crate::service::admin::AdminServices;
    use crate::service::catalog::CatalogService;
    use crate::service::portable_config::schema::{
        ConflictStrategy, PORTABLE_MODULE_VERSION_V1, PORTABLE_SCHEMA_VERSION,
        PortableApplyRequest, PortableBundle, PortableBundleModule, PortableImportPreviewRequest,
        PortableModuleId, PortableModuleSelection, PortableModuleSummary, PortableSubrangeId,
    };
    use crate::service::secret_encryption::{SecretEncryptionService, SensitiveSecret};

    const CURRENT_KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    type SecretTuple = (
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<i32>,
        Option<String>,
    );

    fn config(mode: &str) -> SecretEncryptionConfig {
        serde_yaml::from_str(&format!(
            "downstream_mode: {mode}\nencryption_key: '{CURRENT_KEY}'\n"
        ))
        .expect("portable secret config should parse")
    }

    async fn services(mode: &str) -> Arc<AdminServices> {
        let catalog = Arc::new(CatalogService::new(true).await);
        let encryption = Arc::new(SecretEncryptionService::from_config(&config(mode)));
        Arc::new(AdminServices::new(catalog, encryption))
    }

    fn api_key_item(raw: &str, name: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "description": "portable R2.6 fixture",
            "default_action": "ALLOW",
            "is_enabled": true,
            "expires_at": null,
            "rate_limit_rpm": null,
            "max_concurrent_requests": null,
            "quota_daily_requests": null,
            "quota_daily_tokens": null,
            "quota_monthly_tokens": null,
            "budget_daily_nanos": null,
            "budget_daily_currency": null,
            "budget_monthly_nanos": null,
            "budget_monthly_currency": null,
            "api_key": raw,
            "acl_rules": []
        })
    }

    fn bundle(items: Vec<serde_json::Value>) -> String {
        serde_json::to_string(&PortableBundle {
            schema_version: PORTABLE_SCHEMA_VERSION.to_string(),
            exported_at: 1_778_236_800_000,
            cyder_version: "0.7.1".to_string(),
            modules: vec![PortableBundleModule {
                module_id: PortableModuleId::ApiKeys,
                module_version: PORTABLE_MODULE_VERSION_V1,
                subranges: vec![PortableSubrangeId::ApiKeyCore],
                summary: PortableModuleSummary {
                    total: items.len() as u64,
                    ..Default::default()
                },
                items: serde_json::Value::Array(items),
            }],
        })
        .expect("portable bundle should serialize")
    }

    async fn apply(
        services: &AdminServices,
        content: &str,
        strategy: ConflictStrategy,
    ) -> Result<(), crate::controller::BaseError> {
        let preview = services
            .portable_config
            .preview_import(PortableImportPreviewRequest {
                content: content.to_string(),
                password: None,
            })
            .await?;
        services
            .portable_config
            .apply_import(PortableApplyRequest {
                content: content.to_string(),
                password: None,
                bundle_digest: preview.bundle_digest,
                selected_modules: vec![PortableModuleSelection {
                    module_id: PortableModuleId::ApiKeys,
                    subranges: vec![PortableSubrangeId::ApiKeyCore],
                }],
                conflict_strategy: strategy,
                reason: "R2.6 portable import test".to_string(),
                dangerous_patch_confirmations: Vec::new(),
            })
            .await
            .map(|_| ())
    }

    fn load_secret_tuple(id_value: i64) -> SecretTuple {
        let mut connection = get_connection().expect("test connection should load");
        let DbConnection::Sqlite(connection) = &mut connection else {
            panic!("portable unit test requires sqlite")
        };
        use crate::database::_sqlite_schema::api_key::dsl as key;
        key::api_key
            .filter(key::id.eq(id_value))
            .select((
                key::secret_ciphertext,
                key::secret_nonce,
                key::secret_format_version,
                key::secret_key_fingerprint,
            ))
            .first(connection)
            .expect("secret tuple should load")
    }

    fn replace_fingerprint(id_value: i64, fingerprint: &str) {
        let mut connection = get_connection().expect("test connection should load");
        let DbConnection::Sqlite(connection) = &mut connection else {
            panic!("portable unit test requires sqlite")
        };
        use crate::database::_sqlite_schema::api_key::dsl as key;
        diesel::update(key::api_key.filter(key::id.eq(id_value)))
            .set(key::secret_key_fingerprint.eq(Some(fingerprint.to_string())))
            .execute(connection)
            .expect("fingerprint fixture should update");
    }

    fn execute_sql(sql: &str) {
        let mut connection = get_connection().expect("test connection should load");
        let DbConnection::Sqlite(connection) = &mut connection else {
            panic!("portable unit test requires sqlite")
        };
        connection
            .batch_execute(sql)
            .expect("portable test SQL should execute");
    }

    #[tokio::test]
    async fn portable_api_key_import_obeys_mode_and_duplicate_secret_rules() {
        let database = TestDbContext::new_sqlite("portable-api-key-mode.sqlite");
        database
            .run_async(async {
                let raw = "cyder-portable-mode-secret-1234";
                let content = bundle(vec![api_key_item(raw, "portable-mode")]);
                let one_time = services("one_time").await;
                apply(&one_time, &content, ConflictStrategy::FailOnConflict)
                    .await
                    .expect("one-time import should apply");
                let imported = ApiKey::get_by_hash(&hash_api_key(raw))
                    .expect("one-time imported key should authenticate");
                assert_eq!(load_secret_tuple(imported.id), (None, None, None, None));
                assert!(
                    !one_time
                        .api_key
                        .get_api_key_detail(imported.id)
                        .expect("one-time detail should load")
                        .can_reveal
                );

                let recoverable = services("recoverable").await;
                apply(&recoverable, &content, ConflictStrategy::SkipExisting)
                    .await
                    .expect("recoverable duplicate should supplement secret");
                let recoverable_tuple = load_secret_tuple(imported.id);
                assert!(recoverable_tuple.0.is_some());
                assert_eq!(
                    recoverable
                        .api_key
                        .reveal_api_key(imported.id)
                        .expect("supplemented key should reveal")
                        .api_key,
                    raw
                );

                apply(&one_time, &content, ConflictStrategy::SkipExisting)
                    .await
                    .expect("one-time duplicate should apply without secret changes");
                assert_eq!(load_secret_tuple(imported.id), recoverable_tuple);
                assert!(
                    !one_time
                        .api_key
                        .get_api_key_detail(imported.id)
                        .expect("one-time detail should load")
                        .can_reveal
                );
            })
            .await;
    }

    #[tokio::test]
    async fn portable_api_key_import_preserves_unknown_in_one_time_and_repairs_in_recoverable() {
        let database = TestDbContext::new_sqlite("portable-api-key-repair.sqlite");
        database
            .run_async(async {
                let raw = "cyder-portable-repair-secret-5678";
                let content = bundle(vec![api_key_item(raw, "portable-repair")]);
                let recoverable = services("recoverable").await;
                apply(&recoverable, &content, ConflictStrategy::FailOnConflict)
                    .await
                    .expect("recoverable import should apply");
                let imported = ApiKey::get_by_hash(&hash_api_key(raw))
                    .expect("imported key should authenticate");
                replace_fingerprint(imported.id, &"f".repeat(64));
                let unknown = load_secret_tuple(imported.id);

                let one_time = services("one_time").await;
                apply(&one_time, &content, ConflictStrategy::SkipExisting)
                    .await
                    .expect("one-time duplicate should preserve unknown tuple");
                assert_eq!(load_secret_tuple(imported.id), unknown);

                apply(&recoverable, &content, ConflictStrategy::SkipExisting)
                    .await
                    .expect("recoverable duplicate should replace unknown tuple");
                assert_ne!(load_secret_tuple(imported.id), unknown);
                assert_eq!(
                    recoverable
                        .api_key
                        .reveal_api_key(imported.id)
                        .expect("repaired secret should reveal")
                        .api_key,
                    raw
                );
            })
            .await;
    }

    #[tokio::test]
    async fn portable_api_key_preview_is_safe_and_total_transaction_rolls_back() {
        let database = TestDbContext::new_sqlite("portable-api-key-safe-preview.sqlite");
        database
            .run_async(async {
                let raw = "cyder-portable-never-preview-9012";
                let duplicate_content = bundle(vec![
                    api_key_item(raw, "portable-safe-a"),
                    api_key_item(raw, "portable-safe-b"),
                ]);
                let recoverable = services("recoverable").await;
                let preview = recoverable
                    .portable_config
                    .preview_import(PortableImportPreviewRequest {
                        content: duplicate_content.clone(),
                        password: None,
                    })
                    .await
                    .expect("duplicate preview should return safely");
                let serialized = serde_json::to_string(&preview)
                    .expect("preview response should serialize");
                assert!(!serialized.contains(raw));
                assert!(!format!("{preview:?}").contains(raw));
                let module = preview.modules.first().expect("API key module should preview");
                assert_eq!(module.api_key_items.len(), 2);
                assert_eq!(module.api_key_items[0].name, "portable-safe-a");
                assert_eq!(module.api_key_items[0].key_last4, "9012");
                assert_eq!(module.api_key_items[1].outcome, "blocked");

                let single_content = bundle(vec![api_key_item(raw, "portable-safe-a")]);
                execute_sql(
                    "CREATE TRIGGER fail_portable_secret BEFORE UPDATE OF secret_ciphertext ON api_key WHEN NEW.secret_ciphertext IS NOT NULL BEGIN SELECT RAISE(ABORT, 'blocked portable secret'); END;",
                );
                assert!(
                    apply(
                        &recoverable,
                        &single_content,
                        ConflictStrategy::FailOnConflict,
                    )
                    .await
                    .is_err()
                );
                execute_sql("DROP TRIGGER fail_portable_secret;");
                assert!(matches!(
                    ApiKey::get_by_hash(&hash_api_key(raw)),
                    Err(crate::controller::BaseError::NotFound(_))
                ));

                let sensitive = SensitiveSecret::new(raw.to_string());
                assert!(!format!("{sensitive:?}").contains(raw));
            })
            .await;
    }

    #[test]
    fn portable_api_key_debug_contract_redacts_raw_material() {
        let raw = "cyder-portable-debug-secret-3456";
        let content = bundle(vec![api_key_item(raw, "portable-debug")]);
        let parsed: PortableBundle =
            serde_json::from_str(&content).expect("debug bundle should parse");
        let request = PortableApplyRequest {
            content: content.clone(),
            password: Some("portable-password".to_string()),
            bundle_digest: "sha256:test".to_string(),
            selected_modules: Vec::new(),
            conflict_strategy: ConflictStrategy::FailOnConflict,
            reason: "debug contract".to_string(),
            dangerous_patch_confirmations: Vec::new(),
        };

        for output in [format!("{parsed:?}"), format!("{request:?}")] {
            assert!(!output.contains(raw));
            assert!(!output.contains("portable-password"));
        }
    }
}
