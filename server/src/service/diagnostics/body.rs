use std::{collections::BTreeMap, fmt::Display, io::Read};

use bytes::Bytes;
use flate2::read::GzDecoder;
use futures::{Stream, StreamExt};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    controller::BaseError,
    proxy::ProxyError,
    service::diagnostics::{
        policy::{
            DiagnosticsPolicy, disallowed_replay_request_header_names, redacted_header_names,
        },
        replay::types::{
            RequestReplayBody, RequestReplayBodyCaptureMetadata, RequestReplayNameValue,
        },
    },
    utils::storage::{LogBodyCaptureState, RequestLogBundleRequestSnapshot},
};

pub(crate) const REPLAY_BODY_CAPTURE_COMPLETE: &str = "complete";
pub(crate) const REPLAY_BODY_CAPTURE_INCOMPLETE: &str = "incomplete";
pub(crate) const REPLAY_BODY_CAPTURE_NOT_CAPTURED: &str = "not_captured";
pub(crate) const REPLAY_BODY_CAPTURE_NOT_EXECUTED: &str = "not_executed";

#[derive(Debug, Clone)]
pub(crate) struct ReplayResponseBodyCapture {
    pub(crate) body: Bytes,
    pub(crate) state: LogBodyCaptureState,
    pub(crate) original_size_bytes: Option<i64>,
    pub(crate) original_size_known: bool,
    pub(crate) truncated: bool,
    pub(crate) sha256: String,
    pub(crate) capture_limit_bytes: i64,
    pub(crate) body_encoding: String,
}

pub(crate) fn replay_response_capture_limit(policy: &DiagnosticsPolicy) -> usize {
    policy.response_capture_max_bytes()
}

pub(crate) async fn read_replay_response_body_bounded<S, E, F>(
    stream: S,
    is_gzip: bool,
    capture_limit_bytes: usize,
    mut map_error: F,
) -> Result<ReplayResponseBodyCapture, ProxyError>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: Display,
    F: FnMut(E) -> ProxyError,
{
    let limit = capture_limit_bytes.max(1);
    let mut encoded = Vec::new();
    let mut encoded_truncated = false;
    futures::pin_mut!(stream);

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(&mut map_error)?;
        if chunk.is_empty() {
            continue;
        }
        let remaining = limit.saturating_sub(encoded.len());
        if chunk.len() > remaining {
            encoded.extend_from_slice(&chunk[..remaining]);
            encoded_truncated = true;
            break;
        }
        encoded.extend_from_slice(&chunk);
        if encoded.len() >= limit {
            while let Some(next_result) = stream.next().await {
                let next = next_result.map_err(&mut map_error)?;
                if !next.is_empty() {
                    encoded_truncated = true;
                    break;
                }
            }
            break;
        }
    }

    let decoded = if is_gzip {
        decode_gzip_replay_capture_bounded(&encoded, encoded_truncated, limit)
    } else {
        ReplayDecodedBody {
            body: Bytes::from(encoded),
            truncated: encoded_truncated,
            decode_failed: false,
        }
    };
    let state = if decoded.truncated || decoded.decode_failed {
        LogBodyCaptureState::Incomplete
    } else {
        LogBodyCaptureState::Complete
    };
    let original_size_known = state == LogBodyCaptureState::Complete;
    let original_size_bytes = original_size_known.then_some(decoded.body.len() as i64);
    let sha256 = sha256_hex(&decoded.body);

    Ok(ReplayResponseBodyCapture {
        body: decoded.body,
        state,
        original_size_bytes,
        original_size_known,
        truncated: state == LogBodyCaptureState::Incomplete,
        sha256,
        capture_limit_bytes: limit as i64,
        body_encoding: if is_gzip && !decoded.decode_failed {
            "decoded:gzip".to_string()
        } else if is_gzip {
            "encoded:gzip-decode-failed".to_string()
        } else {
            "identity".to_string()
        },
    })
}

struct ReplayDecodedBody {
    body: Bytes,
    truncated: bool,
    decode_failed: bool,
}

