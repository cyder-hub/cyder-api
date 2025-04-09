use std::fs;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum DbType {
    Sqlite,
    Postgres,
}

impl DbType {
    fn to_str(&self) -> &'static str {
        match self {
            DbType::Sqlite => "sqlite",
            DbType::Postgres => "postgres",
        }
    }
}

#[derive(Debug, Deserialize)]
struct Config {
    db_type: DbType,
}

fn parse_config() -> Config {
    let yaml_str = fs::read_to_string("config.local.yaml")
        .unwrap_or(fs::read_to_string("config.yaml").unwrap());
    serde_yaml::from_str(&yaml_str).unwrap()
}

fn main() {
    println!("cargo:rerun-if-changed=config.yaml");
    println!("cargo:rerun-if-changed=config.local.yaml");
    // let config = parse_config();
    // let db_type = config.db_type.to_str();
    // let diesel_config_path = format!("database/{}/config", db_type);

    // for entry in fs::read_dir(diesel_config_path).unwrap() {
    //     let entry = entry.unwrap();
    //     let path = entry.path();
    //     let file_name = entry.file_name();
    //     if path.is_file() {
    //         let _ = fs::copy(&path, file_name);
    //     }
    // }
}
