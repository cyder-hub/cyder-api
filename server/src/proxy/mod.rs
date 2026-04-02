mod auth;
mod core;
mod error;
mod gemini;
mod handlers;
pub(super) mod logging;
mod models;
mod prepare;
mod router;
mod unified;
mod util;

use error::ProxyError;
pub use router::create_proxy_router;
