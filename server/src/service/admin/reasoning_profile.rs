use std::sync::Arc;

use crate::controller::BaseError;
use crate::database::reasoning_profile::{
    ReasoningPatchFamily, ReasoningPreset, ReasoningProfile, ReasoningProfilePreset,
    ReasoningProfileWithPresets, UpdateReasoningProfileData, UpdateReasoningProfilePresetData,
};

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{AdminCatalogInvalidation, AdminMutationEffect, AdminMutationRunner};

#[derive(Debug, Clone)]
pub struct CreateReasoningProfileInput {
    pub profile_key: String,
    pub name: String,
    pub description: Option<String>,
    pub family_key: String,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateReasoningProfileInput {
    pub profile_key: Option<String>,
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub family_key: Option<String>,
    pub is_enabled: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct UpsertReasoningProfilePresetInput {
    pub preset_key: String,
    pub expose_in_models: bool,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateReasoningProfilePresetInput {
    pub preset_key: Option<String>,
    pub expose_in_models: Option<bool>,
    pub is_enabled: Option<bool>,
}

pub struct ReasoningProfileAdminService {
    mutation_runner: Arc<AdminMutationRunner>,
}

impl ReasoningProfileAdminService {
    pub(crate) fn new(mutation_runner: Arc<AdminMutationRunner>) -> Self {
        Self { mutation_runner }
    }

    #[cfg(test)]
    pub(crate) fn mutation_runner(&self) -> &Arc<AdminMutationRunner> {
        &self.mutation_runner
    }

    pub fn list_profiles(&self) -> Result<Vec<ReasoningProfileWithPresets>, BaseError> {
        ReasoningProfile::list_with_presets()
    }

    pub fn get_profile(&self, id: i64) -> Result<ReasoningProfileWithPresets, BaseError> {
        load_profile(id)
    }

    pub async fn create_profile(
        &self,
        input: CreateReasoningProfileInput,
    ) -> Result<ReasoningProfileWithPresets, BaseError> {
        let created = ReasoningProfile::create(
            &input.profile_key,
            &input.name,
            input.description.as_deref(),
            &input.family_key,
            input.is_enabled,
        )?;

        self.run_profile_effects("create", &created).await;
        load_profile(created.id)
    }

    pub async fn update_profile(
        &self,
        id: i64,
        mut input: UpdateReasoningProfileInput,
    ) -> Result<ReasoningProfileWithPresets, BaseError> {
        let before = load_profile(id)?;
        if let Some(family_key) = input.family_key.as_deref() {
            let target_family = parse_family_key_for_update(family_key)?;
            input.family_key = Some(target_family.as_key().to_string());
            if target_family != before.family {
                validate_existing_presets_for_family(&before, target_family)?;
            }
        }

        let updated = ReasoningProfile::update(
            id,
            &UpdateReasoningProfileData {
                profile_key: input.profile_key,
                name: input.name,
                description: input.description,
                family_key: input.family_key,
                is_enabled: input.is_enabled,
            },
        )?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ReasoningProfile {
                id: updated.id,
                key: Some(before.profile.profile_key),
            }),
            AdminMutationEffect::audit(reasoning_profile_audit_event("update", &updated)),
        ])
        .await;

        load_profile(updated.id)
    }

