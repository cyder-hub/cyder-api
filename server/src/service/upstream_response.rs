use std::io::{self, Write};

use bytes::{Bytes, BytesMut};
use flate2::write::MultiGzDecoder;
use futures::StreamExt;
use reqwest::{
    Response,
    header::{
        ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, HeaderValue,
    },
};
use thiserror::Error;
use tokio::time::{Duration, Instant, timeout_at};

use crate::{config::NonStreamResponseConfig, proxy::TimeoutPhase};

const MAX_CONTENT_TYPE_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpstreamContentEncoding {
    Identity,
    Gzip,
}

impl UpstreamContentEncoding {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Gzip => "gzip",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpstreamHttpErrorKind {
    Timeout,
    Connect,
    Body,
    Decode,
    Request,
    Status,
    Other,
}

impl UpstreamHttpErrorKind {
    pub(crate) fn from_reqwest(error: &reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else if error.is_connect() {
            Self::Connect
        } else if error.is_body() {
            Self::Body
        } else if error.is_decode() {
            Self::Decode
        } else if error.is_request() {
            Self::Request
        } else if error.status().is_some() {
            Self::Status
        } else {
            Self::Other
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Connect => "connect",
            Self::Body => "body",
            Self::Decode => "decode",
            Self::Request => "request",
            Self::Status => "status",
            Self::Other => "other",
        }
    }
}

#[cfg(test)]
pub(crate) fn safe_http_error_message(context: &'static str, error: &reqwest::Error) -> String {
    format!(
        "{context} ({})",
        UpstreamHttpErrorKind::from_reqwest(error).as_str()
    )
}

pub(crate) fn apply_upstream_accept_encoding(headers: &mut HeaderMap, is_stream: bool) {
    let value = if is_stream {
        HeaderValue::from_static("identity")
    } else {
        HeaderValue::from_static("gzip, identity")
    };
    headers.insert(ACCEPT_ENCODING, value);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResponseBodyLimitKind {
    Raw,
    Decoded,
}

impl ResponseBodyLimitKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Decoded => "decoded",
        }
    }
}

#[derive(Debug)]
pub(crate) struct CompleteResponseBody {
    pub(crate) bytes: Bytes,
    pub(crate) raw_bytes: usize,
    pub(crate) decoded_bytes: usize,
    pub(crate) encoding: UpstreamContentEncoding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResponseBodyReadTimeouts {
    pub(crate) first_byte: Duration,
    pub(crate) response_idle: Duration,
}

impl ResponseBodyReadTimeouts {
    fn for_next_chunk(self, saw_nonempty_chunk: bool) -> (Duration, TimeoutPhase) {
        if saw_nonempty_chunk {
            (self.response_idle, TimeoutPhase::ResponseIdle)
        } else {
            (self.first_byte, TimeoutPhase::FirstByte)
        }
    }
}

pub(crate) struct CapturedErrorBody {
    pub(crate) captured: Bytes,
    pub(crate) raw_bytes: usize,
    pub(crate) decoded_bytes: usize,
    pub(crate) encoding: UpstreamContentEncoding,
    pub(crate) truncated: bool,
    pub(crate) hard_limit_reached: Option<ResponseBodyLimitKind>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NormalizedContentType {
    pub(crate) essence: String,
    pub(crate) value: String,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum UpstreamResponseReadError {
    #[error("upstream response timed out during {phase:?}")]
    Timeout { phase: TimeoutPhase },
    #[error("upstream response Content-Encoding is invalid or unsupported")]
    InvalidContentEncoding,
    #[error("upstream response Content-Length is invalid or conflicting")]
    InvalidContentLength,
    #[error(
        "upstream raw response body exceeded {limit_bytes} bytes (observed at least {observed_bytes})"
    )]
    RawBodyLimitExceeded {
        limit_bytes: usize,
        observed_bytes: usize,
    },
    #[error(
        "upstream decoded response body exceeded {limit_bytes} bytes (observed at least {observed_bytes})"
    )]
    DecodedBodyLimitExceeded {
        limit_bytes: usize,
        observed_bytes: usize,
    },
    #[error("upstream gzip response body is truncated, corrupt, or has trailing data")]
    InvalidGzip,
    #[error("upstream response transport failed ({kind})")]
    Transport { kind: &'static str },
}

