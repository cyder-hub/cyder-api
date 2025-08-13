#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LlmApiType {
    OpenAI,
    Gemini,
    Ollama,
    Anthropic,
}
