use super::{
    POSTGRES_ARCHIVED_UPGRADE_VERSIONS, POSTGRES_CLEAN_BASELINE_MIGRATIONS,
    POSTGRES_UPGRADE_MIGRATIONS, SQLITE_ARCHIVED_UPGRADE_VERSIONS,
    SQLITE_CLEAN_BASELINE_MIGRATIONS, SQLITE_CLEAN_BASELINE_VERSION, SQLITE_UPGRADE_MIGRATIONS,
    open_test_sqlite_connection, postgres_user_table_count, record_postgres_migration_versions,
    record_sqlite_migration_versions, run_postgres_migrations, run_sqlite_migrations,
    sqlite_user_table_count,
};
use diesel::{
    Connection, PgConnection, QueryableByName, RunQueryDsl,
    connection::SimpleConnection,
    migration::Migration,
    pg::Pg,
    sql_types::{BigInt, Text},
    sqlite::Sqlite,
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
const R39_UPSTREAM_SOURCE_VERSION: &str = "20260805090000";
const R310_PROVIDER_MULTI_SOURCE_VERSION: &str = "20260806090000";
const R311_MODEL_SOURCE_SELECTION_VERSION: &str = "20260807090000";
const R312_SOURCE_BOUND_REQUEST_PATCH_VARIANTS_VERSION: &str = "20260810090000";

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

#[derive(QueryableByName)]
struct TextValueRow {
    #[diesel(sql_type = Text)]
    value: String,
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

fn migrate_sqlite_to_before_r39(
    connection: &mut diesel::SqliteConnection,
) -> Box<dyn Migration<Sqlite>> {
    connection
        .run_pending_migrations(SQLITE_CLEAN_BASELINE_MIGRATIONS)
        .expect("sqlite clean baseline should run");
    record_sqlite_migration_versions(connection, SQLITE_ARCHIVED_UPGRADE_VERSIONS)
        .expect("sqlite archived versions should be recorded");

    let mut migrations = connection
        .pending_migrations(SQLITE_UPGRADE_MIGRATIONS)
        .expect("pending sqlite upgrade migrations should load");
    let r312 = migrations
        .pop()
        .expect("R3.12 sqlite migration should exist");
    assert_eq!(
        r312.name().version().to_string(),
        R312_SOURCE_BOUND_REQUEST_PATCH_VARIANTS_VERSION,
        "R3.12 must be the final sqlite migration in this release"
    );
    let r311 = migrations
        .pop()
        .expect("R3.11 sqlite migration should exist");
    assert_eq!(
        r311.name().version().to_string(),
        R311_MODEL_SOURCE_SELECTION_VERSION,
        "R3.11 must be the final sqlite migration in this release"
    );
    let r310 = migrations
        .pop()
        .expect("R3.10 sqlite migration should exist");
    assert_eq!(
        r310.name().version().to_string(),
        R310_PROVIDER_MULTI_SOURCE_VERSION,
        "R3.10 must remain immediately before R3.11"
    );
    let r39 = migrations
        .pop()
        .expect("R3.9 sqlite migration should exist");
    assert_eq!(
        r39.name().version().to_string(),
        R39_UPSTREAM_SOURCE_VERSION,
        "R3.9 must remain immediately before R3.10"
    );
    connection
        .run_migrations(&migrations)
        .expect("sqlite migrations before R3.9 should run");
    r39
}

fn migrate_postgres_to_before_r39(connection: &mut PgConnection) -> Box<dyn Migration<Pg>> {
    connection
        .run_pending_migrations(POSTGRES_CLEAN_BASELINE_MIGRATIONS)
        .expect("postgres clean baseline should run");
    record_postgres_migration_versions(connection, POSTGRES_ARCHIVED_UPGRADE_VERSIONS)
        .expect("postgres archived versions should be recorded");

    let mut migrations = connection
        .pending_migrations(POSTGRES_UPGRADE_MIGRATIONS)
        .expect("pending postgres upgrade migrations should load");
    let r312 = migrations
        .pop()
        .expect("R3.12 postgres migration should exist");
    assert_eq!(
        r312.name().version().to_string(),
        R312_SOURCE_BOUND_REQUEST_PATCH_VARIANTS_VERSION,
        "R3.12 must be the final postgres migration in this release"
    );
    let r311 = migrations
        .pop()
        .expect("R3.11 postgres migration should exist");
    assert_eq!(
        r311.name().version().to_string(),
        R311_MODEL_SOURCE_SELECTION_VERSION,
        "R3.11 must be the final postgres migration in this release"
    );
    let r310 = migrations
        .pop()
        .expect("R3.10 postgres migration should exist");
    assert_eq!(
        r310.name().version().to_string(),
        R310_PROVIDER_MULTI_SOURCE_VERSION,
        "R3.10 must remain immediately before R3.11"
    );
    let r39 = migrations
        .pop()
        .expect("R3.9 postgres migration should exist");
    assert_eq!(
        r39.name().version().to_string(),
        R39_UPSTREAM_SOURCE_VERSION,
        "R3.9 must remain immediately before R3.10"
    );
    connection
        .run_migrations(&migrations)
        .expect("postgres migrations before R3.9 should run");
    r39
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

fn assert_sqlite_request_log_timing_schema(connection: &mut diesel::SqliteConnection) {
    for column in [
        "upstream_request_sent_at",
        "upstream_response_headers_at",
        "upstream_first_body_chunk_at",
        "first_response_body_at",
        "first_token_at",
        "max_upstream_response_idle_ms",
        "completed_at",
    ] {
        assert_eq!(
            sqlite_table_column_count(connection, "request_log", column),
            1,
            "SQLite request_log should contain {column}"
        );
    }
    for column in [
        "response_started_to_client_at",
        "llm_response_first_chunk_at",
        "llm_response_completed_at",
    ] {
        assert_eq!(
            sqlite_table_column_count(connection, "request_log", column),
            0,
            "SQLite request_log should not contain legacy {column}"
        );
    }
}

fn assert_postgres_request_log_protocol_schema(connection: &mut PgConnection) {
    let downstream_column = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'request_log'
           AND column_name = 'downstream_protocol'
           AND is_nullable = 'NO'
           AND udt_name = 'downstream_protocol_enum'",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL downstream protocol column should query")
    .count;
    assert_eq!(downstream_column, 1);

    let upstream_column = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'request_log'
           AND column_name = 'upstream_protocol'
           AND is_nullable = 'YES'
           AND udt_name = 'upstream_protocol_enum'",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL upstream protocol column should query")
    .count;
    assert_eq!(upstream_column, 1);

    let legacy_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'request_log'
           AND column_name IN ('user_api_type', 'llm_api_type')",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL legacy request-log columns should query")
    .count;
    assert_eq!(legacy_columns, 0);

    let downstream_values = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM pg_type t
         JOIN pg_enum e ON e.enumtypid = t.oid
         WHERE t.typname = 'downstream_protocol_enum'
           AND e.enumlabel IN ('OPENAI', 'RESPONSES', 'ANTHROPIC', 'GEMINI')",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL downstream enum should query")
    .count;
    assert_eq!(downstream_values, 4);
    let downstream_extra = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM pg_type t
         JOIN pg_enum e ON e.enumtypid = t.oid
         WHERE t.typname = 'downstream_protocol_enum'
           AND e.enumlabel NOT IN ('OPENAI', 'RESPONSES', 'ANTHROPIC', 'GEMINI')",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL downstream enum extras should query")
    .count;
    assert_eq!(downstream_extra, 0);

    let upstream_values = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM pg_type t
         JOIN pg_enum e ON e.enumtypid = t.oid
         WHERE t.typname = 'upstream_protocol_enum'
           AND e.enumlabel IN ('OPENAI', 'RESPONSES', 'ANTHROPIC', 'GEMINI', 'OLLAMA')",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL upstream enum should query")
    .count;
    assert_eq!(upstream_values, 5);
    let upstream_extra = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM pg_type t
         JOIN pg_enum e ON e.enumtypid = t.oid
         WHERE t.typname = 'upstream_protocol_enum'
           AND e.enumlabel NOT IN ('OPENAI', 'RESPONSES', 'ANTHROPIC', 'GEMINI', 'OLLAMA')",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL upstream enum extras should query")
    .count;
    assert_eq!(upstream_extra, 0);

    let legacy_type = diesel::sql_query(
        "SELECT COUNT(*) AS count FROM pg_type WHERE typname = 'llm_api_type_enum'",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL legacy protocol enum should query")
    .count;
    assert_eq!(legacy_type, 0);
}

fn assert_postgres_request_log_timing_schema(connection: &mut PgConnection) {
    let timing_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'request_log'
           AND column_name IN (
               'upstream_request_sent_at',
               'upstream_response_headers_at',
               'upstream_first_body_chunk_at',
               'first_response_body_at',
               'first_token_at',
               'max_upstream_response_idle_ms',
               'completed_at'
           )
           AND is_nullable = 'YES'
           AND data_type = 'bigint'",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL request timing columns should query")
    .count;
    assert_eq!(timing_columns, 7);

    let legacy_column_count = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'request_log'
           AND column_name IN (
               'response_started_to_client_at',
               'llm_response_first_chunk_at',
               'llm_response_completed_at'
           )",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL legacy timing columns should query")
    .count;
    assert_eq!(legacy_column_count, 0);

    let constraint_count = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM pg_constraint c
         JOIN pg_class t ON t.oid = c.conrelid
         WHERE t.relname = 'request_log'
           AND c.conname = 'chk_request_log_timing_contract'",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL request timing constraint should query")
    .count;
    assert_eq!(constraint_count, 1);
}

fn assert_postgres_request_identity_schema(connection: &mut PgConnection) {
    let canonical_column = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'request_log'
           AND column_name = 'request_id'
           AND is_nullable = 'NO'
           AND data_type = 'text'",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL request_id column should query")
    .count;
    assert_eq!(canonical_column, 1);

    let client_column = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'request_log'
           AND column_name = 'client_request_id'
           AND is_nullable = 'YES'
           AND data_type = 'text'",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL client_request_id column should query")
    .count;
    assert_eq!(client_column, 1);

    let identity_indexes = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM pg_indexes
         WHERE schemaname = 'public'
           AND tablename = 'request_log'
           AND indexname IN (
               'idx_request_log_request_id',
               'idx_request_log_client_request_id'
           )",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL request identity indexes should query")
    .count;
    assert_eq!(identity_indexes, 2);
}

fn assert_sqlite_r39_source_schema(connection: &mut diesel::SqliteConnection) {
    for column in ["endpoint", "use_proxy", "provider_type"] {
        assert_eq!(
            sqlite_table_column_count(connection, "provider", column),
            0,
            "SQLite provider must not retain execution field {column}"
        );
    }
    for column in [
        "id",
        "provider_id",
        "source_key",
        "profile_type",
        "endpoint",
        "use_proxy",
        "deleted_at",
        "created_at",
        "updated_at",
    ] {
        assert_eq!(
            sqlite_table_column_count(connection, "upstream_source", column),
            1,
            "SQLite upstream_source must contain {column}"
        );
    }
    for column in [
        "source_id",
        "source_key_snapshot",
        "source_profile_type_snapshot",
        "source_endpoint_snapshot",
    ] {
        assert_eq!(
            sqlite_table_column_count(connection, "request_log", column),
            1,
            "SQLite request_log must contain {column}"
        );
    }
}

