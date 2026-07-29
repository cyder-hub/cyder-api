use super::*;

use crate::database::api_key::{_postgres_model, _sqlite_model, ApiKey, CreateApiKeyPayload};
use crate::database::manager_credential::{MANAGER_ID, ManagerCredential, NewManagerCredential};
use crate::database::provider::{
    NewProvider, NewProviderApiKey, Provider, ProviderApiKeyRepository, StoredProviderApiKey,
};
use crate::database::{DbConnection, TestDbContext, get_connection};
use crate::db_execute;
use crate::schema::enum_def::{Action, ProviderApiKeyMode, ProviderType};
use diesel::connection::SimpleConnection;
use diesel::{Connection, PgConnection, QueryableByName, RunQueryDsl, sql_types::Text};
use std::env;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

const KEY_A: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
const KEY_B: &str = "101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f";
const KEY_C: &str = "202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f";
const POSTGRES_SMOKE_URL_ENV: &str = "CYDER_R1_POSTGRES_MIGRATION_SMOKE_URL";
const POSTGRES_SMOKE_DATABASE: &str = "cyder_r1_migration_smoke";

#[derive(QueryableByName)]
struct DatabaseNameRow {
    #[diesel(sql_type = Text)]
    name: String,
}

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

fn create_test_api_key(name: &str) -> i64 {
    ApiKey::create(&CreateApiKeyPayload {
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
    })
    .expect("test api key should create")
    .detail
    .id
}

fn store_encrypted_secret(id: i64, encrypted: &EncryptedSecret) {
    let conn = &mut get_connection().expect("test connection should load");
    db_execute!(conn, {
        let updated = diesel::update(api_key::table.find(id))
            .set((
                api_key::dsl::secret_ciphertext.eq(Some(encrypted.ciphertext().to_vec())),
                api_key::dsl::secret_nonce.eq(Some(encrypted.nonce().to_vec())),
                api_key::dsl::secret_format_version.eq(Some(encrypted.format_version())),
                api_key::dsl::secret_key_fingerprint
                    .eq(Some(encrypted.key_fingerprint().as_str().to_string())),
            ))
            .execute(conn)
            .expect("test encrypted secret should persist");
        assert_eq!(updated, 1);
    });
}