impl UpstreamResponseReadError {
    fn transport(error: &reqwest::Error) -> Self {
        Self::Transport {
            kind: UpstreamHttpErrorKind::from_reqwest(error).as_str(),
        }
    }

    pub(crate) fn is_timeout(&self) -> bool {
        match self {
            Self::Timeout { .. } => true,
            Self::Transport { kind } => *kind == UpstreamHttpErrorKind::Timeout.as_str(),
            _ => false,
        }
    }

    pub(crate) fn timeout_phase(&self) -> Option<TimeoutPhase> {
        match self {
            Self::Timeout { phase } => Some(*phase),
            _ => None,
        }
    }
}

pub(crate) fn parse_content_encoding(
    headers: &HeaderMap,
) -> Result<UpstreamContentEncoding, UpstreamResponseReadError> {
    let values = headers.get_all(CONTENT_ENCODING);
    let mut iter = values.iter();
    let Some(value) = iter.next() else {
        return Ok(UpstreamContentEncoding::Identity);
    };
    if iter.next().is_some() {
        return Err(UpstreamResponseReadError::InvalidContentEncoding);
    }

    let token = value
        .to_str()
        .map_err(|_| UpstreamResponseReadError::InvalidContentEncoding)?
        .trim();
    if token.is_empty() || token.contains(',') {
        return Err(UpstreamResponseReadError::InvalidContentEncoding);
    }
    if token.eq_ignore_ascii_case("identity") {
        Ok(UpstreamContentEncoding::Identity)
    } else if token.eq_ignore_ascii_case("gzip") {
        Ok(UpstreamContentEncoding::Gzip)
    } else {
        Err(UpstreamResponseReadError::InvalidContentEncoding)
    }
}

pub(crate) fn normalize_content_type(headers: &HeaderMap) -> Option<NormalizedContentType> {
    let values = headers.get_all(CONTENT_TYPE);
    let mut iter = values.iter();
    let value = iter.next()?;
    if iter.next().is_some() {
        return None;
    }
    let raw = value.to_str().ok()?.trim();
    if raw.is_empty() || raw.len() > MAX_CONTENT_TYPE_BYTES {
        return None;
    }

    let parsed = raw.parse::<mime::Mime>().ok()?;
    let essence = parsed.essence_str().to_ascii_lowercase();
    let preserve_utf8 = parsed
        .params()
        .filter(|(name, _)| *name == mime::CHARSET)
        .all(|(_, value)| value.as_str().eq_ignore_ascii_case("utf-8"))
        && parsed.params().any(|(name, _)| name == mime::CHARSET);
    let value = if preserve_utf8 {
        format!("{essence}; charset=utf-8")
    } else {
        essence.clone()
    };
    Some(NormalizedContentType { essence, value })
}

pub(crate) async fn read_complete_response_body(
    response: Response,
    limits: &NonStreamResponseConfig,
) -> Result<CompleteResponseBody, UpstreamResponseReadError> {
    read_complete_response_body_with_timeouts(response, limits, None, |_, _, _| {}).await
}

pub(crate) async fn read_complete_response_body_with_timeouts<F>(
    response: Response,
    limits: &NonStreamResponseConfig,
    read_timeouts: Option<ResponseBodyReadTimeouts>,
    mut observe_chunk: F,
) -> Result<CompleteResponseBody, UpstreamResponseReadError>
where
    F: FnMut(&Bytes, Instant, Instant) + Send,
{
    let encoding = parse_content_encoding(response.headers())?;
    if let Some(content_length) = parse_content_length(response.headers())? {
        if content_length > limits.raw_body_limit_bytes {
            return Err(UpstreamResponseReadError::RawBodyLimitExceeded {
                limit_bytes: limits.raw_body_limit_bytes,
                observed_bytes: content_length,
            });
        }
    }

    match read_response_body(
        response,
        limits,
        usize::MAX,
        false,
        encoding,
        read_timeouts,
        &mut observe_chunk,
    )
    .await?
    {
        ReadBodyOutcome::Complete(body) => Ok(body),
        ReadBodyOutcome::Captured(_) => unreachable!("complete mode must return a complete body"),
    }
}