fn assert_postgres_r39_source_schema(connection: &mut PgConnection) {
    let legacy_provider_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'provider'
           AND column_name IN ('endpoint', 'use_proxy', 'provider_type')",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL provider columns should query")
    .count;
    assert_eq!(legacy_provider_columns, 0);

    let source_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'upstream_source'
           AND column_name IN (
               'id', 'provider_id', 'source_key', 'profile_type', 'endpoint',
               'use_proxy', 'deleted_at', 'created_at', 'updated_at'
           )",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL upstream_source columns should query")
    .count;
    assert_eq!(source_columns, 9);

    let request_source_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'request_log'
           AND column_name IN (
               'source_id', 'source_key_snapshot',
               'source_profile_type_snapshot', 'source_endpoint_snapshot'
           )",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL request source columns should query")
    .count;
    assert_eq!(request_source_columns, 4);

    let profile_enum = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM pg_type
         WHERE typname = 'upstream_profile_type_enum'",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL upstream profile enum should query")
    .count;
    assert_eq!(profile_enum, 1);
    let legacy_enum = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM pg_type
         WHERE typname = 'provider_type_enum'",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL legacy provider enum should query")
    .count;
    assert_eq!(legacy_enum, 0);
}

fn assert_sqlite_r310_source_schema(connection: &mut diesel::SqliteConnection) {
    for column in [
        "id",
        "provider_id",
        "profile_type",
        "endpoint",
        "use_proxy",
        "is_enabled",
        "is_default",
        "deleted_at",
        "created_at",
        "updated_at",
    ] {
        assert_eq!(
            sqlite_table_column_count(connection, "upstream_source", column),
            1,
            "SQLite R3.10 upstream_source must contain {column}"
        );
    }
    for column in ["source_key"] {
        assert_eq!(
            sqlite_table_column_count(connection, "upstream_source", column),
            0,
            "SQLite R3.10 upstream_source must not contain {column}"
        );
    }
    for column in [
        "source_id",
        "source_profile_type_snapshot",
        "source_endpoint_snapshot",
    ] {
        assert_eq!(
            sqlite_table_column_count(connection, "request_log", column),
            1,
            "SQLite R3.10 request_log must contain {column}"
        );
    }
    assert_eq!(
        sqlite_table_column_count(connection, "request_log", "source_key_snapshot"),
        0,
        "SQLite R3.10 request_log must not contain source_key_snapshot"
    );

    for index in [
        "idx_upstream_source_provider_default_unique",
        "idx_upstream_source_provider_wire_family_unique",
    ] {
        let count = diesel::sql_query(format!(
            "SELECT COUNT(*) AS count FROM sqlite_master WHERE type = 'index' AND name = '{index}'"
        ))
        .get_result::<CountRow>(connection)
        .expect("SQLite R3.10 source index should query")
        .count;
        assert_eq!(count, 1, "SQLite R3.10 must contain index {index}");
    }
}

fn assert_postgres_r310_source_schema(connection: &mut PgConnection) {
    let source_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'upstream_source'
           AND column_name IN (
               'id', 'provider_id', 'profile_type', 'endpoint', 'use_proxy',
               'is_enabled', 'is_default', 'deleted_at', 'created_at', 'updated_at'
           )",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL R3.10 upstream_source columns should query")
    .count;
    assert_eq!(source_columns, 10);
    let legacy_source_key = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'upstream_source'
           AND column_name = 'source_key'",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL source_key column should query")
    .count;
    assert_eq!(legacy_source_key, 0);

    let request_source_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'request_log'
           AND column_name IN (
               'source_id', 'source_profile_type_snapshot', 'source_endpoint_snapshot'
           )",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL R3.10 request source columns should query")
    .count;
    assert_eq!(request_source_columns, 3);
    let legacy_snapshot = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = 'request_log'
           AND column_name = 'source_key_snapshot'",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL source_key_snapshot column should query")
    .count;
    assert_eq!(legacy_snapshot, 0);

    let indexes = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM pg_indexes
         WHERE schemaname = 'public'
           AND tablename = 'upstream_source'
           AND indexname IN (
               'idx_upstream_source_provider_default_unique',
               'idx_upstream_source_provider_wire_family_unique'
           )",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL R3.10 source indexes should query")
    .count;
    assert_eq!(indexes, 2);
}

fn assert_sqlite_r311_model_source_schema(connection: &mut diesel::SqliteConnection) {
    for column in [
        "id",
        "provider_id",
        "cost_catalog_id",
        "model_name",
        "real_model_name",
        "source_selection_mode",
        "is_enabled",
        "deleted_at",
        "created_at",
        "updated_at",
    ] {
        assert_eq!(
            sqlite_table_column_count(connection, "model", column),
            1,
            "SQLite R3.11 model must contain {column}"
        );
    }
    for column in [
        "supports_streaming",
        "supports_tools",
        "supports_reasoning",
        "supports_image_input",
        "supports_embeddings",
        "supports_rerank",
    ] {
        assert_eq!(
            sqlite_table_column_count(connection, "model", column),
            0,
            "SQLite R3.11 model must not contain {column}"
        );
    }
    for column in [
        "model_id",
        "source_id",
        "is_default",
        "created_at",
        "updated_at",
    ] {
        assert_eq!(
            sqlite_table_column_count(connection, "model_source_binding", column),
            1,
            "SQLite R3.11 binding must contain {column}"
        );
    }
    assert_eq!(
        sqlite_table_column_count(connection, "request_log", "source_selection_reason"),
        1,
        "SQLite R3.11 request_log must contain source_selection_reason"
    );
    for index in [
        "idx_model_source_binding_source_id",
        "idx_model_source_binding_model_default_unique",
    ] {
        let count = diesel::sql_query(format!(
            "SELECT COUNT(*) AS count FROM sqlite_master WHERE type = 'index' AND name = '{index}'"
        ))
        .get_result::<CountRow>(connection)
        .expect("SQLite R3.11 binding index should query")
        .count;
        assert_eq!(count, 1, "SQLite R3.11 must contain index {index}");
    }
}

fn assert_postgres_r311_model_source_schema(connection: &mut PgConnection) {
    let model_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name = 'model'
           AND column_name IN (
               'id', 'provider_id', 'cost_catalog_id', 'model_name',
               'real_model_name', 'source_selection_mode', 'is_enabled',
               'deleted_at', 'created_at', 'updated_at'
           )",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL R3.11 model columns should query")
    .count;
    assert_eq!(model_columns, 10);
    let legacy_model_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name = 'model'
           AND column_name IN (
               'supports_streaming', 'supports_tools', 'supports_reasoning',
               'supports_image_input', 'supports_embeddings', 'supports_rerank'
           )",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL legacy model columns should query")
    .count;
    assert_eq!(legacy_model_columns, 0);

    let binding_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name = 'model_source_binding'
           AND column_name IN ('model_id', 'source_id', 'is_default', 'created_at', 'updated_at')",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL R3.11 binding columns should query")
    .count;
    assert_eq!(binding_columns, 5);

    let request_reason = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name = 'request_log'
           AND column_name = 'source_selection_reason'
           AND is_nullable = 'YES'",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL request selection reason should query")
    .count;
    assert_eq!(request_reason, 1);

    let indexes = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM pg_indexes
         WHERE schemaname = current_schema()
           AND indexname IN (
               'idx_model_source_binding_source_id',
               'idx_model_source_binding_model_default_unique'
           )",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL R3.11 binding indexes should query")
    .count;
    assert_eq!(indexes, 2);
}

fn assert_sqlite_r312_request_patch_schema(connection: &mut diesel::SqliteConnection) {
    for table in [
        "request_patch_rule",
        "reasoning_config",
        "reasoning_config_preset",
        "runtime_feature_config",
    ] {
        let count = diesel::sql_query(format!(
            "SELECT COUNT(*) AS count FROM sqlite_master WHERE type = 'table' AND name = '{table}'"
        ))
        .get_result::<CountRow>(connection)
        .expect("SQLite R3.12 legacy table query should succeed")
        .count;
        if table == "request_patch_rule" {
            assert_eq!(count, 1, "SQLite R3.12 must recreate request_patch_rule");
        } else {
            assert_eq!(count, 0, "SQLite R3.12 must remove legacy table {table}");
        }
    }

    for column in [
        "id",
        "source_id",
        "model_id",
        "suffix",
        "enabled",
        "expose_in_models",
        "deleted_at",
        "created_at",
        "updated_at",
    ] {
        assert_eq!(
            sqlite_table_column_count(connection, "request_patch_variant", column),
            1,
            "SQLite R3.12 variant must contain {column}"
        );
    }
    for column in ["provider_id", "is_enabled"] {
        assert_eq!(
            sqlite_table_column_count(connection, "request_patch_rule", column),
            0,
            "SQLite R3.12 rule must not contain {column}"
        );
    }
    assert_eq!(
        sqlite_table_column_count(connection, "request_patch_rule", "variant_id"),
        1
    );
    assert_eq!(
        sqlite_table_column_count(connection, "request_log", "resolved_patch_suffix"),
        1
    );
    assert_eq!(
        sqlite_table_column_count(connection, "request_log", "resolved_reasoning_suffix"),
        0
    );
    assert_eq!(
        sqlite_table_column_count(connection, "request_log", "resolved_reasoning_preset"),
        0
    );
    for index in [
        "idx_request_patch_variant_source_base_active",
        "idx_request_patch_variant_source_suffix_active",
        "idx_request_patch_variant_model_base_active",
        "idx_request_patch_variant_model_suffix_active",
        "idx_request_patch_rule_variant_identity_active",
    ] {
        let count = diesel::sql_query(format!(
            "SELECT COUNT(*) AS count FROM sqlite_master WHERE type = 'index' AND name = '{index}'"
        ))
        .get_result::<CountRow>(connection)
        .expect("SQLite R3.12 index query should succeed")
        .count;
        assert_eq!(count, 1, "SQLite R3.12 must contain index {index}");
    }
    assert_eq!(
        diesel::sql_query("SELECT COUNT(*) AS count FROM request_patch_variant")
            .get_result::<CountRow>(connection)
            .expect("SQLite R3.12 variant row count should query")
            .count,
        0,
        "R3.12 clean schema must not synthesize Patch Variants"
    );
    assert_eq!(
        diesel::sql_query("SELECT COUNT(*) AS count FROM pragma_foreign_key_check")
            .get_result::<CountRow>(connection)
            .expect("SQLite R3.12 foreign key check should query")
            .count,
        0,
        "SQLite R3.12 schema must have no foreign key violations"
    );
}

fn assert_postgres_r312_request_patch_schema(connection: &mut PgConnection) {
    for table in [
        "reasoning_config",
        "reasoning_config_preset",
        "runtime_feature_config",
    ] {
        let count = diesel::sql_query(format!(
            "SELECT COUNT(*) AS count
             FROM information_schema.tables
             WHERE table_schema = current_schema() AND table_name = '{table}'"
        ))
        .get_result::<CountRow>(connection)
        .expect("PostgreSQL R3.12 legacy table query should succeed")
        .count;
        assert_eq!(
            count, 0,
            "PostgreSQL R3.12 must remove legacy table {table}"
        );
    }

    let variant_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name = 'request_patch_variant'
           AND column_name IN (
               'id', 'source_id', 'model_id', 'suffix', 'enabled',
               'expose_in_models', 'deleted_at', 'created_at', 'updated_at'
           )",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL R3.12 variant columns should query")
    .count;
    assert_eq!(variant_columns, 9);

    let legacy_rule_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name = 'request_patch_rule'
           AND column_name IN ('provider_id', 'model_id', 'is_enabled')",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL R3.12 legacy rule columns should query")
    .count;
    assert_eq!(legacy_rule_columns, 0);

    let rule_variant_column = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name = 'request_patch_rule'
           AND column_name = 'variant_id'",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL R3.12 rule owner should query")
    .count;
    assert_eq!(rule_variant_column, 1);

    let request_suffix_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name = 'request_log'
           AND column_name = 'resolved_patch_suffix'",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL R3.12 request suffix should query")
    .count;
    assert_eq!(request_suffix_columns, 1);

    let legacy_request_suffix_columns = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name = 'request_log'
           AND column_name IN ('resolved_reasoning_suffix', 'resolved_reasoning_preset')",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL R3.12 legacy request suffix should query")
    .count;
    assert_eq!(legacy_request_suffix_columns, 0);

    let indexes = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM pg_indexes
         WHERE schemaname = current_schema()
           AND indexname IN (
               'idx_request_patch_variant_source_base_active',
               'idx_request_patch_variant_source_suffix_active',
               'idx_request_patch_variant_model_base_active',
               'idx_request_patch_variant_model_suffix_active',
               'idx_request_patch_rule_variant_identity_active'
           )",
    )
    .get_result::<CountRow>(connection)
    .expect("PostgreSQL R3.12 indexes should query")
    .count;
    assert_eq!(indexes, 5);
}

