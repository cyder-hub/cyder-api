use serde::{Deserialize, Serialize};

use super::schema::{
    ConflictStrategy, PORTABLE_MODULE_VERSION_V1, PORTABLE_SCHEMA_VERSION, PortableModuleId,
    PortableSubrangeId,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableModuleDependency {
    pub module_id: PortableModuleId,
    pub required_for_export: bool,
    pub required_for_fresh_import: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableSubrangeRegistryItem {
    pub subrange_id: PortableSubrangeId,
    pub label: String,
    pub default_selected: bool,
    pub required: bool,
    pub contains_secrets: bool,
    pub deferred: bool,
    pub deferred_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableModuleRegistryItem {
    pub module_id: PortableModuleId,
    pub label: String,
    pub description: String,
    pub module_version: u32,
    pub default_selected: bool,
    pub contains_secrets: bool,
    pub deferred: bool,
    pub deferred_reason: Option<String>,
    pub dependencies: Vec<PortableModuleDependency>,
    pub subranges: Vec<PortableSubrangeRegistryItem>,
    pub conflict_strategies: Vec<ConflictStrategy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableModuleRegistryResponse {
    pub schema_version: String,
    pub modules: Vec<PortableModuleRegistryItem>,
    pub default_selected_modules: Vec<PortableModuleId>,
    pub apply_order: Vec<PortableModuleId>,
}

pub fn registry_response() -> PortableModuleRegistryResponse {
    let modules = module_registry();
    let default_selected_modules = modules
        .iter()
        .filter(|module| module.default_selected && !module.deferred)
        .map(|module| module.module_id.clone())
        .collect();

    PortableModuleRegistryResponse {
        schema_version: PORTABLE_SCHEMA_VERSION.to_string(),
        modules,
        default_selected_modules,
        apply_order: apply_order(),
    }
}

pub fn module_registry() -> Vec<PortableModuleRegistryItem> {
    vec![
        PortableModuleRegistryItem {
            module_id: PortableModuleId::ProviderProfile,
            label: "Provider Profile".to_string(),
            description:
                "Provider, provider key, model, request patch, and reasoning configuration."
                    .to_string(),
            module_version: PORTABLE_MODULE_VERSION_V1,
            default_selected: true,
            contains_secrets: true,
            deferred: false,
            deferred_reason: None,
            dependencies: Vec::new(),
            subranges: vec![
                subrange(
                    PortableSubrangeId::ProviderCore,
                    "Provider core",
                    true,
                    true,
                    false,
                    false,
                    None,
                ),
                subrange(
                    PortableSubrangeId::ProviderKeys,
                    "Provider keys",
                    true,
                    true,
                    true,
                    false,
                    None,
                ),
                subrange(
                    PortableSubrangeId::ProviderModels,
                    "Provider models",
                    true,
                    false,
                    false,
                    false,
                    None,
                ),
                subrange(
                    PortableSubrangeId::ProviderRequestPatches,
                    "Request patches",
                    false,
                    false,
                    false,
                    false,
                    None,
                ),
                subrange(
                    PortableSubrangeId::ProviderReasoningConfig,
                    "Reasoning config",
                    false,
                    false,
                    false,
                    false,
                    None,
                ),
            ],
            conflict_strategies: supported_conflict_strategies(),
        },
        PortableModuleRegistryItem {
            module_id: PortableModuleId::ApiKeys,
            label: "API Keys".to_string(),
            description: "Downstream API keys, ACL rules, and model override references."
                .to_string(),
            module_version: PORTABLE_MODULE_VERSION_V1,
            default_selected: true,
            contains_secrets: true,
            deferred: false,
            deferred_reason: None,
            dependencies: vec![PortableModuleDependency {
                module_id: PortableModuleId::ProviderProfile,
                required_for_export: false,
                required_for_fresh_import: true,
                reason: "ACL provider/model references need provider_profile on a fresh database."
                    .to_string(),
            }],
            subranges: vec![
                subrange(
                    PortableSubrangeId::ApiKeyCore,
                    "API key core",
                    true,
                    true,
                    true,
                    false,
                    None,
                ),
                subrange(
                    PortableSubrangeId::ApiKeyAcl,
                    "ACL rules",
                    true,
                    false,
                    false,
                    false,
                    None,
                ),
                subrange(
                    PortableSubrangeId::ApiKeyModelOverride,
                    "Model overrides",
                    true,
                    false,
                    false,
                    false,
                    None,
                ),
            ],
            conflict_strategies: supported_conflict_strategies(),
        },
        PortableModuleRegistryItem {
            module_id: PortableModuleId::CostCatalogs,
            label: "Cost Catalogs".to_string(),
            description: "Cost catalogs, versions, and components.".to_string(),
            module_version: PORTABLE_MODULE_VERSION_V1,
            default_selected: false,
            contains_secrets: false,
            deferred: false,
            deferred_reason: None,
            dependencies: Vec::new(),
            subranges: vec![
                subrange(
                    PortableSubrangeId::CostCatalogCore,
                    "Catalog core",
                    true,
                    true,
                    false,
                    false,
                    None,
                ),
                subrange(
                    PortableSubrangeId::CostCatalogVersions,
                    "Catalog versions",
                    true,
                    true,
                    false,
                    false,
                    None,
                ),
                subrange(
                    PortableSubrangeId::CostComponents,
                    "Cost components",
                    true,
                    true,
                    false,
                    false,
                    None,
                ),
            ],
            conflict_strategies: supported_conflict_strategies(),
        },
        PortableModuleRegistryItem {
            module_id: PortableModuleId::CostBindings,
            label: "Cost Bindings".to_string(),
            description: "Model to cost catalog bindings.".to_string(),
            module_version: PORTABLE_MODULE_VERSION_V1,
            default_selected: false,
            contains_secrets: false,
            deferred: false,
            deferred_reason: None,
            dependencies: vec![
                PortableModuleDependency {
                    module_id: PortableModuleId::ProviderProfile,
                    required_for_export: true,
                    required_for_fresh_import: true,
                    reason: "Bindings target models exported by provider_profile.".to_string(),
                },
                PortableModuleDependency {
                    module_id: PortableModuleId::CostCatalogs,
                    required_for_export: true,
                    required_for_fresh_import: true,
                    reason: "Bindings reference cost catalogs.".to_string(),
                },
            ],
            subranges: vec![subrange(
                PortableSubrangeId::CostModelBindings,
                "Model bindings",
                true,
                true,
                false,
                false,
                None,
            )],
            conflict_strategies: supported_conflict_strategies(),
        },
    ]
}

pub fn apply_order() -> Vec<PortableModuleId> {
    vec![
        PortableModuleId::CostCatalogs,
        PortableModuleId::ProviderProfile,
        PortableModuleId::CostBindings,
        PortableModuleId::ApiKeys,
    ]
}

fn supported_conflict_strategies() -> Vec<ConflictStrategy> {
    vec![
        ConflictStrategy::FailOnConflict,
        ConflictStrategy::SkipExisting,
        ConflictStrategy::OverwriteExisting,
    ]
}

fn subrange(
    subrange_id: PortableSubrangeId,
    label: &str,
    default_selected: bool,
    required: bool,
    contains_secrets: bool,
    deferred: bool,
    deferred_reason: Option<String>,
) -> PortableSubrangeRegistryItem {
    PortableSubrangeRegistryItem {
        subrange_id,
        label: label.to_string(),
        default_selected,
        required,
        contains_secrets,
        deferred,
        deferred_reason,
    }
}

#[cfg(test)]
mod tests {
    use super::{PortableModuleId, registry_response};

    #[test]
    fn registry_contains_only_portable_config_modules() {
        let registry = registry_response();
        let module_ids = registry
            .modules
            .iter()
            .map(|module| module.module_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            module_ids,
            vec![
                "provider_profile",
                "api_keys",
                "cost_catalogs",
                "cost_bindings"
            ]
        );
        assert!(!module_ids.contains(&"request_logs"));
        assert!(!module_ids.contains(&"metrics"));
        assert!(!module_ids.contains(&"alerts"));
        assert!(!module_ids.contains(&"manager_session"));
    }

    #[test]
    fn registry_marks_core_secret_modules_as_default_selected() {
        let registry = registry_response();

        assert_eq!(
            registry.default_selected_modules,
            vec![PortableModuleId::ProviderProfile, PortableModuleId::ApiKeys]
        );
        assert_eq!(
            registry
                .modules
                .iter()
                .filter(|module| module.deferred)
                .count(),
            0
        );
        assert_eq!(
            registry
                .modules
                .iter()
                .filter(|module| module.contains_secrets)
                .map(|module| module.module_id.clone())
                .collect::<Vec<_>>(),
            vec![PortableModuleId::ProviderProfile, PortableModuleId::ApiKeys]
        );
    }
}
