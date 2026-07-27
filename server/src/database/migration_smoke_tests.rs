use super::{
    POSTGRES_ARCHIVED_UPGRADE_VERSIONS, POSTGRES_UPGRADE_MIGRATIONS,
    SQLITE_ARCHIVED_UPGRADE_VERSIONS, SQLITE_CLEAN_BASELINE_VERSION, SQLITE_UPGRADE_MIGRATIONS,
    open_test_sqlite_connection, postgres_user_table_count, run_postgres_migrations,
    run_sqlite_migrations, sqlite_user_table_count,
};
use diesel::{
    Connection, PgConnection, QueryableByName, RunQueryDsl,
    connection::SimpleConnection,
    sql_types::{BigInt, Text},
};
use diesel_migrations::MigrationHarness;
use std::{
    env,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
};

const POSTGRES_SMOKE_URL_ENV: &str = "CYDER_R1_POSTGRES_MIGRATION_SMOKE_URL";
const POSTGRES_SMOKE_DATABASE: &str = "cyder_r1_migration_smoke";
const POSTGRES_CLEAN_BASELINE_VERSION: &str = "20260423180000";
const R26_HASH: &str = "bb70cc6e62109d41551197d981876cd7b8ae92140ca73a2fc95c55a43c860d6b";

const LEGACY_SQLITE_API_KEY_SCHEMA: &str = r#"
CREATE TABLE api_key (
    id BIGINT PRIMARY KEY NOT NULL,
    api_key TEXT NOT NULL,
    api_key_hash TEXT,
    key_prefix TEXT NOT NULL,
    key_last4 TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    default_action TEXT NOT NULL DEFAULT 'ALLOW',
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    expires_at BIGINT,
    rate_limit_rpm INTEGER,
    max_concurrent_requests INTEGER,
    quota_daily_requests BIGINT,
    quota_daily_tokens BIGINT,
    quota_monthly_tokens BIGINT,
    budget_daily_nanos BIGINT,
    budget_daily_currency TEXT,
    budget_monthly_nanos BIGINT,
    budget_monthly_currency TEXT,
    deleted_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT chk_provider_api_key_timestamps CHECK (updated_at >= created_at)
);
CREATE UNIQUE INDEX idx_api_key_key_uq_active
    ON api_key (api_key) WHERE deleted_at IS NULL AND is_enabled = true;
CREATE UNIQUE INDEX idx_api_key_hash_uq_active
    ON api_key (api_key_hash)
    WHERE deleted_at IS NULL AND is_enabled = true AND api_key_hash IS NOT NULL;
CREATE INDEX idx_api_key_name ON api_key (name);
CREATE INDEX idx_api_key_deleted_at ON api_key (deleted_at);
CREATE INDEX idx_api_key_expires_at ON api_key (expires_at);
CREATE TABLE api_key_child (
    id BIGINT PRIMARY KEY NOT NULL,
    api_key_id BIGINT NOT NULL REFERENCES api_key(id) ON DELETE CASCADE ON UPDATE CASCADE
);
"#;