fn seed_sqlite_r39_boundary_fixture(connection: &mut diesel::SqliteConnection) {
    connection
        .batch_execute(
            r#"
            INSERT INTO manager_credential (
                manager_id, manager_subject, password_verifier, credential_epoch,
                created_at, updated_at
            ) VALUES (
                0, 'admin', 'preserved-manager-verifier',
                '018fa7d8-6a00-7c9a-8f7e-111111111111', 1, 1
            );
            INSERT INTO manager_auth_instance (
                id, manager_id, manager_subject, current_refresh_jti,
                refresh_generation, session_version, signing_key_id,
                credential_epoch, created_at, last_rotated_at,
                idle_expires_at, absolute_expires_at
            ) VALUES (
                1, 0, 'admin', 'preserved-refresh-jti', 1, 1,
                'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                '018fa7d8-6a00-7c9a-8f7e-111111111111', 1, 2, 3, 4
            );
            INSERT INTO manager_totp_recovery_code (
                code_id, manager_id, code_verifier, created_at
            ) VALUES ('R390', 0, 'preserved-recovery-verifier', 1);

            INSERT INTO api_key (
                id, api_key_hash, key_prefix, key_last4, name, default_action,
                is_enabled, created_at, updated_at
            ) VALUES (
                1, 'preserved-api-key-hash', 'ck-r39', 'r390',
                'Preserved API key', 'ALLOW', 1, 1, 1
            );
            INSERT INTO api_key_rollup_daily (
                api_key_id, day_bucket, currency, request_count,
                total_input_tokens, total_output_tokens, total_reasoning_tokens,
                total_tokens, billed_amount_nanos, last_request_at,
                created_at, updated_at
            ) VALUES (1, 0, 'USD', 1, 2, 3, 1, 6, 100, 10, 1, 1);
            INSERT INTO api_key_rollup_monthly (
                api_key_id, month_bucket, currency, request_count,
                total_input_tokens, total_output_tokens, total_reasoning_tokens,
                total_tokens, billed_amount_nanos, last_request_at,
                created_at, updated_at
            ) VALUES (1, 0, 'USD', 1, 2, 3, 1, 6, 100, 10, 1, 1);

            INSERT INTO cost_catalogs (
                id, name, description, created_at, updated_at
            ) VALUES (100, 'Preserved catalog', 'R3.9 boundary fixture', 1, 1);
            INSERT INTO cost_catalog_versions (
                id, catalog_id, version, currency, source, effective_from,
                is_archived, is_enabled, created_at, updated_at
            ) VALUES (101, 100, 'r39-fixture', 'USD', 'test', 1, 0, 1, 1, 1);

            INSERT INTO provider (
                id, provider_key, name, endpoint, use_proxy, is_enabled,
                created_at, updated_at, provider_type, provider_api_key_mode
            ) VALUES (
                10, 'cleared-provider', 'Cleared Provider',
                'https://provider.example/v1', 0, 1, 1, 1, 'OPENAI', 'QUEUE'
            );
            INSERT INTO provider_api_key (
                id, provider_id, description, key_prefix, key_last4,
                secret_ciphertext, secret_nonce, secret_format_version,
                secret_key_fingerprint, secret_hmac,
                is_enabled, created_at, updated_at
            ) VALUES (
                11, 10, 'cleared provider credential', 'sk-r39', 'r390',
                X'01', X'000000000000000000000000000000000000000000000000', 1,
                'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                1, 1, 1
            );
            INSERT INTO model (
                id, provider_id, cost_catalog_id, model_name, real_model_name,
                supports_streaming, supports_tools, supports_reasoning,
                supports_image_input, supports_embeddings, supports_rerank,
                is_enabled, created_at, updated_at
            ) VALUES (
                12, 10, 100, 'cleared-model', 'cleared-real-model',
                1, 1, 1, 1, 0, 0, 1, 1, 1
            );
            INSERT INTO api_key_acl_rule (
                id, api_key_id, effect, scope, provider_id, priority,
                is_enabled, description, created_at, updated_at
            ) VALUES (13, 1, 'ALLOW', 'PROVIDER', 10, 0, 1, 'cleared ACL', 1, 1);
            INSERT INTO request_patch_rule (
                id, provider_id, placement, target, operation, value_json,
                description, is_enabled, created_at, updated_at
            ) VALUES (
                14, 10, 'HEADER', 'x-r39-fixture', 'SET', '"value"',
                'cleared patch', 1, 1, 1
            );
            INSERT INTO reasoning_config (
                id, scope_kind, provider_id, mode, family_key, created_at, updated_at
            ) VALUES (
                15, 'provider', 10, 'custom',
                'openai_chat_reasoning_effort', 1, 1
            );
            INSERT INTO reasoning_config_preset (
                id, config_id, preset_key, expose_in_models, is_enabled,
                created_at, updated_at
            ) VALUES (16, 15, 'high', 1, 1, 1, 1);
            INSERT INTO runtime_feature_config (
                id, scope_kind, provider_id, feature_key, enabled,
                created_at, updated_at
            ) VALUES (
                17, 'provider', 10, 'openai_reasoning_content_repair', 1, 1, 1
            );

            INSERT INTO request_log (
                id, request_id, api_key_id, requested_model_name,
                downstream_protocol, overall_status, request_received_at,
                upstream_request_sent_at, upstream_response_headers_at,
                upstream_first_body_chunk_at, first_response_body_at,
                first_token_at, max_upstream_response_idle_ms, completed_at,
                is_stream, provider_id, provider_api_key_id, model_id,
                provider_key_snapshot, provider_name_snapshot,
                model_name_snapshot, real_model_name_snapshot,
                upstream_protocol, cost_catalog_id, cost_catalog_version_id,
                created_at, updated_at
            ) VALUES (
                20, '018fa7d8-6a00-4c9a-8f7e-222222222222', 1, 'cleared-model',
                'OPENAI', 'SUCCESS', 10, 11, 12, 13, 13, 14, 1, 15,
                1, 10, 11, 12, 'cleared-provider', 'Cleared Provider',
                'cleared-model', 'cleared-real-model', 'OPENAI', 100, 101,
                10, 15
            );
            INSERT INTO metric_ingested_request_log (
                request_log_id, request_received_at, completed_at, ingested_at
            ) VALUES (20, 10, 15, 16);
            INSERT INTO metric_request_rollup_minute (
                bucket_start_ms, scope_type, scope_id, scope_label,
                request_count, success_count, error_count, cancelled_count,
                time_to_first_response_body_sum_ms,
                time_to_first_response_body_count, ttft_sum_ms, ttft_count,
                total_latency_sum_ms, total_latency_count,
                input_tokens, output_tokens, reasoning_tokens, total_tokens,
                created_at, updated_at
            ) VALUES (
                0, 'provider', '10', 'Cleared Provider',
                1, 1, 0, 0, 3, 1, 4, 1, 5, 1, 2, 3, 1, 6, 1, 1
            );
            INSERT INTO metric_http_status_rollup_minute (
                bucket_start_ms, scope_type, scope_id, http_status,
                count, created_at, updated_at
            ) VALUES (0, 'provider', '10', 200, 1, 1, 1);
            INSERT INTO metric_cost_rollup_minute (
                bucket_start_ms, scope_type, scope_id, currency,
                amount_nanos, created_at, updated_at
            ) VALUES (0, 'provider', '10', 'USD', 100, 1, 1);
            "#,
        )
        .expect("SQLite pre-R3.9 boundary fixture should insert");
}