fn decode_gzip_replay_capture_bounded(
    encoded: &[u8],
    encoded_truncated: bool,
    limit: usize,
) -> ReplayDecodedBody {
    if encoded.is_empty() {
        return ReplayDecodedBody {
            body: Bytes::new(),
            truncated: encoded_truncated,
            decode_failed: false,
        };
    }

    let mut decoder = GzDecoder::new(encoded);
    let mut output = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match decoder.read(&mut buf) {
            Ok(0) => {
                return ReplayDecodedBody {
                    body: Bytes::from(output),
                    truncated: encoded_truncated,
                    decode_failed: false,
                };
            }
            Ok(read) => {
                let remaining = limit.saturating_sub(output.len());
                if read > remaining {
                    output.extend_from_slice(&buf[..remaining]);
                    return ReplayDecodedBody {
                        body: Bytes::from(output),
                        truncated: true,
                        decode_failed: false,
                    };
                }
                output.extend_from_slice(&buf[..read]);
            }
            Err(_) => {
                let fallback_len = encoded.len().min(limit);
                return ReplayDecodedBody {
                    body: Bytes::copy_from_slice(&encoded[..fallback_len]),
                    truncated: encoded_truncated || encoded.len() > limit,
                    decode_failed: true,
                };
            }
        }
    }
}

pub(crate) fn replay_body_capture_metadata(
    capture: &ReplayResponseBodyCapture,
) -> RequestReplayBodyCaptureMetadata {
    RequestReplayBodyCaptureMetadata {
        state: log_capture_state_to_string(&capture.state),
        bytes_captured: capture.body.len() as i64,
        original_size_bytes: capture.original_size_bytes,
        original_size_known: capture.original_size_known,
        truncated: capture.truncated,
        sha256: capture.sha256.clone(),
        capture_limit_bytes: capture.capture_limit_bytes,
        body_encoding: capture.body_encoding.clone(),
    }
}

pub(crate) fn replay_body_capture_metadata_from_bytes(
    body: &Bytes,
    capture_state: Option<&str>,
    capture_limit_bytes: usize,
) -> RequestReplayBodyCaptureMetadata {
    let state = capture_state
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(REPLAY_BODY_CAPTURE_COMPLETE)
        .to_string();
    let truncated = state == REPLAY_BODY_CAPTURE_INCOMPLETE;
    RequestReplayBodyCaptureMetadata {
        state,
        bytes_captured: body.len() as i64,
        original_size_bytes: (!truncated).then_some(body.len() as i64),
        original_size_known: !truncated,
        truncated,
        sha256: sha256_hex(body),
        capture_limit_bytes: capture_limit_bytes.max(1) as i64,
        body_encoding: "unknown".to_string(),
    }
}

pub(crate) fn body_from_bytes(
    bytes: &Bytes,
    media_type: Option<String>,
    capture_state: Option<String>,
) -> RequestReplayBody {
    let json = serde_json::from_slice::<Value>(bytes).ok();
    let text = if json.is_none() {
        Some(String::from_utf8_lossy(bytes).to_string())
    } else {
        None
    };

    RequestReplayBody {
        media_type,
        json,
        text,
        capture_state,
    }
}

pub(crate) fn build_replay_request_headers(
    historical_headers: &HeaderMap,
) -> Result<HeaderMap, BaseError> {
    let mut headers = HeaderMap::new();
    for (name, value) in historical_headers.iter() {
        let normalized_name = name.as_str().to_ascii_lowercase();
        if disallowed_replay_request_header_names().contains(&normalized_name.as_str()) {
            continue;
        }
        headers.insert(name.clone(), value.clone());
    }

    Ok(headers)
}

pub(crate) fn build_header_map_from_name_values(
    headers: &[RequestReplayNameValue],
) -> Result<HeaderMap, BaseError> {
    let mut header_map = HeaderMap::new();
    for item in headers {
        let Some(value) = item.value.as_deref() else {
            continue;
        };
        let name = HeaderName::try_from(item.name.as_str()).map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "Invalid replay header name '{}': {}",
                item.name, err
            )))
        })?;
        let value = HeaderValue::try_from(value).map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "Invalid replay header value for '{}': {}",
                item.name, err
            )))
        })?;
        header_map.insert(name, value);
    }
    Ok(header_map)
}

pub(crate) fn parse_name_values_json_map(
    raw: &str,
    label: &str,
) -> Result<Vec<RequestReplayNameValue>, BaseError> {
    let map = serde_json::from_str::<BTreeMap<String, String>>(raw).map_err(|err| {
        BaseError::ParamInvalid(Some(format!(
            "Failed to parse replay {} JSON map: {}",
            label, err
        )))
    })?;
    Ok(map
        .into_iter()
        .map(|(name, value)| RequestReplayNameValue {
            name,
            value: Some(value),
        })
        .collect())
}

