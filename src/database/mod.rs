use diesel::{
    r2d2::{ConnectionManager, Pool, PooledConnection}, Connection, MysqlConnection, PgConnection, SqliteConnection
};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use once_cell::sync::Lazy;

use crate::{config::CONFIG, controller::BaseError};
use serde::Serialize;

pub mod api_key;
pub mod model;
pub mod price;
pub mod provider;
pub mod record;
pub mod model_transform;

pub enum DbType {
    Postgres,
    Sqlite,
    Mysql,
}

pub enum DbPool {
    Postgres(Pool<ConnectionManager<PgConnection>>),
    Sqlite(Pool<ConnectionManager<SqliteConnection>>),
    Mysql(Pool<ConnectionManager<MysqlConnection>>),
}

pub enum DbConnection {
    Postgres(PooledConnection<ConnectionManager<PgConnection>>),
    Sqlite(PooledConnection<ConnectionManager<SqliteConnection>>),
    Mysql(PooledConnection<ConnectionManager<MysqlConnection>>),
}

pub fn get_connection() -> DbConnection {
    match &*DB_POOL {
        DbPool::Postgres(pool) => DbConnection::Postgres(pool.get().unwrap()),
        DbPool::Sqlite(pool) => DbConnection::Sqlite(pool.get().unwrap()),
        DbPool::Mysql(pool) => DbConnection::Mysql(pool.get().unwrap()),
    }
}

fn parse_db_type(db_url: &str) -> DbType {
    if db_url.starts_with("postgres") {
        DbType::Postgres
    } else if db_url.starts_with("mysql") {
        DbType::Mysql
    } else {
        DbType::Sqlite
    }
}

impl DbPool {
    pub fn establish() -> Self {
        let db_url = &CONFIG.db_url;
        let db_type = parse_db_type(db_url);
        match db_type {
            DbType::Postgres => {
                let pool = init_pg_pool(db_url);
                DbPool::Postgres(pool)
            }
            DbType::Sqlite => {
                let pool = init_sqlite_pool(db_url);
                DbPool::Sqlite(pool)
            }
            DbType::Mysql => {
                let pool = init_mysql_pool(db_url);
                DbPool::Mysql(pool)
            }
        }
    }
}

#[path = "../schema/sqlite.rs"]
pub mod _sqlite_schema;

#[path = "../schema/postgres.rs"]
pub mod _postgres_schema;

#[path = "../schema/mysql.rs"]
pub mod _mysql_schema;

#[macro_export]
macro_rules! db_object {
    (
        $(
            $( #[$attr:meta] )*
            pub struct $name:ident {
                $( $( #[$field_attr:meta] )* $vis:vis $field:ident : $typ:ty ),+
                $(,)?
            }
        )+
    ) => {
        $(
            #[derive(Debug, serde::Serialize)]
            pub struct $name { $( $vis $field : $typ, )+ }
        )+

        pub mod _postgres_model {
            $( $crate::db_object! { @expand postgres |  $( #[$attr] )* | $name |  $( $( #[$field_attr] )* $field : $typ ),+ } )+
        }
        pub mod _sqlite_model {
            $( $crate::db_object! { @expand sqlite |  $( #[$attr] )* | $name |  $( $( #[$field_attr] )* $field : $typ ),+ } )+
        }
        pub mod _mysql_model {
            $( $crate::db_object! { @expand mysql |  $( #[$attr] )* | $name |  $( $( #[$field_attr] )* $field : $typ ),+ } )+
        }
    };
    ( @expand $db_type:ident | $( #[$attr:meta] )* | $name:ident | $( $( #[$field_attr:meta] )* $vis:vis $field:ident : $typ:ty),+) => {
        paste::paste! {
            #[allow(unused_imports)] use super::*;
            #[allow(unused_imports)] use crate::database::[<_ $db_type _schema>]::*;
            #[allow(unused_imports)] use diesel::prelude::*;

            $( #[$attr] )*
            pub struct [<$name Db>] { $(
                $( #[$field_attr] )* $vis $field : $typ,
            )+ }

            impl [<$name Db>] {
                #[inline(always)]
                pub fn from_db(self) -> super::$name {
                    super::$name { $( $field: self.$field, )+ }
                }

                #[inline(always)]
                pub fn to_db(x: &super::$name) -> Self {
                    Self {
                        $( $field: x.$field.clone(), )+
                    }
                }
            }
        }
    }
}

#[macro_export]
macro_rules! db_execute {
    ($conn:ident, $block:block) => {
        match $conn {
            crate::database::DbConnection::Postgres($conn) => {
                use crate::database::_postgres_schema::*;
                #[allow(unused_imports)]
                use _postgres_model::*;
                #[allow(unused_imports)]
                use diesel::prelude::*;

                $block
            }
            crate::database::DbConnection::Sqlite($conn) => {
                use crate::database::_sqlite_schema::*;
                #[allow(unused_imports)]
                use _sqlite_model::*;
                #[allow(unused_imports)]
                use diesel::prelude::*;

                $block
            }
            crate::database::DbConnection::Mysql($conn) => {
                use crate::database::_mysql_schema::*;
                #[allow(unused_imports)]
                use _mysql_model::*;
                #[allow(unused_imports)]
                use diesel::prelude::*;

                $block
            }
        }
    };
}

static DB_POOL: Lazy<DbPool> = Lazy::new(|| DbPool::establish());
const SQLITE_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/sqlite");
const POSTGRES_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/postgres");
const MYSQL_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/mysql");

fn init_sqlite_pool(db_url: &str) -> Pool<ConnectionManager<SqliteConnection>> {
    let mut connection = SqliteConnection::establish(db_url).expect("failed to establish migration connection");

    connection.run_pending_migrations(SQLITE_MIGRATIONS).expect("failed to run migrations");

    let manager = ConnectionManager::<SqliteConnection>::new(db_url);
    Pool::builder()
        .test_on_check_out(true)
        .max_size(5)
        .build(manager)
        .expect("Failed to create pool.")
}

fn init_pg_pool(db_url: &str) -> Pool<ConnectionManager<PgConnection>> {
    let mut connection = PgConnection::establish(db_url).expect("failed to establish migration connection");

    connection.run_pending_migrations(POSTGRES_MIGRATIONS).expect("failed to run migrations");

    let manager = ConnectionManager::<PgConnection>::new(db_url);
    Pool::builder()
        .max_size(5)
        .build(manager)
        .expect("Failed to create pool.")
}

fn init_mysql_pool(db_url: &str) -> Pool<ConnectionManager<MysqlConnection>> {
    let mut connection = MysqlConnection::establish(db_url).expect("failed to establish migration connection");

    connection.run_pending_migrations(MYSQL_MIGRATIONS).expect("failed to run migrations");

    let manager = ConnectionManager::<MysqlConnection>::new(db_url);
    Pool::builder()
        .max_size(5)
        .build(manager)
        .expect("Failed to create pool.")
}

pub type DbResult<T> = Result<T, BaseError>;

#[derive(Serialize)]
pub struct ListResult<T> {
    total: i64,
    page: i64,
    page_size: i64,
    list: Vec<T>,
}