fn seed_postgres_r39_boundary_fixture(connection: &mut PgConnection) {
    connection
        .batch_execute(
            r#"
            INSERT INTO manager_credential (
                manager_id, manager_subject, password_verifier, credential_epoch,
                created_at, updated_at
            ) VALUES (
                0, 'admin', 'preserved-manager-verifier',
                '018fa7d8-6a00-7c9a-8f7e-111111111111', 1, 1
            );
            INSERT INTO manager_auth_instance (
                id, manager_id, manager_subject, current_refresh_jti,
                refresh_generation, session_version, signing_key_id,
                credential_epoch, created_at, last_rotated_at,
                idle_expires_at, absolute_expires_at
            ) VALUES (
                1, 0, 'admin', 'preserved-refresh-jti', 1, 1,
                'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                '018fa7d8-6a00-7c9a-8f7e-111111111111', 1, 2, 3, 4
            );
            INSERT INTO manager_totp_recovery_code (
                code_id, manager_id, code_verifier, created_at
            ) VALUES ('R390', 0, 'preserved-recovery-verifier', 1);

            INSERT INTO api_key (
                id, api_key_hash, key_prefix, key_last4, name, default_action,
                is_enabled, created_at, updated_at
            ) VALUES (
                1, 'preserved-api-key-hash', 'ck-r39', 'r390',
                'Preserved API key', 'ALLOW', TRUE, 1, 1
            );
            INSERT INTO api_key_rollup_daily (
                api_key_id, day_bucket, currency, request_count,
                total_input_tokens, total_output_tokens, total_reasoning_tokens,
                total_tokens, billed_amount_nanos, last_request_at,
                created_at, updated_at
            ) VALUES (1, 0, 'USD', 1, 2, 3, 1, 6, 100, 10, 1, 1);
            INSERT INTO api_key_rollup_monthly (
                api_key_id, month_bucket, currency, request_count,
                total_input_tokens, total_output_tokens, total_reasoning_tokens,
                total_tokens, billed_amount_nanos, last_request_at,
                created_at, updated_at
            ) VALUES (1, 0, 'USD', 1, 2, 3, 1, 6, 100, 10, 1, 1);

            INSERT INTO cost_catalogs (
                id, name, description, created_at, updated_at
            ) VALUES (100, 'Preserved catalog', 'R3.9 boundary fixture', 1, 1);
            INSERT INTO cost_catalog_versions (
                id, catalog_id, version, currency, source, effective_from,
                is_archived, is_enabled, created_at, updated_at
            ) VALUES (101, 100, 'r39-fixture', 'USD', 'test', 1, FALSE, TRUE, 1, 1);

            INSERT INTO provider (
                id, provider_key, name, endpoint, use_proxy, is_enabled,
                created_at, updated_at, provider_type, provider_api_key_mode
            ) VALUES (
                10, 'cleared-provider', 'Cleared Provider',
                'https://provider.example/v1', FALSE, TRUE, 1, 1, 'OPENAI', 'QUEUE'
            );
            INSERT INTO provider_api_key (
                id, provider_id, description, key_prefix, key_last4,
                secret_ciphertext, secret_nonce, secret_format_version,
                secret_key_fingerprint, secret_hmac,
                is_enabled, created_at, updated_at
            ) VALUES (
                11, 10, 'cleared provider credential', 'sk-r39', 'r390',
                decode('01', 'hex'), decode('000000000000000000000000000000000000000000000000', 'hex'), 1,
                'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                TRUE, 1, 1
            );
            INSERT INTO model (
                id, provider_id, cost_catalog_id, model_name, real_model_name,
                supports_streaming, supports_tools, supports_reasoning,
                supports_image_input, supports_embeddings, supports_rerank,
                is_enabled, created_at, updated_at
            ) VALUES (
                12, 10, 100, 'cleared-model', 'cleared-real-model',
                TRUE, TRUE, TRUE, TRUE, FALSE, FALSE, TRUE, 1, 1
            );
            INSERT INTO api_key_acl_rule (
                id, api_key_id, effect, scope, provider_id, priority,
                is_enabled, description, created_at, updated_at
            ) VALUES (13, 1, 'ALLOW', 'PROVIDER', 10, 0, TRUE, 'cleared ACL', 1, 1);
            INSERT INTO request_patch_rule (
                id, provider_id, placement, target, operation, value_json,
                description, is_enabled, created_at, updated_at
            ) VALUES (
                14, 10, 'HEADER', 'x-r39-fixture', 'SET', '"value"',
                'cleared patch', TRUE, 1, 1
            );
            INSERT INTO reasoning_config (
                id, scope_kind, provider_id, mode, family_key, created_at, updated_at
            ) VALUES (
                15, 'provider', 10, 'custom',
                'openai_chat_reasoning_effort', 1, 1
            );
            INSERT INTO reasoning_config_preset (
                id, config_id, preset_key, expose_in_models, is_enabled,
                created_at, updated_at
            ) VALUES (16, 15, 'high', TRUE, TRUE, 1, 1);
            INSERT INTO runtime_feature_config (
                id, scope_kind, provider_id, feature_key, enabled,
                created_at, updated_at
            ) VALUES (
                17, 'provider', 10, 'openai_reasoning_content_repair', TRUE, 1, 1
            );

            INSERT INTO request_log (
                id, request_id, api_key_id, requested_model_name,
                downstream_protocol, overall_status, request_received_at,
                upstream_request_sent_at, upstream_response_headers_at,
                upstream_first_body_chunk_at, first_response_body_at,
                first_token_at, max_upstream_response_idle_ms, completed_at,
                is_stream, provider_id, provider_api_key_id, model_id,
                provider_key_snapshot, provider_name_snapshot,
                model_name_snapshot, real_model_name_snapshot,
                upstream_protocol, cost_catalog_id, cost_catalog_version_id,
                created_at, updated_at
            ) VALUES (
                20, '018fa7d8-6a00-4c9a-8f7e-222222222222', 1, 'cleared-model',
                'OPENAI', 'SUCCESS', 10, 11, 12, 13, 13, 14, 1, 15,
                TRUE, 10, 11, 12, 'cleared-provider', 'Cleared Provider',
                'cleared-model', 'cleared-real-model', 'OPENAI', 100, 101,
                10, 15
            );
            INSERT INTO metric_ingested_request_log (
                request_log_id, request_received_at, completed_at, ingested_at
            ) VALUES (20, 10, 15, 16);
            INSERT INTO metric_request_rollup_minute (
                bucket_start_ms, scope_type, scope_id, scope_label,
                request_count, success_count, error_count, cancelled_count,
                time_to_first_response_body_sum_ms,
                time_to_first_response_body_count, ttft_sum_ms, ttft_count,
                total_latency_sum_ms, total_latency_count,
                input_tokens, output_tokens, reasoning_tokens, total_tokens,
                created_at, updated_at
            ) VALUES (
                0, 'provider', '10', 'Cleared Provider',
                1, 1, 0, 0, 3, 1, 4, 1, 5, 1, 2, 3, 1, 6, 1, 1
            );
            INSERT INTO metric_http_status_rollup_minute (
                bucket_start_ms, scope_type, scope_id, http_status,
                count, created_at, updated_at
            ) VALUES (0, 'provider', '10', 200, 1, 1, 1);
            INSERT INTO metric_cost_rollup_minute (
                bucket_start_ms, scope_type, scope_id, currency,
                amount_nanos, created_at, updated_at
            ) VALUES (0, 'provider', '10', 'USD', 100, 1, 1);
            "#,
        )
        .expect("PostgreSQL pre-R3.9 boundary fixture should insert");
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
    assert_sqlite_request_log_timing_schema(&mut connection);
    assert_sqlite_r310_source_schema(&mut connection);
    assert_sqlite_r311_model_source_schema(&mut connection);
    assert_sqlite_r312_request_patch_schema(&mut connection);

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
fn sqlite_r310_source_migration_preserves_rows_and_enforces_source_contract() {
    let (_temp_dir, mut connection) =
        open_test_sqlite_connection("r310-provider-multi-source.sqlite");
    let r39 = migrate_sqlite_to_before_r39(&mut connection);

    connection
        .run_migration(r39.as_ref())
        .expect("R3.9 sqlite migration should run before R3.10 fixture");
    connection
        .batch_execute(
            "INSERT INTO api_key (
                id, api_key_hash, key_prefix, key_last4, name, default_action,
                is_enabled, created_at, updated_at
             ) VALUES (
                1, 'r310-api-key-hash', 'r310', '0001', 'R3.10 API key', 'ALLOW', 1, 1, 1
             );
             INSERT INTO provider (
                id, provider_key, name, is_enabled, created_at, updated_at,
                provider_api_key_mode
             ) VALUES (
                30, 'r310-provider', 'R3.10 Provider', 1, 1, 1, 'QUEUE'
             );
             INSERT INTO upstream_source (
                id, provider_id, source_key, profile_type, endpoint, use_proxy,
                created_at, updated_at
             ) VALUES (
                31, 30, 'primary', 'OPENAI', 'https://source.example/v1', 0, 1, 1
             );
             INSERT INTO model (
                id, provider_id, cost_catalog_id, model_name, real_model_name,
                supports_streaming, supports_tools, supports_reasoning,
                supports_image_input, supports_embeddings, supports_rerank,
                is_enabled, created_at, updated_at
             ) VALUES (
                33, 30, NULL, 'r310-model', 'r310-real-model',
                1, 1, 1, 1, 1, 1, 1, 1, 1
             );
             INSERT INTO request_log (
                id, request_id, api_key_id, requested_model_name,
                downstream_protocol, overall_status, request_received_at,
                is_stream, provider_id, source_id, provider_key_snapshot,
                provider_name_snapshot, source_key_snapshot,
                source_profile_type_snapshot, source_endpoint_snapshot,
                upstream_protocol, created_at, updated_at
             ) VALUES (
                32, '018fa7d8-6a00-4c9a-8f7e-333333333333', 1,
                'r310-model', 'OPENAI', 'SUCCESS', 10, 0, 30, 31,
                'r310-provider', 'R3.10 Provider', 'primary', 'OPENAI',
                'https://source.example/v1', 'OPENAI', 10, 10
             );
             INSERT INTO metric_request_rollup_minute (
                bucket_start_ms, scope_type, scope_id, scope_label,
                request_count, success_count, error_count, cancelled_count,
                time_to_first_response_body_sum_ms,
                time_to_first_response_body_count, ttft_sum_ms, ttft_count,
                total_latency_sum_ms, total_latency_count,
                input_tokens, output_tokens, reasoning_tokens, total_tokens,
                created_at, updated_at
             ) VALUES (
                0, 'source', '31', 'primary', 1, 1, 0, 0,
                1, 1, 1, 1, 1, 1, 1, 1, 0, 2, 1, 1
             );
             INSERT INTO metric_cost_rollup_minute (
                bucket_start_ms, scope_type, scope_id,
                currency, amount_nanos, created_at, updated_at
             ) VALUES (0, 'source', '31', 'USD', 100, 1, 1);",
        )
        .expect("R3.9 source fixture should insert");

    let mut pending = connection
        .pending_migrations(SQLITE_UPGRADE_MIGRATIONS)
        .expect("pending sqlite migrations should load");
    let r312 = pending
        .pop()
        .expect("R3.12 sqlite migration should be pending");
    assert_eq!(
        r312.name().version().to_string(),
        R312_SOURCE_BOUND_REQUEST_PATCH_VARIANTS_VERSION
    );
    let r311 = pending
        .pop()
        .expect("R3.11 sqlite migration should be pending");
    let r310 = pending
        .pop()
        .expect("R3.10 sqlite migration should be pending");
    assert_eq!(
        r310.name().version().to_string(),
        R310_PROVIDER_MULTI_SOURCE_VERSION
    );
    connection
        .run_migration(r310.as_ref())
        .expect("R3.10 sqlite migration should run");

    assert_sqlite_r310_source_schema(&mut connection);
    assert_eq!(
        diesel::sql_query("SELECT COUNT(*) AS count FROM upstream_source WHERE id = 31")
            .get_result::<CountRow>(&mut connection)
            .expect("migrated source should query")
            .count,
        1
    );
    connection
        .run_migration(r311.as_ref())
        .expect("R3.11 sqlite migration should run");
    assert_sqlite_r311_model_source_schema(&mut connection);
    let model_snapshot = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM model
         WHERE id = 33
           AND model_name = 'r310-model'
           AND real_model_name = 'r310-real-model'
           AND source_selection_mode = 'INHERIT_ALL'",
    )
    .get_result::<CountRow>(&mut connection)
    .expect("migrated model snapshot should query")
    .count;
    assert_eq!(
        model_snapshot, 1,
        "model identity and mode must be preserved"
    );
    assert_eq!(
        diesel::sql_query("SELECT COUNT(*) AS count FROM model_source_binding")
            .get_result::<CountRow>(&mut connection)
            .expect("new binding table should query")
            .count,
        0,
        "R3.11 must not synthesize bindings from existing Sources"
    );
    connection
        .batch_execute(
            "INSERT INTO model_source_binding (
                model_id, source_id, is_default, created_at, updated_at
             ) VALUES (33, 31, 1, 20, 20);",
        )
        .expect("valid model Source binding should insert");
    assert!(
        connection
            .batch_execute(
                "INSERT INTO model_source_binding (
                    model_id, source_id, is_default, created_at, updated_at
                 ) VALUES (33, 31, 1, 21, 21);",
            )
            .is_err(),
        "a model may have only one default binding"
    );
    connection
        .batch_execute(
            "UPDATE request_log
             SET resolved_reasoning_suffix = 'legacy-high'
             WHERE id = 32;",
        )
        .expect("historical reasoning suffix should be writable before R3.12");
    connection
        .run_migration(r312.as_ref())
        .expect("R3.12 sqlite migration should run");
    assert_sqlite_r312_request_patch_schema(&mut connection);
    let migrated_suffix = diesel::sql_query(
        "SELECT resolved_patch_suffix AS value
         FROM request_log WHERE id = 32",
    )
    .get_result::<TextValueRow>(&mut connection)
    .expect("migrated request patch suffix should query");
    assert_eq!(migrated_suffix.value, "legacy-high");
    assert!(
        connection
            .batch_execute(
                "INSERT INTO request_log (
                    id, request_id, api_key_id, downstream_protocol,
                    overall_status, request_received_at, is_stream,
                    source_selection_reason, created_at, updated_at
                 ) VALUES (
                    34, '018fa7d8-6a00-4c9a-8f7e-444444444444', 1,
                    'OPENAI', 'SUCCESS', 20, 0,
                    'invalid_reason', 20, 20
                 );",
            )
            .is_err(),
        "request log selection reason must use the stable enum values"
    );
    let snapshot = diesel::sql_query(
        "SELECT source_profile_type_snapshot AS value
         FROM request_log WHERE id = 32",
    )
    .get_result::<TextValueRow>(&mut connection)
    .expect("migrated request log snapshot should query");
    assert_eq!(snapshot.value, "OPENAI");
    let source_rollup = diesel::sql_query(
        "SELECT scope_label AS value
         FROM metric_request_rollup_minute
         WHERE scope_type = 'source' AND scope_id = '31'",
    )
    .get_result::<TextValueRow>(&mut connection)
    .expect("migrated source metric rollup should query");
    assert_eq!(source_rollup.value, "OPENAI");
    assert_eq!(
        diesel::sql_query(
            "SELECT COUNT(*) AS count
             FROM metric_cost_rollup_minute
             WHERE scope_type = 'source' AND scope_id = '31'",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("migrated source cost rollup should query")
        .count,
        1,
        "cost rollup identity and value should remain intact"
    );

    connection
        .batch_execute(
            "INSERT INTO provider (
                id, provider_key, name, is_enabled, created_at, updated_at,
                provider_api_key_mode
             ) VALUES (40, 'constraint-provider', 'Constraint Provider', 1, 1, 1, 'QUEUE');
             INSERT INTO upstream_source (
                id, provider_id, profile_type, endpoint, use_proxy,
                is_enabled, is_default, created_at, updated_at
             ) VALUES (41, 40, 'OPENAI', 'https://one.example/v1', 0, 1, 1, 1, 1);",
        )
        .expect("constraint provider and first source should insert");
    assert!(
        connection
            .batch_execute(
                "INSERT INTO upstream_source (
                    id, provider_id, profile_type, endpoint, use_proxy,
                    is_enabled, is_default, created_at, updated_at
                 ) VALUES (42, 40, 'VERTEX_OPENAI', 'https://two.example/v1', 0, 0, 0, 1, 1);"
            )
            .is_err(),
        "disabled sources must still reserve their active wire family"
    );
    assert!(
        connection
            .batch_execute(
                "INSERT INTO upstream_source (
                    id, provider_id, profile_type, endpoint, use_proxy,
                    is_enabled, is_default, created_at, updated_at
                 ) VALUES (43, 40, 'RESPONSES', 'https://responses.example/v1', 0, 1, 0, 1, 1);"
            )
            .is_ok(),
        "different source families should coexist"
    );
    assert!(
        connection
            .batch_execute(
                "INSERT INTO upstream_source (
                    id, provider_id, profile_type, endpoint, use_proxy,
                    is_enabled, is_default, created_at, updated_at
                 ) VALUES (44, 40, 'ANTHROPIC', 'https://anthropic.example/v1', 0, 1, 1, 1, 1);"
            )
            .is_err(),
        "a provider may have only one active default source"
    );
    assert!(
        connection
            .batch_execute(
                "INSERT INTO upstream_source (
                    id, provider_id, profile_type, endpoint, use_proxy,
                    is_enabled, is_default, created_at, updated_at
                 ) VALUES (45, 40, 'GEMINI', 'https://gemini.example/v1', 0, 0, 1, 1, 1);"
            )
            .is_err(),
        "a disabled source may not be the default"
    );
    connection
        .batch_execute(
            "UPDATE upstream_source
             SET deleted_at = 2, is_enabled = 0, is_default = 0
             WHERE id = 41;
             INSERT INTO upstream_source (
                id, provider_id, profile_type, endpoint, use_proxy,
                is_enabled, is_default, created_at, updated_at
             ) VALUES (46, 40, 'VERTEX_OPENAI', 'https://replacement.example/v1', 0, 1, 1, 1, 1);",
        )
        .expect("soft deletion should release the source family and default slot");
    assert_eq!(
        diesel::sql_query("SELECT COUNT(*) AS count FROM pragma_foreign_key_check")
            .get_result::<CountRow>(&mut connection)
            .expect("SQLite foreign key check should run")
            .count,
        0
    );
}

