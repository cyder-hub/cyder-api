use super::*;

use crate::database::api_key::{ApiKey, ApiKeyIssuance, CreateApiKeyPayload};
use crate::database::manager_credential::{MANAGER_ID, ManagerCredential, NewManagerCredential};
use crate::database::provider::{
    NewProvider, NewProviderApiKey, Provider, ProviderApiKeyRepository, StoredProviderApiKey,
};
use crate::database::upstream_source::NewUpstreamSource;
use crate::database::{TestDatabase, runtime::DatabaseRuntime};
use crate::schema::enum_def::{Action, ProviderApiKeyMode, UpstreamProfileType};
use crate::utils::ID_GENERATOR;
use std::env;

const KEY_A: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
const KEY_B: &str = "101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f";
const KEY_C: &str = "202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f";
const POSTGRES_SMOKE_URL_ENV: &str = "CYDER_R1_POSTGRES_MIGRATION_SMOKE_URL";
const POSTGRES_SMOKE_DATABASE: &str = "cyder_r1_migration_smoke";

fn rotation_config(mode: DownstreamSecretMode) -> SecretEncryptionConfig {
    serde_yaml::from_str(&format!(
        "downstream_mode: {}\nencryption_key: '{KEY_A}'\nprevious_encryption_key: '{KEY_B}'\n",
        match mode {
            DownstreamSecretMode::OneTime => "one_time",
            DownstreamSecretMode::Recoverable => "recoverable",
        }
    ))
    .expect("rotation config should parse")
}

fn service_for_key(key: &str) -> SecretEncryptionService {
    let config: SecretEncryptionConfig = serde_yaml::from_str(&format!(
        "downstream_mode: one_time\nencryption_key: '{key}'\n"
    ))
    .expect("single key config should parse");
    SecretEncryptionService::from_config(&config)
}

fn prepare_with_context(
    context: &TestDatabase,
    config: &SecretEncryptionConfig,
) -> Result<SecretPreparationSummary, BaseError> {
    let mut startup = context.startup_connection();
    prepare_secrets_before_startup(config, &mut startup)
}

async fn create_test_api_key(database: &DatabaseRuntime, name: &str) -> i64 {
    let secret = format!("cyder-startup-fixture-{}", ID_GENERATOR.generate_id());
    let issuance = ApiKeyIssuance::new(ID_GENERATOR.generate_id(), &secret, None);
    ApiKey::create_issued(
        database,
        &CreateApiKeyPayload {
            name: name.to_string(),
            description: None,
            default_action: Some(Action::Allow),
            is_enabled: Some(true),
            expires_at: None,
            rate_limit_rpm: None,
            max_concurrent_requests: None,
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            acl_rules: None,
        },
        &issuance,
    )
    .await
    .expect("test api key should create")
    .id
}

async fn store_encrypted_secret(database: &DatabaseRuntime, id: i64, encrypted: &EncryptedSecret) {
    let issuance = ApiKeyIssuance::new(
        id,
        &format!("cyder-startup-rotated-{id}"),
        Some(encrypted.clone()),
    );
    ApiKey::rotate_issued(database, id, &issuance)
        .await
        .expect("test encrypted secret should persist");
}

async fn load_encrypted_secret(database: &DatabaseRuntime, id: i64) -> EncryptedSecret {
    let row = ApiKey::get_stored_secret(database, id)
        .await
        .expect("stored secret should load");
    EncryptedSecret::from_parts(
        row.ciphertext,
        row.nonce,
        row.format_version,
        row.key_fingerprint,
    )
    .expect("stored secret should be structurally valid")
}

fn encrypt_for(id: i64, service: &SecretEncryptionService, value: &str) -> EncryptedSecret {
    service
        .encrypt_current(
            SecretDomain::DownstreamApiKey(id),
            &SensitiveSecret::new(value.to_string()),
        )
        .expect("test secret should encrypt")
}

async fn create_manager_credential(database: &DatabaseRuntime) {
    ManagerCredential::insert_once(
        database,
        NewManagerCredential {
            password_verifier: "$argon2id$v=19$m=65536,t=3,p=4$fixture$fixture".to_string(),
            credential_epoch: "018fa7d8-6a00-7c9a-8f7e-999999999999".to_string(),
            now: 1,
        },
    )
    .await
    .expect("manager credential should insert");
}