const LEGACY_POSTGRES_API_KEY_SCHEMA: &str = r#"
CREATE TABLE api_key (
    id BIGINT PRIMARY KEY,
    api_key TEXT NOT NULL,
    api_key_hash TEXT NULL,
    key_prefix TEXT NOT NULL,
    key_last4 TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NULL,
    default_action TEXT NOT NULL DEFAULT 'ALLOW',
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    expires_at BIGINT NULL,
    rate_limit_rpm INTEGER NULL,
    max_concurrent_requests INTEGER NULL,
    quota_daily_requests BIGINT NULL,
    quota_daily_tokens BIGINT NULL,
    quota_monthly_tokens BIGINT NULL,
    budget_daily_nanos BIGINT NULL,
    budget_daily_currency TEXT NULL,
    budget_monthly_nanos BIGINT NULL,
    budget_monthly_currency TEXT NULL,
    deleted_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE UNIQUE INDEX idx_api_key_key_uq_active
    ON api_key (api_key) WHERE deleted_at IS NULL AND is_enabled = TRUE;
CREATE UNIQUE INDEX idx_api_key_hash_uq_active
    ON api_key (api_key_hash)
    WHERE deleted_at IS NULL AND is_enabled = TRUE AND api_key_hash IS NOT NULL;
CREATE INDEX idx_api_key_name ON api_key (name);
CREATE INDEX idx_api_key_deleted_at ON api_key (deleted_at);
CREATE INDEX idx_api_key_expires_at ON api_key (expires_at);
CREATE TABLE api_key_child (
    id BIGINT PRIMARY KEY,
    api_key_id BIGINT NOT NULL REFERENCES api_key(id) ON DELETE CASCADE ON UPDATE CASCADE
);
"#;

const LEGACY_SQLITE_PROVIDER_SECRET_SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;
CREATE TABLE provider (
    id BIGINT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL
);
CREATE TABLE model (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT NOT NULL REFERENCES provider(id),
    name TEXT NOT NULL
);
CREATE TABLE provider_api_key (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT NOT NULL REFERENCES provider(id) ON DELETE CASCADE ON UPDATE CASCADE,
    api_key TEXT NOT NULL,
    description TEXT,
    deleted_at BIGINT,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE UNIQUE INDEX idx_provider_api_key_pid_apikey_uq_active
    ON provider_api_key (provider_id, api_key)
    WHERE deleted_at IS NULL AND is_enabled = true;
CREATE TABLE request_log (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_api_key_id BIGINT REFERENCES provider_api_key(id) ON DELETE SET NULL,
    status TEXT NOT NULL
);
INSERT INTO provider VALUES (1, 'preserved provider');
INSERT INTO model VALUES (11, 1, 'preserved model');
INSERT INTO provider_api_key VALUES
    (21, 1, 'secret-one', 'first', NULL, true, 1, 1),
    (22, 1, 'secret-two', 'second', NULL, false, 2, 2);
INSERT INTO request_log VALUES
    (31, 21, 'SUCCESS'),
    (32, 22, 'FAILED');
"#;

const LEGACY_POSTGRES_PROVIDER_SECRET_SCHEMA: &str = r#"
CREATE TABLE provider (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL
);
CREATE TABLE model (
    id BIGINT PRIMARY KEY,
    provider_id BIGINT NOT NULL REFERENCES provider(id),
    name TEXT NOT NULL
);
CREATE TABLE provider_api_key (
    id BIGINT PRIMARY KEY,
    provider_id BIGINT NOT NULL REFERENCES provider(id) ON DELETE CASCADE ON UPDATE CASCADE,
    api_key TEXT NOT NULL,
    description TEXT NULL,
    deleted_at BIGINT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE UNIQUE INDEX idx_provider_api_key_pid_apikey_uq_active
    ON provider_api_key (provider_id, api_key)
    WHERE deleted_at IS NULL AND is_enabled = TRUE;
CREATE TABLE request_log (
    id BIGINT PRIMARY KEY,
    provider_api_key_id BIGINT NULL REFERENCES provider_api_key(id) ON DELETE SET NULL,
    status TEXT NOT NULL
);
INSERT INTO provider VALUES (1, 'preserved provider');
INSERT INTO model VALUES (11, 1, 'preserved model');
INSERT INTO provider_api_key VALUES
    (21, 1, 'secret-one', 'first', NULL, TRUE, 1, 1),
    (22, 1, 'secret-two', 'second', NULL, FALSE, 2, 2);
INSERT INTO request_log VALUES
    (31, 21, 'SUCCESS'),
    (32, 22, 'FAILED');
"#;

#[derive(QueryableByName)]
struct DatabaseNameRow {
    #[diesel(sql_type = Text)]
    name: String,
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn assert_bootstrap_versions_recorded(
    applied_versions: Vec<String>,
    clean_baseline_version: &str,
    archived_upgrade_versions: &[&str],
) {
    assert!(
        applied_versions
            .iter()
            .any(|version| version == clean_baseline_version),
        "clean baseline migration {clean_baseline_version} should be recorded"
    );

    for expected in archived_upgrade_versions {
        assert!(
            applied_versions.iter().any(|version| version == expected),
            "archived upgrade migration {expected} should be recorded"
        );
    }
}

fn postgres_database_name(connection: &mut PgConnection) -> String {
    diesel::sql_query("SELECT current_database()::text AS name")
        .get_result::<DatabaseNameRow>(connection)
        .expect("postgres smoke database name should be queryable")
        .name
}

fn rebuild_postgres_public_schema(connection: &mut PgConnection) {
    connection
        .batch_execute("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .expect("postgres smoke public schema should be rebuilt");
}

fn insert_legacy_sqlite_api_key(connection: &mut impl SimpleConnection, id: i64, hash_sql: &str) {
    connection
        .batch_execute(&format!(
            "INSERT INTO api_key (
                id, api_key, api_key_hash, key_prefix, key_last4, name,
                default_action, is_enabled, created_at, updated_at
             ) VALUES (
                {id}, 'cyder-legacy-{id}', {hash_sql}, 'cyder-legacy', '{id:04}',
                'legacy {id}', 'ALLOW', true, 1, 1
             );
             INSERT INTO api_key_child (id, api_key_id) VALUES ({id}, {id});"
        ))
        .expect("legacy api key fixture should insert");
}

fn assert_sqlite_column_count(
    connection: &mut diesel::SqliteConnection,
    column: &str,
    expected: i64,
) {
    let query = format!(
        "SELECT COUNT(*) AS count FROM pragma_table_info('api_key') WHERE name = '{column}'"
    );
    let count = diesel::sql_query(query)
        .get_result::<CountRow>(connection)
        .expect("sqlite api key column should be queryable")
        .count;
    assert_eq!(count, expected, "unexpected api_key column state: {column}");
}

fn sqlite_table_column_count(
    connection: &mut diesel::SqliteConnection,
    table: &str,
    column: &str,
) -> i64 {
    diesel::sql_query(format!(
        "SELECT COUNT(*) AS count FROM pragma_table_info('{table}') WHERE name = '{column}'"
    ))
    .get_result::<CountRow>(connection)
    .expect("sqlite table column should be queryable")
    .count
}

#[test]
fn sqlite_clean_upgrade_chain_from_empty() {
    let (_temp_dir, mut connection) = open_test_sqlite_connection("r1-migration-smoke.sqlite");

    assert_eq!(
        sqlite_user_table_count(&mut connection).expect("sqlite tables should be countable"),
        0,
        "fresh sqlite smoke database should not contain business tables"
    );

    run_sqlite_migrations(&mut connection).expect("sqlite clean + upgrade migrations should run");

    let applied_versions = connection
        .applied_migrations()
        .expect("sqlite applied migrations should be queryable")
        .into_iter()
        .map(|version| version.to_string())
        .collect();
    assert_bootstrap_versions_recorded(
        applied_versions,
        SQLITE_CLEAN_BASELINE_VERSION,
        SQLITE_ARCHIVED_UPGRADE_VERSIONS,
    );
    assert!(
        !connection
            .has_pending_migration(SQLITE_UPGRADE_MIGRATIONS)
            .expect("sqlite pending migrations should be queryable"),
        "sqlite upgrade migrations should have no pending entries"
    );
    assert!(
        sqlite_user_table_count(&mut connection).expect("sqlite tables should be countable") > 0,
        "sqlite migration chain should create business tables"
    );

    run_sqlite_migrations(&mut connection)
        .expect("sqlite migration chain should be safe to run a second time");
    assert!(
        !connection
            .has_pending_migration(SQLITE_UPGRADE_MIGRATIONS)
            .expect("sqlite pending migrations should remain queryable"),
        "sqlite second migration run should remain fully applied"
    );
}

#[test]
fn sqlite_r26_api_key_secret_upgrade_is_hash_only_and_preserves_foreign_keys() {
    let (_temp_dir, mut connection) = open_test_sqlite_connection("r26-api-key-upgrade.sqlite");
    connection
        .batch_execute(LEGACY_SQLITE_API_KEY_SCHEMA)
        .expect("legacy sqlite api key schema should create");
    insert_legacy_sqlite_api_key(&mut connection, 1, &format!("'{R26_HASH}'"));
    insert_legacy_sqlite_api_key(&mut connection, 2, "'x'");

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-07-22-090000_downstream_api_key_secret/up.sql"
        ))
        .expect("R2.6 sqlite migration should run");

    assert_sqlite_column_count(&mut connection, "api_key", 0);
    assert_sqlite_column_count(&mut connection, "api_key_hash", 1);
    for column in [
        "secret_ciphertext",
        "secret_nonce",
        "secret_format_version",
        "secret_key_fingerprint",
    ] {
        assert_sqlite_column_count(&mut connection, column, 1);
    }
    let rows = diesel::sql_query(
        "SELECT COUNT(*) AS count FROM api_key
         WHERE secret_ciphertext IS NULL
           AND secret_nonce IS NULL
           AND secret_format_version IS NULL
           AND secret_key_fingerprint IS NULL
           AND api_key_hash IN ('bb70cc6e62109d41551197d981876cd7b8ae92140ca73a2fc95c55a43c860d6b', 'x')",
    )
    .get_result::<CountRow>(&mut connection)
    .expect("migrated api keys should query")
    .count;
    assert_eq!(
        rows, 2,
        "standard and nonstandard non-null hashes must migrate"
    );
    let children = diesel::sql_query("SELECT COUNT(*) AS count FROM api_key_child")
        .get_result::<CountRow>(&mut connection)
        .expect("child rows should query")
        .count;
    assert_eq!(children, 2, "api key child rows must be preserved");
    let foreign_key_violations =
        diesel::sql_query("SELECT COUNT(*) AS count FROM pragma_foreign_key_check")
            .get_result::<CountRow>(&mut connection)
            .expect("foreign key check should run")
            .count;
    assert_eq!(foreign_key_violations, 0);

    let partial_secret =
        connection.batch_execute("UPDATE api_key SET secret_ciphertext = X'01' WHERE id = 1;");
    assert!(
        partial_secret.is_err(),
        "partial secret tuple must be rejected"
    );
    let duplicate_live = connection.batch_execute(&format!(
        "INSERT INTO api_key (
            id, api_key_hash, key_prefix, key_last4, name, default_action,
            is_enabled, created_at, updated_at
         ) VALUES (3, '{R26_HASH}', 'duplicate', '0003', 'duplicate', 'ALLOW', false, 1, 1);"
    ));
    assert!(
        duplicate_live.is_err(),
        "disabled but non-deleted hash must remain unique"
    );
    connection
        .batch_execute(&format!(
            "INSERT INTO api_key (
                id, api_key_hash, key_prefix, key_last4, name, default_action,
                is_enabled, deleted_at, created_at, updated_at
             ) VALUES (3, '{R26_HASH}', 'deleted', '0003', 'deleted', 'ALLOW', false, 2, 1, 2);"
        ))
        .expect("deleted record may retain a duplicate historical hash");
}

