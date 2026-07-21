use super::*;

use crate::database::api_key::{_postgres_model, _sqlite_model, ApiKey, CreateApiKeyPayload};
use crate::database::{DbConnection, TestDbContext, get_connection};
use crate::db_execute;
use crate::schema::enum_def::Action;
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
        let summary = rotate_downstream_secrets_before_startup(&startup_config)
            .expect("downstream startup rotation should continue past unavailable rows");
        assert_eq!(
            summary,
            SecretRotationSummary {
                current: 1,
                rotated: 1,
                unavailable_preserved: 2,
            }
        );
        assert_eq!(summary.total(), 4);
        assert!(!config.has_previous_encryption_key());

        let rotated = load_encrypted_secret(previous_id);
        let plaintext = current_service
            .decrypt_current(SecretDomain::DownstreamApiKey(previous_id), &rotated)
            .expect("rotated secret should decrypt with current key");
        assert_eq!(plaintext.expose(), "previous-secret");
        assert_eq!(load_encrypted_secret(unknown_id), unknown_before);
        assert_eq!(load_encrypted_secret(corrupt_id), corrupt);

        let second = rotate_downstream_secrets_before_startup(&config)
            .expect("rotation should be idempotent after previous key is consumed");
        assert_eq!(second.current, 2);
        assert_eq!(second.rotated, 0);
        assert_eq!(second.unavailable_preserved, 2);
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
        let summary = rotate_downstream_secrets_before_startup(&config)
            .expect("one-time mode should still rotate historical ciphertext");
        assert_eq!(summary.rotated, 1);
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
            rotate_downstream_secrets_before_startup(&config).is_err(),
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
        let retry_summary = rotate_downstream_secrets_before_startup(&retry_config)
            .expect("a fresh startup should retry the rolled-back rotation");
        assert_eq!(retry_summary.rotated, 2);
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
        .find("let secret_rotation = rotate_downstream_secrets_before_startup")
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
            let previous_id = create_test_api_key("postgres-previous");
            let unknown_id = create_test_api_key("postgres-unknown");
            let previous = encrypt_for(previous_id, &previous_service, "postgres-secret");
            let unknown = encrypt_for(unknown_id, &unknown_service, "postgres-unknown");
            store_encrypted_secret(previous_id, &previous);
            store_encrypted_secret(unknown_id, &unknown);

            let config = rotation_config(DownstreamSecretMode::Recoverable);
            let summary = rotate_downstream_secrets_before_startup(&config)
                .expect("postgres startup rotation should succeed");
            assert_eq!(summary.rotated, 1);
            assert_eq!(summary.unavailable_preserved, 1);
            let plaintext = current_service
                .decrypt_current(
                    SecretDomain::DownstreamApiKey(previous_id),
                    &load_encrypted_secret(previous_id),
                )
                .expect("postgres rotated secret should use current key");
            assert_eq!(plaintext.expose(), "postgres-secret");
            assert_eq!(load_encrypted_secret(unknown_id), unknown);
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