#[cfg(test)]
pub(crate) async fn capture_error_response_body(
    response: Response,
    limits: &NonStreamResponseConfig,
    disclosure_limit: usize,
) -> Result<CapturedErrorBody, UpstreamResponseReadError> {
    capture_error_response_body_with_timeouts(response, limits, disclosure_limit, None).await
}

pub(crate) async fn capture_error_response_body_with_timeouts(
    response: Response,
    limits: &NonStreamResponseConfig,
    disclosure_limit: usize,
    read_timeouts: Option<ResponseBodyReadTimeouts>,
) -> Result<CapturedErrorBody, UpstreamResponseReadError> {
    let encoding = parse_content_encoding(response.headers())?;
    match read_response_body(
        response,
        limits,
        disclosure_limit,
        true,
        encoding,
        read_timeouts,
        &mut |_, _, _| {},
    )
    .await?
    {
        ReadBodyOutcome::Captured(body) => Ok(body),
        ReadBodyOutcome::Complete(_) => unreachable!("capture mode must return a captured body"),
    }
}

enum ReadBodyOutcome {
    Complete(CompleteResponseBody),
    Captured(CapturedErrorBody),
}

async fn read_response_body(
    response: Response,
    limits: &NonStreamResponseConfig,
    capture_limit: usize,
    capture_mode: bool,
    encoding: UpstreamContentEncoding,
    read_timeouts: Option<ResponseBodyReadTimeouts>,
    observe_chunk: &mut (dyn FnMut(&Bytes, Instant, Instant) + Send),
) -> Result<ReadBodyOutcome, UpstreamResponseReadError> {
    let collector = DecodedCollector::new(limits.decoded_body_limit_bytes, capture_limit);
    let mut decoder = ResponseDecoder::new(encoding, collector);
    let mut raw_bytes = 0usize;
    let mut stream = response.bytes_stream();

    let mut saw_nonempty_chunk = false;
    let mut active_wait_deadline = None;
    loop {
        let wait_started_at = Instant::now();
        let next_chunk = stream.next();
        let chunk = match read_timeouts {
            Some(timeouts) => {
                let (duration, phase) = timeouts.for_next_chunk(saw_nonempty_chunk);
                let deadline =
                    *active_wait_deadline.get_or_insert_with(|| wait_started_at + duration);
                match timeout_at(deadline, next_chunk).await {
                    Ok(Some(chunk)) => chunk,
                    Ok(None) => break,
                    Err(_) => return Err(UpstreamResponseReadError::Timeout { phase }),
                }
            }
            None => match next_chunk.await {
                Some(chunk) => chunk,
                None => break,
            },
        };
        let chunk = chunk.map_err(|error| UpstreamResponseReadError::transport(&error))?;
        let received_at = Instant::now();
        observe_chunk(&chunk, wait_started_at, received_at);
        if !chunk.is_empty() {
            saw_nonempty_chunk = true;
            active_wait_deadline = None;
        }
        let remaining_raw = limits.raw_body_limit_bytes.saturating_sub(raw_bytes);
        if chunk.len() > remaining_raw {
            if !capture_mode {
                return Err(UpstreamResponseReadError::RawBodyLimitExceeded {
                    limit_bytes: limits.raw_body_limit_bytes,
                    observed_bytes: limits.raw_body_limit_bytes.saturating_add(1),
                });
            }
            if remaining_raw > 0 {
                match decoder.write_all(&chunk[..remaining_raw]) {
                    Ok(()) => {}
                    Err(DecodeFailure::DecodedLimit) => {
                        return Ok(ReadBodyOutcome::Captured(captured_from_decoder(
                            &decoder,
                            raw_bytes.saturating_add(chunk.len()),
                            encoding,
                            Some(ResponseBodyLimitKind::Decoded),
                        )));
                    }
                    Err(DecodeFailure::InvalidGzip) => {
                        return Err(UpstreamResponseReadError::InvalidGzip);
                    }
                }
            }
            return Ok(ReadBodyOutcome::Captured(captured_from_decoder(
                &decoder,
                limits.raw_body_limit_bytes.saturating_add(1),
                encoding,
                Some(ResponseBodyLimitKind::Raw),
            )));
        }

        raw_bytes = raw_bytes.saturating_add(chunk.len());
        match decoder.write_all(&chunk) {
            Ok(()) => {}
            Err(DecodeFailure::DecodedLimit) if capture_mode => {
                return Ok(ReadBodyOutcome::Captured(captured_from_decoder(
                    &decoder,
                    raw_bytes,
                    encoding,
                    Some(ResponseBodyLimitKind::Decoded),
                )));
            }
            Err(DecodeFailure::DecodedLimit) => {
                return Err(UpstreamResponseReadError::DecodedBodyLimitExceeded {
                    limit_bytes: limits.decoded_body_limit_bytes,
                    observed_bytes: limits.decoded_body_limit_bytes.saturating_add(1),
                });
            }
            Err(DecodeFailure::InvalidGzip) => {
                return Err(UpstreamResponseReadError::InvalidGzip);
            }
        }
    }

    match decoder.finish() {
        Ok(collector) => {
            if capture_mode {
                Ok(ReadBodyOutcome::Captured(CapturedErrorBody {
                    truncated: collector.decoded_bytes > capture_limit,
                    captured: collector.bytes.freeze(),
                    raw_bytes,
                    decoded_bytes: collector.decoded_bytes,
                    encoding,
                    hard_limit_reached: None,
                }))
            } else {
                let decoded_bytes = collector.decoded_bytes;
                Ok(ReadBodyOutcome::Complete(CompleteResponseBody {
                    bytes: collector.bytes.freeze(),
                    raw_bytes,
                    decoded_bytes,
                    encoding,
                }))
            }
        }
        Err(DecodeFailure::DecodedLimit) if capture_mode => {
            Ok(ReadBodyOutcome::Captured(captured_from_decoder(
                &decoder,
                raw_bytes,
                encoding,
                Some(ResponseBodyLimitKind::Decoded),
            )))
        }
        Err(DecodeFailure::DecodedLimit) => {
            Err(UpstreamResponseReadError::DecodedBodyLimitExceeded {
                limit_bytes: limits.decoded_body_limit_bytes,
                observed_bytes: limits.decoded_body_limit_bytes.saturating_add(1),
            })
        }
        Err(DecodeFailure::InvalidGzip) => Err(UpstreamResponseReadError::InvalidGzip),
    }
}

