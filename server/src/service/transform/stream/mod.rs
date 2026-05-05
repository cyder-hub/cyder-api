//! Stream transform owner module.
//!
//! Task 7 moves transformer orchestration, session state, usage fallback,
//! legacy bridge, and controlled stream error helpers into these submodules.

pub(crate) mod bridge;
pub(crate) mod error;
pub(crate) mod session;
pub(crate) mod transformer;
pub(crate) mod usage;

pub(crate) use session::AnthropicActiveBlockKind;
pub(crate) use session::StreamTransformContext;
pub use session::{
    AnthropicActiveBlockState, AnthropicSessionState, GeminiSessionState, ResponsesSessionState,
    SessionContext,
};
pub use transformer::StreamTransformer;

#[cfg(test)]
mod tests;
