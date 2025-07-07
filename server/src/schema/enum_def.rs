use diesel_derive_enum::DbEnum;
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DbEnum, Default)]
#[db_enum(pg_type = "provider_type_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderType {
    #[default]
    Openai,
    Gemini,
    Vertex,
    VertexOpenai,
    Ollama,
}