fn captured_from_decoder(
    decoder: &ResponseDecoder,
    raw_bytes: usize,
    encoding: UpstreamContentEncoding,
    hard_limit_reached: Option<ResponseBodyLimitKind>,
) -> CapturedErrorBody {
    let collector = decoder.collector();
    CapturedErrorBody {
        captured: Bytes::copy_from_slice(&collector.bytes),
        raw_bytes,
        decoded_bytes: collector.decoded_bytes,
        encoding,
        truncated: true,
        hard_limit_reached,
    }
}

fn parse_content_length(headers: &HeaderMap) -> Result<Option<usize>, UpstreamResponseReadError> {
    let mut parsed = None;
    for value in headers.get_all(CONTENT_LENGTH) {
        let raw = value
            .to_str()
            .map_err(|_| UpstreamResponseReadError::InvalidContentLength)?;
        for item in raw.split(',') {
            let item = item.trim();
            if item.is_empty() || !item.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(UpstreamResponseReadError::InvalidContentLength);
            }
            let current = item
                .parse::<usize>()
                .map_err(|_| UpstreamResponseReadError::InvalidContentLength)?;
            if let Some(previous) = parsed {
                if previous != current {
                    return Err(UpstreamResponseReadError::InvalidContentLength);
                }
            } else {
                parsed = Some(current);
            }
        }
    }
    Ok(parsed)
}

struct DecodedCollector {
    bytes: BytesMut,
    decoded_bytes: usize,
    decoded_limit: usize,
    capture_limit: usize,
    limit_reached: bool,
}

impl DecodedCollector {
    fn new(decoded_limit: usize, capture_limit: usize) -> Self {
        Self {
            bytes: BytesMut::new(),
            decoded_bytes: 0,
            decoded_limit,
            capture_limit,
            limit_reached: false,
        }
    }
}

