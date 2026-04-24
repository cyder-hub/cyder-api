use compact_str::{CompactString, format_compact};

pub(super) enum CacheKey<'a> {
    ApiKeyHash(&'a str),
    ModelRouteById(i64),
    ModelRouteByName(&'a str),
    ApiKeyModelOverride(i64, &'a str),
    ModelsCatalog,
    ProviderById(i64),
    ProviderByKey(&'a str),
    ModelById(i64),
    ModelByName(&'a str, &'a str),
    ProviderApiKeys(i64),
    ProviderRequestPatchRules(i64),
    ModelRequestPatchRules(i64),
    ModelEffectiveRequestPatches(i64),
    CostCatalogVersion(i64),
}

impl<'a> CacheKey<'a> {
    pub(super) fn to_compact_string(&self) -> CompactString {
        match self {
            CacheKey::ApiKeyHash(key) => format_compact!("api_key:hash:{}", key),
            CacheKey::ModelRouteById(id) => format_compact!("route:id:{}", id),
            CacheKey::ModelRouteByName(name) => format_compact!("route:name:{}", name),
            CacheKey::ApiKeyModelOverride(api_key_id, source_name) => {
                format_compact!("api_key_override:{}/{}", api_key_id, source_name)
            }
            CacheKey::ModelsCatalog => format_compact!("models:catalog"),
            CacheKey::ProviderById(id) => format_compact!("provider:id:{}", id),
            CacheKey::ProviderByKey(key) => format_compact!("provider:key:{}", key),
            CacheKey::ModelById(id) => format_compact!("model:id:{}", id),
            CacheKey::ModelByName(provider_key, model_name) => {
                format_compact!("model:name:{}/{}", provider_key, model_name)
            }
            CacheKey::ProviderApiKeys(provider_id) => {
                format_compact!("provider_keys:{}", provider_id)
            }
            CacheKey::ProviderRequestPatchRules(provider_id) => {
                format_compact!("request_patch:provider:{}", provider_id)
            }
            CacheKey::ModelRequestPatchRules(model_id) => {
                format_compact!("request_patch:model:{}", model_id)
            }
            CacheKey::ModelEffectiveRequestPatches(model_id) => {
                format_compact!("request_patch:model_effective:{}", model_id)
            }
            CacheKey::CostCatalogVersion(id) => format_compact!("cost_catalog_version:id:{}", id),
        }
    }
}