#[test]
fn sqlite_r26_api_key_secret_upgrade_rolls_back_when_hash_is_null() {
    let (_temp_dir, mut connection) = open_test_sqlite_connection("r26-api-key-null-hash.sqlite");
    connection
        .batch_execute(LEGACY_SQLITE_API_KEY_SCHEMA)
        .expect("legacy sqlite api key schema should create");
    insert_legacy_sqlite_api_key(&mut connection, 1, "NULL");

    let result = connection.batch_execute(include_str!(
        "../../migrations/sqlite/2026-07-22-090000_downstream_api_key_secret/up.sql"
    ));
    assert!(result.is_err(), "NULL api_key_hash must stop the migration");
    let _ = connection.batch_execute("ROLLBACK;");
    connection
        .batch_execute("PRAGMA foreign_keys = ON;")
        .expect("foreign keys should be restored after the expected failure");

    assert_sqlite_column_count(&mut connection, "api_key", 1);
    assert_sqlite_column_count(&mut connection, "secret_ciphertext", 0);
    let legacy_rows = diesel::sql_query(
        "SELECT COUNT(*) AS count FROM api_key
         WHERE api_key = 'cyder-legacy-1' AND api_key_hash IS NULL",
    )
    .get_result::<CountRow>(&mut connection)
    .expect("legacy row should remain after rollback")
    .count;
    assert_eq!(legacy_rows, 1);
}

#[test]
fn sqlite_r27_provider_secret_upgrade_discards_keys_and_enforces_contract() {
    let (_temp_dir, mut connection) = open_test_sqlite_connection("r27-provider-secret.sqlite");
    connection
        .batch_execute(LEGACY_SQLITE_PROVIDER_SECRET_SCHEMA)
        .expect("legacy sqlite provider secret schema should create");

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-07-23-090000_provider_api_key_secret/up.sql"
        ))
        .expect("R2.7 sqlite migration should run");

    let key_count = diesel::sql_query("SELECT COUNT(*) AS count FROM provider_api_key")
        .get_result::<CountRow>(&mut connection)
        .expect("provider key count should query")
        .count;
    let preserved = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM provider p
         JOIN model m ON m.provider_id = p.id
         JOIN request_log r ON r.status IN ('SUCCESS', 'FAILED')
         WHERE p.name = 'preserved provider' AND m.name = 'preserved model'
           AND r.provider_api_key_id IS NULL",
    )
    .get_result::<CountRow>(&mut connection)
    .expect("preserved R2.7 rows should query")
    .count;
    assert_eq!(
        key_count, 0,
        "all historical provider keys must be discarded"
    );
    assert_eq!(
        preserved, 2,
        "provider/model/log data and log statuses must survive"
    );
    assert_eq!(
        sqlite_table_column_count(&mut connection, "provider_api_key", "api_key"),
        0
    );
    for column in [
        "key_prefix",
        "key_last4",
        "secret_ciphertext",
        "secret_nonce",
        "secret_format_version",
        "secret_key_fingerprint",
        "secret_hmac",
    ] {
        assert_eq!(
            sqlite_table_column_count(&mut connection, "provider_api_key", column),
            1,
            "missing provider secret column {column}"
        );
    }

    let fingerprint = "a".repeat(64);
    let hmac = "b".repeat(64);
    connection
        .batch_execute(&format!(
            "INSERT INTO provider_api_key (
                id, provider_id, key_prefix, key_last4, secret_ciphertext, secret_nonce,
                secret_format_version, secret_key_fingerprint, secret_hmac,
                is_enabled, created_at, updated_at
             ) VALUES (41, 1, 'sk-a', 'last', X'01', zeroblob(24), 1,
                '{fingerprint}', '{hmac}', false, 10, 10);"
        ))
        .expect("complete disabled provider key should insert");
    let duplicate_disabled = connection.batch_execute(&format!(
        "INSERT INTO provider_api_key (
            id, provider_id, key_prefix, key_last4, secret_ciphertext, secret_nonce,
            secret_format_version, secret_key_fingerprint, secret_hmac,
            is_enabled, created_at, updated_at
         ) VALUES (42, 1, 'sk-b', 'last', X'02', zeroblob(24), 1,
            '{fingerprint}', '{hmac}', false, 10, 10);"
    ));
    assert!(
        duplicate_disabled.is_err(),
        "disabled live key must remain unique"
    );
    connection
        .batch_execute(
            "UPDATE provider_api_key SET
                deleted_at = 11, is_enabled = false,
                secret_ciphertext = NULL, secret_nonce = NULL,
                secret_format_version = NULL, secret_key_fingerprint = NULL,
                secret_hmac = NULL, updated_at = 11
             WHERE id = 41;",
        )
        .expect("soft delete must clear provider secret material");
    connection
        .batch_execute(&format!(
            "INSERT INTO provider_api_key (
                id, provider_id, key_prefix, key_last4, secret_ciphertext, secret_nonce,
                secret_format_version, secret_key_fingerprint, secret_hmac,
                is_enabled, created_at, updated_at
             ) VALUES (42, 1, 'sk-b', 'last', X'02', zeroblob(24), 1,
                '{fingerprint}', '{hmac}', true, 12, 12);"
        ))
        .expect("soft deletion should release provider secret fingerprint");
    connection
        .batch_execute(&format!(
            "INSERT INTO provider_api_key (
                id, provider_id, key_prefix, key_last4, secret_ciphertext, secret_nonce,
                secret_format_version, secret_key_fingerprint, secret_hmac,
                is_enabled, created_at, updated_at
             ) VALUES (45, 1, '', '', X'03', zeroblob(24), 1,
                '{}', '{}', true, 12, 12);",
            "d".repeat(64),
            "e".repeat(64)
        ))
        .expect("empty mask fragments must support single-character provider secrets");
    assert!(
        connection
            .batch_execute(
                "INSERT INTO provider_api_key (
                    id, provider_id, key_prefix, key_last4, is_enabled, created_at, updated_at
                 ) VALUES (43, 1, 'bad', 'last', true, 1, 1);"
            )
            .is_err(),
        "live rows without a complete secret tuple must be rejected"
    );
    assert!(
        connection
            .batch_execute(&format!(
                "INSERT INTO provider_api_key (
                    id, provider_id, key_prefix, key_last4, secret_ciphertext, secret_nonce,
                    secret_format_version, secret_key_fingerprint, secret_hmac,
                    is_enabled, created_at, updated_at
                 ) VALUES (44, 1, 'bad', 'last', X'01', zeroblob(23), 1,
                    '{fingerprint}', '{}', true, 2, 1);",
                "c".repeat(64)
            ))
            .is_err(),
        "nonce length and timestamp constraints must be enforced"
    );
    let foreign_key_violations =
        diesel::sql_query("SELECT COUNT(*) AS count FROM pragma_foreign_key_check")
            .get_result::<CountRow>(&mut connection)
            .expect("sqlite foreign key check should run")
            .count;
    assert_eq!(foreign_key_violations, 0);
}