fn load_encrypted_secret(id: i64) -> EncryptedSecret {
    let conn = &mut get_connection().expect("test connection should load");
    let row = db_execute!(conn, {
        api_key::table
            .find(id)
            .select((
                api_key::dsl::secret_ciphertext,
                api_key::dsl::secret_nonce,
                api_key::dsl::secret_format_version,
                api_key::dsl::secret_key_fingerprint,
            ))
            .first::<(
                Option<Vec<u8>>,
                Option<Vec<u8>>,
                Option<i32>,
                Option<String>,
            )>(conn)
            .expect("stored secret should load")
    });
    EncryptedSecret::from_parts(
        row.0.expect("ciphertext"),
        row.1.expect("nonce"),
        row.2.expect("version"),
        row.3.expect("fingerprint"),
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

fn create_manager_credential() {
    ManagerCredential::insert_once(NewManagerCredential {
        password_verifier: "$argon2id$v=19$m=65536,t=3,p=4$fixture$fixture".to_string(),
        credential_epoch: "018fa7d8-6a00-7c9a-8f7e-999999999999".to_string(),
        now: 1,
    })
    .expect("manager credential should insert");
}

fn store_manager_totp_secret(
    encrypted: &EncryptedSecret,
    last_accepted_step: i64,
    enabled_at: i64,
) {
    let conn = &mut get_connection().expect("test connection should load");
    db_execute!(conn, {
        let updated = diesel::update(
            manager_credential::table.filter(manager_credential::dsl::manager_id.eq(MANAGER_ID)),
        )
        .set((
            manager_credential::dsl::totp_secret_ciphertext
                .eq(Some(encrypted.ciphertext().to_vec())),
            manager_credential::dsl::totp_secret_nonce.eq(Some(encrypted.nonce().to_vec())),
            manager_credential::dsl::totp_secret_format_version
                .eq(Some(encrypted.format_version())),
            manager_credential::dsl::totp_secret_key_fingerprint
                .eq(Some(encrypted.key_fingerprint().as_str().to_string())),
            manager_credential::dsl::totp_last_accepted_step.eq(Some(last_accepted_step)),
            manager_credential::dsl::totp_enabled_at.eq(Some(enabled_at)),
        ))
        .execute(conn)
        .expect("manager TOTP fixture should persist");
        assert_eq!(updated, 1);
    });
}

fn load_manager_totp_secret() -> EncryptedSecret {
    let credential = ManagerCredential::load()
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

fn create_test_provider(id: i64) -> Provider {
    Provider::create(&NewProvider {
        id,
        provider_key: format!("secret-startup-provider-{id}"),
        name: format!("Secret Startup Provider {id}"),
        endpoint: "https://api.example.com/v1".to_string(),
        use_proxy: false,
        is_enabled: true,
        created_at: 1,
        updated_at: 1,
        provider_type: ProviderType::Openai,
        provider_api_key_mode: ProviderApiKeyMode::Queue,
    })
    .expect("test provider should create")
}

fn create_test_provider_secret(
    id: i64,
    provider_id: i64,
    service: &SecretEncryptionService,
    value: &str,
) {
    let plaintext = SensitiveSecret::new(value.to_string());
    ProviderApiKeyRepository::insert(&NewProviderApiKey {
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
    })
    .expect("provider secret should insert");
}

fn load_provider_secret(provider_id: i64, id: i64) -> StoredProviderApiKey {
    ProviderApiKeyRepository::get_stored_by_id(provider_id, id)
        .expect("stored provider secret should load")
}

#[test]
fn startup_rotation_rotates_previous_and_preserves_unknown_or_corrupt_downstream_secrets() {
    let context = TestDbContext::new_sqlite("secret-startup-rotation.sqlite");
    context.run_sync(|| {
        let current_service = service_for_key(KEY_A);
        let previous_service = service_for_key(KEY_B);
        let unknown_service = service_for_key(KEY_C);
        let previous_id = create_test_api_key("previous");
        let current_id = create_test_api_key("current");
        let unknown_id = create_test_api_key("unknown");
        let corrupt_id = create_test_api_key("corrupt");
        store_encrypted_secret(
            previous_id,
            &encrypt_for(previous_id, &previous_service, "previous-secret"),
        );
        store_encrypted_secret(
            current_id,
            &encrypt_for(current_id, &current_service, "current-secret"),
        );
        let unknown_before = encrypt_for(unknown_id, &unknown_service, "unknown-secret");
        store_encrypted_secret(unknown_id, &unknown_before);
        let mut corrupt = encrypt_for(corrupt_id, &previous_service, "corrupt-secret");
        corrupt.ciphertext[0] ^= 1;
        store_encrypted_secret(corrupt_id, &corrupt);

        let config = rotation_config(DownstreamSecretMode::Recoverable);
        let startup_config = config.clone();
        let summary = prepare_secrets_before_startup(&startup_config)
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

        let rotated = load_encrypted_secret(previous_id);
        let plaintext = current_service
            .decrypt_current(SecretDomain::DownstreamApiKey(previous_id), &rotated)
            .expect("rotated secret should decrypt with current key");
        assert_eq!(plaintext.expose(), "previous-secret");
        assert_eq!(load_encrypted_secret(unknown_id), unknown_before);
        assert_eq!(load_encrypted_secret(corrupt_id), corrupt);

        let second = prepare_secrets_before_startup(&config)
            .expect("rotation should be idempotent after previous key is consumed");
        assert_eq!(second.downstream.current, 2);
        assert_eq!(second.downstream.rotated, 0);
        assert_eq!(second.downstream.unavailable_preserved, 2);
    });
}

#[test]
fn startup_rotation_runs_in_one_time_mode() {
    let context = TestDbContext::new_sqlite("secret-startup-one-time.sqlite");
    context.run_sync(|| {
        let previous_service = service_for_key(KEY_B);
        let current_service = service_for_key(KEY_A);
        let id = create_test_api_key("one-time-previous");
        store_encrypted_secret(
            id,
            &encrypt_for(id, &previous_service, "preserved-for-future"),
        );

        let config = rotation_config(DownstreamSecretMode::OneTime);
        let summary = prepare_secrets_before_startup(&config)
            .expect("one-time mode should still rotate historical ciphertext");
        assert_eq!(summary.downstream.rotated, 1);
        let plaintext = current_service
            .decrypt_current(
                SecretDomain::DownstreamApiKey(id),
                &load_encrypted_secret(id),
            )
            .expect("historical secret should use current key");
        assert_eq!(plaintext.expose(), "preserved-for-future");
    });
}

#[test]
fn startup_manager_totp_authenticates_current_and_rotates_previous_secret() {
    let context = TestDbContext::new_sqlite("manager-totp-startup-rotation.sqlite");
    context.run_sync(|| {
        create_manager_credential();
        let current_service = service_for_key(KEY_A);
        let previous_service = service_for_key(KEY_B);
        let plaintext = SensitiveSecret::new("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_string());
        let previous = previous_service
            .encrypt_current(SecretDomain::ManagerTotp(MANAGER_ID), &plaintext)
            .expect("previous manager TOTP secret should encrypt");
        store_manager_totp_secret(&previous, 123, 456);

        let config = rotation_config(DownstreamSecretMode::OneTime);
        let summary = prepare_secrets_before_startup(&config)
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

        let rotated = load_manager_totp_secret();
        assert_eq!(
            current_service
                .decrypt_current(SecretDomain::ManagerTotp(MANAGER_ID), &rotated)
                .expect("rotated manager TOTP should use current key")
                .expose(),
            plaintext.expose()
        );
        let credential = ManagerCredential::load().unwrap().unwrap();
        assert_eq!(credential.totp_last_accepted_step, Some(123));
        assert_eq!(credential.totp_enabled_at, Some(456));
        assert_eq!(
            credential.credential_epoch,
            "018fa7d8-6a00-7c9a-8f7e-999999999999"
        );

        let second = prepare_secrets_before_startup(&config)
            .expect("current manager TOTP should be authenticated");
        assert_eq!(
            second.manager_totp.state,
            ManagerTotpPreparationState::Current
        );
        assert_eq!(second.manager_totp.failure, None);
    });
}

#[test]
fn corrupt_manager_totp_is_preserved_while_other_secret_domains_prepare() {
    let context = TestDbContext::new_sqlite("manager-totp-startup-degraded.sqlite");
    context.run_sync(|| {
        create_manager_credential();
        let current_service = service_for_key(KEY_A);
        let previous_service = service_for_key(KEY_B);
        let plaintext = SensitiveSecret::new("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_string());
        let mut corrupt = current_service
            .encrypt_current(SecretDomain::ManagerTotp(MANAGER_ID), &plaintext)
            .expect("current manager TOTP should encrypt");
        corrupt.ciphertext[0] ^= 1;
        store_manager_totp_secret(&corrupt, 100, 200);
        let corrupt_before = load_manager_totp_secret();

        let downstream_id = create_test_api_key("manager-degraded-downstream");
        store_encrypted_secret(
            downstream_id,
            &encrypt_for(
                downstream_id,
                &previous_service,
                "rotatable-downstream-secret",
            ),
        );
        let provider = create_test_provider(9_001);
        create_test_provider_secret(
            9_101,
            provider.id,
            &current_service,
            "valid-provider-secret",
        );

        let config = rotation_config(DownstreamSecretMode::Recoverable);
        let summary = prepare_secrets_before_startup(&config)
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
        assert_eq!(load_manager_totp_secret(), corrupt_before);
        assert!(!config.has_previous_encryption_key());
    });
}

#[test]
fn unknown_key_and_invalid_plaintext_manager_totp_are_preserved() {
    let unknown_context = TestDbContext::new_sqlite("manager-totp-startup-unknown.sqlite");
    unknown_context.run_sync(|| {
        create_manager_credential();
        let unknown_service = service_for_key(KEY_C);
        let plaintext = SensitiveSecret::new("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_string());
        let unknown = unknown_service
            .encrypt_current(SecretDomain::ManagerTotp(MANAGER_ID), &plaintext)
            .expect("unknown manager TOTP should encrypt");
        store_manager_totp_secret(&unknown, 10, 20);
        let before = load_manager_totp_secret();

        let summary =
            prepare_secrets_before_startup(&rotation_config(DownstreamSecretMode::OneTime))
                .expect("unknown manager TOTP key should degrade without failing startup");
        assert_eq!(
            summary.manager_totp.failure,
            Some(ManagerTotpPreparationFailure::UnknownKey)
        );
        assert_eq!(load_manager_totp_secret(), before);
    });

    let invalid_context = TestDbContext::new_sqlite("manager-totp-startup-invalid.sqlite");
    invalid_context.run_sync(|| {
        create_manager_credential();
        let previous_service = service_for_key(KEY_B);
        let invalid_plaintext = SensitiveSecret::new("NOT-VALID-BASE32".to_string());
        let invalid = previous_service
            .encrypt_current(SecretDomain::ManagerTotp(MANAGER_ID), &invalid_plaintext)
            .expect("invalid plaintext fixture should encrypt");
        store_manager_totp_secret(&invalid, 10, 20);
        let before = load_manager_totp_secret();

        let summary =
            prepare_secrets_before_startup(&rotation_config(DownstreamSecretMode::OneTime))
                .expect("invalid manager TOTP plaintext should not fail startup");
        assert_eq!(
            summary.manager_totp.failure,
            Some(ManagerTotpPreparationFailure::InvalidSecret)
        );
        assert_eq!(load_manager_totp_secret(), before);
    });
}

#[test]
fn startup_preparation_authenticates_current_and_rotates_previous_provider_secrets() {
    let context = TestDbContext::new_sqlite("provider-secret-startup-success.sqlite");
    context.run_sync(|| {
        let provider = create_test_provider(1_001);
        let current_service = service_for_key(KEY_A);
        let previous_service = service_for_key(KEY_B);
        create_test_provider_secret(
            1_101,
            provider.id,
            &current_service,
            "current-provider-secret",
        );
        create_test_provider_secret(
            1_102,
            provider.id,
            &previous_service,
            "previous-provider-secret",
        );
        let previous_before = load_provider_secret(provider.id, 1_102)
            .secret_nonce
            .expect("previous nonce should exist");

        let config = rotation_config(DownstreamSecretMode::OneTime);
        let summary =
            prepare_secrets_before_startup(&config).expect("valid provider secrets should prepare");
        assert_eq!(summary.provider.current, 1);
        assert_eq!(summary.provider.rotated, 1);
        assert_eq!(summary.provider.unavailable_preserved, 0);
        assert!(!config.has_previous_encryption_key());

        let rotated = load_provider_secret(provider.id, 1_102);
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
    });
}

#[test]
fn provider_failure_rolls_back_downstream_rotation_and_preserves_previous_key_for_retry() {
    let context = TestDbContext::new_sqlite("provider-secret-startup-rollback.sqlite");
    context.run_sync(|| {
        let previous_service = service_for_key(KEY_B);
        let current_service = service_for_key(KEY_A);
        let downstream_id = create_test_api_key("rollback-with-provider");
        let downstream_before = encrypt_for(downstream_id, &previous_service, "downstream-value");
        store_encrypted_secret(downstream_id, &downstream_before);
        let provider = create_test_provider(2_001);
        create_test_provider_secret(2_101, provider.id, &current_service, "provider-value");
        {
            let conn = &mut get_connection().expect("test connection should load");
            db_execute!(conn, {
                diesel::update(provider_api_key::table.find(2_101_i64))
                    .set(provider_api_key::dsl::secret_hmac.eq(Some("f".repeat(64))))
                    .execute(conn)
                    .expect("provider HMAC should tamper")
            });
        }

        let config = rotation_config(DownstreamSecretMode::Recoverable);
        let error = prepare_secrets_before_startup(&config)
            .expect_err("provider HMAC mismatch must reject startup");
        assert!(format!("{error:?}").contains("reason=hmac_mismatch"));
        assert_eq!(load_encrypted_secret(downstream_id), downstream_before);
        assert!(config.has_previous_encryption_key());

        let provider_plaintext = SensitiveSecret::new("provider-value".to_string());
        let correct_hmac = current_service
            .provider_secret_fingerprint(provider.id, &provider_plaintext)
            .expect("correct HMAC should compute");
        {
            let conn = &mut get_connection().expect("test connection should load");
            db_execute!(conn, {
                diesel::update(provider_api_key::table.find(2_101_i64))
                    .set(
                        provider_api_key::dsl::secret_hmac
                            .eq(Some(correct_hmac.as_str().to_string())),
                    )
                    .execute(conn)
                    .expect("provider HMAC should repair")
            });
        }

        let retry = prepare_secrets_before_startup(&config)
            .expect("same config should retry after provider repair");
        assert_eq!(retry.downstream.rotated, 1);
        assert_eq!(retry.provider.current, 1);
        assert!(!config.has_previous_encryption_key());
    });
}

fn assert_sqlite_provider_secret_failure(
    file_name: &str,
    stored_key: &str,
    mutation_sql: &str,
    expected_reason: &str,
) {
    let context = TestDbContext::new_sqlite(file_name);
    context.run_sync(|| {
        let provider = create_test_provider(3_001);
        let stored_service = service_for_key(stored_key);
        create_test_provider_secret(
            3_101,
            provider.id,
            &stored_service,
            "strict-provider-secret",
        );
        if !mutation_sql.is_empty() {
            let mut connection = get_connection().expect("test connection should load");
            match &mut connection {
                DbConnection::Sqlite(connection) => connection
                    .batch_execute(mutation_sql)
                    .expect("provider corruption fixture should apply"),
                DbConnection::Postgres(_) => unreachable!("test uses sqlite"),
            }
        }
        let config: SecretEncryptionConfig = serde_yaml::from_str(&format!(
            "downstream_mode: one_time\nencryption_key: '{KEY_A}'\n"
        ))
        .expect("current-only config should parse");
        let error = prepare_secrets_before_startup(&config)
            .expect_err("invalid provider secret must reject startup");
        let message = format!("{error:?}");
        assert!(
            message.contains(expected_reason),
            "unexpected error: {message}"
        );
        assert!(!message.contains("strict-provider-secret"));
        assert!(!message.contains(&"f".repeat(64)));
    });
}

#[test]
fn startup_preparation_rejects_provider_corruption_with_safe_reason_codes() {
    assert_sqlite_provider_secret_failure(
        "provider-secret-corrupt-ciphertext.sqlite",
        KEY_A,
        "UPDATE provider_api_key SET secret_ciphertext = X'00' WHERE id = 3101;",
        "reason=decrypt_failed",
    );
    assert_sqlite_provider_secret_failure(
        "provider-secret-wrong-hmac.sqlite",
        KEY_A,
        &format!(
            "UPDATE provider_api_key SET secret_hmac = '{}' WHERE id = 3101;",
            "f".repeat(64)
        ),
        "reason=hmac_mismatch",
    );
    assert_sqlite_provider_secret_failure(
        "provider-secret-incomplete.sqlite",
        KEY_A,
        "PRAGMA ignore_check_constraints = ON;
         UPDATE provider_api_key SET secret_nonce = NULL WHERE id = 3101;
         PRAGMA ignore_check_constraints = OFF;",
        "reason=incomplete_fields",
    );
    assert_sqlite_provider_secret_failure(
        "provider-secret-unknown-key.sqlite",
        KEY_C,
        "",
        "reason=unknown_key",
    );
}

#[test]
fn startup_rotation_database_error_rolls_back_all_prior_updates() {
    let context = TestDbContext::new_sqlite("secret-startup-rollback.sqlite");
    context.run_sync(|| {
        let previous_service = service_for_key(KEY_B);
        let first_id = create_test_api_key("rollback-first");
        let second_id = create_test_api_key("rollback-second");
        let first_before = encrypt_for(first_id, &previous_service, "first");
        let second_before = encrypt_for(second_id, &previous_service, "second");
        store_encrypted_secret(first_id, &first_before);
        store_encrypted_secret(second_id, &second_before);

        let mut connection = get_connection().expect("test connection should load");
        match &mut connection {
            DbConnection::Sqlite(connection) => connection
                .batch_execute(&format!(
                    "CREATE TRIGGER reject_second_secret_rotation
                     BEFORE UPDATE OF secret_ciphertext ON api_key
                     WHEN OLD.id = {second_id}
                     BEGIN
                       SELECT RAISE(ABORT, 'forced rotation failure');
                     END;"
                ))
                .expect("failure trigger should create"),
            DbConnection::Postgres(_) => unreachable!("test uses sqlite"),
        }
        drop(connection);

        let config = rotation_config(DownstreamSecretMode::Recoverable);
        assert!(
            prepare_secrets_before_startup(&config).is_err(),
            "database failure should fail startup rotation"
        );
        assert_eq!(load_encrypted_secret(first_id), first_before);
        assert_eq!(load_encrypted_secret(second_id), second_before);

        let mut connection = get_connection().expect("test connection should load");
        match &mut connection {
            DbConnection::Sqlite(connection) => connection
                .batch_execute("DROP TRIGGER reject_second_secret_rotation;")
                .expect("failure trigger should drop"),
            DbConnection::Postgres(_) => unreachable!("test uses sqlite"),
        }
        drop(connection);

        let retry_config = rotation_config(DownstreamSecretMode::Recoverable);
        let retry_summary = prepare_secrets_before_startup(&retry_config)
            .expect("a fresh startup should retry the rolled-back rotation");
        assert_eq!(retry_summary.downstream.rotated, 2);
        let current_service = service_for_key(KEY_A);
        assert_eq!(
            current_service
                .decrypt_current(
                    SecretDomain::DownstreamApiKey(first_id),
                    &load_encrypted_secret(first_id),
                )
                .expect("retried first secret should use current key")
                .expose(),
            "first"
        );
        assert_eq!(
            current_service
                .decrypt_current(
                    SecretDomain::DownstreamApiKey(second_id),
                    &load_encrypted_secret(second_id),
                )
                .expect("retried second secret should use current key")
                .expose(),
            "second"
        );
    });
}

#[test]
fn main_prepares_state_and_rotates_secrets_before_binding_or_serving() {
    let main_source = include_str!("../../main.rs");
    let rotation = main_source
        .find("let secret_preparation = prepare_secrets_before_startup")
        .expect("main must run startup secret rotation");
    let app_state = main_source
        .find("create_app_state().await")
        .expect("main must construct app state");
    let bind = main_source
        .find("TcpListener::bind")
        .expect("main must bind a listener");
    let serve = main_source
        .find("axum::serve")
        .expect("main must serve axum");

    assert!(rotation < app_state);
    assert!(app_state < bind);
    assert!(bind < serve);
}

#[test]
#[ignore = "requires a dedicated PostgreSQL 17 database"]
fn postgres_startup_rotation_reencrypts_previous_and_preserves_unknown() {
    let database_url = env::var(POSTGRES_SMOKE_URL_ENV).unwrap_or_else(|_| {
        panic!("{POSTGRES_SMOKE_URL_ENV} must point to the dedicated PostgreSQL smoke database")
    });
    let mut setup = PgConnection::establish(&database_url)
        .expect("dedicated postgres smoke database should be reachable");
    let database_name = diesel::sql_query("SELECT current_database()::text AS name")
        .get_result::<DatabaseNameRow>(&mut setup)
        .expect("postgres database name should query")
        .name;
    assert_eq!(database_name, POSTGRES_SMOKE_DATABASE);
    setup
        .batch_execute("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .expect("postgres smoke schema should reset");
    drop(setup);

    let context = TestDbContext::new_postgres(&database_url);
    let test_result = catch_unwind(AssertUnwindSafe(|| {
        context.run_sync(|| {
            let previous_service = service_for_key(KEY_B);
            let unknown_service = service_for_key(KEY_C);
            let current_service = service_for_key(KEY_A);
            create_manager_credential();
            let manager_totp_plaintext =
                SensitiveSecret::new("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_string());
            let previous_manager_totp = previous_service
                .encrypt_current(
                    SecretDomain::ManagerTotp(MANAGER_ID),
                    &manager_totp_plaintext,
                )
                .expect("postgres previous manager TOTP should encrypt");
            store_manager_totp_secret(&previous_manager_totp, 123, 456);
            let previous_id = create_test_api_key("postgres-previous");
            let unknown_id = create_test_api_key("postgres-unknown");
            let previous = encrypt_for(previous_id, &previous_service, "postgres-secret");
            let unknown = encrypt_for(unknown_id, &unknown_service, "postgres-unknown");
            store_encrypted_secret(previous_id, &previous);
            store_encrypted_secret(unknown_id, &unknown);
            let provider = create_test_provider(4_001);
            create_test_provider_secret(
                4_101,
                provider.id,
                &previous_service,
                "postgres-provider-secret",
            );

            let config = rotation_config(DownstreamSecretMode::Recoverable);
            let summary = prepare_secrets_before_startup(&config)
                .expect("postgres startup rotation should succeed");
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
                    &load_encrypted_secret(previous_id),
                )
                .expect("postgres rotated secret should use current key");
            assert_eq!(plaintext.expose(), "postgres-secret");
            assert_eq!(load_encrypted_secret(unknown_id), unknown);
            let provider_plaintext = current_service
                .decrypt_current(
                    SecretDomain::ProviderApiKey(4_101),
                    &load_provider_secret(provider.id, 4_101)
                        .encrypted_secret()
                        .expect("postgres provider tuple should load"),
                )
                .expect("postgres provider secret should rotate");
            assert_eq!(provider_plaintext.expose(), "postgres-provider-secret");

            assert_eq!(
                current_service
                    .decrypt_current(
                        SecretDomain::ManagerTotp(MANAGER_ID),
                        &load_manager_totp_secret(),
                    )
                    .expect("postgres manager TOTP should rotate to current")
                    .expose(),
                manager_totp_plaintext.expose()
            );
            let current_summary = prepare_secrets_before_startup(&config)
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
            store_manager_totp_secret(&unknown_manager_totp, 124, 457);
            let unknown_before = load_manager_totp_secret();
            let unknown_summary = prepare_secrets_before_startup(&config)
                .expect("postgres unknown manager TOTP must not block startup");
            assert_eq!(
                unknown_summary.manager_totp.state,
                ManagerTotpPreparationState::UnavailablePreserved
            );
            assert_eq!(
                unknown_summary.manager_totp.failure,
                Some(ManagerTotpPreparationFailure::UnknownKey)
            );
            assert_eq!(load_manager_totp_secret(), unknown_before);
        });
    }));
    drop(context);

    let mut cleanup = PgConnection::establish(&database_url)
        .expect("dedicated postgres smoke database should remain reachable");
    cleanup
        .batch_execute("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .expect("postgres smoke schema should clean up");
    if let Err(panic_payload) = test_result {
        resume_unwind(panic_payload);
    }
}