#[test]
fn sqlite_r39_destructive_upgrade_clears_execution_domain_and_preserves_governance() {
    let (_temp_dir, mut connection) = open_test_sqlite_connection("r39-destructive-upgrade.sqlite");
    let r39 = migrate_sqlite_to_before_r39(&mut connection);
    seed_sqlite_r39_boundary_fixture(&mut connection);

    for table in [
        "provider",
        "provider_api_key",
        "model",
        "api_key_acl_rule",
        "request_patch_rule",
        "reasoning_config",
        "reasoning_config_preset",
        "runtime_feature_config",
        "request_log",
        "metric_ingested_request_log",
        "metric_request_rollup_minute",
        "metric_http_status_rollup_minute",
        "metric_cost_rollup_minute",
    ] {
        let count = diesel::sql_query(format!("SELECT COUNT(*) AS count FROM {table}"))
            .get_result::<CountRow>(&mut connection)
            .expect("SQLite pre-R3.9 fixture count should query")
            .count;
        assert_eq!(count, 1, "SQLite fixture should populate {table}");
    }

    connection
        .run_migration(r39.as_ref())
        .expect("R3.9 sqlite migration should run");
    assert_sqlite_r39_source_schema(&mut connection);

    for table in [
        "provider",
        "upstream_source",
        "provider_api_key",
        "model",
        "api_key_acl_rule",
        "request_patch_rule",
        "reasoning_config",
        "reasoning_config_preset",
        "runtime_feature_config",
        "request_log",
        "metric_ingested_request_log",
        "metric_request_rollup_minute",
        "metric_http_status_rollup_minute",
        "metric_cost_rollup_minute",
    ] {
        let count = diesel::sql_query(format!("SELECT COUNT(*) AS count FROM {table}"))
            .get_result::<CountRow>(&mut connection)
            .expect("SQLite post-R3.9 cleared count should query")
            .count;
        assert_eq!(count, 0, "R3.9 must clear SQLite table {table}");
    }
    for table in [
        "manager_credential",
        "manager_auth_instance",
        "manager_totp_recovery_code",
        "api_key",
        "api_key_rollup_daily",
        "api_key_rollup_monthly",
        "cost_catalogs",
        "cost_catalog_versions",
    ] {
        let count = diesel::sql_query(format!("SELECT COUNT(*) AS count FROM {table}"))
            .get_result::<CountRow>(&mut connection)
            .expect("SQLite post-R3.9 preserved count should query")
            .count;
        assert_eq!(count, 1, "R3.9 must preserve SQLite table {table}");
    }

    connection
        .transaction::<(), diesel::result::Error, _>(|connection| {
            connection.batch_execute(
                "INSERT INTO provider (
                    id, provider_key, name, is_enabled, created_at, updated_at,
                    provider_api_key_mode
                 ) VALUES (30, 'new-provider', 'New Provider', 1, 1, 1, 'QUEUE');
                 INSERT INTO upstream_source (
                    id, provider_id, source_key, profile_type, endpoint, use_proxy,
                    created_at, updated_at
                 ) VALUES (
                    31, 30, 'primary', 'OPENAI', 'https://new.example/v1', 0, 1, 1
                 );",
            )
        })
        .expect("SQLite Provider and primary Source should create atomically");
    assert!(
        connection
            .batch_execute(
                "INSERT INTO upstream_source (
                    id, provider_id, source_key, profile_type, endpoint, use_proxy,
                    created_at, updated_at
                 ) VALUES (
                    32, 30, 'primary', 'GEMINI', 'https://second.example/v1', 0, 1, 1
                 );"
            )
            .is_err(),
        "SQLite must reject a second active Source"
    );
    connection
        .batch_execute(
            "INSERT INTO provider (
                id, provider_key, name, is_enabled, created_at, updated_at,
                provider_api_key_mode
             ) VALUES (40, 'invalid-source-provider', 'Invalid Source', 1, 1, 1, 'QUEUE');",
        )
        .expect("SQLite constraint fixture provider should insert");
    assert!(
        connection
            .batch_execute(
                "INSERT INTO upstream_source (
                    id, provider_id, source_key, profile_type, endpoint, use_proxy,
                    created_at, updated_at
                 ) VALUES (
                    41, 40, 'secondary', 'OPENAI', 'https://invalid.example/v1', 0, 1, 1
                 );"
            )
            .is_err(),
        "SQLite must reject a non-primary source_key"
    );
    let source_count = diesel::sql_query("SELECT COUNT(*) AS count FROM upstream_source")
        .get_result::<CountRow>(&mut connection)
        .expect("SQLite Source count should query")
        .count;
    assert_eq!(source_count, 1);
    let foreign_key_violations =
        diesel::sql_query("SELECT COUNT(*) AS count FROM pragma_foreign_key_check")
            .get_result::<CountRow>(&mut connection)
            .expect("SQLite foreign key check should run")
            .count;
    assert_eq!(foreign_key_violations, 0);
}

#[test]
fn sqlite_request_log_protocol_boundary_upgrade_clears_history_and_enforces_domains() {
    let (_temp_dir, mut connection) =
        open_test_sqlite_connection("request-log-protocol-upgrade.sqlite");
    run_sqlite_migrations(&mut connection).expect("sqlite migrations should run");

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-07-29-090000_request_log_protocol_boundaries/down.sql"
        ))
        .expect("request log protocol down migration should run");
    connection
        .batch_execute(
            "INSERT INTO api_key (
                id, api_key_hash, key_prefix, key_last4, name, description,
                default_action, is_enabled, expires_at, rate_limit_rpm,
                max_concurrent_requests, quota_daily_requests, quota_daily_tokens,
                quota_monthly_tokens, budget_daily_nanos, budget_daily_currency,
                budget_monthly_nanos, budget_monthly_currency, deleted_at,
                created_at, updated_at
            ) VALUES (
                9001, 'protocol-boundary-hash', 'ck-test', '9001',
                'Protocol migration key', NULL, 'ALLOW', 1, NULL, NULL,
                NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 1, 1
            );
            INSERT INTO request_log (
                id, api_key_id, user_api_type, llm_api_type, overall_status,
                request_received_at, is_stream, created_at, updated_at
            ) VALUES (
                9002, 9001, 'OLLAMA', 'GEMINI_OPENAI', 'SUCCESS',
                1, 0, 1, 1
            );",
        )
        .expect("legacy mixed-direction log should insert");

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-07-29-090000_request_log_protocol_boundaries/up.sql"
        ))
        .expect("request log protocol up migration should run");

    assert_eq!(
        sqlite_table_column_count(&mut connection, "request_log", "user_api_type"),
        0
    );
    assert_eq!(
        sqlite_table_column_count(&mut connection, "request_log", "llm_api_type"),
        0
    );
    assert_eq!(
        sqlite_table_column_count(&mut connection, "request_log", "downstream_protocol"),
        1
    );
    assert_eq!(
        sqlite_table_column_count(&mut connection, "request_log", "upstream_protocol"),
        1
    );
    let rows = diesel::sql_query("SELECT COUNT(*) AS count FROM request_log")
        .get_result::<CountRow>(&mut connection)
        .expect("request log count should query")
        .count;
    assert_eq!(
        rows, 0,
        "protocol migration must discard request-log history"
    );

    connection
        .batch_execute(
            "INSERT INTO request_log (
                id, api_key_id, downstream_protocol, upstream_protocol,
                overall_status, request_received_at, is_stream, created_at, updated_at
            ) VALUES (
                9003, 9001, 'RESPONSES', 'OLLAMA', 'SUCCESS', 2, 0, 2, 2
            );
            INSERT INTO request_log (
                id, api_key_id, downstream_protocol, upstream_protocol,
                overall_status, request_received_at, is_stream, created_at, updated_at
            ) VALUES (
                9004, 9001, 'GEMINI', NULL, 'SUCCESS', 3, 0, 3, 3
            );",
        )
        .expect("valid directional protocol values should insert");
    assert!(
        connection
            .batch_execute(
                "INSERT INTO request_log (
                    id, api_key_id, downstream_protocol, overall_status,
                    request_received_at, is_stream, created_at, updated_at
                ) VALUES (
                    9005, 9001, 'OLLAMA', 'SUCCESS', 4, 0, 4, 4
                );"
            )
            .is_err(),
        "Ollama must be rejected as a downstream protocol"
    );
    assert!(
        connection
            .batch_execute(
                "INSERT INTO request_log (
                    id, api_key_id, downstream_protocol, upstream_protocol,
                    overall_status, request_received_at, is_stream, created_at, updated_at
                ) VALUES (
                    9006, 9001, 'OPENAI', 'GEMINI_OPENAI',
                    'SUCCESS', 5, 0, 5, 5
                );"
            )
            .is_err(),
        "provider dialects must be rejected as upstream protocols"
    );
    let foreign_key_violations =
        diesel::sql_query("SELECT COUNT(*) AS count FROM pragma_foreign_key_check")
            .get_result::<CountRow>(&mut connection)
            .expect("foreign key check should run")
            .count;
    assert_eq!(foreign_key_violations, 0);
}