async fn store_manager_totp_secret(
    database: &DatabaseRuntime,
    encrypted: &EncryptedSecret,
    last_accepted_step: i64,
    enabled_at: i64,
) {
    ManagerCredential::set_totp_secret_for_test(
        database,
        crate::database::manager_credential::ManagerTotpSecret {
            secret_ciphertext: encrypted.ciphertext().to_vec(),
            secret_nonce: encrypted.nonce().to_vec(),
            secret_format_version: encrypted.format_version(),
            secret_key_fingerprint: encrypted.key_fingerprint().as_str().to_string(),
            last_accepted_step,
            enabled_at,
        },
    )
    .await
    .expect("manager TOTP fixture should persist");
}

async fn load_manager_totp_secret(database: &DatabaseRuntime) -> EncryptedSecret {
    let credential = ManagerCredential::load(database)
        .await
        .expect("manager credential should load")
        .expect("manager credential should exist");
    EncryptedSecret::from_parts(
        credential
            .totp_secret_ciphertext
            .expect("manager TOTP ciphertext"),
        credential.totp_secret_nonce.expect("manager TOTP nonce"),
        credential
            .totp_secret_format_version
            .expect("manager TOTP format"),
        credential
            .totp_secret_key_fingerprint
            .expect("manager TOTP fingerprint"),
    )
    .expect("manager TOTP tuple should be structurally valid")
}

async fn create_test_provider(database: &DatabaseRuntime, id: i64) -> Provider {
    Provider::create_optional(
        database,
        &NewProvider {
            id,
            provider_key: format!("secret-startup-provider-{id}"),
            name: format!("Secret Startup Provider {id}"),
            is_enabled: true,
            created_at: 1,
            updated_at: 1,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        },
        Some(&NewUpstreamSource {
            id,
            provider_id: id,
            profile_type: UpstreamProfileType::Openai,
            base_url: "https://api.example.com/v1".to_string(),
            use_proxy: false,
            is_enabled: true,
            is_default: true,
            created_at: 1,
            updated_at: 1,
            ..NewUpstreamSource::test_defaults(UpstreamProfileType::Openai)
        }),
    )
    .await
    .expect("test provider should create")
    .provider
}

async fn create_test_provider_secret(
    database: &DatabaseRuntime,
    id: i64,
    provider_id: i64,
    service: &SecretEncryptionService,
    value: &str,
) {
    let plaintext = SensitiveSecret::new(value.to_string());
    ProviderApiKeyRepository::insert(
        database,
        &NewProviderApiKey {
            id,
            provider_id,
            description: Some("startup rotation".to_string()),
            key_prefix: value.chars().take(4).collect(),
            key_last4: value
                .chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
            encrypted_secret: service
                .encrypt_current(SecretDomain::ProviderApiKey(id), &plaintext)
                .expect("provider secret should encrypt"),
            secret_hmac: service
                .provider_secret_fingerprint(provider_id, &plaintext)
                .expect("provider secret should fingerprint"),
            is_enabled: true,
            created_at: id,
            updated_at: id,
        },
    )
    .await
    .expect("provider secret should insert");
}

async fn load_provider_secret(
    database: &DatabaseRuntime,
    provider_id: i64,
    id: i64,
) -> StoredProviderApiKey {
    ProviderApiKeyRepository::get_stored_by_id(database, provider_id, id)
        .await
        .expect("stored provider secret should load")
}