impl Write for DecodedCollector {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if input.is_empty() {
            return Ok(0);
        }
        let remaining = self.decoded_limit.saturating_sub(self.decoded_bytes);
        let accepted = input.len().min(remaining);
        let capture_remaining = self.capture_limit.saturating_sub(self.bytes.len());
        let captured = accepted.min(capture_remaining);
        self.bytes.extend_from_slice(&input[..captured]);
        self.decoded_bytes = self.decoded_bytes.saturating_add(accepted);
        if accepted < input.len() {
            self.decoded_bytes = self.decoded_limit.saturating_add(1);
            self.limit_reached = true;
            return Err(io::Error::other("decoded response body limit reached"));
        }
        Ok(accepted)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

enum ResponseDecoder {
    Identity(DecodedCollector),
    Gzip(MultiGzDecoder<DecodedCollector>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecodeFailure {
    DecodedLimit,
    InvalidGzip,
}

impl ResponseDecoder {
    fn new(encoding: UpstreamContentEncoding, collector: DecodedCollector) -> Self {
        match encoding {
            UpstreamContentEncoding::Identity => Self::Identity(collector),
            UpstreamContentEncoding::Gzip => Self::Gzip(MultiGzDecoder::new(collector)),
        }
    }

    fn collector(&self) -> &DecodedCollector {
        match self {
            Self::Identity(collector) => collector,
            Self::Gzip(decoder) => decoder.get_ref(),
        }
    }

    fn write_all(&mut self, input: &[u8]) -> Result<(), DecodeFailure> {
        let result = match self {
            Self::Identity(collector) => collector.write_all(input),
            Self::Gzip(decoder) => decoder.write_all(input),
        };
        result.map_err(|_| {
            if self.collector().limit_reached {
                DecodeFailure::DecodedLimit
            } else {
                DecodeFailure::InvalidGzip
            }
        })
    }

    fn finish(&mut self) -> Result<DecodedCollector, DecodeFailure> {
        match self {
            Self::Identity(_) => {
                let Self::Identity(collector) =
                    std::mem::replace(self, Self::Identity(DecodedCollector::new(0, 0)))
                else {
                    unreachable!()
                };
                Ok(collector)
            }
            Self::Gzip(decoder) => {
                if decoder.try_finish().is_err() {
                    return Err(if decoder.get_ref().limit_reached {
                        DecodeFailure::DecodedLimit
                    } else {
                        DecodeFailure::InvalidGzip
                    });
                }
                let Self::Gzip(decoder) =
                    std::mem::replace(self, Self::Identity(DecodedCollector::new(0, 0)))
                else {
                    unreachable!()
                };
                decoder.finish().map_err(|_| DecodeFailure::InvalidGzip)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use flate2::{Compression, write::GzEncoder};
    use reqwest::header::HeaderValue;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };

    use super::*;

    fn limits(raw: usize, decoded: usize) -> NonStreamResponseConfig {
        NonStreamResponseConfig {
            raw_body_limit_bytes: raw,
            decoded_body_limit_bytes: decoded,
        }
    }

    fn gzip(input: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(input).unwrap();
        encoder.finish().unwrap()
    }

    async fn response(headers: &[(&str, &str)], body: &[u8]) -> Response {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut wire = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for (name, value) in headers {
            wire.push_str(name);
            wire.push_str(": ");
            wire.push_str(value);
            wire.push_str("\r\n");
        }
        wire.push_str("\r\n");
        let body = body.to_vec();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            socket.write_all(wire.as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
        });
        reqwest::Client::new()
            .get(format!("http://{address}/response"))
            .send()
            .await
            .unwrap()
    }

    async fn chunked_response(chunks: &[&[u8]]) -> Response {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let chunks = chunks
            .iter()
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            for chunk in chunks {
                socket
                    .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                    .await
                    .unwrap();
                socket.write_all(&chunk).await.unwrap();
                socket.write_all(b"\r\n").await.unwrap();
                tokio::task::yield_now().await;
            }
            socket.write_all(b"0\r\n\r\n").await.unwrap();
        });
        reqwest::Client::new()
            .get(format!("http://{address}/response"))
            .send()
            .await
            .unwrap()
    }

    async fn hanging_chunked_response(
        first_chunk: Option<&[u8]>,
    ) -> (Response, oneshot::Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release_tx, release_rx) = oneshot::channel();
        let first_chunk = first_chunk.map(ToOwned::to_owned);
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            if let Some(chunk) = first_chunk {
                let _ = socket
                    .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                    .await;
                let _ = socket.write_all(&chunk).await;
                let _ = socket.write_all(b"\r\n").await;
            }
            let _ = release_rx.await;
            let _ = socket.write_all(b"0\r\n\r\n").await;
        });
        let response = reqwest::Client::new()
            .get(format!("http://{address}/response"))
            .send()
            .await
            .unwrap();
        (response, release_tx)
    }

    #[tokio::test]
    async fn safe_http_error_message_never_copies_url_query_from_reqwest() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let marker = "manager-query-secret";
        let error = reqwest::Client::new()
            .get(format!("http://{address}/auxiliary?api_key={marker}"))
            .send()
            .await
            .expect_err("closed local address must reject the request");
        assert!(error.url().unwrap().as_str().contains(marker));

        let message = safe_http_error_message("Auxiliary request failed", &error);
        assert_eq!(message, "Auxiliary request failed (connect)");
        assert!(!message.contains(marker));
        assert!(!message.contains(&address.to_string()));
    }

    #[test]
    fn content_encoding_accepts_only_absent_identity_or_single_gzip() {
        let mut headers = HeaderMap::new();
        assert_eq!(
            parse_content_encoding(&headers).unwrap(),
            UpstreamContentEncoding::Identity
        );
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static(" GZip "));
        assert_eq!(
            parse_content_encoding(&headers).unwrap(),
            UpstreamContentEncoding::Gzip
        );
        for value in ["", "br", "gzip, identity", "deflate", "zstd"] {
            headers.insert(CONTENT_ENCODING, HeaderValue::from_str(value).unwrap());
            assert_eq!(
                parse_content_encoding(&headers),
                Err(UpstreamResponseReadError::InvalidContentEncoding)
            );
        }
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        headers.append(CONTENT_ENCODING, HeaderValue::from_static("identity"));
        assert_eq!(
            parse_content_encoding(&headers),
            Err(UpstreamResponseReadError::InvalidContentEncoding)
        );
    }

    #[test]
    fn accept_encoding_is_owned_by_the_gateway_and_has_only_two_fixed_forms() {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("br, zstd"));
        apply_upstream_accept_encoding(&mut headers, false);
        assert_eq!(headers.get(ACCEPT_ENCODING).unwrap(), "gzip, identity");

        apply_upstream_accept_encoding(&mut headers, true);
        assert_eq!(headers.get(ACCEPT_ENCODING).unwrap(), "identity");
    }

    #[test]
    fn content_type_is_bounded_normalized_and_parameter_limited() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("Application/Problem+JSON; Charset=UTF-8; profile=private"),
        );
        assert_eq!(
            normalize_content_type(&headers),
            Some(NormalizedContentType {
                essence: "application/problem+json".to_string(),
                value: "application/problem+json; charset=utf-8".to_string(),
            })
        );
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream; charset=latin1"),
        );
        assert_eq!(
            normalize_content_type(&headers).unwrap().value,
            "text/event-stream"
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("not a media type"));
        assert!(normalize_content_type(&headers).is_none());
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(&format!("text/plain; x={}", "a".repeat(257))).unwrap(),
        );
        assert!(normalize_content_type(&headers).is_none());
    }

    #[test]
    fn content_length_accepts_identical_values_and_rejects_conflicts() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("5, 5"));
        assert_eq!(parse_content_length(&headers).unwrap(), Some(5));
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("5, 6"));
        assert_eq!(
            parse_content_length(&headers),
            Err(UpstreamResponseReadError::InvalidContentLength)
        );
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("-1"));
        assert_eq!(
            parse_content_length(&headers),
            Err(UpstreamResponseReadError::InvalidContentLength)
        );
    }

    #[tokio::test]
    async fn complete_identity_allows_exact_boundary_and_rejects_content_length_plus_one() {
        let body = read_complete_response_body(response(&[], b"12345").await, &limits(5, 5))
            .await
            .unwrap();
        assert_eq!(body.bytes, Bytes::from_static(b"12345"));
        assert_eq!(body.raw_bytes, 5);
        assert_eq!(body.decoded_bytes, 5);
        assert_eq!(body.encoding, UpstreamContentEncoding::Identity);

        let error = read_complete_response_body(response(&[], b"123456").await, &limits(5, 10))
            .await
            .err()
            .expect("raw limit must reject the response");
        assert_eq!(
            error,
            UpstreamResponseReadError::RawBodyLimitExceeded {
                limit_bytes: 5,
                observed_bytes: 6,
            }
        );
    }

    #[tokio::test]
    async fn complete_identity_reads_split_chunked_transport_without_content_length() {
        let body = read_complete_response_body(
            chunked_response(&[b"a", b"bc", b"def"]).await,
            &limits(6, 6),
        )
        .await
        .unwrap();
        assert_eq!(body.bytes, Bytes::from_static(b"abcdef"));
        assert_eq!(body.raw_bytes, 6);
        assert_eq!(body.decoded_bytes, 6);
        assert_eq!(body.encoding, UpstreamContentEncoding::Identity);
    }

    #[tokio::test]
    async fn timed_complete_read_distinguishes_first_byte_and_response_idle() {
        let (response, release) = hanging_chunked_response(None).await;
        let error = read_complete_response_body_with_timeouts(
            response,
            &limits(64, 64),
            Some(ResponseBodyReadTimeouts {
                first_byte: Duration::from_millis(50),
                response_idle: Duration::from_millis(50),
            }),
            |_, _, _| {},
        )
        .await
        .expect_err("missing first raw body chunk should time out");
        assert_eq!(
            error,
            UpstreamResponseReadError::Timeout {
                phase: TimeoutPhase::FirstByte,
            }
        );
        let _ = release.send(());

        let (response, release) = hanging_chunked_response(Some(b"a")).await;
        let error = read_complete_response_body_with_timeouts(
            response,
            &limits(64, 64),
            Some(ResponseBodyReadTimeouts {
                first_byte: Duration::from_millis(100),
                response_idle: Duration::from_millis(50),
            }),
            |_, _, _| {},
        )
        .await
        .expect_err("active response idle should time out after the first chunk");
        assert_eq!(
            error,
            UpstreamResponseReadError::Timeout {
                phase: TimeoutPhase::ResponseIdle,
            }
        );
        let _ = release.send(());
    }

    #[tokio::test]
    async fn gzip_supports_multiple_members_and_enforces_decoded_limit() {
        let mut encoded = gzip(b"abc");
        encoded.extend_from_slice(&gzip(b"def"));
        let body = read_complete_response_body(
            response(&[("Content-Encoding", "gzip")], &encoded).await,
            &limits(encoded.len(), 6),
        )
        .await
        .unwrap();
        assert_eq!(body.bytes, Bytes::from_static(b"abcdef"));
        assert_eq!(body.decoded_bytes, 6);
        assert_eq!(body.encoding, UpstreamContentEncoding::Gzip);

        let error = read_complete_response_body(
            response(&[("Content-Encoding", "gzip")], &gzip(b"abcdefg")).await,
            &limits(128, 6),
        )
        .await
        .err()
        .expect("decoded limit must reject the response");
        assert_eq!(
            error,
            UpstreamResponseReadError::DecodedBodyLimitExceeded {
                limit_bytes: 6,
                observed_bytes: 7,
            }
        );
    }

    #[tokio::test]
    async fn gzip_rejects_truncation_checksum_failure_and_trailing_garbage() {
        let encoded = gzip(b"payload");
        let truncated = &encoded[..encoded.len() - 1];
        for invalid in [
            truncated.to_vec(),
            {
                let mut checksum = encoded.clone();
                let last = checksum.len() - 8;
                checksum[last] ^= 0xff;
                checksum
            },
            {
                let mut trailing = encoded.clone();
                trailing.extend_from_slice(b"garbage");
                trailing
            },
        ] {
            let error = read_complete_response_body(
                response(&[("Content-Encoding", "gzip")], &invalid).await,
                &limits(1024, 1024),
            )
            .await
            .err()
            .expect("invalid gzip must be rejected");
            assert_eq!(error, UpstreamResponseReadError::InvalidGzip);
        }
    }

    #[tokio::test]
    async fn error_capture_preserves_prefix_and_reports_hard_limit() {
        let body =
            capture_error_response_body(response(&[], b"abcdefghij").await, &limits(5, 8), 3)
                .await
                .unwrap();
        assert_eq!(body.captured, Bytes::from_static(b"abc"));
        assert_eq!(body.raw_bytes, 6);
        assert_eq!(body.decoded_bytes, 5);
        assert_eq!(body.encoding, UpstreamContentEncoding::Identity);
        assert!(body.truncated);
        assert_eq!(body.hard_limit_reached, Some(ResponseBodyLimitKind::Raw));
    }
}