#[test]
fn sqlite_request_identity_upgrade_is_destructive_constrained_and_preserves_rollups() {
    let (_temp_dir, mut connection) =
        open_test_sqlite_connection("request-log-identity-upgrade.sqlite");
    run_sqlite_migrations(&mut connection).expect("sqlite migrations should run");

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-07-30-090000_request_log_request_identity/down.sql"
        ))
        .expect("request identity down migration should run");
    connection
        .batch_execute(
            "INSERT INTO api_key (
                id, api_key_hash, key_prefix, key_last4, name, description,
                default_action, is_enabled, expires_at, rate_limit_rpm,
                max_concurrent_requests, quota_daily_requests, quota_daily_tokens,
                quota_monthly_tokens, budget_daily_nanos, budget_daily_currency,
                budget_monthly_nanos, budget_monthly_currency, deleted_at,
                created_at, updated_at
            ) VALUES (
                9101, 'request-identity-hash', 'ck-test', '9101',
                'Request identity migration key', NULL, 'ALLOW', 1, NULL, NULL,
                NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 1, 1
            );
            INSERT INTO request_log (
                id, api_key_id, downstream_protocol, overall_status,
                request_received_at, is_stream, created_at, updated_at
            ) VALUES (
                9102, 9101, 'OPENAI', 'SUCCESS', 1, 0, 1, 1
            );
            INSERT INTO metric_ingested_request_log (
                request_log_id, request_received_at, completed_at, ingested_at
            ) VALUES (9102, 1, 1, 1);
            INSERT INTO metric_request_rollup_minute (
                bucket_start_ms, scope_type, scope_id, scope_label,
                request_count, success_count, error_count, cancelled_count,
                time_to_first_response_body_sum_ms, time_to_first_response_body_count,
                ttft_sum_ms, ttft_count,
                total_latency_sum_ms, total_latency_count,
                input_tokens, output_tokens, reasoning_tokens, total_tokens,
                created_at, updated_at
            ) VALUES (
                0, 'global', 'global', NULL,
                1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 2, 1, 1
            );",
        )
        .expect("pre-identity request data should insert");

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-07-30-090000_request_log_request_identity/up.sql"
        ))
        .expect("request identity up migration should run");

    assert_eq!(
        sqlite_table_column_count(&mut connection, "request_log", "request_id"),
        1
    );
    assert_eq!(
        sqlite_table_column_count(&mut connection, "request_log", "client_request_id"),
        1
    );
    for table in ["request_log", "metric_ingested_request_log"] {
        let count = diesel::sql_query(format!("SELECT COUNT(*) AS count FROM {table}"))
            .get_result::<CountRow>(&mut connection)
            .expect("destructive migration table count should query")
            .count;
        assert_eq!(count, 0, "{table} must be cleared");
    }
    let rollup_count =
        diesel::sql_query("SELECT COUNT(*) AS count FROM metric_request_rollup_minute")
            .get_result::<CountRow>(&mut connection)
            .expect("rollup count should query")
            .count;
    assert_eq!(rollup_count, 1, "minute rollups must be preserved");

    connection
        .batch_execute(
            "INSERT INTO request_log (
                id, request_id, client_request_id, api_key_id,
                downstream_protocol, overall_status, request_received_at,
                is_stream, created_at, updated_at
            ) VALUES
                (9103, '018fa7d8-6a00-4c9a-8f7e-111111111111', 'caller.same-1',
                 9101, 'OPENAI', 'SUCCESS', 2, 0, 2, 2),
                (9104, '018fa7d8-6a00-4c9a-9f7e-222222222222', 'caller.same-1',
                 9101, 'RESPONSES', 'SUCCESS', 3, 0, 3, 3);",
        )
        .expect("valid request identities and duplicate client ids should insert");
    assert!(
        connection
            .batch_execute(
                "INSERT INTO request_log (
                    id, request_id, api_key_id, downstream_protocol, overall_status,
                    request_received_at, is_stream, created_at, updated_at
                ) VALUES (
                    9105, '018fa7d8-6a00-4c9a-8f7e-111111111111',
                    9101, 'OPENAI', 'SUCCESS', 4, 0, 4, 4
                );"
            )
            .is_err(),
        "duplicate canonical id must be rejected"
    );
    assert!(
        connection
            .batch_execute(
                "INSERT INTO request_log (
                    id, request_id, api_key_id, downstream_protocol, overall_status,
                    request_received_at, is_stream, created_at, updated_at
                ) VALUES (
                    9106, '018fa7d8-6a00-7c9a-8f7e-333333333333',
                    9101, 'OPENAI', 'SUCCESS', 5, 0, 5, 5
                );"
            )
            .is_err(),
        "non-v4 canonical id must be rejected"
    );
    assert!(
        connection
            .batch_execute(
                "INSERT INTO request_log (
                    id, request_id, client_request_id, api_key_id,
                    downstream_protocol, overall_status, request_received_at,
                    is_stream, created_at, updated_at
                ) VALUES (
                    9107, '018fa7d8-6a00-4c9a-af7e-444444444444', 'unsafe value',
                    9101, 'OPENAI', 'SUCCESS', 6, 0, 6, 6
                );"
            )
            .is_err(),
        "unsafe client request id must be rejected"
    );

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-07-30-090000_request_log_request_identity/down.sql"
        ))
        .expect("request identity down migration should clear and revert");
    assert_eq!(
        sqlite_table_column_count(&mut connection, "request_log", "request_id"),
        0
    );
    let rollup_count =
        diesel::sql_query("SELECT COUNT(*) AS count FROM metric_request_rollup_minute")
            .get_result::<CountRow>(&mut connection)
            .expect("down migration rollup count should query")
            .count;
    assert_eq!(
        rollup_count, 1,
        "down migration must preserve minute rollups"
    );
}