#[test]
fn sqlite_manager_auth_session_version_upgrade_clears_sessions_only() {
    let (_temp_dir, mut connection) =
        open_test_sqlite_connection("manager-auth-session-version-upgrade.sqlite");
    connection
        .batch_execute(
            "CREATE TABLE manager_credential (
                manager_id BIGINT PRIMARY KEY NOT NULL,
                manager_subject TEXT NOT NULL,
                password_verifier TEXT NOT NULL,
                credential_epoch TEXT NOT NULL,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL
            );
            CREATE TABLE manager_auth_instance (
                id BIGINT PRIMARY KEY NOT NULL,
                manager_id BIGINT NOT NULL,
                manager_subject TEXT NOT NULL,
                current_refresh_jti TEXT NOT NULL,
                created_at BIGINT NOT NULL,
                last_rotated_at BIGINT NOT NULL,
                expires_at BIGINT NOT NULL,
                revoked_at BIGINT NULL,
                revoked_reason TEXT NULL
            );
            INSERT INTO manager_credential VALUES
                (0, 'admin', 'verifier', 'epoch', 1, 1);
            INSERT INTO manager_auth_instance VALUES
                (1, 0, 'admin', 'refresh-jti', 1, 1, 999999, NULL, NULL);",
        )
        .expect("pre-version manager auth schema should create");

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-07-21-090000_manager_auth_session_version/up.sql"
        ))
        .expect("session version migration should run");

    let credential_count = diesel::sql_query("SELECT COUNT(*) AS count FROM manager_credential")
        .get_result::<CountRow>(&mut connection)
        .expect("credential count should query")
        .count;
    let session_count = diesel::sql_query("SELECT COUNT(*) AS count FROM manager_auth_instance")
        .get_result::<CountRow>(&mut connection)
        .expect("session count should query")
        .count;
    let version_default = diesel::sql_query(
        "SELECT COUNT(*) AS count FROM pragma_table_info('manager_auth_instance')
         WHERE name = 'session_version' AND \"notnull\" = 1 AND \"dflt_value\" = '1'",
    )
    .get_result::<CountRow>(&mut connection)
    .expect("session version column should query")
    .count;

    assert_eq!(credential_count, 1, "manager credential must be preserved");
    assert_eq!(session_count, 0, "pre-version sessions must be cleared");
    assert_eq!(
        version_default, 1,
        "session version column must be required"
    );
}

#[test]
fn sqlite_r211_manager_token_mediator_upgrade_clears_sessions_and_enforces_family_contract() {
    let (_temp_dir, mut connection) =
        open_test_sqlite_connection("r211-manager-token-mediator-upgrade.sqlite");
    connection
        .batch_execute(
            "CREATE TABLE manager_credential (
                manager_id BIGINT PRIMARY KEY NOT NULL,
                manager_subject TEXT NOT NULL,
                password_verifier TEXT NOT NULL,
                credential_epoch TEXT NOT NULL,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL
            );
            CREATE TABLE manager_auth_instance (
                id BIGINT PRIMARY KEY NOT NULL,
                manager_id BIGINT NOT NULL,
                manager_subject TEXT NOT NULL,
                current_refresh_jti TEXT NOT NULL UNIQUE,
                session_version BIGINT NOT NULL DEFAULT 1,
                created_at BIGINT NOT NULL,
                last_rotated_at BIGINT NOT NULL,
                expires_at BIGINT NOT NULL,
                revoked_at BIGINT NULL,
                revoked_reason TEXT NULL
            );
            INSERT INTO manager_credential VALUES
                (0, 'admin', 'verifier', '018fa7d8-6a00-7c9a-8f7e-111111111111', 1, 1);
            INSERT INTO manager_auth_instance VALUES
                (1, 0, 'admin', 'legacy-jti', 1, 1, 1, 999999, NULL, NULL);",
        )
        .expect("legacy R2.11 sqlite schema should create");

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-07-23-120000_manager_token_mediator/up.sql"
        ))
        .expect("R2.11 sqlite migration should run");

    let credential_count = diesel::sql_query("SELECT COUNT(*) AS count FROM manager_credential")
        .get_result::<CountRow>(&mut connection)
        .expect("credential count should query")
        .count;
    let session_count = diesel::sql_query("SELECT COUNT(*) AS count FROM manager_auth_instance")
        .get_result::<CountRow>(&mut connection)
        .expect("session count should query")
        .count;
    let required_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM pragma_table_info('manager_auth_instance')
         WHERE name IN (
            'refresh_generation', 'session_version', 'signing_key_id', 'credential_epoch',
            'idle_expires_at', 'absolute_expires_at'
         ) AND \"notnull\" = 1",
    )
    .get_result::<CountRow>(&mut connection)
    .expect("R2.11 sqlite columns should query")
    .count;

    assert_eq!(credential_count, 1, "manager credential must be preserved");
    assert_eq!(session_count, 0, "legacy sessions must be cleared");
    assert_eq!(required_columns, 6, "R2.11 columns must be required");

    let key_id = "a".repeat(64);
    connection
        .batch_execute(&format!(
            "INSERT INTO manager_auth_instance (
                id, manager_id, manager_subject, current_refresh_jti,
                refresh_generation, session_version, signing_key_id, credential_epoch,
                created_at, last_rotated_at, idle_expires_at, absolute_expires_at,
                revoked_at, revoked_reason
             ) VALUES (
                2, 0, 'admin', 'current-jti', 1, 1, '{key_id}',
                '018fa7d8-6a00-7c9a-8f7e-111111111111',
                10, 10, 20, 30, NULL, NULL
             );"
        ))
        .expect("valid R2.11 sqlite family should insert");
    assert!(
        connection
            .batch_execute(&format!(
                "INSERT INTO manager_auth_instance (
                    id, manager_id, manager_subject, current_refresh_jti,
                    refresh_generation, session_version, signing_key_id, credential_epoch,
                    created_at, last_rotated_at, idle_expires_at, absolute_expires_at,
                    revoked_at, revoked_reason
                 ) VALUES (
                    3, 0, 'admin', 'invalid-generation', 0, 1, '{key_id}',
                    '018fa7d8-6a00-7c9a-8f7e-111111111111',
                    10, 10, 20, 30, NULL, NULL
                 );"
            ))
            .is_err(),
        "zero refresh generation must be rejected"
    );
}

