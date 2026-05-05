use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::service::transform::unified::{UnifiedFunctionDefinition, UnifiedSyntheticMetadata};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct GeminiRequestPayload {
    pub(crate) contents: Vec<GeminiRequestContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) system_instruction: Option<GeminiSystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<Vec<GeminiTools>>,
    #[serde(rename = "generationConfig")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) generation_config: Option<GeminiGenerationConfig>,
    #[serde(rename = "safetySettings")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) safety_settings: Option<Vec<GeminiSafetySetting>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum GeminiSystemInstruction {
    String(String),
    Object { parts: Vec<GeminiPart> },
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct GeminiRequestContent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<String>,
    pub(crate) parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct GeminiResponseContent {
    pub(crate) role: String,
    pub(crate) parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum GeminiPart {
    Text {
        text: String,
    },
    ExecutableCode {
        #[serde(rename = "executableCode")]
        executable_code: GeminiExecutableCode,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
    InlineData {
        #[serde(rename = "inlineData")]
        inline_data: GeminiInlineData,
    },
    FileData {
        #[serde(rename = "fileData")]
        file_data: GeminiFileData,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct GeminiExecutableCode {
    pub(crate) language: String,
    pub(crate) code: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct GeminiFunctionCall {
    pub(crate) name: String,
    pub(crate) args: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct GeminiFunctionResponse {
    pub(crate) name: String,
    pub(crate) response: Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiInlineData {
    pub(crate) mime_type: String,
    pub(crate) data: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiFileData {
    pub(crate) mime_type: String,
    pub(crate) file_uri: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct GeminiTools {
    #[serde(rename = "functionDeclarations")]
    pub(crate) function_declarations: Vec<UnifiedFunctionDefinition>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature: Option<f64>,
    #[serde(rename = "maxOutputTokens")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_output_tokens: Option<u32>,
    #[serde(rename = "topP")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) top_p: Option<f64>,
    #[serde(rename = "stopSequences")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stop_sequences: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiSafetySetting {
    pub(crate) category: String,
    pub(crate) threshold: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiChunkResponse {
    pub(crate) candidates: Vec<GeminiCandidate>,
    #[serde(rename = "promptFeedback")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) prompt_feedback: Option<GeminiPromptFeedback>,
    #[serde(rename = "usageMetadata")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) usage_metadata: Option<GeminiChunkUsageMetadata>,
    #[serde(skip)]
    pub(crate) synthetic_metadata: Option<UnifiedSyntheticMetadata>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiCandidate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) content: Option<GeminiResponseContent>,
    #[serde(rename = "finishReason")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) finish_reason: Option<String>,
    #[serde(rename = "safetyRatings")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) safety_ratings: Option<Vec<GeminiSafetyRating>>,
    #[serde(rename = "tokenCount")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) token_count: Option<u32>,
    #[serde(rename = "citationMetadata")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) citation_metadata: Option<GeminiCitationMetadata>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum Modality {
    ModalityUnspecified,
    Text,
    Image,
    Video,
    Audio,
    Document,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModalityTokenCount {
    pub(crate) modality: Modality,
    pub(crate) token_count: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiUsageMetadata {
    pub(crate) prompt_token_count: u32,
    pub(crate) candidates_token_count: u32,
    pub(crate) total_token_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) thoughts_token_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cached_content_token_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_use_prompt_token_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) prompt_tokens_details: Vec<ModalityTokenCount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) cache_tokens_details: Vec<ModalityTokenCount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) candidates_tokens_details: Vec<ModalityTokenCount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) tool_use_prompt_tokens_details: Vec<ModalityTokenCount>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiChunkUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    pub(crate) prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) candidates_token_count: Option<u32>,
    #[serde(rename = "totalTokenCount")]
    pub(crate) total_token_count: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiSafetyRating {
    pub(crate) category: String,
    pub(crate) probability: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiCitationMetadata {
    pub(crate) citation_sources: Vec<GeminiCitationSource>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiCitationSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) start_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) end_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) license: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiPromptFeedback {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) block_reason: Option<String>,
    pub(crate) safety_ratings: Vec<GeminiSafetyRating>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiResponse {
    pub(crate) candidates: Vec<GeminiCandidate>,
    #[serde(rename = "promptFeedback")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) prompt_feedback: Option<GeminiPromptFeedback>,
    #[serde(rename = "usageMetadata")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) usage_metadata: Option<GeminiUsageMetadata>,
    #[serde(skip)]
    pub(crate) synthetic_metadata: Option<UnifiedSyntheticMetadata>,
}