pub(crate) fn serialize_headers_for_output(
    headers: &HeaderMap,
    stripped_names: &[&str],
) -> Vec<RequestReplayNameValue> {
    let mut items = headers
        .iter()
        .filter_map(|(name, value)| {
            let normalized_name = name.as_str().to_ascii_lowercase();
            if stripped_names.contains(&normalized_name.as_str()) {
                return None;
            }

            Some(RequestReplayNameValue {
                name: normalized_name.clone(),
                value: if redacted_header_names().contains(&normalized_name.as_str()) {
                    None
                } else {
                    Some(value.to_str().unwrap_or("").to_string())
                },
            })
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.value.cmp(&right.value))
    });
    items
}

pub(crate) fn canonical_name_values(
    items: &[RequestReplayNameValue],
    lowercase_names: bool,
) -> Vec<RequestReplayNameValue> {
    let mut values = items
        .iter()
        .map(|item| RequestReplayNameValue {
            name: if lowercase_names {
                item.name.to_ascii_lowercase()
            } else {
                item.name.clone()
            },
            value: item.value.clone(),
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.value.cmp(&right.value))
    });
    values
}

pub(crate) fn header_map_from_snapshot(
    snapshot: &RequestLogBundleRequestSnapshot,
) -> Result<HeaderMap, BaseError> {
    let mut headers = HeaderMap::new();
    for item in &snapshot.sanitized_original_headers {
        let name = HeaderName::try_from(item.name.as_str()).map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "Invalid gateway replay snapshot header name '{}': {}",
                item.name, err
            )))
        })?;
        let value = HeaderValue::try_from(item.value.as_str()).map_err(|err| {
            BaseError::ParamInvalid(Some(format!(
                "Invalid gateway replay snapshot header value for '{}': {}",
                item.name, err
            )))
        })?;
        headers.insert(name, value);
    }
    Ok(headers)
}

pub(crate) fn normalized_name_values(
    items: &[RequestReplayNameValue],
) -> BTreeMap<String, Option<String>> {
    items
        .iter()
        .map(|item| (item.name.to_ascii_lowercase(), item.value.clone()))
        .collect()
}

pub(crate) struct ReplayBodyComparison {
    pub(crate) changed: Option<bool>,
    pub(crate) reason: String,
    pub(crate) partial: bool,
}

pub(crate) fn compare_replay_body_capture(
    baseline: Option<(&[u8], Option<&str>)>,
    replay: Option<(&[u8], Option<&str>)>,
    missing_reason: &'static str,
    incomplete_reason: &'static str,
) -> ReplayBodyComparison {
    let (Some((baseline_body, baseline_state)), Some((replay_body, replay_state))) =
        (baseline, replay)
    else {
        return ReplayBodyComparison {
            changed: None,
            reason: missing_reason.to_string(),
            partial: true,
        };
    };

    let baseline_complete = replay_capture_state_is_complete(baseline_state);
    let replay_complete = replay_capture_state_is_complete(replay_state);
    if baseline_complete && replay_complete {
        return ReplayBodyComparison {
            changed: Some(!body_bytes_equal(baseline_body, replay_body)),
            reason: String::new(),
            partial: false,
        };
    }

    let comparable_len = baseline_body.len().min(replay_body.len());
    let changed = if baseline_body[..comparable_len] != replay_body[..comparable_len] {
        Some(true)
    } else {
        None
    };

    ReplayBodyComparison {
        changed,
        reason: incomplete_reason.to_string(),
        partial: true,
    }
}

fn replay_capture_state_is_complete(state: Option<&str>) -> bool {
    !matches!(
        state,
        Some(REPLAY_BODY_CAPTURE_INCOMPLETE)
            | Some(REPLAY_BODY_CAPTURE_NOT_CAPTURED)
            | Some(REPLAY_BODY_CAPTURE_NOT_EXECUTED)
    )
}

fn body_bytes_equal(left: &[u8], right: &[u8]) -> bool {
    match (
        serde_json::from_slice::<Value>(left),
        serde_json::from_slice::<Value>(right),
    ) {
        (Ok(left_json), Ok(right_json)) => left_json == right_json,
        _ => left == right,
    }
}