#[test]
fn sqlite_request_log_timing_upgrade_preserves_body_timing_and_round_trips() {
    let (_temp_dir, mut connection) =
        open_test_sqlite_connection("request-log-timing-contract-upgrade.sqlite");
    let mut migrations = connection
        .pending_migrations(SQLITE_UPGRADE_MIGRATIONS)
        .expect("sqlite upgrade migrations should load");
    let r312 = migrations
        .pop()
        .expect("R3.12 sqlite migration should exist");
    assert_eq!(
        r312.name().version().to_string(),
        R312_SOURCE_BOUND_REQUEST_PATCH_VARIANTS_VERSION,
        "R3.12 must remain outside this historical timing down-migration fixture"
    );
    connection
        .run_migrations(&migrations)
        .expect("pre-R3.12 sqlite migrations should run");

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-08-04-090000_request_log_timing_contract/down.sql"
        ))
        .expect("request log timing down migration should run");
    connection
        .batch_execute(
            "INSERT INTO api_key (
                id, api_key_hash, key_prefix, key_last4, name, description,
                default_action, is_enabled, expires_at, rate_limit_rpm,
                max_concurrent_requests, quota_daily_requests, quota_daily_tokens,
                quota_monthly_tokens, budget_daily_nanos, budget_daily_currency,
                budget_monthly_nanos, budget_monthly_currency, deleted_at,
                created_at, updated_at
            ) VALUES (
                9201, 'timing-contract-hash', 'ck-time', '9201',
                'Timing contract key', NULL, 'ALLOW', 1, NULL, NULL,
                NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 1, 1
            );
            INSERT INTO request_log (
                id, request_id, client_request_id, api_key_id,
                downstream_protocol, overall_status, request_received_at,
                upstream_request_sent_at, response_started_to_client_at,
                completed_at, is_stream, created_at, updated_at
            ) VALUES (
                9202, '018fa7d8-6a00-4c9a-8f7e-111111111111', 'timing-client', 9201,
                'OPENAI', 'SUCCESS', 100, 110, 120, 140, 1, 100, 140
            );
            INSERT INTO metric_ingested_request_log (
                request_log_id, request_received_at, completed_at, ingested_at
            ) VALUES (9202, 100, 140, 150);",
        )
        .expect("pre-timing request log and marker should insert");

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-08-04-090000_request_log_timing_contract/up.sql"
        ))
        .expect("request log timing up migration should run");
    assert_sqlite_request_log_timing_schema(&mut connection);

    let preserved = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM request_log
         WHERE id = 9202
           AND upstream_request_sent_at = 110
           AND first_response_body_at = 120
           AND upstream_response_headers_at IS NULL
           AND upstream_first_body_chunk_at IS NULL
           AND first_token_at IS NULL
           AND max_upstream_response_idle_ms IS NULL
           AND completed_at = 140",
    )
    .get_result::<CountRow>(&mut connection)
    .expect("preserved timing row should query")
    .count;
    assert_eq!(preserved, 1, "old body timing must be preserved exactly");

    let marker_count = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM metric_ingested_request_log
         WHERE request_log_id = 9202 AND request_received_at = 100 AND completed_at = 140",
    )
    .get_result::<CountRow>(&mut connection)
    .expect("ingest marker should query")
    .count;
    assert_eq!(
        marker_count, 1,
        "timing migration must not clear ingest markers"
    );

    assert!(
        connection
            .batch_execute(
                "INSERT INTO request_log (
                    id, request_id, api_key_id, downstream_protocol, overall_status,
                    request_received_at, upstream_request_sent_at,
                    upstream_response_headers_at, upstream_first_body_chunk_at,
                    first_response_body_at, first_token_at, completed_at,
                    is_stream, created_at, updated_at
                ) VALUES (
                    9203, '018fa7d8-6a00-4c9a-8f7e-222222222222', 9201,
                    'OPENAI', 'SUCCESS', 100, 110, 115, 120, 125, 130, 140,
                    0, 100, 140
                );",
            )
            .is_err(),
        "non-streaming first_token_at must be rejected"
    );

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-08-04-090000_request_log_timing_contract/down.sql"
        ))
        .expect("request log timing down migration should round-trip");
    assert_eq!(
        sqlite_table_column_count(
            &mut connection,
            "request_log",
            "response_started_to_client_at"
        ),
        1
    );
    let down_preserved = diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM request_log
         WHERE id = 9202 AND response_started_to_client_at = 120",
    )
    .get_result::<CountRow>(&mut connection)
    .expect("down-migrated body timing should query")
    .count;
    assert_eq!(down_preserved, 1, "down migration must preserve old timing");

    connection
        .batch_execute(include_str!(
            "../../migrations/sqlite/2026-08-04-090000_request_log_timing_contract/up.sql"
        ))
        .expect("request log timing up migration should run after down");
    assert_sqlite_request_log_timing_schema(&mut connection);
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
        assert_postgres_request_log_protocol_schema(&mut connection);
        assert_postgres_request_identity_schema(&mut connection);
        assert_postgres_request_log_timing_schema(&mut connection);
        assert_postgres_r310_source_schema(&mut connection);
        assert_postgres_r311_model_source_schema(&mut connection);
        assert_postgres_r312_request_patch_schema(&mut connection);

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
#[ignore = "requires the dedicated PostgreSQL 17 R3.10 migration smoke database"]
fn postgres_r310_source_migration_preserves_rows_and_enforces_source_contract() {
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
        let r39 = migrate_postgres_to_before_r39(&mut connection);
        connection
            .run_migration(r39.as_ref())
            .expect("R3.9 postgres migration should run before R3.10 fixture");
        connection
            .batch_execute(
                "INSERT INTO api_key (
                    id, api_key_hash, key_prefix, key_last4, name, default_action,
                    is_enabled, created_at, updated_at
                 ) VALUES (
                    1, 'r310-api-key-hash', 'r310', '0001', 'R3.10 API key', 'ALLOW', TRUE, 1, 1
                 );
                 INSERT INTO provider (
                    id, provider_key, name, is_enabled, created_at, updated_at,
                    provider_api_key_mode
                 ) VALUES (
                    30, 'r310-provider', 'R3.10 Provider', TRUE, 1, 1, 'QUEUE'
                 );
                 INSERT INTO upstream_source (
                    id, provider_id, source_key, profile_type, endpoint, use_proxy,
                    created_at, updated_at
                 ) VALUES (
                    31, 30, 'primary', 'OPENAI', 'https://source.example/v1', FALSE, 1, 1
                 );
                 INSERT INTO model (
                    id, provider_id, cost_catalog_id, model_name, real_model_name,
                    supports_streaming, supports_tools, supports_reasoning,
                    supports_image_input, supports_embeddings, supports_rerank,
                    is_enabled, created_at, updated_at
                 ) VALUES (
                    33, 30, NULL, 'r310-model', 'r310-real-model',
                    TRUE, TRUE, TRUE, TRUE, TRUE, TRUE, TRUE, 1, 1
                 );
                 INSERT INTO request_log (
                    id, request_id, api_key_id, requested_model_name,
                    downstream_protocol, overall_status, request_received_at,
                    is_stream, provider_id, source_id, provider_key_snapshot,
                    provider_name_snapshot, source_key_snapshot,
                    source_profile_type_snapshot, source_endpoint_snapshot,
                    upstream_protocol, created_at, updated_at
                 ) VALUES (
                    32, '018fa7d8-6a00-4c9a-8f7e-333333333333', 1,
                    'r310-model', 'OPENAI', 'SUCCESS', 10, FALSE, 30, 31,
                    'r310-provider', 'R3.10 Provider', 'primary', 'OPENAI',
                    'https://source.example/v1', 'OPENAI', 10, 10
                 );
                 INSERT INTO metric_request_rollup_minute (
                    bucket_start_ms, scope_type, scope_id, scope_label,
                    request_count, success_count, error_count, cancelled_count,
                    time_to_first_response_body_sum_ms,
                    time_to_first_response_body_count, ttft_sum_ms, ttft_count,
                    total_latency_sum_ms, total_latency_count,
                    input_tokens, output_tokens, reasoning_tokens, total_tokens,
                    created_at, updated_at
                 ) VALUES (
                    0, 'source', '31', 'primary', 1, 1, 0, 0,
                    1, 1, 1, 1, 1, 1, 1, 1, 0, 2, 1, 1
                 );
                 INSERT INTO metric_cost_rollup_minute (
                    bucket_start_ms, scope_type, scope_id,
                    currency, amount_nanos, created_at, updated_at
                 ) VALUES (0, 'source', '31', 'USD', 100, 1, 1);",
            )
            .expect("R3.9 postgres source fixture should insert");

        let mut pending = connection
            .pending_migrations(POSTGRES_UPGRADE_MIGRATIONS)
            .expect("pending postgres migrations should load");
        let r312 = pending
            .pop()
            .expect("R3.12 postgres migration should be pending");
        assert_eq!(
            r312.name().version().to_string(),
            R312_SOURCE_BOUND_REQUEST_PATCH_VARIANTS_VERSION
        );
        let r311 = pending
            .pop()
            .expect("R3.11 postgres migration should be pending");
        let r310 = pending
            .pop()
            .expect("R3.10 postgres migration should be pending");
        assert_eq!(
            r310.name().version().to_string(),
            R310_PROVIDER_MULTI_SOURCE_VERSION
        );
        connection
            .run_migration(r310.as_ref())
            .expect("R3.10 postgres migration should run");

        assert_postgres_r310_source_schema(&mut connection);
        assert_eq!(
            diesel::sql_query("SELECT COUNT(*) AS count FROM upstream_source WHERE id = 31")
                .get_result::<CountRow>(&mut connection)
                .expect("migrated postgres source should query")
                .count,
            1
        );
        connection
            .run_migration(r311.as_ref())
            .expect("R3.11 postgres migration should run");
        assert_postgres_r311_model_source_schema(&mut connection);
        let model_snapshot = diesel::sql_query(
            "SELECT COUNT(*) AS count
             FROM model
             WHERE id = 33
               AND model_name = 'r310-model'
               AND real_model_name = 'r310-real-model'
               AND source_selection_mode = 'INHERIT_ALL'",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("migrated postgres model snapshot should query")
        .count;
        assert_eq!(model_snapshot, 1);
        assert_eq!(
            diesel::sql_query("SELECT COUNT(*) AS count FROM model_source_binding")
                .get_result::<CountRow>(&mut connection)
                .expect("new postgres binding table should query")
                .count,
            0,
            "R3.11 must not synthesize postgres bindings"
        );
        connection
            .batch_execute(
                "INSERT INTO model_source_binding (
                    model_id, source_id, is_default, created_at, updated_at
                 ) VALUES (33, 31, TRUE, 20, 20);",
            )
            .expect("valid postgres model Source binding should insert");
        assert!(
            connection
                .batch_execute(
                    "INSERT INTO model_source_binding (
                        model_id, source_id, is_default, created_at, updated_at
                     ) VALUES (33, 31, TRUE, 21, 21);",
                )
                .is_err(),
            "a postgres model may have only one default binding"
        );
        connection
            .batch_execute(
                "UPDATE request_log
                 SET resolved_reasoning_suffix = 'legacy-high'
                 WHERE id = 32;",
            )
            .expect("historical postgres reasoning suffix should be writable before R3.12");
        connection
            .run_migration(r312.as_ref())
            .expect("R3.12 postgres migration should run");
        assert_postgres_r312_request_patch_schema(&mut connection);
        let migrated_suffix = diesel::sql_query(
            "SELECT resolved_patch_suffix AS value
             FROM request_log WHERE id = 32",
        )
        .get_result::<TextValueRow>(&mut connection)
        .expect("migrated postgres request patch suffix should query");
        assert_eq!(migrated_suffix.value, "legacy-high");
        assert!(
            connection
                .batch_execute(
                    "INSERT INTO request_log (
                        id, request_id, api_key_id, downstream_protocol,
                        overall_status, request_received_at, is_stream,
                        source_selection_reason, created_at, updated_at
                     ) VALUES (
                        34, '018fa7d8-6a00-4c9a-8f7e-444444444444', 1,
                        'OPENAI', 'SUCCESS', 20, FALSE,
                        'invalid_reason', 20, 20
                     );",
                )
                .is_err(),
            "postgres request log selection reason must use stable values"
        );
        let snapshot = diesel::sql_query(
            "SELECT source_profile_type_snapshot::text AS value
             FROM request_log WHERE id = 32",
        )
        .get_result::<TextValueRow>(&mut connection)
        .expect("migrated postgres request log snapshot should query");
        assert_eq!(snapshot.value, "OPENAI");
        let source_rollup = diesel::sql_query(
            "SELECT scope_label AS value
             FROM metric_request_rollup_minute
             WHERE scope_type = 'source' AND scope_id = '31'",
        )
        .get_result::<TextValueRow>(&mut connection)
        .expect("migrated postgres source metric rollup should query");
        assert_eq!(source_rollup.value, "OPENAI");
        assert_eq!(
            diesel::sql_query(
                "SELECT amount_nanos AS count
                 FROM metric_cost_rollup_minute
                 WHERE scope_type = 'source' AND scope_id = '31'",
            )
            .get_result::<CountRow>(&mut connection)
            .expect("migrated postgres source cost rollup should query")
            .count,
            100,
            "cost rollup identity and value should remain intact"
        );

        connection
            .batch_execute(
                "INSERT INTO provider (
                    id, provider_key, name, is_enabled, created_at, updated_at,
                    provider_api_key_mode
                 ) VALUES (40, 'constraint-provider', 'Constraint Provider', TRUE, 1, 1, 'QUEUE');
                 INSERT INTO upstream_source (
                    id, provider_id, profile_type, endpoint, use_proxy,
                    is_enabled, is_default, created_at, updated_at
                 ) VALUES (41, 40, 'OPENAI', 'https://one.example/v1', FALSE, TRUE, TRUE, 1, 1);",
            )
            .expect("postgres constraint provider and first source should insert");
        assert!(
            connection
                .batch_execute(
                    "INSERT INTO upstream_source (
                        id, provider_id, profile_type, endpoint, use_proxy,
                        is_enabled, is_default, created_at, updated_at
                     ) VALUES (42, 40, 'VERTEX_OPENAI', 'https://two.example/v1', FALSE, FALSE, FALSE, 1, 1);"
                )
                .is_err(),
            "disabled sources must still reserve their active wire family"
        );
        assert!(
            connection
                .batch_execute(
                    "INSERT INTO upstream_source (
                        id, provider_id, profile_type, endpoint, use_proxy,
                        is_enabled, is_default, created_at, updated_at
                     ) VALUES (43, 40, 'RESPONSES', 'https://responses.example/v1', FALSE, TRUE, FALSE, 1, 1);"
                )
                .is_ok(),
            "different source families should coexist"
        );
        assert!(
            connection
                .batch_execute(
                    "INSERT INTO upstream_source (
                        id, provider_id, profile_type, endpoint, use_proxy,
                        is_enabled, is_default, created_at, updated_at
                     ) VALUES (44, 40, 'ANTHROPIC', 'https://anthropic.example/v1', FALSE, TRUE, TRUE, 1, 1);"
                )
                .is_err(),
            "a provider may have only one active default source"
        );
        assert!(
            connection
                .batch_execute(
                    "INSERT INTO upstream_source (
                        id, provider_id, profile_type, endpoint, use_proxy,
                        is_enabled, is_default, created_at, updated_at
                     ) VALUES (45, 40, 'GEMINI', 'https://gemini.example/v1', FALSE, FALSE, TRUE, 1, 1);"
                )
                .is_err(),
            "a disabled source may not be the default"
        );
        connection
            .batch_execute(
                "UPDATE upstream_source
                 SET deleted_at = 2, is_enabled = FALSE, is_default = FALSE
                 WHERE id = 41;
                 INSERT INTO upstream_source (
                    id, provider_id, profile_type, endpoint, use_proxy,
                    is_enabled, is_default, created_at, updated_at
                 ) VALUES (46, 40, 'VERTEX_OPENAI', 'https://replacement.example/v1', FALSE, TRUE, TRUE, 1, 1);",
            )
            .expect("postgres soft deletion should release source family and default slot");
        assert_eq!(
            diesel::sql_query("SELECT COUNT(*) AS count FROM upstream_source")
                .get_result::<CountRow>(&mut connection)
                .expect("postgres source count should query")
                .count,
            4
        );
    }));

    rebuild_postgres_public_schema(&mut connection);
    if let Err(panic_payload) = test_result {
        resume_unwind(panic_payload);
    }
}