    pub async fn delete_profile(&self, id: i64) -> Result<(), BaseError> {
        let before = load_profile(id)?;
        ReasoningProfile::delete(id)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ReasoningProfile {
                id,
                key: Some(before.profile.profile_key.clone()),
            }),
            AdminMutationEffect::audit(reasoning_profile_audit_event("delete", &before.profile)),
        ])
        .await;

        Ok(())
    }

    pub async fn upsert_profile_preset(
        &self,
        profile_id: i64,
        input: UpsertReasoningProfilePresetInput,
    ) -> Result<ReasoningProfileWithPresets, BaseError> {
        let profile = load_profile(profile_id)?;
        let preset = parse_preset_for_profile(profile.family, &input.preset_key)?;

        if let Some(existing) =
            ReasoningProfilePreset::find_by_profile_and_preset(profile_id, preset.as_key())?
        {
            ReasoningProfilePreset::update(
                existing.id,
                &UpdateReasoningProfilePresetData {
                    preset_key: Some(preset.as_key().to_string()),
                    expose_in_models: Some(input.expose_in_models),
                    is_enabled: Some(input.is_enabled),
                },
            )?;
        } else {
            ReasoningProfilePreset::create(
                profile_id,
                preset.as_key(),
                input.expose_in_models,
                input.is_enabled,
            )?;
        }

        self.run_profile_preset_effects("upsert_preset", &profile.profile)
            .await;
        load_profile(profile_id)
    }

    pub async fn update_profile_preset(
        &self,
        profile_id: i64,
        preset_id: i64,
        input: UpdateReasoningProfilePresetInput,
    ) -> Result<ReasoningProfileWithPresets, BaseError> {
        let profile = load_profile(profile_id)?;
        let existing = ReasoningProfilePreset::get_by_id(preset_id)?;
        if existing.profile_id != profile_id {
            return Err(BaseError::ParamInvalid(Some(format!(
                "reasoning profile preset {} does not belong to profile {}",
                preset_id, profile_id
            ))));
        }

        let next_preset_key = input
            .preset_key
            .as_deref()
            .unwrap_or(existing.preset_key.as_str());
        let preset = parse_preset_for_profile(profile.family, next_preset_key)?;

        ReasoningProfilePreset::update(
            preset_id,
            &UpdateReasoningProfilePresetData {
                preset_key: Some(preset.as_key().to_string()),
                expose_in_models: input.expose_in_models,
                is_enabled: input.is_enabled,
            },
        )?;

        self.run_profile_preset_effects("update_preset", &profile.profile)
            .await;
        load_profile(profile_id)
    }

    pub async fn delete_profile_preset(
        &self,
        profile_id: i64,
        preset_id: i64,
    ) -> Result<ReasoningProfileWithPresets, BaseError> {
        let profile = load_profile(profile_id)?;
        let existing = ReasoningProfilePreset::get_by_id(preset_id)?;
        if existing.profile_id != profile_id {
            return Err(BaseError::ParamInvalid(Some(format!(
                "reasoning profile preset {} does not belong to profile {}",
                preset_id, profile_id
            ))));
        }
        ReasoningProfilePreset::delete(preset_id)?;

        self.run_profile_preset_effects("delete_preset", &profile.profile)
            .await;
        load_profile(profile_id)
    }

    async fn run_profile_effects(&self, action: &'static str, profile: &ReasoningProfile) {
        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ReasoningProfile {
                id: profile.id,
                key: Some(profile.profile_key.clone()),
            }),
            AdminMutationEffect::audit(reasoning_profile_audit_event(action, profile)),
        ])
        .await;
    }

    async fn run_profile_preset_effects(&self, action: &'static str, profile: &ReasoningProfile) {
        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ReasoningProfile {
                id: profile.id,
                key: Some(profile.profile_key.clone()),
            }),
            AdminMutationEffect::audit(reasoning_profile_audit_event(action, profile)),
        ])
        .await;
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }
}

fn load_profile(id: i64) -> Result<ReasoningProfileWithPresets, BaseError> {
    ReasoningProfile::get_with_presets_by_id(id)?
        .ok_or_else(|| BaseError::NotFound(Some(format!("reasoning profile {} not found", id))))
}

fn parse_family_key_for_update(family_key: &str) -> Result<ReasoningPatchFamily, BaseError> {
    family_key
        .parse::<ReasoningPatchFamily>()
        .map_err(|err| BaseError::ParamInvalid(Some(format!("invalid reasoning family: {err}"))))
}