pub(crate) fn log_capture_state_to_string(state: &LogBodyCaptureState) -> String {
    match state {
        LogBodyCaptureState::Complete => REPLAY_BODY_CAPTURE_COMPLETE,
        LogBodyCaptureState::Incomplete => REPLAY_BODY_CAPTURE_INCOMPLETE,
        LogBodyCaptureState::NotCaptured => REPLAY_BODY_CAPTURE_NOT_CAPTURED,
    }
    .to_string()
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};
    use reqwest::header::CONTENT_TYPE;

    use super::*;

    #[test]
    fn replay_response_capture_limit_uses_supplied_policy() {
        let mut config = crate::config::DiagnosticsConfig::default();
        config.response_capture_max_bytes = 2048;
        let policy = DiagnosticsPolicy::from_config(&config);

        assert_eq!(replay_response_capture_limit(&policy), 2048);
    }

    #[tokio::test]
    async fn replay_response_capture_marks_large_plain_body_incomplete() {
        let limit = 32usize;
        let stream = futures::stream::iter(vec![
            Ok::<Bytes, std::io::Error>(Bytes::from(vec![b'a'; 20])),
            Ok::<Bytes, std::io::Error>(Bytes::from(vec![b'b'; 20])),
        ]);

        let capture = read_replay_response_body_bounded(stream, false, limit, |err| {
            ProxyError::BadGateway(err.to_string())
        })
        .await
        .expect("capture should succeed");

        assert_eq!(capture.state, LogBodyCaptureState::Incomplete);
        assert_eq!(capture.body.len(), limit);
        assert!(capture.truncated);
        let metadata = replay_body_capture_metadata(&capture);
        assert_eq!(metadata.bytes_captured, limit as i64);
        assert!(!metadata.original_size_known);
        assert_eq!(metadata.body_encoding, "identity");
    }

    #[tokio::test]
    async fn replay_response_capture_limits_gzip_after_decode() {
        let limit = 64usize;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&vec![b'x'; limit * 4])
            .expect("gzip write should succeed");
        let compressed = encoder.finish().expect("gzip finish should succeed");
        let stream =
            futures::stream::iter(vec![Ok::<Bytes, std::io::Error>(Bytes::from(compressed))]);

        let capture = read_replay_response_body_bounded(stream, true, limit, |err| {
            ProxyError::BadGateway(err.to_string())
        })
        .await
        .expect("capture should succeed");

        assert_eq!(capture.state, LogBodyCaptureState::Incomplete);
        assert_eq!(capture.body.len(), limit);
        assert_eq!(capture.body, Bytes::from(vec![b'x'; limit]));
        assert_eq!(capture.body_encoding, "decoded:gzip");
    }

    #[test]
    fn body_from_bytes_uses_json_when_available_and_text_otherwise() {
        let json_body = body_from_bytes(
            &Bytes::from_static(br#"{"ok":true}"#),
            Some("application/json".to_string()),
            Some(REPLAY_BODY_CAPTURE_COMPLETE.to_string()),
        );
        assert_eq!(json_body.json, Some(serde_json::json!({"ok": true})));
        assert_eq!(json_body.text, None);

        let text_body = body_from_bytes(
            &Bytes::from_static(b"plain response"),
            Some("text/plain".to_string()),
            Some(REPLAY_BODY_CAPTURE_COMPLETE.to_string()),
        );
        assert_eq!(text_body.json, None);
        assert_eq!(text_body.text.as_deref(), Some("plain response"));
    }

    #[test]
    fn build_replay_request_headers_only_rebuilds_safe_historical_headers() {
        let mut historical_headers = HeaderMap::new();
        historical_headers.insert("authorization", HeaderValue::from_static("Bearer stale"));
        historical_headers.insert("x-api-key", HeaderValue::from_static("stale-key"));
        historical_headers.insert("x-goog-api-key", HeaderValue::from_static("stale-goog"));
        historical_headers.insert("cookie", HeaderValue::from_static("session=stale"));
        historical_headers.insert("host", HeaderValue::from_static("stale.example"));
        historical_headers.insert("content-length", HeaderValue::from_static("999"));
        historical_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        historical_headers.insert("x-trace-id", HeaderValue::from_static("trace-1"));

        let headers =
            build_replay_request_headers(&historical_headers).expect("headers should rebuild");

        assert!(headers.get("authorization").is_none());
        assert!(headers.get("x-api-key").is_none());
        assert!(headers.get("x-goog-api-key").is_none());
        assert!(headers.get("cookie").is_none());
        assert!(headers.get("host").is_none());
        assert!(headers.get("content-length").is_none());
        assert_eq!(
            headers
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            headers
                .get("x-trace-id")
                .and_then(|value| value.to_str().ok()),
            Some("trace-1")
        );
    }

    #[test]
    fn compare_replay_body_capture_keeps_incomplete_matches_partial() {
        let comparison = compare_replay_body_capture(
            Some((b"abcdef", Some(REPLAY_BODY_CAPTURE_INCOMPLETE))),
            Some((b"abcdef", Some(REPLAY_BODY_CAPTURE_COMPLETE))),
            "missing",
            "incomplete",
        );

        assert_eq!(comparison.changed, None);
        assert!(comparison.partial);
        assert_eq!(comparison.reason, "incomplete");
    }
}