#[tokio::test]
async fn startup_rotation_rotates_previous_and_preserves_unknown_or_corrupt_downstream_secrets() {
    let context = TestDatabase::new_sqlite_default("secret-startup-rotation.sqlite").await;
    let database = context.runtime();
    let current_service = service_for_key(KEY_A);
    let previous_service = service_for_key(KEY_B);
    let unknown_service = service_for_key(KEY_C);
    let previous_id = create_test_api_key(&database, "previous").await;
    let current_id = create_test_api_key(&database, "current").await;
    let unknown_id = create_test_api_key(&database, "unknown").await;
    let corrupt_id = create_test_api_key(&database, "corrupt").await;
    store_encrypted_secret(
        &database,
        previous_id,
        &encrypt_for(previous_id, &previous_service, "previous-secret"),
    )
    .await;
    store_encrypted_secret(
        &database,
        current_id,
        &encrypt_for(current_id, &current_service, "current-secret"),
    )
    .await;
    let unknown_before = encrypt_for(unknown_id, &unknown_service, "unknown-secret");
    store_encrypted_secret(&database, unknown_id, &unknown_before).await;
    let mut corrupt = encrypt_for(corrupt_id, &previous_service, "corrupt-secret");
    corrupt.ciphertext[0] ^= 1;
    store_encrypted_secret(&database, corrupt_id, &corrupt).await;

    let config = rotation_config(DownstreamSecretMode::Recoverable);
    let startup_config = config.clone();
    let summary = prepare_with_context(&context, &startup_config)
        .expect("downstream startup rotation should continue past unavailable rows");
    assert_eq!(
        summary.downstream,
        SecretRotationSummary {
            current: 1,
            rotated: 1,
            unavailable_preserved: 2,
        }
    );
    assert_eq!(summary.downstream.total(), 4);
    assert_eq!(summary.provider.total(), 0);
    assert!(!config.has_previous_encryption_key());

    let rotated = load_encrypted_secret(&database, previous_id).await;
    let plaintext = current_service
        .decrypt_current(SecretDomain::DownstreamApiKey(previous_id), &rotated)
        .expect("rotated secret should decrypt with current key");
    assert_eq!(plaintext.expose(), "previous-secret");
    assert_eq!(
        load_encrypted_secret(&database, unknown_id).await,
        unknown_before
    );
    assert_eq!(load_encrypted_secret(&database, corrupt_id).await, corrupt);

    let second = prepare_with_context(&context, &config)
        .expect("rotation should be idempotent after previous key is consumed");
    assert_eq!(second.downstream.current, 2);
    assert_eq!(second.downstream.rotated, 0);
    assert_eq!(second.downstream.unavailable_preserved, 2);
}

#[tokio::test]
async fn startup_rotation_runs_in_one_time_mode() {
    let context = TestDatabase::new_sqlite_default("secret-startup-one-time.sqlite").await;
    let database = context.runtime();
    let previous_service = service_for_key(KEY_B);
    let current_service = service_for_key(KEY_A);
    let id = create_test_api_key(&database, "one-time-previous").await;
    store_encrypted_secret(
        &database,
        id,
        &encrypt_for(id, &previous_service, "preserved-for-future"),
    )
    .await;

    let config = rotation_config(DownstreamSecretMode::OneTime);
    let summary = prepare_with_context(&context, &config)
        .expect("one-time mode should still rotate historical ciphertext");
    assert_eq!(summary.downstream.rotated, 1);
    let plaintext = current_service
        .decrypt_current(
            SecretDomain::DownstreamApiKey(id),
            &load_encrypted_secret(&database, id).await,
        )
        .expect("historical secret should use current key");
    assert_eq!(plaintext.expose(), "preserved-for-future");
}

