use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};

const MIGRATIONS_TABLE: &str = "migrations";
const UP_FILE: &str = "up.sql";

struct MigrationTask {
    version: String,
    path: PathBuf,
}

pub fn init_migration(conn: &Connection) -> Result<()> {
    create_migrations_table(&conn)?;

    let migrations_dir = Path::new("migrations"); // Replace with your actual directory

    let migrations = find_migrations(migrations_dir).unwrap();
    let applied_migrations = get_applied_migrations(&conn)?;

    for migration in &migrations {
        if !applied_migrations.contains(&migration.version) {
            println!("try to migrate to {}", migration.version);
            let up_file_path = migration.path.join(UP_FILE);
            if up_file_path.exists() {
                conn.execute_batch(&fs::read_to_string(up_file_path).unwrap())?;
                conn.execute(
                    &format!("INSERT INTO {} (version) VALUES (?)", MIGRATIONS_TABLE),
                    params![migration.version],
                )?;
                println!("migration {} done", migration.version);
            } else {
                panic!("migration file {} do not exist", up_file_path.display());
            }
        }
    }
    Ok(())
}

fn create_migrations_table(conn: &Connection) -> Result<()> {
    conn.execute(
        &format!(
            "CREATE TABLE IF NOT EXISTS {} (
                version TEXT PRIMARY KEY,
                run_on DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            MIGRATIONS_TABLE
        ),
        [],
    )?;
    Ok(())
}

fn find_migrations(migrations_dir: &Path) -> Result<Vec<MigrationTask>, io::Error> {
    let mut migrations: Vec<MigrationTask> = Vec::new();

    for entry in fs::read_dir(migrations_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let version: String =
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map_or("".to_string(), |name| {
                        let parts: Vec<&str> = name.split('_').collect();
                        parts[0].chars().filter(|&c| c != '-').collect()
                    });
            if !version.is_empty() {
                migrations.push(MigrationTask {
                    version: version.to_string(),
                    path,
                });
            }
        }
    }

    migrations.sort_by_key(|task| task.version.to_string());

    Ok(migrations)
}

fn get_applied_migrations(conn: &Connection) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT version FROM {} ORDER BY version ASC",
        MIGRATIONS_TABLE
    ))?;
    let mut rows = stmt.query([])?;

    let mut applied_migrations = HashSet::new();
    while let Some(row) = rows.next()? {
        applied_migrations.insert(row.get(0)?);
    }
    Ok(applied_migrations)
}