#[test]
fn sqlite_r213_manager_totp_upgrade_preserves_credential_and_sessions() {
    let (_temp_dir, mut connection) =
        open_test_sqlite_connection("r213-manager-totp-upgrade.sqlite");
    let key_id = "a".repeat(64);
    connection
        .batch_execute(&format!(
            "PRAGMA foreign_keys = ON;
            CREATE TABLE manager_credential (
                manager_id BIGINT PRIMARY KEY NOT NULL CHECK (manager_id = 0),
                manager_subject TEXT NOT NULL CHECK (manager_subject = 'admin'),
                password_verifier TEXT NOT NULL,
                credential_epoch TEXT NOT NULL UNIQUE,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL
            );
            CREATE TABLE manager_auth_instance (
                id BIGINT PRIMARY KEY NOT NULL,
                manager_id BIGINT NOT NULL CHECK (manager_id = 0),
                manager_subject TEXT NOT NULL CHECK (manager_subject = 'admin'),
                current_refresh_jti TEXT NOT NULL UNIQUE,
                refresh_generation BIGINT NOT NULL DEFAULT 1 CHECK (refresh_generation >= 1),
                session_version BIGINT NOT NULL DEFAULT 1 CHECK (session_version >= 1),
                signing_key_id TEXT NOT NULL CHECK (length(signing_key_id) = 64),
                credential_epoch TEXT NOT NULL CHECK (length(credential_epoch) = 36),
                created_at BIGINT NOT NULL,
                last_rotated_at BIGINT NOT NULL,
                idle_expires_at BIGINT NOT NULL,
                absolute_expires_at BIGINT NOT NULL,
                revoked_at BIGINT NULL,
                revoked_reason TEXT NULL
            );
            INSERT INTO manager_credential VALUES (
                0, 'admin', 'preserved-verifier',
                '018fa7d8-6a00-7c9a-8f7e-111111111111', 10, 20
            );
            INSERT INTO manager_auth_instance VALUES (
                1, 0, 'admin', 'preserved-jti', 2, 3, '{key_id}',
                '018fa7d8-6a00-7c9a-8f7e-111111111111',
                10, 11, 100, 200, NULL, NULL
            );"
        ))
        .expect("pre-R2.13 sqlite manager auth schema should create");

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-07-27-090000_manager_totp/up.sql"
        ))
        .expect("R2.13 sqlite migration should run");

    let preserved_credential = diesel::sql_query(
        "SELECT COUNT(*) AS count FROM manager_credential
         WHERE manager_id = 0
           AND manager_subject = 'admin'
           AND password_verifier = 'preserved-verifier'
           AND credential_epoch = '018fa7d8-6a00-7c9a-8f7e-111111111111'
           AND created_at = 10
           AND updated_at = 20
           AND totp_secret_ciphertext IS NULL
           AND totp_secret_nonce IS NULL
           AND totp_secret_format_version IS NULL
           AND totp_secret_key_fingerprint IS NULL
           AND totp_last_accepted_step IS NULL
           AND totp_enabled_at IS NULL",
    )
    .get_result::<CountRow>(&mut connection)
    .expect("migrated credential should query")
    .count;
    let preserved_session = diesel::sql_query(
        "SELECT COUNT(*) AS count FROM manager_auth_instance
         WHERE id = 1
           AND current_refresh_jti = 'preserved-jti'
           AND refresh_generation = 2
           AND session_version = 3
           AND revoked_at IS NULL",
    )
    .get_result::<CountRow>(&mut connection)
    .expect("migrated session should query")
    .count;
    assert_eq!(preserved_credential, 1, "credential must be preserved");
    assert_eq!(preserved_session, 1, "active session must be preserved");
    assert_eq!(
        sqlite_table_column_count(
            &mut connection,
            "manager_totp_recovery_code",
            "code_verifier"
        ),
        1
    );

    assert!(
        connection
            .batch_execute(
                "UPDATE manager_credential
                 SET totp_secret_ciphertext = X'01'
                 WHERE manager_id = 0;"
            )
            .is_err(),
        "partial manager TOTP tuple must be rejected"
    );
    assert!(
        connection
            .batch_execute(
                "INSERT INTO manager_totp_recovery_code
                    (code_id, manager_id, code_verifier, created_at)
                 VALUES ('BAD', 0, 'verifier', 30);"
            )
            .is_err(),
        "recovery code id must contain exactly four characters"
    );
    connection
        .batch_execute(
            "INSERT INTO manager_totp_recovery_code
                (code_id, manager_id, code_verifier, created_at)
             VALUES ('A000', 0, 'verifier', 30);",
        )
        .expect("valid recovery verifier should insert");
    let foreign_key_violations =
        diesel::sql_query("SELECT COUNT(*) AS count FROM pragma_foreign_key_check")
            .get_result::<CountRow>(&mut connection)
            .expect("sqlite foreign key check should run")
            .count;
    assert_eq!(foreign_key_violations, 0);

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-07-27-090000_manager_totp/down.sql"
        ))
        .expect("R2.13 sqlite down migration should run");
    assert_eq!(
        sqlite_table_column_count(
            &mut connection,
            "manager_credential",
            "totp_secret_ciphertext"
        ),
        0,
        "down migration must remove TOTP columns"
    );
    assert_eq!(
        sqlite_table_column_count(
            &mut connection,
            "manager_totp_recovery_code",
            "code_verifier"
        ),
        0,
        "down migration must remove recovery code table"
    );
    let down_preserved = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM manager_credential c
         JOIN manager_auth_instance s ON s.manager_id = c.manager_id
         WHERE c.password_verifier = 'preserved-verifier'
           AND c.credential_epoch = '018fa7d8-6a00-7c9a-8f7e-111111111111'
           AND s.current_refresh_jti = 'preserved-jti'",
    )
    .get_result::<CountRow>(&mut connection)
    .expect("down-migrated manager state should query")
    .count;
    assert_eq!(
        down_preserved, 1,
        "down migration must preserve credential and sessions"
    );
}

#[test]
#[ignore = "requires a dedicated PostgreSQL 17 database"]
fn postgres_clean_upgrade_chain_from_empty() {
    let database_url = env::var(POSTGRES_SMOKE_URL_ENV).unwrap_or_else(|_| {
        panic!("{POSTGRES_SMOKE_URL_ENV} must point to the dedicated PostgreSQL smoke database")
    });
    let mut connection = PgConnection::establish(&database_url)
        .expect("dedicated postgres smoke database should be reachable");

    let database_name = postgres_database_name(&mut connection);
    assert_eq!(
        database_name, POSTGRES_SMOKE_DATABASE,
        "refusing to rebuild a PostgreSQL database not dedicated to the R1 migration smoke test"
    );

    rebuild_postgres_public_schema(&mut connection);
    let test_result = catch_unwind(AssertUnwindSafe(|| {
        assert_eq!(
            postgres_user_table_count(&mut connection)
                .expect("postgres tables should be countable"),
            0,
            "fresh postgres public schema should not contain business tables"
        );

        run_postgres_migrations(&mut connection)
            .expect("postgres clean + upgrade migrations should run");

        let applied_versions = connection
            .applied_migrations()
            .expect("postgres applied migrations should be queryable")
            .into_iter()
            .map(|version| version.to_string())
            .collect();
        assert_bootstrap_versions_recorded(
            applied_versions,
            POSTGRES_CLEAN_BASELINE_VERSION,
            POSTGRES_ARCHIVED_UPGRADE_VERSIONS,
        );
        assert!(
            !connection
                .has_pending_migration(POSTGRES_UPGRADE_MIGRATIONS)
                .expect("postgres pending migrations should be queryable"),
            "postgres upgrade migrations should have no pending entries"
        );
        assert!(
            postgres_user_table_count(&mut connection)
                .expect("postgres tables should be countable")
                > 0,
            "postgres migration chain should create business tables"
        );

        run_postgres_migrations(&mut connection)
            .expect("postgres migration chain should be safe to run a second time");
        assert!(
            !connection
                .has_pending_migration(POSTGRES_UPGRADE_MIGRATIONS)
                .expect("postgres pending migrations should remain queryable"),
            "postgres second migration run should remain fully applied"
        );
    }));

    rebuild_postgres_public_schema(&mut connection);
    if let Err(panic_payload) = test_result {
        resume_unwind(panic_payload);
    }
}

