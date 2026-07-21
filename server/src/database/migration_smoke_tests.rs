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
    updated_at BIGINT NOT NULL
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
