use serde_json::{Value, json};
use thiserror::Error;

use crate::schema::enum_def::DownstreamProtocol;
use crate::utils::sse::SseEvent;

#[derive(Debug, Clone, Copy)]
pub(crate) struct FatalStreamErrorFact<'a> {
    pub request_id: &'a str,
    pub code: &'a str,
    pub category: &'a str,
    pub public_message: &'a str,
    pub http_status: u16,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub(crate) enum FatalStreamEncodeError {
    #[error("fatal stream error request identity is invalid")]
    InvalidRequestIdentity,
    #[error("fatal stream error serialization failed")]
    Serialization,
}

pub(crate) fn encode_fatal_stream_error(
    downstream_protocol: DownstreamProtocol,
    fact: FatalStreamErrorFact<'_>,
) -> Result<SseEvent, FatalStreamEncodeError> {
    if fact.request_id.is_empty()
        || fact
            .request_id
            .chars()
            .any(|character| character.is_control())
    {
        return Err(FatalStreamEncodeError::InvalidRequestIdentity);
    }

    let (event, payload) = match downstream_protocol {
        DownstreamProtocol::Openai => (
            None,
            json!({
                "error": {
                    "message": fact.public_message,
                    "type": fact.category,
                    "param": null,
                    "code": fact.code
                },
                "request_id": fact.request_id
            }),
        ),
        DownstreamProtocol::Responses => (
            None,
            json!({
                "type": "response.error",
                "error": {
                    "type": fact.category,
                    "code": fact.code,
                    "message": fact.public_message
                },
                "request_id": fact.request_id
            }),
        ),
        DownstreamProtocol::Anthropic => (
            Some("error".to_string()),
            json!({
                "type": "error",
                "error": {
                    "type": fact.category,
                    "message": fact.public_message,
                    "code": fact.code
                },
                "request_id": fact.request_id
            }),
        ),
        DownstreamProtocol::Gemini => (
            None,
            json!({
                "error": {
                    "code": fact.http_status,
                    "message": fact.public_message,
                    "status": fact.category,
                    "details": [{
                        "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                        "reason": fact.code.to_ascii_uppercase(),
                        "domain": "cyder.gateway",
                        "metadata": {"request_id": fact.request_id}
                    }]
                }
            }),
        ),
    };

    serialize_terminal_event(event, payload)
}

fn serialize_terminal_event(
    event: Option<String>,
    payload: Value,
) -> Result<SseEvent, FatalStreamEncodeError> {
    let data =
        serde_json::to_string(&payload).map_err(|_| FatalStreamEncodeError::Serialization)?;
    Ok(SseEvent {
        event,
        data,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST_ID: &str = "req-terminal-1";
    const CODE: &str = "protocol_transform_error";
    const MESSAGE: &str = "The gateway could not safely transform the provider response.";
    const OPENAI_GOLDEN: &str = include_str!("../testdata/fatal_stream_error_openai.sse");
    const RESPONSES_GOLDEN: &str = include_str!("../testdata/fatal_stream_error_responses.sse");
    const ANTHROPIC_GOLDEN: &str = include_str!("../testdata/fatal_stream_error_anthropic.sse");
    const GEMINI_GOLDEN: &str = include_str!("../testdata/fatal_stream_error_gemini.sse");

    fn fact(category: &'static str) -> FatalStreamErrorFact<'static> {
        FatalStreamErrorFact {
            request_id: REQUEST_ID,
            code: CODE,
            category,
            public_message: MESSAGE,
            http_status: 500,
        }
    }

    #[test]
    fn four_downstream_terminal_errors_have_exact_native_framing() {
        let cases = [
            (
                DownstreamProtocol::Openai,
                "server_error",
                None,
                json!({
                    "error": {"message": MESSAGE, "type": "server_error", "param": null, "code": CODE},
                    "request_id": REQUEST_ID
                }),
                OPENAI_GOLDEN,
            ),
            (
                DownstreamProtocol::Responses,
                "server_error",
                None,
                json!({
                    "type": "response.error",
                    "error": {"type": "server_error", "code": CODE, "message": MESSAGE},
                    "request_id": REQUEST_ID
                }),
                RESPONSES_GOLDEN,
            ),
            (
                DownstreamProtocol::Anthropic,
                "api_error",
                Some("error"),
                json!({
                    "type": "error",
                    "error": {"type": "api_error", "message": MESSAGE, "code": CODE},
                    "request_id": REQUEST_ID
                }),
                ANTHROPIC_GOLDEN,
            ),
            (
                DownstreamProtocol::Gemini,
                "INTERNAL",
                None,
                json!({
                    "error": {
                        "code": 500,
                        "message": MESSAGE,
                        "status": "INTERNAL",
                        "details": [{
                            "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                            "reason": "PROTOCOL_TRANSFORM_ERROR",
                            "domain": "cyder.gateway",
                            "metadata": {"request_id": REQUEST_ID}
                        }]
                    }
                }),
                GEMINI_GOLDEN,
            ),
        ];

        for (protocol, category, expected_event, expected_payload, golden) in cases {
            let event = encode_fatal_stream_error(protocol, fact(category))
                .expect("terminal error must encode");
            assert_eq!(event.event.as_deref(), expected_event, "{protocol:?}");
            assert_eq!(
                serde_json::from_str::<Value>(&event.data).unwrap(),
                expected_payload,
                "{protocol:?}"
            );
            let framing = String::from_utf8(event.to_bytes().to_vec()).unwrap();
            assert_eq!(framing, golden, "{protocol:?} golden framing drifted");
            assert_eq!(framing.matches("data: ").count(), 1, "{protocol:?}");
            assert!(!framing.contains("[DONE]"), "{protocol:?}");
            assert!(!framing.contains("message_stop"), "{protocol:?}");
            assert!(!framing.contains("response.completed"), "{protocol:?}");
            assert!(!framing.contains("transform_diagnostic"), "{protocol:?}");
        }
    }

    #[test]
    fn terminal_error_contains_no_operator_or_diagnostic_details() {
        let event = encode_fatal_stream_error(DownstreamProtocol::Openai, fact("server_error"))
            .expect("terminal error");
        for forbidden in [
            "operator_message",
            "safe_summary",
            "sha256",
            "tool_arguments",
            "raw_parse_error",
        ] {
            assert!(!event.data.contains(forbidden));
        }
    }

    #[test]
    fn invalid_request_identity_is_an_explicit_encoder_failure() {
        let error = encode_fatal_stream_error(
            DownstreamProtocol::Anthropic,
            FatalStreamErrorFact {
                request_id: "bad\nrequest",
                ..fact("api_error")
            },
        )
        .expect_err("control characters must fail encoding");
        assert_eq!(error, FatalStreamEncodeError::InvalidRequestIdentity);
    }
}