#[test]
#[ignore = "requires a dedicated PostgreSQL 17 database"]
fn postgres_r211_manager_token_mediator_upgrade() {
    let database_url = env::var(POSTGRES_SMOKE_URL_ENV).unwrap_or_else(|_| {
        panic!("{POSTGRES_SMOKE_URL_ENV} must point to the dedicated PostgreSQL smoke database")
    });
    let mut connection = PgConnection::establish(&database_url)
        .expect("dedicated postgres smoke database should be reachable");
    assert_eq!(
        postgres_database_name(&mut connection),
        POSTGRES_SMOKE_DATABASE,
        "refusing to rebuild a non-dedicated PostgreSQL database"
    );

    rebuild_postgres_public_schema(&mut connection);
    let test_result = catch_unwind(AssertUnwindSafe(|| {
        connection
            .batch_execute(
                "CREATE TABLE manager_credential (
                    manager_id BIGINT PRIMARY KEY NOT NULL,
                    manager_subject TEXT NOT NULL,
                    password_verifier TEXT NOT NULL,
                    credential_epoch TEXT NOT NULL,
                    created_at BIGINT NOT NULL,
                    updated_at BIGINT NOT NULL
                );
                CREATE TABLE manager_auth_instance (
                    id BIGINT PRIMARY KEY,
                    manager_id BIGINT NOT NULL,
                    manager_subject TEXT NOT NULL,
                    current_refresh_jti TEXT NOT NULL UNIQUE,
                    session_version BIGINT NOT NULL DEFAULT 1,
                    created_at BIGINT NOT NULL,
                    last_rotated_at BIGINT NOT NULL,
                    expires_at BIGINT NOT NULL,
                    revoked_at BIGINT NULL,
                    revoked_reason TEXT NULL
                );
                INSERT INTO manager_credential VALUES
                    (0, 'admin', 'verifier', '018fa7d8-6a00-7c9a-8f7e-111111111111', 1, 1);
                INSERT INTO manager_auth_instance VALUES
                    (1, 0, 'admin', 'legacy-jti', 1, 1, 1, 999999, NULL, NULL);",
            )
            .expect("legacy R2.11 postgres schema should create");
        connection
            .batch_execute(include_str!(
                "../../migrations/postgres/2026-07-23-120000_manager_token_mediator/up.sql"
            ))
            .expect("R2.11 postgres migration should run");

        let credential_count =
            diesel::sql_query("SELECT COUNT(*) AS count FROM manager_credential")
                .get_result::<CountRow>(&mut connection)
                .expect("postgres credential count should query")
                .count;
        let session_count =
            diesel::sql_query("SELECT COUNT(*) AS count FROM manager_auth_instance")
                .get_result::<CountRow>(&mut connection)
                .expect("postgres session count should query")
                .count;
        let required_columns = diesel::sql_query(
            "SELECT COUNT(*) AS count
             FROM information_schema.columns
             WHERE table_schema = current_schema()
               AND table_name = 'manager_auth_instance'
               AND column_name IN (
                  'refresh_generation', 'session_version', 'signing_key_id', 'credential_epoch',
                  'idle_expires_at', 'absolute_expires_at'
               )
               AND is_nullable = 'NO'",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("R2.11 postgres columns should query")
        .count;

        assert_eq!(credential_count, 1, "manager credential must be preserved");
        assert_eq!(session_count, 0, "legacy sessions must be cleared");
        assert_eq!(required_columns, 6, "R2.11 columns must be required");
    }));

    rebuild_postgres_public_schema(&mut connection);
    if let Err(panic_payload) = test_result {
        resume_unwind(panic_payload);
    }
}

