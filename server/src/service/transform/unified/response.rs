use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::request::{UnifiedItem, UnifiedMessage, legacy_content_to_unified_items};
use super::usage::UnifiedUsage;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedSyntheticMetadata {
    #[serde(default)]
    pub id: bool,
    #[serde(default)]
    pub model: bool,
    #[serde(default)]
    pub gemini_safety_ratings: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedOpenAiResponseExtension {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
}

impl UnifiedOpenAiResponseExtension {
    pub fn is_empty(&self) -> bool {
        self.system_fingerprint.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedGeminiSafetyRating {
    pub category: String,
    pub probability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedGeminiCitationSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedGeminiCitationMetadata {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub citation_sources: Vec<UnifiedGeminiCitationSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedGeminiPromptFeedback {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub safety_ratings: Vec<UnifiedGeminiSafetyRating>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedGeminiCandidateMetadata {
    pub index: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub safety_ratings: Vec<UnifiedGeminiSafetyRating>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citation_metadata: Option<UnifiedGeminiCitationMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedGeminiResponseMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_feedback: Option<UnifiedGeminiPromptFeedback>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<UnifiedGeminiCandidateMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedAnthropicResponseMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedResponsesUrlCitation {
    pub url: String,
    pub start_index: u32,
    pub end_index: u32,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedResponsesRefusal {
    pub refusal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedResponsesFileReference {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UnifiedResponsesIncompleteDetails {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedResponsesResponseMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub citations: Vec<UnifiedResponsesUrlCitation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refusals: Vec<UnifiedResponsesRefusal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<UnifiedResponsesFileReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incomplete_details: Option<UnifiedResponsesIncompleteDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedProviderResponseMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini: Option<UnifiedGeminiResponseMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<UnifiedAnthropicResponseMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responses: Option<UnifiedResponsesResponseMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedProviderSessionMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini: Option<UnifiedGeminiResponseMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<UnifiedAnthropicResponseMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responses: Option<UnifiedResponsesResponseMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedResponseExtensions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai: Option<UnifiedOpenAiResponseExtension>,
}

impl UnifiedResponseExtensions {
    pub fn is_empty(&self) -> bool {
        self.openai
            .as_ref()
            .is_none_or(UnifiedOpenAiResponseExtension::is_empty)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedResponseCore {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub choices: Vec<UnifiedChoice>,
    pub usage: Option<UnifiedUsage>,
    pub created: Option<i64>,
    pub object: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedResponseContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<UnifiedResponseExtensions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<UnifiedProviderResponseMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthetic_metadata: Option<UnifiedSyntheticMetadata>,
}

impl UnifiedResponseContext {
    pub fn is_empty(&self) -> bool {
        self.extensions
            .as_ref()
            .is_none_or(UnifiedResponseExtensions::is_empty)
            && self.provider_metadata.is_none()
            && self.synthetic_metadata.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedChoice {
    pub index: u32,
    pub message: UnifiedMessage,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<UnifiedItem>,
    pub finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<Value>,
}

impl UnifiedChoice {
    pub fn content_items(&self) -> Vec<UnifiedItem> {
        if !self.items.is_empty() {
            return self.items.clone();
        }

        legacy_content_to_unified_items(self.message.role.clone(), self.message.content.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub choices: Vec<UnifiedChoice>,
    pub usage: Option<UnifiedUsage>,
    pub created: Option<i64>,
    pub object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_response_metadata: Option<UnifiedProviderResponseMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthetic_metadata: Option<UnifiedSyntheticMetadata>,
}

impl UnifiedResponse {
    pub fn core(&self) -> UnifiedResponseCore {
        UnifiedResponseCore {
            id: self.id.clone(),
            model: self.model.clone(),
            choices: self.choices.clone(),
            usage: self.usage.clone(),
            created: self.created,
            object: self.object.clone(),
        }
    }

    pub fn extensions(&self) -> Option<UnifiedResponseExtensions> {
        let openai = (!UnifiedOpenAiResponseExtension {
            system_fingerprint: self.system_fingerprint.clone(),
        }
        .is_empty())
        .then(|| UnifiedOpenAiResponseExtension {
            system_fingerprint: self.system_fingerprint.clone(),
        });

        let extensions = UnifiedResponseExtensions { openai };
        (!extensions.is_empty()).then_some(extensions)
    }

    pub fn context(&self) -> UnifiedResponseContext {
        UnifiedResponseContext {
            extensions: self.extensions(),
            provider_metadata: self.provider_response_metadata.clone(),
            synthetic_metadata: self.synthetic_metadata.clone(),
        }
    }

    pub fn from_core_and_context(
        core: UnifiedResponseCore,
        context: UnifiedResponseContext,
    ) -> Self {
        Self {
            id: core.id,
            model: core.model,
            choices: core.choices,
            usage: core.usage,
            created: core.created,
            object: core.object,
            system_fingerprint: context
                .extensions
                .and_then(|ext| ext.openai)
                .and_then(|openai| openai.system_fingerprint),
            provider_response_metadata: context.provider_metadata,
            synthetic_metadata: context.synthetic_metadata,
        }
    }

    pub fn into_core_and_context(self) -> (UnifiedResponseCore, UnifiedResponseContext) {
        let system_fingerprint = self.system_fingerprint;
        (
            UnifiedResponseCore {
                id: self.id,
                model: self.model,
                choices: self.choices,
                usage: self.usage,
                created: self.created,
                object: self.object,
            },
            UnifiedResponseContext {
                extensions: (!UnifiedOpenAiResponseExtension {
                    system_fingerprint: system_fingerprint.clone(),
                }
                .is_empty())
                .then(|| UnifiedResponseExtensions {
                    openai: Some(UnifiedOpenAiResponseExtension { system_fingerprint }),
                }),
                provider_metadata: self.provider_response_metadata,
                synthetic_metadata: self.synthetic_metadata,
            },
        )
    }

    pub fn system_fingerprint(&self) -> Option<&str> {
        self.system_fingerprint.as_deref()
    }

    pub fn synthetic_metadata(&self) -> Option<&UnifiedSyntheticMetadata> {
        self.synthetic_metadata.as_ref()
    }

    pub fn provider_response_metadata(&self) -> Option<&UnifiedProviderResponseMetadata> {
        self.provider_response_metadata.as_ref()
    }
}
