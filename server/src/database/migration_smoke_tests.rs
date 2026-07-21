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