#[test]
#[ignore = "requires a dedicated PostgreSQL 17 database"]
fn postgres_r213_manager_totp_upgrade_preserves_credential_and_sessions() {
    let database_url = env::var(POSTGRES_SMOKE_URL_ENV).unwrap_or_else(|_| {
        panic!("{POSTGRES_SMOKE_URL_ENV} must point to the dedicated PostgreSQL smoke database")
    });
    let mut connection = PgConnection::establish(&database_url)
        .expect("dedicated postgres smoke database should be reachable");
    assert_eq!(
        postgres_database_name(&mut connection),
        POSTGRES_SMOKE_DATABASE,
        "refusing to rebuild a non-dedicated PostgreSQL database"
    );

    rebuild_postgres_public_schema(&mut connection);
    let test_result = catch_unwind(AssertUnwindSafe(|| {
        let key_id = "a".repeat(64);
        connection
            .batch_execute(&format!(
                "CREATE TABLE manager_credential (
                    manager_id BIGINT PRIMARY KEY CHECK (manager_id = 0),
                    manager_subject TEXT NOT NULL CHECK (manager_subject = 'admin'),
                    password_verifier TEXT NOT NULL,
                    credential_epoch TEXT NOT NULL UNIQUE,
                    created_at BIGINT NOT NULL,
                    updated_at BIGINT NOT NULL
                );
                CREATE TABLE manager_auth_instance (
                    id BIGINT PRIMARY KEY,
                    manager_id BIGINT NOT NULL CHECK (manager_id = 0),
                    manager_subject TEXT NOT NULL CHECK (manager_subject = 'admin'),
                    current_refresh_jti TEXT NOT NULL UNIQUE,
                    refresh_generation BIGINT NOT NULL DEFAULT 1 CHECK (refresh_generation >= 1),
                    session_version BIGINT NOT NULL DEFAULT 1 CHECK (session_version >= 1),
                    signing_key_id TEXT NOT NULL CHECK (char_length(signing_key_id) = 64),
                    credential_epoch TEXT NOT NULL CHECK (char_length(credential_epoch) = 36),
                    created_at BIGINT NOT NULL,
                    last_rotated_at BIGINT NOT NULL,
                    idle_expires_at BIGINT NOT NULL,
                    absolute_expires_at BIGINT NOT NULL,
                    revoked_at BIGINT NULL,
                    revoked_reason TEXT NULL
                );
                INSERT INTO manager_credential VALUES (
                    0, 'admin', 'preserved-verifier',
                    '018fa7d8-6a00-7c9a-8f7e-111111111111', 10, 20
                );
                INSERT INTO manager_auth_instance VALUES (
                    1, 0, 'admin', 'preserved-jti', 2, 3, '{key_id}',
                    '018fa7d8-6a00-7c9a-8f7e-111111111111',
                    10, 11, 100, 200, NULL, NULL
                );"
            ))
            .expect("pre-R2.13 postgres manager auth schema should create");
        connection
            .batch_execute(include_str!(
                "../../migrations/postgres/2026-07-27-090000_manager_totp/up.sql"
            ))
            .expect("R2.13 postgres migration should run");

        let preserved_credential = diesel::sql_query(
            "SELECT COUNT(*) AS count FROM manager_credential
             WHERE manager_id = 0
               AND password_verifier = 'preserved-verifier'
               AND credential_epoch = '018fa7d8-6a00-7c9a-8f7e-111111111111'
               AND created_at = 10
               AND updated_at = 20
               AND totp_secret_ciphertext IS NULL
               AND totp_secret_nonce IS NULL
               AND totp_secret_format_version IS NULL
               AND totp_secret_key_fingerprint IS NULL
               AND totp_last_accepted_step IS NULL
               AND totp_enabled_at IS NULL",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("migrated postgres credential should query")
        .count;
        let preserved_session = diesel::sql_query(
            "SELECT COUNT(*) AS count FROM manager_auth_instance
             WHERE id = 1
               AND current_refresh_jti = 'preserved-jti'
               AND refresh_generation = 2
               AND session_version = 3
               AND revoked_at IS NULL",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("migrated postgres session should query")
        .count;
        let recovery_table = diesel::sql_query(
            "SELECT COUNT(*) AS count
             FROM information_schema.tables
             WHERE table_schema = current_schema()
               AND table_name = 'manager_totp_recovery_code'",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("postgres recovery table should query")
        .count;
        assert_eq!(preserved_credential, 1, "credential must be preserved");
        assert_eq!(preserved_session, 1, "active session must be preserved");
        assert_eq!(recovery_table, 1, "recovery table must be created");

        assert!(
            connection
                .batch_execute(
                    "UPDATE manager_credential
                     SET totp_secret_ciphertext = decode('01', 'hex')
                     WHERE manager_id = 0;"
                )
                .is_err(),
            "partial postgres manager TOTP tuple must be rejected"
        );
        assert!(
            connection
                .batch_execute(
                    "INSERT INTO manager_totp_recovery_code
                        (code_id, manager_id, code_verifier, created_at)
                     VALUES ('BAD', 0, 'verifier', 30);"
                )
                .is_err(),
            "postgres recovery code id must contain exactly four characters"
        );

        connection
            .batch_execute(include_str!(
                "../../migrations/postgres/2026-07-27-090000_manager_totp/down.sql"
            ))
            .expect("R2.13 postgres down migration should run");
        let down_columns = diesel::sql_query(
            "SELECT COUNT(*) AS count
             FROM information_schema.columns
             WHERE table_schema = current_schema()
               AND table_name = 'manager_credential'
               AND column_name LIKE 'totp_%'",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("down-migrated postgres columns should query")
        .count;
        let down_preserved = diesel::sql_query(
            "SELECT COUNT(*) AS count
             FROM manager_credential c
             JOIN manager_auth_instance s ON s.manager_id = c.manager_id
             WHERE c.password_verifier = 'preserved-verifier'
               AND s.current_refresh_jti = 'preserved-jti'",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("down-migrated postgres manager state should query")
        .count;
        assert_eq!(down_columns, 0, "down migration must remove TOTP columns");
        assert_eq!(
            down_preserved, 1,
            "down migration must preserve credential and sessions"
        );
    }));

    rebuild_postgres_public_schema(&mut connection);
    if let Err(panic_payload) = test_result {
        resume_unwind(panic_payload);
    }
}

#[test]
#[ignore = "requires a dedicated PostgreSQL 17 database"]
fn postgres_r27_provider_secret_upgrade_discards_keys_and_enforces_contract() {
    let database_url = env::var(POSTGRES_SMOKE_URL_ENV).unwrap_or_else(|_| {
        panic!("{POSTGRES_SMOKE_URL_ENV} must point to the dedicated PostgreSQL smoke database")
    });
    let mut connection = PgConnection::establish(&database_url)
        .expect("dedicated postgres smoke database should be reachable");
    assert_eq!(
        postgres_database_name(&mut connection),
        POSTGRES_SMOKE_DATABASE,
        "refusing to rebuild a non-dedicated PostgreSQL database"
    );

    rebuild_postgres_public_schema(&mut connection);
    let test_result = catch_unwind(AssertUnwindSafe(|| {
        connection
            .batch_execute(LEGACY_POSTGRES_PROVIDER_SECRET_SCHEMA)
            .expect("legacy postgres provider secret schema should create");
        connection
            .batch_execute(include_str!(
                "../../migrations/postgres/2026-07-23-090000_provider_api_key_secret/up.sql"
            ))
            .expect("R2.7 postgres migration should run");

        let key_count = diesel::sql_query("SELECT COUNT(*) AS count FROM provider_api_key")
            .get_result::<CountRow>(&mut connection)
            .expect("postgres provider key count should query")
            .count;
        let preserved = diesel::sql_query(
            "SELECT COUNT(*) AS count
             FROM provider p
             JOIN model m ON m.provider_id = p.id
             JOIN request_log r ON r.status IN ('SUCCESS', 'FAILED')
             WHERE p.name = 'preserved provider' AND m.name = 'preserved model'
               AND r.provider_api_key_id IS NULL",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("postgres preserved R2.7 rows should query")
        .count;
        let raw_column = diesel::sql_query(
            "SELECT COUNT(*) AS count FROM information_schema.columns
             WHERE table_schema = current_schema()
               AND table_name = 'provider_api_key'
               AND column_name = 'api_key'",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("postgres plaintext provider key column should query")
        .count;
        assert_eq!(
            key_count, 0,
            "all historical provider keys must be discarded"
        );
        assert_eq!(
            preserved, 2,
            "provider/model/log data and log statuses must survive"
        );
        assert_eq!(
            raw_column, 0,
            "plaintext provider key column must be removed"
        );

        let fingerprint = "a".repeat(64);
        let hmac = "b".repeat(64);
        connection
            .batch_execute(&format!(
                "INSERT INTO provider_api_key (
                    id, provider_id, key_prefix, key_last4, secret_ciphertext, secret_nonce,
                    secret_format_version, secret_key_fingerprint, secret_hmac,
                    is_enabled, created_at, updated_at
                 ) VALUES (41, 1, 'sk-a', 'last', decode('01', 'hex'), decode('{}', 'hex'), 1,
                    '{fingerprint}', '{hmac}', FALSE, 10, 10);",
                "00".repeat(24)
            ))
            .expect("complete disabled postgres provider key should insert");
        let duplicate_disabled = connection.batch_execute(&format!(
            "INSERT INTO provider_api_key (
                id, provider_id, key_prefix, key_last4, secret_ciphertext, secret_nonce,
                secret_format_version, secret_key_fingerprint, secret_hmac,
                is_enabled, created_at, updated_at
             ) VALUES (42, 1, 'sk-b', 'last', decode('02', 'hex'), decode('{}', 'hex'), 1,
                '{fingerprint}', '{hmac}', FALSE, 10, 10);",
            "00".repeat(24)
        ));
        assert!(
            duplicate_disabled.is_err(),
            "disabled live key must remain unique"
        );
        connection
            .batch_execute(
                "UPDATE provider_api_key SET
                    deleted_at = 11, is_enabled = FALSE,
                    secret_ciphertext = NULL, secret_nonce = NULL,
                    secret_format_version = NULL, secret_key_fingerprint = NULL,
                    secret_hmac = NULL, updated_at = 11
                 WHERE id = 41;",
            )
            .expect("postgres soft delete must clear provider secret material");
        connection
            .batch_execute(&format!(
                "INSERT INTO provider_api_key (
                    id, provider_id, key_prefix, key_last4, secret_ciphertext, secret_nonce,
                    secret_format_version, secret_key_fingerprint, secret_hmac,
                    is_enabled, created_at, updated_at
                 ) VALUES (42, 1, 'sk-b', 'last', decode('02', 'hex'), decode('{}', 'hex'), 1,
                    '{fingerprint}', '{hmac}', TRUE, 12, 12);",
                "00".repeat(24)
            ))
            .expect("postgres soft deletion should release provider fingerprint");
        connection
            .batch_execute(&format!(
                "INSERT INTO provider_api_key (
                    id, provider_id, key_prefix, key_last4, secret_ciphertext, secret_nonce,
                    secret_format_version, secret_key_fingerprint, secret_hmac,
                    is_enabled, created_at, updated_at
                 ) VALUES (45, 1, '', '', decode('03', 'hex'), decode('{}', 'hex'), 1,
                    '{}', '{}', TRUE, 12, 12);",
                "00".repeat(24),
                "d".repeat(64),
                "e".repeat(64)
            ))
            .expect("empty mask fragments must support single-character provider secrets");
        assert!(
            connection
                .batch_execute(
                    "INSERT INTO provider_api_key (
                        id, provider_id, key_prefix, key_last4, is_enabled, created_at, updated_at
                     ) VALUES (43, 1, 'bad', 'last', TRUE, 1, 1);"
                )
                .is_err(),
            "postgres live rows without a complete secret tuple must be rejected"
        );
        assert!(
            connection
                .batch_execute(&format!(
                    "INSERT INTO provider_api_key (
                        id, provider_id, key_prefix, key_last4, secret_ciphertext, secret_nonce,
                        secret_format_version, secret_key_fingerprint, secret_hmac,
                        is_enabled, created_at, updated_at
                     ) VALUES (44, 1, 'bad', 'last', decode('01', 'hex'), decode('{}', 'hex'), 1,
                        '{fingerprint}', '{}', TRUE, 2, 1);",
                    "00".repeat(23),
                    "c".repeat(64)
                ))
                .is_err(),
            "postgres nonce length and timestamp constraints must be enforced"
        );
    }));

    rebuild_postgres_public_schema(&mut connection);
    if let Err(panic_payload) = test_result {
        resume_unwind(panic_payload);
    }
}

