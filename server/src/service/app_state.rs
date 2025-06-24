use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::fmt::Debug;
use axum::Router;
use cyder_tools::log::{debug, info, warn};
use rand::prelude::*;
use thiserror::Error;

use crate::database::system_api_key::SystemApiKey;
use crate::database::provider::{Provider, ProviderApiKey};
use crate::database::model::Model;
use crate::database::model_alias::ModelAlias;
use crate::database::price::{BillingPlan, PriceRule};
use crate::database::access_control::{AccessControlPolicy as DbAccessControlPolicy, ApiAccessControlPolicy};
use crate::database::custom_field::{CustomFieldDefinition, ModelCustomFieldAssignment, ProviderCustomFieldAssignment};


pub enum GroupItemSelectionStrategy {
    Random,
    Queue,
}

pub trait Storable: Clone + Debug + Send + Sync + 'static {
    fn id(&self) -> i64;
    fn key(&self) -> String;
    fn group_id(&self) -> Option<i64> { None } // Default implementation
}

// --- Generic StateStore Definition and Implementation ---
#[derive(Debug, Clone)]
pub struct StateStore<S: Storable> {
    items_by_id: Arc<RwLock<HashMap<i64, S>>>,
    id_by_item_key: Option<Arc<RwLock<HashMap<String, i64>>>>, // item_key (e.g. api_key, provider_key) to ID
    id_set_by_group_id: Option<Arc<RwLock<HashMap<i64, HashSet<i64>>>>>, // group_id to set of item_ids
    group_queue_counters: Option<Arc<RwLock<HashMap<i64, usize>>>>, // For round-robin queue strategy
    type_name: &'static str,
}

impl<S: Storable> StateStore<S> {
    pub fn new(type_name: &'static str, with_key_map: bool, with_group_map: bool) -> Self {
        StateStore {
            items_by_id: Arc::new(RwLock::new(HashMap::new())),
            id_by_item_key: if with_key_map {
                Some(Arc::new(RwLock::new(HashMap::new())))
            } else {
                None
            },
            id_set_by_group_id: if with_group_map {
                Some(Arc::new(RwLock::new(HashMap::new())))
            } else {
                None
            },
            group_queue_counters: if with_group_map {
                Some(Arc::new(RwLock::new(HashMap::new())))
            } else {
                None
            },
            type_name,
        }
    }