#[tokio::test]
async fn startup_manager_totp_authenticates_current_and_rotates_previous_secret() {
    let context = TestDatabase::new_sqlite_default("manager-totp-startup-rotation.sqlite").await;
    let database = context.runtime();
    create_manager_credential(&database).await;
    let current_service = service_for_key(KEY_A);
    let previous_service = service_for_key(KEY_B);
    let plaintext = SensitiveSecret::new("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_string());
    let previous = previous_service
        .encrypt_current(SecretDomain::ManagerTotp(MANAGER_ID), &plaintext)
        .expect("previous manager TOTP secret should encrypt");
    store_manager_totp_secret(&database, &previous, 123, 456).await;

    let config = rotation_config(DownstreamSecretMode::OneTime);
    let summary = prepare_with_context(&context, &config)
        .expect("previous manager TOTP should rotate without blocking startup");
    assert_eq!(
        summary.manager_totp.state,
        ManagerTotpPreparationState::Rotated
    );
    assert_eq!(summary.manager_totp.failure, None);
    assert_eq!(
        summary.manager_totp.key_fingerprint_short_str(),
        current_service
            .current_fingerprint()
            .map(KeyFingerprint::short)
    );

    let rotated = load_manager_totp_secret(&database).await;
    assert_eq!(
        current_service
            .decrypt_current(SecretDomain::ManagerTotp(MANAGER_ID), &rotated)
            .expect("rotated manager TOTP should use current key")
            .expose(),
        plaintext.expose()
    );
    let credential = ManagerCredential::load(&database).await.unwrap().unwrap();
    assert_eq!(credential.totp_last_accepted_step, Some(123));
    assert_eq!(credential.totp_enabled_at, Some(456));
    assert_eq!(
        credential.credential_epoch,
        "018fa7d8-6a00-7c9a-8f7e-999999999999"
    );

    let second = prepare_with_context(&context, &config)
        .expect("current manager TOTP should be authenticated");
    assert_eq!(
        second.manager_totp.state,
        ManagerTotpPreparationState::Current
    );
    assert_eq!(second.manager_totp.failure, None);
}

#[tokio::test]
async fn corrupt_manager_totp_is_preserved_while_other_secret_domains_prepare() {
    let context = TestDatabase::new_sqlite_default("manager-totp-startup-degraded.sqlite").await;
    let database = context.runtime();
    create_manager_credential(&database).await;
    let current_service = service_for_key(KEY_A);
    let previous_service = service_for_key(KEY_B);
    let plaintext = SensitiveSecret::new("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_string());
    let mut corrupt = current_service
        .encrypt_current(SecretDomain::ManagerTotp(MANAGER_ID), &plaintext)
        .expect("current manager TOTP should encrypt");
    corrupt.ciphertext[0] ^= 1;
    store_manager_totp_secret(&database, &corrupt, 100, 200).await;
    let corrupt_before = load_manager_totp_secret(&database).await;

    let downstream_id = create_test_api_key(&database, "manager-degraded-downstream").await;
    store_encrypted_secret(
        &database,
        downstream_id,
        &encrypt_for(
            downstream_id,
            &previous_service,
            "rotatable-downstream-secret",
        ),
    )
    .await;
    let provider = create_test_provider(&database, 9_001).await;
    create_test_provider_secret(
        &database,
        9_101,
        provider.id,
        &current_service,
        "valid-provider-secret",
    )
    .await;

    let config = rotation_config(DownstreamSecretMode::Recoverable);
    let summary = prepare_with_context(&context, &config)
        .expect("corrupt manager TOTP must not block other secret domains");
    assert_eq!(summary.downstream.rotated, 1);
    assert_eq!(summary.provider.current, 1);
    assert_eq!(
        summary.manager_totp.state,
        ManagerTotpPreparationState::UnavailablePreserved
    );
    assert_eq!(
        summary.manager_totp.failure,
        Some(ManagerTotpPreparationFailure::DecryptFailed)
    );
    assert_eq!(load_manager_totp_secret(&database).await, corrupt_before);
    assert!(!config.has_previous_encryption_key());
}

#[tokio::test]
async fn unknown_key_and_invalid_plaintext_manager_totp_are_preserved() {
    let unknown_context =
        TestDatabase::new_sqlite_default("manager-totp-startup-unknown.sqlite").await;
    let unknown_database = unknown_context.runtime();
    create_manager_credential(&unknown_database).await;
    let unknown_service = service_for_key(KEY_C);
    let plaintext = SensitiveSecret::new("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_string());
    let unknown = unknown_service
        .encrypt_current(SecretDomain::ManagerTotp(MANAGER_ID), &plaintext)
        .expect("unknown manager TOTP should encrypt");
    store_manager_totp_secret(&unknown_database, &unknown, 10, 20).await;
    let before = load_manager_totp_secret(&unknown_database).await;

    let summary = prepare_with_context(
        &unknown_context,
        &rotation_config(DownstreamSecretMode::OneTime),
    )
    .expect("unknown manager TOTP key should degrade without failing startup");
    assert_eq!(
        summary.manager_totp.failure,
        Some(ManagerTotpPreparationFailure::UnknownKey)
    );
    assert_eq!(load_manager_totp_secret(&unknown_database).await, before);

    let invalid_context =
        TestDatabase::new_sqlite_default("manager-totp-startup-invalid.sqlite").await;
    let invalid_database = invalid_context.runtime();
    create_manager_credential(&invalid_database).await;
    let previous_service = service_for_key(KEY_B);
    let invalid_plaintext = SensitiveSecret::new("NOT-VALID-BASE32".to_string());
    let invalid = previous_service
        .encrypt_current(SecretDomain::ManagerTotp(MANAGER_ID), &invalid_plaintext)
        .expect("invalid plaintext fixture should encrypt");
    store_manager_totp_secret(&invalid_database, &invalid, 10, 20).await;
    let before = load_manager_totp_secret(&invalid_database).await;

    let summary = prepare_with_context(
        &invalid_context,
        &rotation_config(DownstreamSecretMode::OneTime),
    )
    .expect("invalid manager TOTP plaintext should not fail startup");
    assert_eq!(
        summary.manager_totp.failure,
        Some(ManagerTotpPreparationFailure::InvalidSecret)
    );
    assert_eq!(load_manager_totp_secret(&invalid_database).await, before);
}

#[tokio::test]
async fn startup_preparation_authenticates_current_and_rotates_previous_provider_secrets() {
    let context = TestDatabase::new_sqlite_default("provider-secret-startup-success.sqlite").await;
    let database = context.runtime();
    let provider = create_test_provider(&database, 1_001).await;
    let current_service = service_for_key(KEY_A);
    let previous_service = service_for_key(KEY_B);
    create_test_provider_secret(
        &database,
        1_101,
        provider.id,
        &current_service,
        "current-provider-secret",
    )
    .await;
    create_test_provider_secret(
        &database,
        1_102,
        provider.id,
        &previous_service,
        "previous-provider-secret",
    )
    .await;
    let previous_before = load_provider_secret(&database, provider.id, 1_102)
        .await
        .secret_nonce
        .expect("previous nonce should exist");

    let config = rotation_config(DownstreamSecretMode::OneTime);
    let summary =
        prepare_with_context(&context, &config).expect("valid provider secrets should prepare");
    assert_eq!(summary.provider.current, 1);
    assert_eq!(summary.provider.rotated, 1);
    assert_eq!(summary.provider.unavailable_preserved, 0);
    assert!(!config.has_previous_encryption_key());

    let rotated = load_provider_secret(&database, provider.id, 1_102).await;
    assert_ne!(
        rotated.secret_nonce.as_deref(),
        Some(previous_before.as_slice())
    );
    let plaintext = current_service
        .decrypt_current(
            SecretDomain::ProviderApiKey(1_102),
            &rotated
                .encrypted_secret()
                .expect("rotated provider tuple should be valid"),
        )
        .expect("rotated provider secret should use current key");
    assert_eq!(plaintext.expose(), "previous-provider-secret");
    assert_eq!(
        rotated.secret_hmac.as_deref(),
        Some(
            current_service
                .provider_secret_fingerprint(provider.id, &plaintext)
                .expect("rotated HMAC should compute")
                .as_str()
        )
    );
}

#[tokio::test]
async fn provider_failure_rolls_back_downstream_rotation_and_preserves_previous_key_for_retry() {
    let context = TestDatabase::new_sqlite_default("provider-secret-startup-rollback.sqlite").await;
    let database = context.runtime();
    let previous_service = service_for_key(KEY_B);
    let current_service = service_for_key(KEY_A);
    let downstream_id = create_test_api_key(&database, "rollback-with-provider").await;
    let downstream_before = encrypt_for(downstream_id, &previous_service, "downstream-value");
    store_encrypted_secret(&database, downstream_id, &downstream_before).await;
    let provider = create_test_provider(&database, 2_001).await;
    create_test_provider_secret(
        &database,
        2_101,
        provider.id,
        &current_service,
        "provider-value",
    )
    .await;
    ProviderApiKeyRepository::set_secret_hmac_for_test(
        &database,
        provider.id,
        2_101,
        "f".repeat(64),
    )
    .await
    .expect("provider HMAC should tamper");

    let config = rotation_config(DownstreamSecretMode::Recoverable);
    let error = prepare_with_context(&context, &config)
        .expect_err("provider HMAC mismatch must reject startup");
    assert!(format!("{error:?}").contains("reason=hmac_mismatch"));
    assert_eq!(
        load_encrypted_secret(&database, downstream_id).await,
        downstream_before
    );
    assert!(config.has_previous_encryption_key());

    let provider_plaintext = SensitiveSecret::new("provider-value".to_string());
    let correct_hmac = current_service
        .provider_secret_fingerprint(provider.id, &provider_plaintext)
        .expect("correct HMAC should compute");
    ProviderApiKeyRepository::set_secret_hmac_for_test(
        &database,
        provider.id,
        2_101,
        correct_hmac.as_str().to_string(),
    )
    .await
    .expect("provider HMAC should repair");

    let retry = prepare_with_context(&context, &config)
        .expect("same config should retry after provider repair");
    assert_eq!(retry.downstream.rotated, 1);
    assert_eq!(retry.provider.current, 1);
    assert!(!config.has_previous_encryption_key());
}

async fn assert_sqlite_provider_secret_failure(
    file_name: &str,
    stored_key: &str,
    mutation_sql: &str,
    expected_reason: &str,
) {
    let context = TestDatabase::new_sqlite_default(file_name).await;
    let database = context.runtime();
    let provider = create_test_provider(&database, 3_001).await;
    let stored_service = service_for_key(stored_key);
    create_test_provider_secret(
        &database,
        3_101,
        provider.id,
        &stored_service,
        "strict-provider-secret",
    )
    .await;
    if !mutation_sql.is_empty() {
        context
            .execute_sqlite_batch(mutation_sql)
            .await
            .expect("provider corruption fixture should apply");
    }
    let config: SecretEncryptionConfig = serde_yaml::from_str(&format!(
        "downstream_mode: one_time\nencryption_key: '{KEY_A}'\n"
    ))
    .expect("current-only config should parse");
    let error = prepare_with_context(&context, &config)
        .expect_err("invalid provider secret must reject startup");
    let message = format!("{error:?}");
    assert!(
        message.contains(expected_reason),
        "unexpected error: {message}"
    );
    assert!(!message.contains("strict-provider-secret"));
    assert!(!message.contains(&"f".repeat(64)));
}

#[tokio::test]
async fn startup_preparation_rejects_provider_corruption_with_safe_reason_codes() {
    assert_sqlite_provider_secret_failure(
        "provider-secret-corrupt-ciphertext.sqlite",
        KEY_A,
        "UPDATE provider_api_key SET secret_ciphertext = X'00' WHERE id = 3101;",
        "reason=decrypt_failed",
    )
    .await;
    assert_sqlite_provider_secret_failure(
        "provider-secret-wrong-hmac.sqlite",
        KEY_A,
        &format!(
            "UPDATE provider_api_key SET secret_hmac = '{}' WHERE id = 3101;",
            "f".repeat(64)
        ),
        "reason=hmac_mismatch",
    )
    .await;
    assert_sqlite_provider_secret_failure(
        "provider-secret-incomplete.sqlite",
        KEY_A,
        "PRAGMA ignore_check_constraints = ON;
         UPDATE provider_api_key SET secret_nonce = NULL WHERE id = 3101;
         PRAGMA ignore_check_constraints = OFF;",
        "reason=incomplete_fields",
    )
    .await;
    assert_sqlite_provider_secret_failure(
        "provider-secret-unknown-key.sqlite",
        KEY_C,
        "",
        "reason=unknown_key",
    )
    .await;
}

#[tokio::test]
async fn startup_rotation_database_error_rolls_back_all_prior_updates() {
    let context = TestDatabase::new_sqlite_default("secret-startup-rollback.sqlite").await;
    let database = context.runtime();
    let previous_service = service_for_key(KEY_B);
    let first_id = create_test_api_key(&database, "rollback-first").await;
    let second_id = create_test_api_key(&database, "rollback-second").await;
    let first_before = encrypt_for(first_id, &previous_service, "first");
    let second_before = encrypt_for(second_id, &previous_service, "second");
    store_encrypted_secret(&database, first_id, &first_before).await;
    store_encrypted_secret(&database, second_id, &second_before).await;

    context
        .execute_sqlite_batch(format!(
            "CREATE TRIGGER reject_second_secret_rotation
                     BEFORE UPDATE OF secret_ciphertext ON api_key
                     WHEN OLD.id = {second_id}
                     BEGIN
                       SELECT RAISE(ABORT, 'forced rotation failure');
                     END;"
        ))
        .await
        .expect("failure trigger should create");

    let config = rotation_config(DownstreamSecretMode::Recoverable);
    assert!(
        prepare_with_context(&context, &config).is_err(),
        "database failure should fail startup rotation"
    );
    assert_eq!(
        load_encrypted_secret(&database, first_id).await,
        first_before
    );
    assert_eq!(
        load_encrypted_secret(&database, second_id).await,
        second_before
    );

    context
        .execute_sqlite_batch("DROP TRIGGER reject_second_secret_rotation;")
        .await
        .expect("failure trigger should drop");

    let retry_config = rotation_config(DownstreamSecretMode::Recoverable);
    let retry_summary = prepare_with_context(&context, &retry_config)
        .expect("a fresh startup should retry the rolled-back rotation");
    assert_eq!(retry_summary.downstream.rotated, 2);
    let current_service = service_for_key(KEY_A);
    assert_eq!(
        current_service
            .decrypt_current(
                SecretDomain::DownstreamApiKey(first_id),
                &load_encrypted_secret(&database, first_id).await,
            )
            .expect("retried first secret should use current key")
            .expose(),
        "first"
    );
    assert_eq!(
        current_service
            .decrypt_current(
                SecretDomain::DownstreamApiKey(second_id),
                &load_encrypted_secret(&database, second_id).await,
            )
            .expect("retried second secret should use current key")
            .expose(),
        "second"
    );
}

#[test]
fn main_prepares_state_and_rotates_secrets_before_binding_or_serving() {
    let main_source = include_str!("../../main.rs");
    let startup_connection = main_source
        .find("let mut startup_database = StartupDatabaseConnection::establish")
        .expect("main must establish one explicit startup database connection");
    let rotation = main_source
        .find("prepare_secrets_before_startup(&CONFIG.secret_encryption, &mut startup_database)")
        .expect("main must run startup secret rotation");
    let startup_drop = main_source
        .find("drop(startup_database)")
        .expect("main must destroy the startup connection before app state");
    let runtime = main_source
        .find("DatabaseRuntime::connect(")
        .expect("main must establish the production database runtime explicitly");
    let readiness = main_source
        .find(".readiness_probe()")
        .expect("main must verify the initial runtime connection");
    let app_state = main_source
        .find("create_app_state(database).await")
        .expect("main must inject the verified runtime into app state");
    let bind = main_source
        .find("TcpListener::bind")
        .expect("main must bind a listener");
    let serve = main_source
        .find("axum::serve")
        .expect("main must serve axum");

    assert!(startup_connection < rotation);
    assert!(rotation < startup_drop);
    assert!(startup_drop < runtime);
    assert!(runtime < readiness);
    assert!(readiness < app_state);
    assert!(app_state < bind);
    assert!(bind < serve);
}

#[test]
fn main_drains_persistence_after_http_shutdown_without_fixed_sleep() {
    let main_source = include_str!("../../main.rs");
    let graceful_shutdown = main_source
        .find(".with_graceful_shutdown(shutdown_signal())")
        .expect("main must await HTTP graceful shutdown");
    let persistence_drain = main_source
        .find("shutdown_app_state.shutdown_persistence().await")
        .expect("main must drain persistence after HTTP handlers");
    let shutdown_complete = main_source
        .find("\"startup.server_shutdown_complete\"")
        .expect("main must emit shutdown complete after drain");

    assert!(graceful_shutdown < persistence_drain);
    assert!(persistence_drain < shutdown_complete);
    assert!(
        !main_source.contains("sleep(std::time::Duration::from_secs(1))"),
        "shutdown must not rely on a fixed sleep"
    );
}

#[tokio::test]
#[ignore = "requires a dedicated PostgreSQL 17 database"]
async fn postgres_startup_rotation_reencrypts_previous_and_preserves_unknown() {
    let database_url = env::var(POSTGRES_SMOKE_URL_ENV).unwrap_or_else(|_| {
        panic!("{POSTGRES_SMOKE_URL_ENV} must point to the dedicated PostgreSQL smoke database")
    });
    TestDatabase::reset_dedicated_postgres_schema(&database_url, POSTGRES_SMOKE_DATABASE);

    let context = TestDatabase::new_postgres(&database_url).await;
    let database = context.runtime();
    let previous_service = service_for_key(KEY_B);
    let unknown_service = service_for_key(KEY_C);
    let current_service = service_for_key(KEY_A);
    create_manager_credential(&database).await;
    let manager_totp_plaintext =
        SensitiveSecret::new("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_string());
    let previous_manager_totp = previous_service
        .encrypt_current(
            SecretDomain::ManagerTotp(MANAGER_ID),
            &manager_totp_plaintext,
        )
        .expect("postgres previous manager TOTP should encrypt");
    store_manager_totp_secret(&database, &previous_manager_totp, 123, 456).await;
    let previous_id = create_test_api_key(&database, "postgres-previous").await;
    let unknown_id = create_test_api_key(&database, "postgres-unknown").await;
    let previous = encrypt_for(previous_id, &previous_service, "postgres-secret");
    let unknown = encrypt_for(unknown_id, &unknown_service, "postgres-unknown");
    store_encrypted_secret(&database, previous_id, &previous).await;
    store_encrypted_secret(&database, unknown_id, &unknown).await;
    let provider = create_test_provider(&database, 4_001).await;
    create_test_provider_secret(
        &database,
        4_101,
        provider.id,
        &previous_service,
        "postgres-provider-secret",
    )
    .await;

    let config = rotation_config(DownstreamSecretMode::Recoverable);
    let summary =
        prepare_with_context(&context, &config).expect("postgres startup rotation should succeed");
    assert_eq!(summary.downstream.rotated, 1);
    assert_eq!(summary.downstream.unavailable_preserved, 1);
    assert_eq!(summary.provider.rotated, 1);
    assert_eq!(
        summary.manager_totp.state,
        ManagerTotpPreparationState::Rotated
    );
    let plaintext = current_service
        .decrypt_current(
            SecretDomain::DownstreamApiKey(previous_id),
            &load_encrypted_secret(&database, previous_id).await,
        )
        .expect("postgres rotated secret should use current key");
    assert_eq!(plaintext.expose(), "postgres-secret");
    assert_eq!(load_encrypted_secret(&database, unknown_id).await, unknown);
    let provider_plaintext = current_service
        .decrypt_current(
            SecretDomain::ProviderApiKey(4_101),
            &load_provider_secret(&database, provider.id, 4_101)
                .await
                .encrypted_secret()
                .expect("postgres provider tuple should load"),
        )
        .expect("postgres provider secret should rotate");
    assert_eq!(provider_plaintext.expose(), "postgres-provider-secret");

    assert_eq!(
        current_service
            .decrypt_current(
                SecretDomain::ManagerTotp(MANAGER_ID),
                &load_manager_totp_secret(&database).await,
            )
            .expect("postgres manager TOTP should rotate to current")
            .expose(),
        manager_totp_plaintext.expose()
    );
    let current_summary = prepare_with_context(&context, &config)
        .expect("postgres current manager TOTP should authenticate");
    assert_eq!(
        current_summary.manager_totp.state,
        ManagerTotpPreparationState::Current
    );

    let unknown_manager_totp = unknown_service
        .encrypt_current(
            SecretDomain::ManagerTotp(MANAGER_ID),
            &manager_totp_plaintext,
        )
        .expect("postgres unknown manager TOTP should encrypt");
    store_manager_totp_secret(&database, &unknown_manager_totp, 124, 457).await;
    let unknown_before = load_manager_totp_secret(&database).await;
    let unknown_summary = prepare_with_context(&context, &config)
        .expect("postgres unknown manager TOTP must not block startup");
    assert_eq!(
        unknown_summary.manager_totp.state,
        ManagerTotpPreparationState::UnavailablePreserved
    );
    assert_eq!(
        unknown_summary.manager_totp.failure,
        Some(ManagerTotpPreparationFailure::UnknownKey)
    );
    assert_eq!(load_manager_totp_secret(&database).await, unknown_before);
    drop(context);

    TestDatabase::reset_dedicated_postgres_schema(&database_url, POSTGRES_SMOKE_DATABASE);
}