fn parse_preset_for_profile(
    family: ReasoningPatchFamily,
    preset_key: &str,
) -> Result<ReasoningPreset, BaseError> {
    let preset = preset_key
        .parse::<ReasoningPreset>()
        .map_err(|err| BaseError::ParamInvalid(Some(format!("invalid reasoning preset: {err}"))))?;
    if let Some(reason) = family.unsupported_preset_reason(preset) {
        return Err(BaseError::ParamInvalid(Some(format!(
            "reasoning family '{}' does not support preset '{}': {}",
            family, preset, reason
        ))));
    }
    Ok(preset)
}

fn validate_existing_presets_for_family(
    profile: &ReasoningProfileWithPresets,
    target_family: ReasoningPatchFamily,
) -> Result<(), BaseError> {
    let conflicts: Vec<String> = profile
        .presets
        .iter()
        .filter(|preset| preset.preset.is_enabled)
        .filter_map(|preset| {
            target_family
                .unsupported_preset_reason(preset.preset_key)
                .map(|reason| format!("{} ({})", preset.preset_key, reason))
        })
        .collect();

    if conflicts.is_empty() {
        return Ok(());
    }

    Err(BaseError::ParamInvalid(Some(format!(
        "reasoning profile '{}' cannot switch to family '{}': enabled preset(s) {} are not supported; disable or delete conflicting presets before changing family",
        profile.profile.profile_key,
        target_family,
        conflicts.join(", ")
    ))))
}

fn reasoning_profile_audit_event(
    action: &'static str,
    profile: &ReasoningProfile,
) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.reasoning_profile_created",
        "update" => "manager.reasoning_profile_updated",
        "delete" => "manager.reasoning_profile_deleted",
        "upsert_preset" => "manager.reasoning_profile_preset_upserted",
        "update_preset" => "manager.reasoning_profile_preset_updated",
        "delete_preset" => "manager.reasoning_profile_preset_deleted",
        _ => unreachable!("unsupported reasoning profile audit action: {action}"),
    };

    AdminAuditEvent::with_fields(
        event_name,
        [
            AdminAuditField::new("action", action),
            AdminAuditField::new("reasoning_profile_id", profile.id),
            AdminAuditField::new("profile_key", &profile.profile_key),
            AdminAuditField::new("family_key", &profile.family_key),
            AdminAuditField::new("is_enabled", profile.is_enabled),
        ],
    )
}

#[cfg(test)]
mod tests {
    use crate::database::TestDbContext;
    use crate::service::app_state::create_test_app_state;

    use super::{
        CreateReasoningProfileInput, UpdateReasoningProfileInput,
        UpdateReasoningProfilePresetInput, UpsertReasoningProfilePresetInput,
    };

