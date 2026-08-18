mod metadata;
mod payload;
mod request;
mod response;
mod stream;
mod target;
mod usage;

#[cfg(test)]
mod tests;

pub(crate) use metadata::{
    GeminiTerminalKind, build_gemini_synthetic_tool_call_id, build_gemini_tool_call_key,
    classify_gemini_finish_reason, classify_gemini_terminal,
};
pub(crate) use payload::*;
#[cfg(test)]
pub(crate) use stream::{
    transform_unified_chunk_to_gemini_events, transform_unified_stream_events_to_gemini_events,
};
pub(crate) use stream::{
    try_transform_unified_chunk_to_gemini_events,
    try_transform_unified_stream_events_to_gemini_events,
};
pub(crate) use target::validate_gemini_target_request;
pub(crate) use usage::{decode_gemini_usage, gemini_usage_snapshot_regressed};