#[test]
#[ignore = "requires the dedicated PostgreSQL 17 R3.9 migration smoke database"]
fn postgres_r39_destructive_upgrade_clears_execution_domain_and_preserves_governance() {
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
        let r39 = migrate_postgres_to_before_r39(&mut connection);
        seed_postgres_r39_boundary_fixture(&mut connection);

        for table in [
            "provider",
            "provider_api_key",
            "model",
            "api_key_acl_rule",
            "request_patch_rule",
            "reasoning_config",
            "reasoning_config_preset",
            "runtime_feature_config",
            "request_log",
            "metric_ingested_request_log",
            "metric_request_rollup_minute",
            "metric_http_status_rollup_minute",
            "metric_cost_rollup_minute",
        ] {
            let count = diesel::sql_query(format!("SELECT COUNT(*) AS count FROM {table}"))
                .get_result::<CountRow>(&mut connection)
                .expect("PostgreSQL pre-R3.9 fixture count should query")
                .count;
            assert_eq!(count, 1, "PostgreSQL fixture should populate {table}");
        }

        connection
            .run_migration(r39.as_ref())
            .expect("R3.9 postgres migration should run");
        assert_postgres_r39_source_schema(&mut connection);

        for table in [
            "provider",
            "upstream_source",
            "provider_api_key",
            "model",
            "api_key_acl_rule",
            "request_patch_rule",
            "reasoning_config",
            "reasoning_config_preset",
            "runtime_feature_config",
            "request_log",
            "metric_ingested_request_log",
            "metric_request_rollup_minute",
            "metric_http_status_rollup_minute",
            "metric_cost_rollup_minute",
        ] {
            let count = diesel::sql_query(format!("SELECT COUNT(*) AS count FROM {table}"))
                .get_result::<CountRow>(&mut connection)
                .expect("PostgreSQL post-R3.9 cleared count should query")
                .count;
            assert_eq!(count, 0, "R3.9 must clear PostgreSQL table {table}");
        }
        for table in [
            "manager_credential",
            "manager_auth_instance",
            "manager_totp_recovery_code",
            "api_key",
            "api_key_rollup_daily",
            "api_key_rollup_monthly",
            "cost_catalogs",
            "cost_catalog_versions",
        ] {
            let count = diesel::sql_query(format!("SELECT COUNT(*) AS count FROM {table}"))
                .get_result::<CountRow>(&mut connection)
                .expect("PostgreSQL post-R3.9 preserved count should query")
                .count;
            assert_eq!(count, 1, "R3.9 must preserve PostgreSQL table {table}");
        }

        connection
            .transaction::<(), diesel::result::Error, _>(|connection| {
                connection.batch_execute(
                    "INSERT INTO provider (
                        id, provider_key, name, is_enabled, created_at, updated_at,
                        provider_api_key_mode
                     ) VALUES (30, 'new-provider', 'New Provider', TRUE, 1, 1, 'QUEUE');
                     INSERT INTO upstream_source (
                        id, provider_id, source_key, profile_type, endpoint, use_proxy,
                        created_at, updated_at
                     ) VALUES (
                        31, 30, 'primary', 'OPENAI', 'https://new.example/v1', FALSE, 1, 1
                     );",
                )
            })
            .expect("PostgreSQL Provider and primary Source should create atomically");
        assert!(
            connection
                .batch_execute(
                    "INSERT INTO upstream_source (
                        id, provider_id, source_key, profile_type, endpoint, use_proxy,
                        created_at, updated_at
                     ) VALUES (
                        32, 30, 'primary', 'GEMINI', 'https://second.example/v1', FALSE, 1, 1
                     );"
                )
                .is_err(),
            "PostgreSQL must reject a second active Source"
        );
        connection
            .batch_execute(
                "INSERT INTO provider (
                    id, provider_key, name, is_enabled, created_at, updated_at,
                    provider_api_key_mode
                 ) VALUES (40, 'invalid-source-provider', 'Invalid Source', TRUE, 1, 1, 'QUEUE');",
            )
            .expect("PostgreSQL constraint fixture provider should insert");
        assert!(
            connection
                .batch_execute(
                    "INSERT INTO upstream_source (
                        id, provider_id, source_key, profile_type, endpoint, use_proxy,
                        created_at, updated_at
                     ) VALUES (
                        41, 40, 'secondary', 'OPENAI', 'https://invalid.example/v1', FALSE, 1, 1
                     );"
                )
                .is_err(),
            "PostgreSQL must reject a non-primary source_key"
        );
        let source_count = diesel::sql_query("SELECT COUNT(*) AS count FROM upstream_source")
            .get_result::<CountRow>(&mut connection)
            .expect("PostgreSQL Source count should query")
            .count;
        assert_eq!(source_count, 1);
    }));

    rebuild_postgres_public_schema(&mut connection);
    if let Err(panic_payload) = test_result {
        resume_unwind(panic_payload);
    }
}

#[test]
#[ignore = "requires a dedicated PostgreSQL 17 database"]
fn postgres_request_log_protocol_boundary_upgrade_clears_history() {
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
        run_postgres_migrations(&mut connection).expect("postgres migrations should run");
        connection
            .batch_execute(include_str!(
                "../../migrations/postgres/2026-07-29-090000_request_log_protocol_boundaries/down.sql"
            ))
            .expect("request log protocol down migration should run");
        connection
            .batch_execute(
                "INSERT INTO api_key (
                    id, api_key_hash, key_prefix, key_last4, name, description,
                    default_action, is_enabled, expires_at, rate_limit_rpm,
                    max_concurrent_requests, quota_daily_requests, quota_daily_tokens,
                    quota_monthly_tokens, budget_daily_nanos, budget_daily_currency,
                    budget_monthly_nanos, budget_monthly_currency, deleted_at,
                    created_at, updated_at
                ) VALUES (
                    9001, 'protocol-boundary-hash', 'ck-test', '9001',
                    'Protocol migration key', NULL, 'ALLOW', TRUE, NULL, NULL,
                    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 1, 1
                );
                INSERT INTO request_log (
                    id, request_id, api_key_id, user_api_type, llm_api_type, overall_status,
                    request_received_at, is_stream, created_at, updated_at
                ) VALUES (
                    9002, '018fa7d8-6a00-4c9a-8f7e-111111111111', 9001,
                    'OLLAMA', 'GEMINI_OPENAI', 'SUCCESS',
                    1, FALSE, 1, 1
                );",
            )
            .expect("legacy PostgreSQL request log should insert");
        connection
            .batch_execute(include_str!(
                "../../migrations/postgres/2026-07-29-090000_request_log_protocol_boundaries/up.sql"
            ))
            .expect("request log protocol up migration should run");

        assert_postgres_request_log_protocol_schema(&mut connection);
        let rows = diesel::sql_query("SELECT COUNT(*) AS count FROM request_log")
            .get_result::<CountRow>(&mut connection)
            .expect("PostgreSQL request log count should query")
            .count;
        assert_eq!(
            rows, 0,
            "protocol migration must discard request-log history"
        );
        connection
            .batch_execute(
                "INSERT INTO request_log (
                    id, request_id, api_key_id, downstream_protocol, upstream_protocol,
                    overall_status, request_received_at, is_stream, created_at, updated_at
                ) VALUES
                    (9003, '018fa7d8-6a00-4c9a-8f7e-222222222222', 9001,
                        'OPENAI', NULL, 'SUCCESS', 2, FALSE, 2, 2),
                    (9004, '018fa7d8-6a00-4c9a-8f7e-333333333333', 9001,
                        'RESPONSES', 'OLLAMA', 'SUCCESS', 3, FALSE, 3, 3);",
            )
            .expect("PostgreSQL directional request logs should insert");
    }));

    rebuild_postgres_public_schema(&mut connection);
    if let Err(panic_payload) = test_result {
        resume_unwind(panic_payload);
    }
}

#[test]
#[ignore = "requires a dedicated PostgreSQL 17 database"]
fn postgres_request_identity_upgrade_is_destructive_constrained_and_preserves_rollups() {
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
        run_postgres_migrations(&mut connection).expect("postgres migrations should run");
        connection
            .batch_execute(include_str!(
                "../../migrations/postgres/2026-07-30-090000_request_log_request_identity/down.sql"
            ))
            .expect("request identity down migration should run");
        connection
            .batch_execute(
                "INSERT INTO api_key (
                    id, api_key_hash, key_prefix, key_last4, name, description,
                    default_action, is_enabled, expires_at, rate_limit_rpm,
                    max_concurrent_requests, quota_daily_requests, quota_daily_tokens,
                    quota_monthly_tokens, budget_daily_nanos, budget_daily_currency,
                    budget_monthly_nanos, budget_monthly_currency, deleted_at,
                    created_at, updated_at
                ) VALUES (
                    9101, 'request-identity-hash', 'ck-test', '9101',
                    'Request identity migration key', NULL, 'ALLOW', TRUE, NULL, NULL,
                    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 1, 1
                );
                INSERT INTO request_log (
                    id, api_key_id, downstream_protocol, overall_status,
                    request_received_at, is_stream, created_at, updated_at
                ) VALUES (
                    9102, 9101, 'OPENAI', 'SUCCESS', 1, FALSE, 1, 1
                );
                INSERT INTO metric_ingested_request_log (
                    request_log_id, request_received_at, completed_at, ingested_at
                ) VALUES (9102, 1, 1, 1);
                INSERT INTO metric_request_rollup_minute (
                    bucket_start_ms, scope_type, scope_id, scope_label,
                    request_count, success_count, error_count, cancelled_count,
                    time_to_first_response_body_sum_ms, time_to_first_response_body_count,
                    ttft_sum_ms, ttft_count,
                    total_latency_sum_ms, total_latency_count,
                    input_tokens, output_tokens, reasoning_tokens, total_tokens,
                    created_at, updated_at
                ) VALUES (
                    0, 'global', 'global', NULL,
                    1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 2, 1, 1
                );",
            )
            .expect("pre-identity PostgreSQL request data should insert");

        connection
            .batch_execute(include_str!(
                "../../migrations/postgres/2026-07-30-090000_request_log_request_identity/up.sql"
            ))
            .expect("request identity up migration should run");
        assert_postgres_request_identity_schema(&mut connection);

        for table in ["request_log", "metric_ingested_request_log"] {
            let count = diesel::sql_query(format!("SELECT COUNT(*) AS count FROM {table}"))
                .get_result::<CountRow>(&mut connection)
                .expect("PostgreSQL destructive migration table count should query")
                .count;
            assert_eq!(count, 0, "{table} must be cleared");
        }
        let rollup_count =
            diesel::sql_query("SELECT COUNT(*) AS count FROM metric_request_rollup_minute")
                .get_result::<CountRow>(&mut connection)
                .expect("PostgreSQL rollup count should query")
                .count;
        assert_eq!(rollup_count, 1, "minute rollups must be preserved");

        connection
            .batch_execute(
                "INSERT INTO request_log (
                    id, request_id, client_request_id, api_key_id,
                    downstream_protocol, overall_status, request_received_at,
                    is_stream, created_at, updated_at
                ) VALUES
                    (9103, '018fa7d8-6a00-4c9a-8f7e-111111111111', 'caller.same-1',
                     9101, 'OPENAI', 'SUCCESS', 2, FALSE, 2, 2),
                    (9104, '018fa7d8-6a00-4c9a-9f7e-222222222222', 'caller.same-1',
                     9101, 'RESPONSES', 'SUCCESS', 3, FALSE, 3, 3);",
            )
            .expect("valid PostgreSQL request identities should insert");
        assert!(
            connection
                .batch_execute(
                    "INSERT INTO request_log (
                        id, request_id, api_key_id, downstream_protocol, overall_status,
                        request_received_at, is_stream, created_at, updated_at
                    ) VALUES (
                        9105, '018fa7d8-6a00-4c9a-8f7e-111111111111',
                        9101, 'OPENAI', 'SUCCESS', 4, FALSE, 4, 4
                    );"
                )
                .is_err(),
            "duplicate PostgreSQL canonical id must be rejected"
        );
        assert!(
            connection
                .batch_execute(
                    "INSERT INTO request_log (
                        id, request_id, api_key_id, downstream_protocol, overall_status,
                        request_received_at, is_stream, created_at, updated_at
                    ) VALUES (
                        9106, '018fa7d8-6a00-7c9a-8f7e-333333333333',
                        9101, 'OPENAI', 'SUCCESS', 5, FALSE, 5, 5
                    );"
                )
                .is_err(),
            "non-v4 PostgreSQL canonical id must be rejected"
        );
        assert!(
            connection
                .batch_execute(
                    "INSERT INTO request_log (
                        id, request_id, client_request_id, api_key_id,
                        downstream_protocol, overall_status, request_received_at,
                        is_stream, created_at, updated_at
                    ) VALUES (
                        9107, '018fa7d8-6a00-4c9a-af7e-444444444444', 'unsafe value',
                        9101, 'OPENAI', 'SUCCESS', 6, FALSE, 6, 6
                    );"
                )
                .is_err(),
            "unsafe PostgreSQL client request id must be rejected"
        );

        connection
            .batch_execute(include_str!(
                "../../migrations/postgres/2026-07-30-090000_request_log_request_identity/down.sql"
            ))
            .expect("request identity down migration should clear and revert");
        let canonical_column = diesel::sql_query(
            "SELECT COUNT(*) AS count
             FROM information_schema.columns
             WHERE table_schema = 'public'
               AND table_name = 'request_log'
               AND column_name = 'request_id'",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("down-migrated PostgreSQL request_id column should query")
        .count;
        assert_eq!(canonical_column, 0);
        let rollup_count =
            diesel::sql_query("SELECT COUNT(*) AS count FROM metric_request_rollup_minute")
                .get_result::<CountRow>(&mut connection)
                .expect("PostgreSQL down migration rollup count should query")
                .count;
        assert_eq!(
            rollup_count, 1,
            "down migration must preserve minute rollups"
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
