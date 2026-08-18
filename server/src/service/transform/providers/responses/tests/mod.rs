use serde_json::{Value, json};

use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};
use crate::service::transform::providers::openai;
use crate::service::transform::{StreamTransformer, unified::*};

use super::*;

mod metadata_fidelity;
mod openai_bridge;
mod request;
mod response;
mod stream;