#[test]
#[ignore = "requires a dedicated PostgreSQL 17 database"]
fn postgres_r26_api_key_secret_upgrade_and_null_hash_rollback() {
    let database_url = env::var(POSTGRES_SMOKE_URL_ENV).unwrap_or_else(|_| {
        panic!("{POSTGRES_SMOKE_URL_ENV} must point to the dedicated PostgreSQL smoke database")
    });
    let mut connection = PgConnection::establish(&database_url)
        .expect("dedicated postgres smoke database should be reachable");
    assert_eq!(
        postgres_database_name(&mut connection),
        POSTGRES_SMOKE_DATABASE,
        "refusing to rebuild a non-dedicated PostgreSQL database"
    );

    rebuild_postgres_public_schema(&mut connection);
    let test_result = catch_unwind(AssertUnwindSafe(|| {
        connection
            .batch_execute(LEGACY_POSTGRES_API_KEY_SCHEMA)
            .expect("legacy postgres api key schema should create");
        connection
            .batch_execute(&format!(
                "INSERT INTO api_key (
                    id, api_key, api_key_hash, key_prefix, key_last4, name,
                    default_action, is_enabled, created_at, updated_at
                 ) VALUES
                    (1, 'cyder-legacy-1', '{R26_HASH}', 'cyder-legacy', '0001', 'legacy 1', 'ALLOW', true, 1, 1),
                    (2, 'cyder-legacy-2', 'x', 'cyder-legacy', '0002', 'legacy 2', 'ALLOW', false, 1, 1);
                 INSERT INTO api_key_child VALUES (1, 1), (2, 2);"
            ))
            .expect("legacy postgres fixtures should insert");
        connection
            .transaction::<(), diesel::result::Error, _>(|connection| {
                connection.batch_execute(include_str!(
                    "../../migrations/postgres/2026-07-22-090000_downstream_api_key_secret/up.sql"
                ))
            })
            .expect("R2.6 postgres migration should run");

        let raw_column = diesel::sql_query(
            "SELECT COUNT(*) AS count FROM information_schema.columns
             WHERE table_schema = current_schema()
               AND table_name = 'api_key'
               AND column_name = 'api_key'",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("postgres raw column should query")
        .count;
        assert_eq!(raw_column, 0);
        let migrated = diesel::sql_query(
            "SELECT COUNT(*) AS count FROM api_key
             WHERE secret_ciphertext IS NULL
               AND api_key_hash IN ('bb70cc6e62109d41551197d981876cd7b8ae92140ca73a2fc95c55a43c860d6b', 'x')",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("postgres migrated rows should query")
        .count;
        assert_eq!(migrated, 2);
        let children = diesel::sql_query("SELECT COUNT(*) AS count FROM api_key_child")
            .get_result::<CountRow>(&mut connection)
            .expect("postgres child rows should query")
            .count;
        assert_eq!(children, 2);

        let partial_secret = connection.batch_execute(
            "UPDATE api_key SET secret_ciphertext = decode('01', 'hex') WHERE id = 1;",
        );
        assert!(
            partial_secret.is_err(),
            "postgres must reject a partial secret tuple"
        );
        let duplicate_live = connection.batch_execute(&format!(
            "INSERT INTO api_key (
                id, api_key_hash, key_prefix, key_last4, name, default_action,
                is_enabled, created_at, updated_at
             ) VALUES (3, '{R26_HASH}', 'duplicate', '0003', 'duplicate', 'ALLOW', false, 1, 1);"
        ));
        assert!(
            duplicate_live.is_err(),
            "postgres disabled but non-deleted hash must remain unique"
        );
        connection
            .batch_execute(&format!(
                "INSERT INTO api_key (
                    id, api_key_hash, key_prefix, key_last4, name, default_action,
                    is_enabled, deleted_at, created_at, updated_at
                 ) VALUES (3, '{R26_HASH}', 'deleted', '0003', 'deleted', 'ALLOW', false, 2, 1, 2);"
            ))
            .expect("postgres deleted record may retain a duplicate historical hash");

        rebuild_postgres_public_schema(&mut connection);
        connection
            .batch_execute(LEGACY_POSTGRES_API_KEY_SCHEMA)
            .expect("legacy postgres api key schema should recreate");
        connection
            .batch_execute(
                "INSERT INTO api_key (
                    id, api_key, api_key_hash, key_prefix, key_last4, name,
                    default_action, is_enabled, created_at, updated_at
                 ) VALUES
                    (1, 'cyder-null', NULL, 'cyder-null', 'null', 'null', 'ALLOW', true, 1, 1);",
            )
            .expect("NULL hash fixture should insert");
        let result = connection.transaction::<(), diesel::result::Error, _>(|connection| {
            connection.batch_execute(include_str!(
                "../../migrations/postgres/2026-07-22-090000_downstream_api_key_secret/up.sql"
            ))
        });
        assert!(result.is_err(), "NULL postgres hash must stop migration");
        let legacy_raw = diesel::sql_query(
            "SELECT COUNT(*) AS count FROM api_key
             WHERE api_key = 'cyder-null' AND api_key_hash IS NULL",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("postgres rollback should preserve raw legacy row")
        .count;
        assert_eq!(legacy_raw, 1);
    }));

    rebuild_postgres_public_schema(&mut connection);
    if let Err(panic_payload) = test_result {
        resume_unwind(panic_payload);
    }
}
