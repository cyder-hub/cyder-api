use super::unified::{UnifiedContentPart, UnifiedContentPartDelta};
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransformValueKind {
    TopKParameter,
    ToolDefinitions,
    ToolRoleMessage,
    ResponsesUnknownItem,
    Text,
    Refusal,
    ImageUrl,
    ImageData,
    AudioData,
    FileUrl,
    FileData,
    FileId,
    ExecutableCode,
    ToolCall,
    ToolResult,
    ReasoningHistory,
    ReasoningContent,
    ImageDelta,
    ToolCallDelta,
    ReasoningDelta,
    BlobDelta,
    StreamError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProtocolCapabilityMatrix {
    pub request: RequestCapabilityMatrix,
    pub response: ResponseCapabilityMatrix,
    pub stream: StreamCapabilityMatrix,
    pub structured_content: StructuredContentCapabilityMatrix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RequestCapabilityMatrix {
    pub tool_definitions: bool,
    pub tool_role_messages: bool,
    pub reasoning_history: bool,
    pub top_k_parameter: bool,
    pub image_url_input: bool,
    pub image_inline_input: bool,
    pub file_url_input: bool,
    pub file_inline_input: bool,
    pub executable_code_input: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResponseCapabilityMatrix {
    pub reasoning_content: bool,
    pub refusal: bool,
    pub citations: bool,
    pub file_output: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StreamCapabilityMatrix {
    pub tool_call_deltas: bool,
    pub reasoning_deltas: bool,
    pub reasoning_summary_parts: bool,
    pub image_deltas: bool,
    pub blob_deltas: bool,
    pub structured_errors: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StructuredContentCapabilityMatrix {
    pub images: bool,
    pub tool_results: bool,
    pub refusal: bool,
    pub citations: bool,
    pub file_references: bool,
    pub json_schema_strict: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapabilityProtocol {
    Openai,
    Gemini,
    Anthropic,
    Responses,
}

impl From<DownstreamProtocol> for CapabilityProtocol {
    fn from(protocol: DownstreamProtocol) -> Self {
        match protocol {
            DownstreamProtocol::Openai => Self::Openai,
            DownstreamProtocol::Gemini => Self::Gemini,
            DownstreamProtocol::Anthropic => Self::Anthropic,
            DownstreamProtocol::Responses => Self::Responses,
        }
    }
}

impl From<UpstreamProtocol> for CapabilityProtocol {
    fn from(protocol: UpstreamProtocol) -> Self {
        match protocol {
            UpstreamProtocol::Openai => Self::Openai,
            UpstreamProtocol::Gemini => Self::Gemini,
            UpstreamProtocol::Anthropic => Self::Anthropic,
            UpstreamProtocol::Responses => Self::Responses,
        }
    }
}

impl ProtocolCapabilityMatrix {
    pub(crate) fn for_downstream(protocol: DownstreamProtocol) -> Self {
        Self::for_protocol(protocol.into())
    }

    pub(crate) fn for_upstream(protocol: UpstreamProtocol) -> Self {
        Self::for_protocol(protocol.into())
    }

    fn for_protocol(protocol: CapabilityProtocol) -> Self {
        match protocol {
            CapabilityProtocol::Openai => Self {
                request: RequestCapabilityMatrix {
                    tool_definitions: true,
                    tool_role_messages: true,
                    reasoning_history: true,
                    top_k_parameter: false,
                    image_url_input: true,
                    image_inline_input: true,
                    file_url_input: false,
                    file_inline_input: true,
                    executable_code_input: false,
                },
                response: ResponseCapabilityMatrix {
                    reasoning_content: true,
                    refusal: true,
                    citations: false,
                    file_output: false,
                },
                stream: StreamCapabilityMatrix {
                    tool_call_deltas: true,
                    reasoning_deltas: true,
                    reasoning_summary_parts: false,
                    image_deltas: false,
                    blob_deltas: false,
                    structured_errors: false,
                },
                structured_content: StructuredContentCapabilityMatrix {
                    images: true,
                    tool_results: true,
                    refusal: true,
                    citations: false,
                    file_references: false,
                    json_schema_strict: true,
                },
            },
            CapabilityProtocol::Gemini => Self {
                request: RequestCapabilityMatrix {
                    tool_definitions: true,
                    tool_role_messages: true,
                    reasoning_history: true,
                    top_k_parameter: false,
                    image_url_input: true,
                    image_inline_input: true,
                    file_url_input: false,
                    file_inline_input: false,
                    executable_code_input: false,
                },
                response: ResponseCapabilityMatrix {
                    reasoning_content: true,
                    refusal: false,
                    citations: true,
                    file_output: false,
                },
                stream: StreamCapabilityMatrix {
                    tool_call_deltas: true,
                    reasoning_deltas: true,
                    reasoning_summary_parts: false,
                    image_deltas: false,
                    blob_deltas: false,
                    structured_errors: false,
                },
                structured_content: StructuredContentCapabilityMatrix {
                    images: true,
                    tool_results: true,
                    refusal: false,
                    citations: true,
                    file_references: false,
                    json_schema_strict: true,
                },
            },
            CapabilityProtocol::Anthropic => Self {
                request: RequestCapabilityMatrix {
                    tool_definitions: true,
                    tool_role_messages: true,
                    reasoning_history: true,
                    top_k_parameter: true,
                    image_url_input: true,
                    image_inline_input: true,
                    file_url_input: true,
                    file_inline_input: true,
                    executable_code_input: true,
                },
                response: ResponseCapabilityMatrix {
                    reasoning_content: true,
                    refusal: false,
                    citations: false,
                    file_output: false,
                },
                stream: StreamCapabilityMatrix {
                    tool_call_deltas: true,
                    reasoning_deltas: true,
                    reasoning_summary_parts: false,
                    image_deltas: false,
                    blob_deltas: true,
                    structured_errors: true,
                },
                structured_content: StructuredContentCapabilityMatrix {
                    images: true,
                    tool_results: true,
                    refusal: false,
                    citations: false,
                    file_references: true,
                    json_schema_strict: true,
                },
            },
            CapabilityProtocol::Responses => Self {
                request: RequestCapabilityMatrix {
                    tool_definitions: true,
                    tool_role_messages: true,
                    reasoning_history: true,
                    top_k_parameter: false,
                    image_url_input: true,
                    image_inline_input: true,
                    file_url_input: true,
                    file_inline_input: true,
                    executable_code_input: true,
                },
                response: ResponseCapabilityMatrix {
                    reasoning_content: true,
                    refusal: true,
                    citations: true,
                    file_output: true,
                },
                stream: StreamCapabilityMatrix {
                    tool_call_deltas: true,
                    reasoning_deltas: true,
                    reasoning_summary_parts: true,
                    image_deltas: false,
                    blob_deltas: true,
                    structured_errors: true,
                },
                structured_content: StructuredContentCapabilityMatrix {
                    images: true,
                    tool_results: true,
                    refusal: true,
                    citations: true,
                    file_references: true,
                    json_schema_strict: true,
                },
            },
        }
    }
}

impl From<&UnifiedContentPart> for TransformValueKind {
    fn from(part: &UnifiedContentPart) -> Self {
        match part {
            UnifiedContentPart::Text { .. } => Self::Text,
            UnifiedContentPart::Refusal { .. } => Self::Refusal,
            UnifiedContentPart::Reasoning { .. } => Self::ReasoningContent,
            UnifiedContentPart::ImageUrl { .. } => Self::ImageUrl,
            UnifiedContentPart::ImageData { .. } => Self::ImageData,
            UnifiedContentPart::AudioData { .. } => Self::AudioData,
            UnifiedContentPart::FileUrl { .. } => Self::FileUrl,
            UnifiedContentPart::FileData { .. } => Self::FileData,
            UnifiedContentPart::FileId { .. } => Self::FileId,
            UnifiedContentPart::ExecutableCode { .. } => Self::ExecutableCode,
            UnifiedContentPart::ToolCall(_) => Self::ToolCall,
            UnifiedContentPart::ToolResult(_) => Self::ToolResult,
        }
    }
}

impl From<&UnifiedContentPartDelta> for TransformValueKind {
    fn from(part: &UnifiedContentPartDelta) -> Self {
        match part {
            UnifiedContentPartDelta::TextDelta { .. } => Self::Text,
            UnifiedContentPartDelta::ReasoningDelta { .. } => Self::ReasoningDelta,
            UnifiedContentPartDelta::ImageDelta { .. } => Self::ImageDelta,
            UnifiedContentPartDelta::ToolCallDelta(_) => Self::ToolCallDelta,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_matrix_reports_expected_responses_capabilities() {
        let caps = ProtocolCapabilityMatrix::for_downstream(DownstreamProtocol::Responses);

        assert!(caps.request.tool_definitions);
        assert!(caps.request.reasoning_history);
        assert!(caps.request.image_inline_input);
        assert!(caps.request.file_inline_input);
        assert!(caps.response.reasoning_content);
        assert!(caps.response.refusal);
        assert!(caps.response.citations);
        assert!(caps.response.file_output);
        assert!(caps.stream.tool_call_deltas);
        assert!(caps.stream.reasoning_deltas);
        assert!(caps.stream.reasoning_summary_parts);
        assert!(caps.stream.blob_deltas);
        assert!(caps.stream.structured_errors);
        assert!(caps.structured_content.images);
        assert!(caps.structured_content.tool_results);
        assert!(caps.structured_content.refusal);
        assert!(caps.structured_content.citations);
        assert!(caps.structured_content.file_references);
        assert!(caps.structured_content.json_schema_strict);
    }

    #[test]
    fn openai_declares_request_history_and_response_reasoning_separately() {
        let caps = ProtocolCapabilityMatrix::for_upstream(UpstreamProtocol::Openai);

        assert!(caps.request.reasoning_history);
        assert!(caps.response.reasoning_content);
    }

    #[test]
    fn test_capability_matrix_reports_expected_gemini_boundaries() {
        let gemini = ProtocolCapabilityMatrix::for_downstream(DownstreamProtocol::Gemini);

        assert!(gemini.request.image_url_input);
        assert!(gemini.request.reasoning_history);
        assert!(gemini.request.image_inline_input);
        assert!(!gemini.request.file_inline_input);
        assert!(gemini.response.reasoning_content);
        assert!(!gemini.response.refusal);
        assert!(gemini.response.citations);
        assert!(gemini.stream.reasoning_deltas);
        assert!(!gemini.stream.reasoning_summary_parts);
        assert!(gemini.structured_content.citations);
    }

    #[test]
    fn capability_matrix_is_declared_for_every_directional_protocol() {
        for protocol in [
            DownstreamProtocol::Openai,
            DownstreamProtocol::Gemini,
            DownstreamProtocol::Anthropic,
            DownstreamProtocol::Responses,
        ] {
            let caps = ProtocolCapabilityMatrix::for_downstream(protocol);
            assert!(caps.structured_content.json_schema_strict);
        }

        for protocol in [
            UpstreamProtocol::Openai,
            UpstreamProtocol::Gemini,
            UpstreamProtocol::Anthropic,
            UpstreamProtocol::Responses,
        ] {
            let caps = ProtocolCapabilityMatrix::for_upstream(protocol);
            assert!(caps.structured_content.json_schema_strict || !caps.request.tool_definitions);
        }

        assert_eq!(
            ProtocolCapabilityMatrix::for_downstream(DownstreamProtocol::Openai),
            ProtocolCapabilityMatrix::for_upstream(UpstreamProtocol::Openai)
        );
    }
}