    fn get_items_by_id_read(&self) -> Result<RwLockReadGuard<'_, HashMap<i64, S>>, AppStoreError> {
        self.items_by_id.read().map_err(|e| AppStoreError::LockError(format!("Failed to acquire read lock on items_by_id for {}: {}", self.type_name, e)))
    }

    fn get_items_by_id_write(&self) -> Result<RwLockWriteGuard<'_, HashMap<i64, S>>, AppStoreError> {
        self.items_by_id.write().map_err(|e| AppStoreError::LockError(format!("Failed to acquire write lock on items_by_id for {}: {}", self.type_name, e)))
    }

    // Internal helper for read lock on id_by_item_key, if it exists.
    fn get_id_by_item_key_read_lock(&self) -> Option<Result<RwLockReadGuard<'_, HashMap<String, i64>>, AppStoreError>> {
        self.id_by_item_key.as_ref().map(|arc| {
            arc.read().map_err(|e| AppStoreError::LockError(format!("Failed to acquire read lock on id_by_item_key for {}: {}", self.type_name, e)))
        })
    }

    // Internal helper for write lock on id_by_item_key, if it exists.
    fn get_id_by_item_key_write_lock(&self) -> Option<Result<RwLockWriteGuard<'_, HashMap<String, i64>>, AppStoreError>> {
        self.id_by_item_key.as_ref().map(|arc| {
            arc.write().map_err(|e| AppStoreError::LockError(format!("Failed to acquire write lock on id_by_item_key for {}: {}", self.type_name, e)))
        })
    }

    // Internal helper for write lock on id_set_by_group_id, if it exists.
    fn get_id_set_by_group_id_write_lock(&self) -> Option<Result<RwLockWriteGuard<'_, HashMap<i64, HashSet<i64>>>, AppStoreError>> {
        self.id_set_by_group_id.as_ref().map(|arc| {
            arc.write().map_err(|e| AppStoreError::LockError(format!("Failed to acquire write lock on id_set_by_group_id for {}: {}", self.type_name, e)))
        })
    }

    pub fn get_by_key(&self, item_key: &str) -> Result<Option<S>, AppStoreError> {
        if let Some(id_map_lock_result) = self.get_id_by_item_key_read_lock() {
            let id_map = id_map_lock_result?;
            if let Some(&id) = id_map.get(item_key) {
                let items_map = self.get_items_by_id_read()?;
                Ok(items_map.get(&id).cloned())
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    pub fn get_by_id(&self, id: i64) -> Result<Option<S>, AppStoreError> {
        let items_map = self.get_items_by_id_read()?;
        Ok(items_map.get(&id).cloned())
    }

    pub fn add(&self, item: S) -> Result<S, AppStoreError> {
        let mut items_map_w = self.get_items_by_id_write()?;
        
        let item_id = item.id();
        let item_key = item.key(); 

        if items_map_w.contains_key(&item_id) {
            return Err(AppStoreError::AlreadyExists(format!("{} with ID {} already exists", self.type_name, item_id)));
        }

        if let Some(id_map_lock_result) = self.get_id_by_item_key_write_lock() {
            let mut id_map_w = id_map_lock_result?;
            if id_map_w.contains_key(&item_key) {
                return Err(AppStoreError::AlreadyExists(format!("{} with key '{}' already exists", self.type_name, item_key)));
            }
            id_map_w.insert(item_key.clone(), item_id);
        }
        
        if let Some(group_map_lock_result) = self.get_id_set_by_group_id_write_lock() {
            if let Some(group_id_val) = item.group_id() {
                let mut group_map_w = group_map_lock_result?;
                group_map_w.entry(group_id_val).or_insert_with(HashSet::new).insert(item_id);
            }
        }

        let item_clone = item.clone();
        items_map_w.insert(item_id, item); 
        info!("{}: Added item ID {}, key '{}', group_id {:?}", self.type_name, item_clone.id(), item_clone.key(), item_clone.group_id());
        Ok(item_clone)
    }

    pub fn update(&self, updated_item: S) -> Result<S, AppStoreError> {
        let mut items_map_w = self.get_items_by_id_write()?;
        
        let updated_item_id = updated_item.id();
        let updated_item_key = updated_item.key();
        let updated_item_group_id = updated_item.group_id();

        if let Some(existing_item) = items_map_w.get_mut(&updated_item_id) {
            let existing_item_key = existing_item.key();
            let existing_item_group_id = existing_item.group_id();

            // Handle key map update
            if self.id_by_item_key.is_some() {
                let mut id_map_w = self.get_id_by_item_key_write_lock().unwrap()?; // Should exist if self.id_by_item_key is Some
                if existing_item_key != updated_item_key {
                    if let Some(&conflicting_id) = id_map_w.get(&updated_item_key) {
                        if conflicting_id != updated_item_id {
                            return Err(AppStoreError::AlreadyExists(format!(
                                "Cannot update {} ID {}: key '{}' is already in use by ID {}",
                                self.type_name, updated_item_id, updated_item_key, conflicting_id
                            )));
                        }
                    }
                    id_map_w.remove(&existing_item_key);
                    id_map_w.insert(updated_item_key.clone(), updated_item_id);
                }
            }

            // Handle group map update
            if self.id_set_by_group_id.is_some() {
                let mut group_map_w = self.get_id_set_by_group_id_write_lock().unwrap()?; // Should exist if self.id_set_by_group_id is Some
                if existing_item_group_id != updated_item_group_id {
                    if let Some(old_gid) = existing_item_group_id {
                        if let Some(set) = group_map_w.get_mut(&old_gid) {
                            set.remove(&updated_item_id);
                            if set.is_empty() {
                                group_map_w.remove(&old_gid);
                            }
                        }
                    }
                    if let Some(new_gid) = updated_item_group_id {
                        group_map_w.entry(new_gid).or_insert_with(HashSet::new).insert(updated_item_id);
                    }
                }
            }
            
            *existing_item = updated_item.clone();
            info!("{}: Updated item ID {}, new key '{}', new group_id {:?}", self.type_name, updated_item_id, updated_item_key, updated_item_group_id);
            Ok(updated_item)
        } else {
            Err(AppStoreError::NotFound(format!("{} with ID {} not found for update", self.type_name, updated_item_id)))
        }
    }

    pub fn delete(&self, id: i64) -> Result<S, AppStoreError> {
        let mut items_map_w = self.get_items_by_id_write()?;

        if let Some(removed_item) = items_map_w.remove(&id) {
            if let Some(id_map_lock_result) = self.get_id_by_item_key_write_lock() {
                let mut id_map_w = id_map_lock_result?;
                id_map_w.remove(&removed_item.key());
            }

            if let Some(group_map_lock_result) = self.get_id_set_by_group_id_write_lock() {
                if let Some(group_id_val) = removed_item.group_id() {
                    let mut group_map_w = group_map_lock_result?;
                    if let Some(set) = group_map_w.get_mut(&group_id_val) {
                        set.remove(&removed_item.id());
                        if set.is_empty() {
                            group_map_w.remove(&group_id_val);
                        }
                    }
                }
            }
            info!("{}: Deleted item ID {}, key '{}', group_id {:?}", self.type_name, removed_item.id(), removed_item.key(), removed_item.group_id());
            Ok(removed_item)
        } else {
            Err(AppStoreError::NotFound(format!("{} with ID {} not found for deletion", self.type_name, id)))
        }
    }

    pub fn get_all(&self) -> Result<Vec<S>, AppStoreError> {
        let items_map = self.get_items_by_id_read()?;
        Ok(items_map.values().cloned().collect())
    }

    pub fn refresh_data(&self, new_items: Vec<S>) -> Result<(), AppStoreError> {
        let mut temp_items_by_id = HashMap::with_capacity(new_items.len());
        let mut temp_id_by_item_key = if self.id_by_item_key.is_some() { Some(HashMap::with_capacity(new_items.len())) } else { None };
        let mut temp_id_set_by_group_id = if self.id_set_by_group_id.is_some() { Some(HashMap::new()) } else { None };

        for item in new_items { // Consuming iteration
            let item_id = item.id();
            if temp_items_by_id.contains_key(&item_id) {
                return Err(AppStoreError::AlreadyExists(format!("Duplicate {} ID {} in refresh dataset.", self.type_name, item_id)));
            }

            if let Some(map) = &mut temp_id_by_item_key {
                let item_key = item.key(); 
                if map.contains_key(&item_key) {
                    return Err(AppStoreError::AlreadyExists(format!("Duplicate {} key '{}' in refresh dataset.", self.type_name, item_key)));
                }
                map.insert(item_key, item_id);
            }

            if let Some(group_map) = &mut temp_id_set_by_group_id {
                if let Some(group_id_val) = item.group_id() {
                    group_map.entry(group_id_val).or_insert_with(HashSet::new).insert(item_id);
                }
            }
            temp_items_by_id.insert(item_id, item); 
        }

        // Update items_by_id
        let mut items_map_w = self.get_items_by_id_write()?;
        items_map_w.clear();
        items_map_w.extend(temp_items_by_id);

        // Update id_by_item_key
        if let Some(id_map_arc) = &self.id_by_item_key {
            if let Some(temp_map_content) = temp_id_by_item_key { 
                let mut id_map_w = id_map_arc.write().map_err(|e| AppStoreError::LockError(format!("Failed to acquire write lock on id_by_item_key for {}: {}", self.type_name, e)))?;
                id_map_w.clear();
                id_map_w.extend(temp_map_content);
            }
        }

        // Update id_set_by_group_id
        if let Some(group_map_arc) = &self.id_set_by_group_id {
            if let Some(temp_group_map_content) = temp_id_set_by_group_id {
                let mut group_map_w = group_map_arc.write().map_err(|e| AppStoreError::LockError(format!("Failed to acquire write lock on id_set_by_group_id for {}: {}", self.type_name, e)))?;
                group_map_w.clear();
                group_map_w.extend(temp_group_map_content);

                // Also clear the queue counters since the groups have been rebuilt.
                if let Some(counters_arc) = &self.group_queue_counters {
                    let mut counters_w = counters_arc.write().map_err(|e| AppStoreError::LockError(format!("Failed to acquire write lock on group_queue_counters for {}: {}", self.type_name, e)))?;
                    counters_w.clear();
                    info!("{}: Cleared group queue counters due to data refresh.", self.type_name);
                }
            }
        }
        Ok(())
    }

    pub fn delete_by_group_id(&self, group_id: i64) -> Result<Vec<S>, AppStoreError> {
        let mut deleted_items = Vec::new();
        if self.id_set_by_group_id.is_none() {
            // If group mapping is not enabled for this store, this operation is a no-op or an error.
            // Returning Ok with empty vec, or could return an error.
            return Ok(deleted_items);
        }

        let item_ids_to_delete: HashSet<i64>;
        // Scope for group_map_w lock
        {
            let mut group_map_w = self.get_id_set_by_group_id_write_lock().unwrap()?; // Assumes lock will succeed
            if let Some(ids_set) = group_map_w.remove(&group_id) {
                item_ids_to_delete = ids_set;
                // Also remove the counter for this group.
                if let Some(counters_arc) = &self.group_queue_counters {
                    let mut counters_w = counters_arc.write().map_err(|e| AppStoreError::LockError(format!("Failed to acquire write lock on group_queue_counters for {}: {}", self.type_name, e)))?;
                    counters_w.remove(&group_id);
                }
            } else {
                // No items for this group_id
                return Ok(deleted_items);
            }
        }

        let mut items_map_w = self.get_items_by_id_write()?;
        let mut id_map_w_opt = self.id_by_item_key.as_ref().map(|arc| arc.write().unwrap());


        for item_id in item_ids_to_delete {
            if let Some(removed_item) = items_map_w.remove(&item_id) {
                if let Some(id_map_w) = id_map_w_opt.as_mut() {
                    id_map_w.remove(&removed_item.key());
                }
                info!("{}: Deleted item ID {} (key '{}', group_id {:?}) via delete_by_group_id for group {}", self.type_name, removed_item.id(), removed_item.key(), removed_item.group_id(), group_id);
                deleted_items.push(removed_item);
            }
        }
        Ok(deleted_items)
    }

    pub fn list_by_group_id(&self, group_id: i64) -> Result<Vec<S>, AppStoreError> {
        if self.id_set_by_group_id.is_none() {
            // Group mapping is not enabled for this store.
            return Ok(Vec::new()); // Or return an error like AppStoreError::NotSupported("Grouping not enabled".to_string())
        }

        let group_map_arc = self.id_set_by_group_id.as_ref().unwrap(); // Safe due to check above
        let group_map_r = group_map_arc.read().map_err(|e| AppStoreError::LockError(format!("Failed to acquire read lock on id_set_by_group_id for {}: {}", self.type_name, e)))?;
        
        let item_ids_in_group = match group_map_r.get(&group_id) {
            Some(ids_set) => ids_set.clone(), // Clone the HashSet of IDs
            None => return Ok(Vec::new()), // No items for this group_id
        };
        drop(group_map_r); // Release read lock on group map

        if item_ids_in_group.is_empty() {
            return Ok(Vec::new());
        }

        let items_map_r = self.get_items_by_id_read()?;
        let mut items_in_group = Vec::with_capacity(item_ids_in_group.len());

        for item_id in item_ids_in_group {
            if let Some(item) = items_map_r.get(&item_id) {
                items_in_group.push(item.clone());
            } else {
                // This case (ID in group map but not in items_by_id) indicates an inconsistency.
                // Log a warning, but decide if this should be a hard error.
                warn!("{}: Item ID {} found in group {} but not in main items map. Cache might be inconsistent.", self.type_name, item_id, group_id);
            }
        }
        Ok(items_in_group)
    }

    pub fn get_one_by_group_id(&self, group_id: i64, strategy: GroupItemSelectionStrategy) -> Result<Option<S>, AppStoreError> {
        if self.id_set_by_group_id.is_none() {
            return Ok(None);
        }

        let group_map_arc = self.id_set_by_group_id.as_ref().unwrap();
        let group_map_r = group_map_arc.read().map_err(|e| AppStoreError::LockError(format!("Failed to acquire read lock on id_set_by_group_id for {}: {}", self.type_name, e)))?;

        let item_id = if let Some(ids_set) = group_map_r.get(&group_id) {
            if ids_set.is_empty() {
                // Early return to drop lock sooner
                drop(group_map_r);
                return Ok(None);
            }

            match strategy {
                GroupItemSelectionStrategy::Random => {
                    let ids_vec: Vec<_> = ids_set.iter().copied().collect();
                    ids_vec.choose(&mut rand::rng()).copied()
                }
                GroupItemSelectionStrategy::Queue => {
                    // To make it deterministic, we sort the IDs.
                    let mut ids_vec: Vec<_> = ids_set.iter().copied().collect();
                    ids_vec.sort();

                    // Round-robin logic
                    let counters_arc = self.group_queue_counters.as_ref().unwrap(); // Safe due to with_group_map check
                    let mut counters_w = counters_arc.write().map_err(|e| AppStoreError::LockError(format!("Failed to acquire write lock on group_queue_counters for {}: {}", self.type_name, e)))?;
                    
                    let counter = counters_w.entry(group_id).or_insert(0);
                    let mut index = *counter;

                    if index >= ids_vec.len() {
                        index = 0; // Reset if counter is out of bounds
                    }

                    let item_id = ids_vec.get(index).copied();
                    *counter = (index + 1) % ids_vec.len();
                    item_id
                }
            }
        } else {
            None
        };

        drop(group_map_r);

        if let Some(id) = item_id {
            self.get_by_id(id)
        } else {
            Ok(None)
        }
    }
}


#[derive(Clone)]
pub struct AppState {
  pub system_api_key_store: StateStore<SystemApiKey>,
  pub provider_store: StateStore<Provider>,
  pub model_store: ModelStore,
  pub model_alias_store: StateStore<ModelAlias>,
  pub access_control_store: StateStore<ApiAccessControlPolicy>,
  pub provider_api_key_store: StateStore<ProviderApiKey>,
  pub billing_plan_store: StateStore<BillingPlan>,
  pub price_rule_store: StateStore<PriceRule>,
  pub custom_field_link_store: CustomFieldLinkStore,
}

impl AppState {
    pub fn reload(&self) {
        info!("Reloading AppState: Starting SystemApiKeys refresh...");
        match SystemApiKey::list_all() {
            Ok(all_keys) => {
                if let Err(e) = self.system_api_key_store.refresh_data(all_keys) {
                    warn!("Failed to refresh SystemApiKeyStore: {:?}", e);
                } else {
                    info!("Reloading AppState: SystemApiKeyStore refreshed.");
                }
            }
            Err(e) => {
                warn!("Failed to load system API keys for SystemApiKeyStore refresh: {:?}", e);
            }
        }

        info!("Reloading AppState: Starting Providers refresh...");
        match Provider::list_all() {
            Ok(all_providers) => {
                if let Err(e) = self.provider_store.refresh_data(all_providers) {
                    warn!("Failed to refresh ProviderStore: {:?}", e);
                } else {
                    info!("Reloading AppState: ProviderStore refreshed.");
                }
            }
            Err(e) => {
                warn!("Failed to load providers for ProviderStore refresh: {:?}", e);
            }
        }

        info!("Reloading AppState: Starting ModelStore refresh...");
        match Model::list_all() {
            Ok(all_models) => {
                if let Err(e) = self.model_store.refresh_data(all_models, &self.provider_store) {
                    warn!("Failed to refresh ModelStore: {:?}", e);
                } else {
                    info!("Reloading AppState: ModelStore refreshed.");
                }
            }
            Err(e) => {
                warn!("Failed to load models for ModelStore refresh: {:?}", e);
            }
        }

        info!("Reloading AppState: Starting ModelAliasStore refresh...");
        match ModelAlias::list_all() {
            Ok(all_aliases) => {
                if let Err(e) = self.model_alias_store.refresh_data(all_aliases) {
                    warn!("Failed to refresh ModelAliasStore: {:?}", e);
                } else {
                    info!("Reloading AppState: ModelAliasStore refreshed.");
                }
            }
            Err(e) => {
                warn!("Failed to load model aliases for ModelAliasStore refresh: {:?}", e);
            }
        }

        info!("Reloading AppState: Starting AccessControlStore refresh...");
        match DbAccessControlPolicy::list_all() { // Use DbAccessControlPolicy here for fetching from DB
            Ok(all_policies) => {
                if let Err(e) = self.access_control_store.refresh_data(all_policies) {
                    warn!("Failed to refresh AccessControlStore: {:?}", e);
                } else {
                    info!("Reloading AppState: AccessControlStore refreshed.");
                }
            }
            Err(e) => {
                warn!("Failed to load access control policies for AccessControlStore refresh: {:?}", e);
            }
        }

        info!("Reloading AppState: Starting ProviderApiKeys refresh...");
        match ProviderApiKey::list_all() {
            Ok(all_keys) => {
                if let Err(e) = self.provider_api_key_store.refresh_data(all_keys) {
                    warn!("Failed to refresh ProviderApiKeyStore: {:?}", e);
                } else {
                    info!("Reloading AppState: ProviderApiKeyStore refreshed.");
                }
            }
            Err(e) => {
                warn!("Failed to load provider API keys for ProviderApiKeyStore refresh: {:?}", e);
            }
        }

        info!("Reloading AppState: Starting CustomFieldLinkStore refresh...");
        match CustomFieldDefinition::list_all_active() {
            Ok(all_definitions) => {
                match CustomFieldDefinition::list_all_enabled_model_assignments() {
                    Ok(all_model_assignments) => {
                        match CustomFieldDefinition::list_all_enabled_provider_assignments() {
                            Ok(all_provider_assignments) => {
                                if let Err(e) = self.custom_field_link_store.refresh_data(
                                    all_definitions,
                                    all_model_assignments,
                                    all_provider_assignments,
                                ) {
                                    warn!("Failed to refresh CustomFieldLinkStore: {:?}", e);
                                } else {
                                    info!("Reloading AppState: CustomFieldLinkStore refreshed.");
                                }
                            }
                            Err(e) => warn!("Failed to load provider custom field assignments for CustomFieldLinkStore refresh: {:?}", e),
                        }
                    }
                    Err(e) => warn!("Failed to load model custom field assignments for CustomFieldLinkStore refresh: {:?}", e),
                }
            }
            Err(e) => warn!("Failed to load custom field definitions for CustomFieldLinkStore refresh: {:?}", e),
        }

        info!("Reloading AppState: Starting BillingPlanStore refresh...");
        match BillingPlan::list_all() {
            Ok(all_plans) => {
                if let Err(e) = self.billing_plan_store.refresh_data(all_plans) {
                    warn!("Failed to refresh BillingPlanStore: {:?}", e);
                } else {
                    info!("Reloading AppState: BillingPlanStore refreshed.");
                }
            }
            Err(e) => {
                warn!(
                    "Failed to load billing plans for BillingPlanStore refresh: {:?}",
                    e
                );
            }
        }

        info!("Reloading AppState: Starting PriceRuleStore refresh...");
        match PriceRule::list_all() {
            Ok(all_rules) => {
                if let Err(e) = self.price_rule_store.refresh_data(all_rules) {
                    warn!("Failed to refresh PriceRuleStore: {:?}", e);
                } else {
                    info!("Reloading AppState: PriceRuleStore refreshed.");
                }
            }
            Err(e) => {
                warn!(
                    "Failed to load price rules for PriceRuleStore refresh: {:?}",
                    e
                );
            }
        }
        info!("AppState reloaded successfully.");
    }
}

#[derive(Debug, Error)]
pub enum AppStoreError {
    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Resource already exists: {0}")]
    AlreadyExists(String),

    #[error("A lock could not be acquired: {0}")]
    LockError(String),
}

// --- ModelStore Definition and Implementation ---
// ModelStore uses StateStore<Model> internally for core storage,
// but retains its own logic for composite key management.
#[derive(Debug, Clone)]
pub struct ModelStore {
    core_store: StateStore<Model>, // Model itself does not use group_id for now.
    id_by_composite_key: Arc<RwLock<HashMap<(String, String), i64>>>, // (provider_key, model_name) -> model_id
}

impl ModelStore {
    pub fn new() -> Self {
        ModelStore {
            // For Model, with_key_map is true (for model.key() which is model_name).
            // with_group_map is false as Model::group_id() currently returns None.
            // If Model were to be grouped by provider_id, this would be true.
            core_store: StateStore::<Model>::new("Model", true, false), 
            id_by_composite_key: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // Removed get_models_by_id_read and get_models_by_id_write
    // Operations will go through self.core_store methods or specific composite key logic.

    fn get_id_by_composite_key_read(&self) -> Result<RwLockReadGuard<'_, HashMap<(String, String), i64>>, AppStoreError> {
        self.id_by_composite_key.read().map_err(|e| AppStoreError::LockError(format!("Failed to acquire read lock on id_by_composite_key: {}", e)))
    }

    fn get_id_by_composite_key_write(&self) -> Result<RwLockWriteGuard<'_, HashMap<(String, String), i64>>, AppStoreError> {
        self.id_by_composite_key.write().map_err(|e| AppStoreError::LockError(format!("Failed to acquire write lock on id_by_composite_key: {}", e)))
    }

    pub fn get_by_composite_key(&self, provider_key: &str, model_name: &str) -> Result<Option<Model>, AppStoreError> {
        let composite_key = (provider_key.to_string(), model_name.to_string());
        let id_map = self.get_id_by_composite_key_read()?;
        debug!("down {:?}", id_map);
        if let Some(&id) = id_map.get(&composite_key) {
            // Fetch from core_store using the ID found from composite key
            self.core_store.get_by_id(id)
        } else {
            debug!("none");
            Ok(None)
        }
    }

    pub fn get_by_id(&self, id: i64) -> Result<Option<Model>, AppStoreError> {
        self.core_store.get_by_id(id)
    }
    
    pub fn add(&self, model: Model, provider_store: &StateStore<Provider>) -> Result<Model, AppStoreError> {
        let provider = provider_store.get_by_id(model.provider_id)?
            .ok_or_else(|| AppStoreError::NotFound(format!("Provider with ID {} not found for model {}", model.provider_id, model.id)))?;
        let composite_key = (provider.key(), model.model_name.clone());

        // First, try to add to core_store. This checks for duplicate model ID and model.key() (model_name).
        let added_to_core_store = self.core_store.add(model.clone())?; // Clone model for core_store

        // If successful, then manage the composite key map.
        let mut composite_key_map_w = self.get_id_by_composite_key_write()?;
        if composite_key_map_w.contains_key(&composite_key) {
            // This case implies a composite key conflict even if the model ID/name was unique in core_store.
            // We need to roll back the add from core_store.
            // This scenario should be rare if data integrity is maintained, but good to handle.
            self.core_store.delete(added_to_core_store.id())?; // Attempt to remove from core_store
            return Err(AppStoreError::AlreadyExists(format!("Model with composite key '{:?}' already exists, but core store add was temporarily successful and then rolled back.", composite_key)));
        }
        
        composite_key_map_w.insert(composite_key.clone(), added_to_core_store.id());
        info!("ModelStore: Added model ID {}, composite key '{:?}'", added_to_core_store.id(), composite_key);
        Ok(added_to_core_store)
    }

    pub fn update(&self, updated_model: Model, provider_store: &StateStore<Provider>) -> Result<Model, AppStoreError> {
        // Get existing model from core_store to find old composite key
        let existing_model_in_core = self.core_store.get_by_id(updated_model.id())?
            .ok_or_else(|| AppStoreError::NotFound(format!("Model with ID {} not found in core store for update", updated_model.id())))?;

        let old_provider = provider_store.get_by_id(existing_model_in_core.provider_id)?
            .ok_or_else(|| AppStoreError::NotFound(format!("Old provider with ID {} not found for model {}", existing_model_in_core.provider_id, existing_model_in_core.id)))?;
        let old_composite_key = (old_provider.key(), existing_model_in_core.model_name.clone());

        // Update in core_store first. This handles model.key() (model_name) changes.
        let updated_in_core_store = self.core_store.update(updated_model.clone())?;

        // Now handle composite key changes
        let new_provider = provider_store.get_by_id(updated_in_core_store.provider_id)?
            .ok_or_else(|| AppStoreError::NotFound(format!("New provider with ID {} not found for model {}", updated_in_core_store.provider_id, updated_in_core_store.id)))?;
        let new_composite_key = (new_provider.key(), updated_in_core_store.model_name.clone());

        if old_composite_key != new_composite_key {
            let mut composite_key_map_w = self.get_id_by_composite_key_write()?;
            if let Some(&conflicting_id) = composite_key_map_w.get(&new_composite_key) {
                if conflicting_id != updated_in_core_store.id() {
                    // Composite key conflict. Rollback core_store update is complex here.
                    // For now, error out. A more robust solution might involve transactions or pre-checks.
                    return Err(AppStoreError::AlreadyExists(format!(
                        "Cannot update model ID {}: new composite key '{:?}' is already in use by model ID {}. Core store update was successful but composite key update failed.",
                        updated_in_core_store.id(), new_composite_key, conflicting_id
                    )));
                }
            }
            composite_key_map_w.remove(&old_composite_key);
            composite_key_map_w.insert(new_composite_key.clone(), updated_in_core_store.id());
        }
        
        info!("ModelStore: Updated model ID {}, new composite key '{:?}'", updated_in_core_store.id(), new_composite_key);
        Ok(updated_in_core_store)
    }

    pub fn delete(&self, id: i64, provider_store: &StateStore<Provider>) -> Result<Model, AppStoreError> {
        // Get the model from core_store to determine its composite key before deleting
        let model_to_delete = self.core_store.get_by_id(id)?
            .ok_or_else(|| AppStoreError::NotFound(format!("Model with ID {} not found in core store for deletion", id)))?;

        let provider = provider_store.get_by_id(model_to_delete.provider_id)?
            .ok_or_else(|| AppStoreError::NotFound(format!("Provider with ID {} not found for model to be deleted {}", model_to_delete.provider_id, model_to_delete.id)))?;
        let composite_key = (provider.key(), model_to_delete.model_name.clone());

        // Delete from core_store first
        let removed_model = self.core_store.delete(id)?;

        // Then remove from composite key map
        let mut composite_key_map_w = self.get_id_by_composite_key_write()?;
        composite_key_map_w.remove(&composite_key);
        
        info!("ModelStore: Deleted model ID {}, composite key '{:?}'", removed_model.id(), composite_key);
        Ok(removed_model)
    }
    
    pub fn get_all(&self) -> Result<Vec<Model>, AppStoreError> {
        self.core_store.get_all()
    }

    pub fn refresh_data(&self, new_models: Vec<Model>, provider_store: &StateStore<Provider>) -> Result<(), AppStoreError> {
        // First, validate and prepare composite keys
        let mut temp_id_by_composite_key = HashMap::with_capacity(new_models.len());
        // The new_models vec will be passed directly to core_store.refresh_data,
        // which will handle its own checks for duplicate IDs and model.key() (model_names).

        for model in &new_models { // Iterate by reference to avoid consuming new_models yet
            let provider = provider_store.get_by_id(model.provider_id)?
                .ok_or_else(|| AppStoreError::NotFound(format!("Provider with ID {} not found for model {} during refresh", model.provider_id, model.id)))?;
            let composite_key = (provider.key(), model.model_name.clone());

            // Check for duplicate composite keys in the incoming dataset
            if temp_id_by_composite_key.contains_key(&composite_key) {
                return Err(AppStoreError::AlreadyExists(format!("Duplicate Model composite key '{:?}' in refresh dataset.", composite_key)));
            }
            temp_id_by_composite_key.insert(composite_key, model.id);
        }

        // Refresh the core_store. This will handle its internal consistency.
        // new_models is moved here.
        self.core_store.refresh_data(new_models)?;

        // Refresh the composite key map
        let mut composite_key_map_w = self.get_id_by_composite_key_write()?;
        composite_key_map_w.clear();
        composite_key_map_w.extend(temp_id_by_composite_key);
        debug!("init model stroe {:?}", composite_key_map_w);
        
        Ok(())
    }
}

// --- CustomFieldLinkStore Definition and Implementation ---
#[derive(Debug, Clone)]
pub struct CustomFieldLinkStore {
    definition_store: StateStore<CustomFieldDefinition>,
    definition_ids_by_entity_id: Arc<RwLock<HashMap<i64, HashSet<i64>>>>,
}

impl CustomFieldLinkStore {
    pub fn new() -> Self {
        CustomFieldLinkStore {
            // CustomFieldDefinition uses id as key, and no grouping within its own store.
            definition_store: StateStore::<CustomFieldDefinition>::new("CustomFieldDefinition", false, false),
            definition_ids_by_entity_id: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn refresh_data(
        &self,
        all_definitions: Vec<CustomFieldDefinition>,
        all_model_assignments: Vec<ModelCustomFieldAssignment>,
        all_provider_assignments: Vec<ProviderCustomFieldAssignment>,
    ) -> Result<(), AppStoreError> {
        // Refresh definitions
        self.definition_store.refresh_data(all_definitions)?;

        let mut temp_def_ids_by_entity_id = HashMap::new();

        // Populate from provider assignments
        for assignment in all_provider_assignments {
            temp_def_ids_by_entity_id
                .entry(assignment.provider_id) // Use provider_id as entity_id
                .or_insert_with(HashSet::new)
                .insert(assignment.custom_field_definition_id);
        }

        // Populate from model assignments
        for assignment in all_model_assignments {
            temp_def_ids_by_entity_id
                .entry(assignment.model_id) // Use model_id as entity_id
                .or_insert_with(HashSet::new)
                .insert(assignment.custom_field_definition_id);
        }

        let mut entity_map_w = self.definition_ids_by_entity_id.write()
            .map_err(|e| AppStoreError::LockError(format!("Failed to acquire write lock on definition_ids_by_entity_id: {}", e)))?;
        entity_map_w.clear();
        entity_map_w.extend(temp_def_ids_by_entity_id);
        
        // Calculate total assignments and entities for logging
        let total_assignments: usize = entity_map_w.values().map(|s| s.len()).sum();
        let total_entities: usize = entity_map_w.len();
        drop(entity_map_w); // Release lock before logging if read lock is taken again by info! macro internals

        info!("CustomFieldLinkStore: Refreshed with {} total assignments across {} entities.",
              total_assignments,
              total_entities);
        Ok(())
    }

    pub fn get_definitions_by_entity_id(&self, entity_id: i64) -> Result<Vec<CustomFieldDefinition>, AppStoreError> {
        let entity_map_r = self.definition_ids_by_entity_id.read()
            .map_err(|e| AppStoreError::LockError(format!("Failed to acquire read lock on definition_ids_by_entity_id: {}", e)))?;

        let definition_ids_for_entity = match entity_map_r.get(&entity_id) {
            Some(ids_set) => ids_set.clone(), // Clone the HashSet of IDs
            None => return Ok(Vec::new()),    // No definitions for this entity_id
        };
        drop(entity_map_r); // Release read lock

        if definition_ids_for_entity.is_empty() {
            return Ok(Vec::new());
        }

        let mut definitions = Vec::with_capacity(definition_ids_for_entity.len());
        for def_id in definition_ids_for_entity {
            if let Some(definition) = self.definition_store.get_by_id(def_id)? {
                definitions.push(definition);
            } else {
                warn!("CustomFieldLinkStore: Definition ID {} found in assignment map for entity_id {} but not in definition_store. Cache might be inconsistent.", def_id, entity_id);
            }
        }
        Ok(definitions)
    }

    pub fn add_definition(&self, definition: CustomFieldDefinition) -> Result<CustomFieldDefinition, AppStoreError> {
        self.definition_store.add(definition)
    }

    pub fn update_definition(&self, definition: CustomFieldDefinition) -> Result<CustomFieldDefinition, AppStoreError> {
        self.definition_store.update(definition)
    }

    pub fn remove_definition(&self, definition_id: i64) -> Result<CustomFieldDefinition, AppStoreError> {
        let removed_definition = self.definition_store.delete(definition_id)?;

        // Also remove this definition ID from all entity links
        let mut entity_map_w = self.definition_ids_by_entity_id.write()
            .map_err(|e| AppStoreError::LockError(format!("Failed to acquire write lock on definition_ids_by_entity_id for remove_definition: {}", e)))?;
        
        for entity_links in entity_map_w.values_mut() {
            entity_links.remove(&definition_id);
        }

        // clean up entities with no links left
        entity_map_w.retain(|_, links| !links.is_empty());

        info!("CustomFieldLinkStore: Removed definition ID {} and its links.", definition_id);
        Ok(removed_definition)
    }

    pub fn add_link(&self, definition_id: i64, entity_id: i64) -> Result<(), AppStoreError> {
        // Ensure the definition exists in the store before adding a link to it.
        if self.definition_store.get_by_id(definition_id)?.is_none() {
            return Err(AppStoreError::NotFound(format!("Cannot add link: CustomFieldDefinition with ID {} not found in store.", definition_id)));
        }

        let mut entity_map_w = self.definition_ids_by_entity_id.write()
            .map_err(|e| AppStoreError::LockError(format!("Failed to acquire write lock on definition_ids_by_entity_id for add_link: {}", e)))?;
        
        entity_map_w.entry(entity_id).or_insert_with(HashSet::new).insert(definition_id);
        info!("CustomFieldLinkStore: Added link for definition ID {} to entity ID {}.", definition_id, entity_id);
        Ok(())
    }

    pub fn remove_link(&self, definition_id: i64, entity_id: i64) -> Result<(), AppStoreError> {
        let mut entity_map_w = self.definition_ids_by_entity_id.write()
            .map_err(|e| AppStoreError::LockError(format!("Failed to acquire write lock on definition_ids_by_entity_id for remove_link: {}", e)))?;

        if let Some(entity_links) = entity_map_w.get_mut(&entity_id) {
            let removed = entity_links.remove(&definition_id);
            if removed {
                info!("CustomFieldLinkStore: Removed link for definition ID {} from entity ID {}.", definition_id, entity_id);
            }
            if entity_links.is_empty() {
                entity_map_w.remove(&entity_id);
                info!("CustomFieldLinkStore: Entity ID {} has no more links, removing from map.", entity_id);
            }
        }
        Ok(())
    }
}


// --- SystemApiKeyStore and ProviderStore are now replaced by StateStore<S> ---

fn create_system_api_key_store() -> StateStore<SystemApiKey> {
  let store = StateStore::<SystemApiKey>::new("SystemApiKey", true, false); // with_key_map: true, with_group_map: false
  match SystemApiKey::list_all() {
      Ok(all_keys) => {
          match store.refresh_data(all_keys) {
              Ok(_) => {
                  match store.get_all() {
                      Ok(keys_vec) => info!("Successfully loaded {} {}s into Store.", keys_vec.len(), store.type_name),
                      Err(e) => warn!("{} Store populated but failed to get count: {:?}", store.type_name, e),
                  }
              }
              Err(e) => warn!("Failed to initially populate {} Store: {:?}", store.type_name, e),
          }
      }
      Err(e) => {
          warn!("Failed to load {}s from DB, Store will be empty: {:?}", store.type_name, e);
      }
  }
  store
}

fn create_provider_store() -> StateStore<Provider> {
  let provider_store = StateStore::<Provider>::new("Provider", true, false); // with_key_map: true, with_group_map: false
  match Provider::list_all() {
      Ok(all_providers) => {
          match provider_store.refresh_data(all_providers) {
              Ok(_) => {
                  match provider_store.get_all() {
                       Ok(p_vec) => info!("Successfully loaded {} {}s into Store.", p_vec.len(), provider_store.type_name),
                       Err(e) => warn!("{} Store populated but failed to get count: {:?}", provider_store.type_name, e),
                  }
              }
              Err(e) => warn!("Failed to initially populate {} Store: {:?}", provider_store.type_name, e),
          }
      }
      Err(e) => {
          warn!("Failed to load {}s from DB, Store will be empty: {:?}", provider_store.type_name, e);
      }
  }
  provider_store
}

fn create_model_store(provider_store: &StateStore<Provider>) -> ModelStore {
    let model_store = ModelStore::new();
    match Model::list_all() {
        Ok(all_models) => {
            match model_store.refresh_data(all_models, provider_store) { // Pass &StateStore<Provider>
                Ok(_) => {
                    match model_store.get_all() {
                        Ok(m_vec) => info!("Successfully loaded {} models into ModelStore.", m_vec.len()),
                        Err(e) => warn!("ModelStore populated but failed to get count: {:?}", e),
                    }
                }
                Err(e) => warn!("Failed to initially populate ModelStore: {:?}", e),
            }
        }
        Err(e) => {
            warn!("Failed to load models from DB, ModelStore will be empty: {:?}", e);
        }
    }
    model_store
}

fn create_model_alias_store() -> StateStore<ModelAlias> {
    let store = StateStore::<ModelAlias>::new("ModelAlias", true, false); // with_key_map: true, with_group_map: false
    match ModelAlias::list_all() {
        Ok(all_aliases) => {
            match store.refresh_data(all_aliases) {
                Ok(_) => {
                    match store.get_all() {
                        Ok(aliases_vec) => info!("Successfully loaded {} {}s into Store.", aliases_vec.len(), store.type_name),
                        Err(e) => warn!("{} Store populated but failed to get count: {:?}", store.type_name, e),
                    }
                }
                Err(e) => warn!("Failed to initially populate {} Store: {:?}", store.type_name, e),
            }
        }
        Err(e) => {
            warn!("Failed to load {}s from DB, Store will be empty: {:?}", store.type_name, e);
        }
    }
    store
}

fn create_access_control_store() -> StateStore<ApiAccessControlPolicy> {
    let store = StateStore::<ApiAccessControlPolicy>::new("AccessControlPolicy", false, false); // with_key_map: false, with_group_map: false
    match DbAccessControlPolicy::list_all() { // Use DbAccessControlPolicy for direct DB interaction
        Ok(all_policies) => {
            match store.refresh_data(all_policies) { // refresh_data expects Vec<ApiAccessControlPolicy>
                Ok(_) => {
                    match store.get_all() {
                        Ok(p_vec) => info!("Successfully loaded {} {}s into Store.", p_vec.len(), store.type_name),
                        Err(e) => warn!("{} Store populated but failed to get count: {:?}", store.type_name, e),
                    }
                }
                Err(e) => warn!("Failed to initially populate {} Store: {:?}", store.type_name, e),
            }
        }
        Err(e) => {
            warn!("Failed to load {}s from DB, Store will be empty: {:?}", store.type_name, e);
        }
    }
    store
}

fn create_provider_api_key_store() -> StateStore<ProviderApiKey> {
    // For ProviderApiKey, with_key_map is false (key() is id.to_string(), not unique api_key content).
    // with_group_map is true, to group by provider_id.
    let store = StateStore::<ProviderApiKey>::new("ProviderApiKey", false, true); 
    match ProviderApiKey::list_all() {
        Ok(all_items) => {
            match store.refresh_data(all_items) {
                Ok(_) => {
                    match store.get_all() {
                        Ok(items_vec) => info!("Successfully loaded {} {}s into Store.", items_vec.len(), store.type_name),
                        Err(e) => warn!("{} Store populated but failed to get count: {:?}", store.type_name, e),
                    }
                }
                Err(e) => warn!("Failed to initially populate {} Store: {:?}", store.type_name, e),
            }
        }
        Err(e) => {
            warn!("Failed to load {}s from DB, Store will be empty: {:?}", store.type_name, e);
        }
    }
    store
}

fn create_custom_field_link_store() -> CustomFieldLinkStore {
    let store = CustomFieldLinkStore::new();
    match CustomFieldDefinition::list_all_active() {
        Ok(all_definitions) => {
            match CustomFieldDefinition::list_all_enabled_model_assignments() {
                Ok(all_model_assignments) => {
                    match CustomFieldDefinition::list_all_enabled_provider_assignments() {
                        Ok(all_provider_assignments) => {
                            if let Err(e) = store.refresh_data(
                                all_definitions.clone(), // Clone if refresh_data consumes
                                all_model_assignments,
                                all_provider_assignments,
                            ) {
                                warn!("Failed to initially populate CustomFieldLinkStore: {:?}", e);
                            } else {
                                match store.definition_store.get_all() {
                                     Ok(defs_vec) => info!("Successfully loaded {} CustomFieldDefinitions into CustomFieldLinkStore.", defs_vec.len()),
                                     Err(e) => warn!("CustomFieldLinkStore populated but failed to get definition count: {:?}", e),
                                }
                            }
                        }
                        Err(e) => warn!("Failed to load provider custom field assignments for CustomFieldLinkStore initial population: {:?}", e),
                    }
                }
                Err(e) => warn!("Failed to load model custom field assignments for CustomFieldLinkStore initial population: {:?}", e),
            }
        }
        Err(e) => warn!("Failed to load custom field definitions for CustomFieldLinkStore initial population: {:?}", e),
    }
    store
}

fn create_billing_plan_store() -> StateStore<BillingPlan> {
    // For BillingPlan, with_key_map is true (name), with_group_map is false.
    let store = StateStore::<BillingPlan>::new("BillingPlan", true, false);
    match BillingPlan::list_all() {
        Ok(all_items) => {
            match store.refresh_data(all_items) {
                Ok(_) => match store.get_all() {
                    Ok(items_vec) => info!(
                        "Successfully loaded {} {}s into Store.",
                        items_vec.len(),
                        store.type_name
                    ),
                    Err(e) => {
                        warn!("{} Store populated but failed to get count: {:?}", store.type_name, e)
                    }
                },
                Err(e) => warn!(
                    "Failed to initially populate {} Store: {:?}",
                    store.type_name, e
                ),
            }
        }
        Err(e) => {
            warn!(
                "Failed to load {}s from DB, Store will be empty: {:?}",
                store.type_name, e
            );
        }
    }
    store
}

fn create_price_rule_store() -> StateStore<PriceRule> {
    // For PriceRule, with_key_map is false.
    // with_group_map is true, to group by plan_id.
    let store = StateStore::<PriceRule>::new("PriceRule", false, true);
    match PriceRule::list_all() {
        Ok(all_items) => {
            match store.refresh_data(all_items) {
                Ok(_) => match store.get_all() {
                    Ok(items_vec) => info!(
                        "Successfully loaded {} {}s into Store.",
                        items_vec.len(),
                        store.type_name
                    ),
                    Err(e) => {
                        warn!("{} Store populated but failed to get count: {:?}", store.type_name, e)
                    }
                },
                Err(e) => warn!(
                    "Failed to initially populate {} Store: {:?}",
                    store.type_name, e
                ),
            }
        }
        Err(e) => {
            warn!(
                "Failed to load {}s from DB, Store will be empty: {:?}",
                store.type_name, e
            );
        }
    }
    store
}

pub fn create_app_state() -> Arc<AppState> {
    let system_api_key_store = create_system_api_key_store();
    let provider_store = create_provider_store();
    let model_store = create_model_store(&provider_store);
    let model_alias_store = create_model_alias_store();
    let access_control_store = create_access_control_store();
    let provider_api_key_store = create_provider_api_key_store();
    let billing_plan_store = create_billing_plan_store();
    let price_rule_store = create_price_rule_store();
    let custom_field_link_store = create_custom_field_link_store();
    Arc::new(AppState {
        system_api_key_store,
        provider_store,
        model_store,
        model_alias_store,
        access_control_store,
        provider_api_key_store,
        billing_plan_store,
        price_rule_store,
        custom_field_link_store,
    })
}

pub type StateRouter = Router<Arc<AppState>>;

pub fn create_state_router() -> StateRouter {
  Router::<Arc<AppState>>::new()
}