    #[tokio::test]
    async fn reasoning_profile_service_creates_profile_and_preset_snapshot() {
        let test_db_context = TestDbContext::new_sqlite("admin-reasoning-profile-create.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let created = app_state
                    .admin
                    .reasoning_profile
                    .create_profile(CreateReasoningProfileInput {
                        profile_key: "openai_responses_reasoning".to_string(),
                        name: "OpenAI Responses Reasoning".to_string(),
                        description: None,
                        family_key: "openai_responses_reasoning".to_string(),
                        is_enabled: true,
                    })
                    .await
                    .expect("profile should create");

                let updated = app_state
                    .admin
                    .reasoning_profile
                    .upsert_profile_preset(
                        created.profile.id,
                        UpsertReasoningProfilePresetInput {
                            preset_key: "high".to_string(),
                            expose_in_models: true,
                            is_enabled: true,
                        },
                    )
                    .await
                    .expect("preset should create");

                assert_eq!(updated.presets.len(), 1);
                assert_eq!(updated.presets[0].suffix, "high");
                assert_eq!(
                    updated.presets[0].allowed_operation_kinds,
                    vec!["generation".to_string()]
                );

                let cached = app_state
                    .catalog
                    .get_reasoning_profile_by_id(created.profile.id)
                    .await
                    .expect("cache should load")
                    .expect("profile should be cached");
                assert_eq!(cached.presets.len(), 1);
            })
            .await;
    }

    #[tokio::test]
    async fn reasoning_profile_service_rejects_family_unsupported_preset() {
        let test_db_context =
            TestDbContext::new_sqlite("admin-reasoning-profile-unsupported.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let created = app_state
                    .admin
                    .reasoning_profile
                    .create_profile(CreateReasoningProfileInput {
                        profile_key: "gemini3_reasoning".to_string(),
                        name: "Gemini 3 Reasoning".to_string(),
                        description: None,
                        family_key: "gemini3_thinking_level".to_string(),
                        is_enabled: true,
                    })
                    .await
                    .expect("profile should create");

                let err = app_state
                    .admin
                    .reasoning_profile
                    .upsert_profile_preset(
                        created.profile.id,
                        UpsertReasoningProfilePresetInput {
                            preset_key: "auto".to_string(),
                            expose_in_models: true,
                            is_enabled: true,
                        },
                    )
                    .await
                    .expect_err("unsupported preset should fail");

                assert!(format!("{err:?}").contains("does not support preset"));

                let updated = app_state
                    .admin
                    .reasoning_profile
                    .update_profile(
                        created.profile.id,
                        UpdateReasoningProfileInput {
                            is_enabled: Some(false),
                            ..Default::default()
                        },
                    )
                    .await
                    .expect("profile should update");
                assert!(!updated.profile.is_enabled);
            })
            .await;
    }

    #[tokio::test]
    async fn reasoning_profile_service_rejects_family_change_with_enabled_unsupported_preset() {
        let test_db_context =
            TestDbContext::new_sqlite("admin-reasoning-profile-family-change-enabled.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let created = app_state
                    .admin
                    .reasoning_profile
                    .create_profile(CreateReasoningProfileInput {
                        profile_key: "gemini_budget_reasoning".to_string(),
                        name: "Gemini Budget Reasoning".to_string(),
                        description: None,
                        family_key: "gemini25_thinking_budget".to_string(),
                        is_enabled: true,
                    })
                    .await
                    .expect("profile should create");

                let with_auto = app_state
                    .admin
                    .reasoning_profile
                    .upsert_profile_preset(
                        created.profile.id,
                        UpsertReasoningProfilePresetInput {
                            preset_key: "auto".to_string(),
                            expose_in_models: true,
                            is_enabled: true,
                        },
                    )
                    .await
                    .expect("auto preset should create on gemini25");

                let err = app_state
                    .admin
                    .reasoning_profile
                    .update_profile(
                        created.profile.id,
                        UpdateReasoningProfileInput {
                            family_key: Some("anthropic_thinking_budget".to_string()),
                            ..Default::default()
                        },
                    )
                    .await
                    .expect_err("enabled unsupported preset should block family change");

                let message = format!("{err:?}");
                assert!(message.contains("gemini_budget_reasoning"));
                assert!(message.contains("anthropic_thinking_budget"));
                assert!(message.contains("auto"));

                let preset_id = with_auto.presets[0].preset.id;
                app_state
                    .admin
                    .reasoning_profile
                    .update_profile_preset(
                        created.profile.id,
                        preset_id,
                        UpdateReasoningProfilePresetInput {
                            is_enabled: Some(false),
                            ..Default::default()
                        },
                    )
                    .await
                    .expect("disabling conflicting preset should succeed");

                let switched = app_state
                    .admin
                    .reasoning_profile
                    .update_profile(
                        created.profile.id,
                        UpdateReasoningProfileInput {
                            family_key: Some("anthropic_thinking_budget".to_string()),
                            ..Default::default()
                        },
                    )
                    .await
                    .expect("disabled unsupported preset should not block family change");

                assert_eq!(switched.family.as_key(), "anthropic_thinking_budget");
                assert_eq!(switched.presets.len(), 1);
                assert_eq!(switched.presets[0].preset_key.as_key(), "auto");
                assert!(!switched.presets[0].preset.is_enabled);
            })
            .await;
    }
}
