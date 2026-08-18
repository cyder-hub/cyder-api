use std::time::Duration;

use tokio::time::{Instant, sleep_until};

use crate::{
    config::{NonStreamResponseConfig, ProxyTimeoutConfig},
    proxy::{
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, TimeoutPhase,
        cancellation::ProxyCancellationContext,
        classify_reqwest_error,
        runtime::transport::lifecycle::{ProxyTerminationCause, TotalWatchdogResult},
    },
    service::upstream_response::{
        CapturedErrorBody, CompleteResponseBody, ResponseBodyReadTimeouts,
        UpstreamResponseReadError, capture_error_response_body_with_timeouts,
        read_complete_response_body_with_timeouts,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProxySendDeadline {
    pub(crate) started_at: Instant,
    pub(crate) request_send_deadline: Instant,
    pub(crate) total_deadline: Instant,
}

impl ProxySendDeadline {
    pub(crate) fn new(started_at: Instant, request_send: Duration, total: Duration) -> Self {
        Self {
            started_at,
            request_send_deadline: started_at + request_send,
            total_deadline: started_at + total,
        }
    }

    pub(crate) fn timeout_phase_at(self, now: Instant) -> Option<TimeoutPhase> {
        if now >= self.request_send_deadline && self.request_send_deadline <= self.total_deadline {
            Some(TimeoutPhase::RequestSend)
        } else if now >= self.total_deadline {
            Some(TimeoutPhase::Total)
        } else {
            None
        }
    }
}

pub(crate) async fn send_with_deadline(
    cancellation: &ProxyCancellationContext,
    request: reqwest::RequestBuilder,
    context: &str,
    timeouts: &ProxyTimeoutConfig,
) -> Result<reqwest::Response, ProxyError> {
    let deadline =
        ProxySendDeadline::new(Instant::now(), timeouts.request_send(), timeouts.total());
    let total_deadline_set = cancellation.set_total_deadline(deadline.total_deadline);
    debug_assert!(
        total_deadline_set,
        "upstream send must own the request total deadline"
    );
    let coordinator = cancellation.coordinator();
    let total_watchdog = coordinator.arm_total_watchdog(deadline.total_deadline);

    if cancellation.is_cancelled() {
        return Err(cancellation
            .cancellation_error(ExecutionStage::Connect, ResponseVisibility::NotVisible)
            .await);
    }
    let request_send_sleep = sleep_until(deadline.request_send_deadline);
    tokio::pin!(request_send_sleep);
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(cancellation.cancellation_error(
            ExecutionStage::Connect,
            ResponseVisibility::NotVisible,
        ).await),
        _ = &mut request_send_sleep => {
            let phase = deadline
                .timeout_phase_at(Instant::now())
                .unwrap_or(TimeoutPhase::RequestSend);
            let proxy_error = ProxyError::upstream_timeout(
                phase,
                ExecutionStage::Connect,
                ResponseVisibility::NotVisible,
                format!("{context} timed out during the {phase:?} phase"),
            );
            coordinator.try_terminate_error(&proxy_error);
            Err(proxy_error)
        },
        watchdog_result = total_watchdog.wait() => match watchdog_result {
            TotalWatchdogResult::Expired => {
                let proxy_error = ProxyError::upstream_timeout(
                    TimeoutPhase::Total,
                    ExecutionStage::Connect,
                    ResponseVisibility::NotVisible,
                    format!("{context} exceeded the proxy total timeout"),
                );
                coordinator.try_terminate_error(&proxy_error);
                Err(proxy_error)
            }
            TotalWatchdogResult::Cancelled => {
                if cancellation.is_cancelled() {
                    Err(cancellation.cancellation_error(
                        ExecutionStage::Connect,
                        ResponseVisibility::NotVisible,
                    ).await)
                } else if let Some(ProxyTerminationCause::Timeout { phase }) = coordinator.terminal() {
                    let proxy_error = ProxyError::upstream_timeout(
                        phase,
                        ExecutionStage::Connect,
                        ResponseVisibility::NotVisible,
                        format!("{context} upstream send terminated at the {phase:?} phase"),
                    );
                    Err(proxy_error)
                } else {
                    Err(ProxyError::upstream_timeout(
                        TimeoutPhase::Total,
                        ExecutionStage::Connect,
                        ResponseVisibility::NotVisible,
                        format!("{context} exceeded the proxy total timeout"),
                    ))
                }
            }
        },
        result = request.send() => result.map_err(|err| {
            classify_reqwest_error(
                context,
                &err,
                ExecutionStage::Connect,
                ResponseVisibility::NotVisible,
            )
        }),
    }
}

fn classify_response_read_error(
    context: &'static str,
    error: UpstreamResponseReadError,
) -> ProxyError {
    if error.is_timeout() {
        ProxyError::upstream_timeout(
            error.timeout_phase().unwrap_or(TimeoutPhase::ResponseIdle),
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::NotVisible,
            format!("{context}: {error}"),
        )
    } else {
        ProxyError::gateway(
            ProxyErrorCode::UpstreamResponseError,
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::NotVisible,
            None,
            format!("{context}: {error}"),
        )
    }
}

pub(super) async fn read_complete_response_with_cancellation(
    response: reqwest::Response,
    cancellation: &ProxyCancellationContext,
    limits: &NonStreamResponseConfig,
    timeouts: &ProxyTimeoutConfig,
) -> Result<CompleteResponseBody, ProxyError> {
    if cancellation.is_cancelled() {
        return Err(cancellation
            .cancellation_error(
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::NotVisible,
            )
            .await);
    }
    let coordinator = cancellation.coordinator();
    let read_future = read_complete_response_body_with_timeouts(
        response,
        limits,
        Some(ResponseBodyReadTimeouts {
            first_byte: timeouts.first_byte(),
            response_idle: timeouts.response_idle(),
        }),
        |_, _, _| {},
    );
    tokio::pin!(read_future);
    let watchdog = coordinator.arm_total_watchdog(
        cancellation
            .total_deadline()
            .expect("proxy total deadline must be initialized before response-body supervision"),
    );
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(cancellation.cancellation_error(
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::NotVisible,
        ).await),
        result = &mut read_future => result.map_err(|error| {
            classify_response_read_error("Failed to read complete upstream response", error)
        }),
        watchdog_result = watchdog.wait() => {
            let proxy_error = match watchdog_result {
                TotalWatchdogResult::Expired => ProxyError::upstream_timeout(
                    TimeoutPhase::Total,
                    ExecutionStage::UpstreamResponse,
                    ResponseVisibility::NotVisible,
                    "Failed to read complete upstream response: proxy total timeout",
                ),
                TotalWatchdogResult::Cancelled if cancellation.is_cancelled() => {
                    cancellation.cancellation_error(
                        ExecutionStage::UpstreamResponse,
                        ResponseVisibility::NotVisible,
                    ).await
                }
                TotalWatchdogResult::Cancelled => {
                    let phase = match coordinator.terminal() {
                        Some(ProxyTerminationCause::Timeout { phase }) => phase,
                        _ => TimeoutPhase::Total,
                    };
                    ProxyError::upstream_timeout(
                        phase,
                        ExecutionStage::UpstreamResponse,
                        ResponseVisibility::NotVisible,
                        format!("Failed to read complete upstream response: proxy {phase:?} timeout"),
                    )
                }
            };
            coordinator.try_terminate_error(&proxy_error);
            Err(proxy_error)
        }
    }
}

pub(super) async fn capture_error_response_with_deadline(
    response: reqwest::Response,
    cancellation: &ProxyCancellationContext,
    timeouts: &ProxyTimeoutConfig,
    limits: &NonStreamResponseConfig,
    disclosure_limit: usize,
) -> Result<CapturedErrorBody, ProxyError> {
    if cancellation.is_cancelled() {
        return Err(cancellation
            .cancellation_error(
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::NotVisible,
            )
            .await);
    }

    let coordinator = cancellation.coordinator();
    let read_future = capture_error_response_body_with_timeouts(
        response,
        limits,
        disclosure_limit,
        Some(ResponseBodyReadTimeouts {
            first_byte: timeouts.first_byte(),
            response_idle: timeouts.response_idle(),
        }),
    );
    tokio::pin!(read_future);
    let watchdog = coordinator.arm_total_watchdog(
        cancellation
            .total_deadline()
            .expect("proxy total deadline must be initialized before error-body supervision"),
    );
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(cancellation.cancellation_error(
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::NotVisible,
        ).await),
        result = &mut read_future => {
            result.map_err(|error| {
                classify_response_read_error("Failed to capture upstream error response", error)
            })
        }
        watchdog_result = watchdog.wait() => {
            let proxy_error = match watchdog_result {
                TotalWatchdogResult::Expired => ProxyError::upstream_timeout(
                    TimeoutPhase::Total,
                    ExecutionStage::UpstreamResponse,
                    ResponseVisibility::NotVisible,
                    "Failed to capture upstream error response: proxy total timeout",
                ),
                TotalWatchdogResult::Cancelled if cancellation.is_cancelled() => {
                    cancellation.cancellation_error(
                        ExecutionStage::UpstreamResponse,
                        ResponseVisibility::NotVisible,
                    ).await
                }
                TotalWatchdogResult::Cancelled => {
                    let phase = match coordinator.terminal() {
                        Some(ProxyTerminationCause::Timeout { phase }) => phase,
                        _ => TimeoutPhase::Total,
                    };
                    ProxyError::upstream_timeout(
                        phase,
                        ExecutionStage::UpstreamResponse,
                        ResponseVisibility::NotVisible,
                        format!("Failed to capture upstream error response: proxy {phase:?} timeout"),
                    )
                }
            };
            coordinator.try_terminate_error(&proxy_error);
            Err(proxy_error)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use super::{
        ProxySendDeadline, capture_error_response_with_deadline, classify_response_read_error,
        read_complete_response_with_cancellation, send_with_deadline,
    };
    use crate::config::{
        NonStreamResponseConfig, OutboundHttpConfig, ProxyRequestConfig, ProxyTimeoutConfig,
    };
    use crate::proxy::{
        ExecutionStage, ProxyErrorCode, ResponseVisibility, TimeoutPhase,
        cancellation::ProxyCancellationContext,
    };
    use crate::service::infra::HttpClientBundle;
    use axum::http::StatusCode;
    use bytes::Bytes;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
        time::{Instant, advance, pause, timeout},
    };

    use super::super::test_support::{ControlledResolver, ControlledUpstream};

    fn client_bundle_with_resolver(resolver: Arc<ControlledResolver>) -> HttpClientBundle {
        HttpClientBundle::build_with_test_resolver(
            OutboundHttpConfig::default(),
            ProxyRequestConfig::default(),
            None,
            Arc::clone(&resolver) as Arc<dyn reqwest::dns::Resolve>,
        )
        .expect("controlled resolver client bundle should build")
    }

    fn assert_timeout_contract(
        error: &crate::proxy::ProxyError,
        phase: TimeoutPhase,
        visibility: ResponseVisibility,
    ) {
        assert_eq!(error.code(), ProxyErrorCode::UpstreamTimeoutError);
        assert_eq!(error.status_code(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(error.stage(), ExecutionStage::UpstreamResponse);
        assert_eq!(error.response_visibility(), visibility);
        assert_eq!(error.timeout_phase(), Some(phase));
        assert!(error.upstream_error().is_none());
        assert_eq!(error.response_hints().retry_after(), None);
    }

    #[tokio::test]
    async fn malformed_reqwest_request_is_classified_as_upstream_request_error() {
        let cancellation = ProxyCancellationContext::new();
        let request = reqwest::Client::new().post("http://[::1");

        let error = send_with_deadline(
            &cancellation,
            request,
            "malformed upstream request",
            &ProxyTimeoutConfig::default(),
        )
        .await
        .expect_err("malformed request must fail before connecting");

        assert_eq!(error.code(), ProxyErrorCode::UpstreamRequestError);
        assert_eq!(error.stage(), ExecutionStage::Connect);
        assert_eq!(error.response_visibility(), ResponseVisibility::NotVisible);
        assert!(error.upstream_error().is_none());
        assert_eq!(
            error.client_payload().public_message(),
            "The gateway could not send the request to the upstream provider."
        );
    }

    #[tokio::test]
    async fn send_with_deadline_returns_headers_before_controlled_body_data() {
        let mut upstream = ControlledUpstream::start().await;
        let cancellation = ProxyCancellationContext::new();
        let request = reqwest::Client::new()
            .post(format!("{}/controlled", upstream.base_url))
            .body("request body");
        let send_task = tokio::spawn(async move {
            send_with_deadline(
                &cancellation,
                request,
                "controlled request",
                &ProxyTimeoutConfig::default(),
            )
            .await
        });

        timeout(Duration::from_secs(2), upstream.wait_request())
            .await
            .expect("first-byte request should reach the fixture");
        timeout(Duration::from_secs(2), upstream.wait_headers_waiting())
            .await
            .expect("first-byte fixture should wait for released headers");
        upstream.release_headers();
        let response = timeout(Duration::from_millis(500), send_task)
            .await
            .expect("headers should return before a controlled body chunk")
            .expect("send task should join")
            .expect("controlled request should succeed");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let body_sender = upstream.take_body().await;
        drop(response);
        drop(body_sender);
        upstream.wait_body_dropped().await;
        assert_eq!(upstream.body_drop_count(), 1);
        upstream.shutdown().await;
    }

    #[tokio::test]
    async fn immediate_dns_failure_is_classified_without_connecting() {
        let resolver = Arc::new(ControlledResolver::immediate_failure());
        let bundle = client_bundle_with_resolver(Arc::clone(&resolver));
        let cancellation = ProxyCancellationContext::new();
        let request = bundle.client.get("http://r3-dns-failure.invalid/");

        let error = send_with_deadline(
            &cancellation,
            request,
            "controlled DNS failure",
            &ProxyTimeoutConfig::default(),
        )
        .await
        .expect_err("controlled DNS failure must fail before connecting");

        assert_eq!(error.code(), ProxyErrorCode::UpstreamConnectError);
        assert_eq!(error.status_code(), StatusCode::BAD_GATEWAY);
        assert_eq!(error.stage(), ExecutionStage::Connect);
        assert_eq!(error.response_visibility(), ResponseVisibility::NotVisible);
        assert_eq!(error.timeout_phase(), None);
        assert!(error.upstream_error().is_none());
        assert_eq!(error.response_hints().retry_after(), None);
        assert_eq!(resolver.resolve_call_count(), 1);
    }

    #[tokio::test]
    async fn pending_dns_resolution_produces_connect_timeout_and_drops_resolver() {
        let resolver = Arc::new(ControlledResolver::pending());
        let outbound_http = OutboundHttpConfig {
            connect_timeout_seconds: 1,
            ..OutboundHttpConfig::default()
        };
        let bundle = HttpClientBundle::build_with_test_resolver(
            outbound_http,
            ProxyRequestConfig::default(),
            None,
            Arc::clone(&resolver) as Arc<dyn reqwest::dns::Resolve>,
        )
        .expect("pending resolver client bundle should build");
        let cancellation = ProxyCancellationContext::new();
        let request = bundle.client.get("http://r3-connect-timeout.invalid/");
        let task = tokio::spawn(async move {
            send_with_deadline(
                &cancellation,
                request,
                "controlled connect timeout",
                &ProxyTimeoutConfig::default(),
            )
            .await
        });

        resolver.wait_resolve().await;
        tokio::task::yield_now().await;
        pause();
        advance(Duration::from_secs(1)).await;
        let error = task
            .await
            .expect("connect timeout task should join")
            .expect_err("pending resolver should time out");

        assert_eq!(error.code(), ProxyErrorCode::UpstreamTimeoutError);
        assert_eq!(error.status_code(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(error.stage(), ExecutionStage::Connect);
        assert_eq!(error.response_visibility(), ResponseVisibility::NotVisible);
        assert_eq!(error.timeout_phase(), Some(TimeoutPhase::Connect));
        assert!(error.upstream_error().is_none());
        assert_eq!(error.response_hints().retry_after(), None);
        assert_eq!(resolver.resolve_call_count(), 1);
        resolver.wait_pending_dropped().await;
        assert_eq!(resolver.pending_drop_count(), 1);
    }

    #[tokio::test]
    async fn request_send_timeout_waits_for_request_start_without_complete_request() {
        let upstream = ControlledUpstream::start().await;
        let cancellation = ProxyCancellationContext::new();
        let request_body =
            reqwest::Body::wrap_stream(futures::stream::pending::<Result<Bytes, std::io::Error>>());
        let request = reqwest::Client::new()
            .post(format!("{}/request-send-timeout", upstream.base_url))
            .body(request_body);
        let task = tokio::spawn(async move {
            send_with_deadline(
                &cancellation,
                request,
                "controlled request-send timeout",
                &ProxyTimeoutConfig::default(),
            )
            .await
        });

        upstream.wait_request_started().await;
        pause();
        advance(Duration::from_secs(60)).await;
        let error = task
            .await
            .expect("request-send timeout task should join")
            .expect_err("pending request body should time out");

        assert_eq!(error.code(), ProxyErrorCode::UpstreamTimeoutError);
        assert_eq!(error.status_code(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(error.stage(), ExecutionStage::Connect);
        assert_eq!(error.response_visibility(), ResponseVisibility::NotVisible);
        assert_eq!(error.timeout_phase(), Some(TimeoutPhase::RequestSend));
        assert!(error.upstream_error().is_none());
        assert_eq!(error.response_hints().retry_after(), None);
        assert_eq!(upstream.request_started_count(), 1);
        assert_eq!(upstream.request_seen_count(), 0);
        upstream.shutdown().await;
    }

    #[tokio::test]
    async fn first_byte_timeout_is_reported_after_headers_before_body() {
        let upstream = ControlledUpstream::start().await;
        let cancellation = ProxyCancellationContext::new();
        let timeouts = ProxyTimeoutConfig {
            request_send_seconds: 600,
            first_byte_seconds: 60,
            response_idle_seconds: 60,
            total_seconds: 300,
        };
        let send_timeouts = timeouts.clone();
        let send_cancellation = cancellation.clone();
        let request =
            reqwest::Client::new().get(format!("{}/first-byte-timeout", upstream.base_url));
        let send_task = tokio::spawn(async move {
            send_with_deadline(
                &send_cancellation,
                request,
                "controlled first-byte timeout",
                &send_timeouts,
            )
            .await
        });

        upstream.wait_request().await;
        upstream.wait_headers_waiting().await;
        upstream.release_headers();
        let response = send_task
            .await
            .expect("first-byte send task should join")
            .expect("headers should be available before the body");
        assert_eq!(response.status(), StatusCode::OK);

        pause();
        let limits = NonStreamResponseConfig::default();
        let read_future =
            read_complete_response_with_cancellation(response, &cancellation, &limits, &timeouts);
        tokio::pin!(read_future);
        let read_result = 'poll: loop {
            for _ in 0..4 {
                tokio::select! {
                    biased;
                    result = &mut read_future => break 'poll result,
                    _ = advance(Duration::from_secs(60)) => {
                        tokio::task::yield_now().await;
                    }
                }
            }
            panic!("first-byte read did not reach its virtual deadline");
        };
        let error = read_result.expect_err("missing first body byte should time out");

        assert_timeout_contract(
            &error,
            TimeoutPhase::FirstByte,
            ResponseVisibility::NotVisible,
        );
        assert_eq!(upstream.request_seen_count(), 1);
        timeout(Duration::from_secs(1), upstream.wait_body_dropped())
            .await
            .expect("first-byte response body should be dropped");
        assert_eq!(upstream.body_drop_count(), 1);
        drop(upstream);
    }

    #[tokio::test]
    async fn response_idle_timeout_is_reported_after_a_nonempty_body_chunk() {
        let mut upstream = ControlledUpstream::start().await;
        let cancellation = ProxyCancellationContext::new();
        let timeouts = ProxyTimeoutConfig {
            request_send_seconds: 600,
            first_byte_seconds: 60,
            response_idle_seconds: 60,
            total_seconds: 300,
        };
        let send_timeouts = timeouts.clone();
        let send_cancellation = cancellation.clone();
        let request =
            reqwest::Client::new().get(format!("{}/response-idle-timeout", upstream.base_url));
        let send_task = tokio::spawn(async move {
            send_with_deadline(
                &send_cancellation,
                request,
                "controlled response-idle timeout",
                &send_timeouts,
            )
            .await
        });

        upstream.wait_request().await;
        upstream.wait_headers_waiting().await;
        upstream.release_headers();
        let response = send_task
            .await
            .expect("response-idle send task should join")
            .expect("headers should be available before the body");
        let body_sender = upstream.take_body().await;
        pause();
        let limits = NonStreamResponseConfig::default();
        let read_future =
            read_complete_response_with_cancellation(response, &cancellation, &limits, &timeouts);
        tokio::pin!(read_future);
        tokio::select! {
            biased;
            result = &mut read_future => {
                let _ = result.expect("response-idle read task should join");
                panic!("response-idle read completed before the first chunk")
            }
            result = body_sender.send(super::super::test_support::BodyCommand::Chunk(
                Bytes::from_static(b"first"),
            ) ) => {
                result.expect("first response chunk should be accepted");
            }
        }
        upstream.wait_body_chunk_seen().await;
        let read_result = 'poll: loop {
            for _ in 0..4 {
                tokio::select! {
                    biased;
                    result = &mut read_future => break 'poll result,
                    _ = advance(Duration::from_secs(60)) => {
                        tokio::task::yield_now().await;
                    }
                }
            }
            panic!("response-idle read did not reach its virtual deadline");
        };
        let error = read_result.expect_err("missing next body chunk should time out");

        assert_timeout_contract(
            &error,
            TimeoutPhase::ResponseIdle,
            ResponseVisibility::NotVisible,
        );
        assert_eq!(upstream.request_seen_count(), 1);
        timeout(Duration::from_secs(1), upstream.wait_body_dropped())
            .await
            .expect("response-idle response body should be dropped");
        assert_eq!(upstream.body_drop_count(), 1);
        drop(body_sender);
        drop(upstream);
    }

    #[tokio::test]
    async fn total_timeout_precedes_a_later_success_body_phase_deadline() {
        let upstream = ControlledUpstream::start().await;
        let cancellation = ProxyCancellationContext::new();
        let request =
            reqwest::Client::new().get(format!("{}/success-total-timeout", upstream.base_url));
        let timeouts = ProxyTimeoutConfig {
            request_send_seconds: 600,
            first_byte_seconds: 600,
            response_idle_seconds: 600,
            total_seconds: 300,
        };
        let send_cancellation = cancellation.clone();
        let send_timeouts = timeouts.clone();
        let send_task = tokio::spawn(async move {
            send_with_deadline(
                &send_cancellation,
                request,
                "controlled successful total timeout",
                &send_timeouts,
            )
            .await
        });

        upstream.wait_request().await;
        upstream.wait_headers_waiting().await;
        upstream.release_headers();
        let response = send_task
            .await
            .expect("successful total-timeout send task should join")
            .expect("successful response headers should be available");
        assert_eq!(response.status(), StatusCode::OK);
        pause();
        let error = read_complete_response_with_cancellation(
            response,
            &cancellation,
            &NonStreamResponseConfig::default(),
            &timeouts,
        )
        .await
        .expect_err("total deadline should win before the later first-byte deadline");

        assert_timeout_contract(&error, TimeoutPhase::Total, ResponseVisibility::NotVisible);
        assert_eq!(upstream.request_seen_count(), 1);
        timeout(Duration::from_secs(1), upstream.wait_body_dropped())
            .await
            .expect("successful total-timeout response body should be dropped");
        assert_eq!(upstream.body_drop_count(), 1);
        drop(upstream);
    }

    #[tokio::test]
    async fn total_timeout_precedes_a_later_error_body_phase_deadline() {
        let upstream =
            ControlledUpstream::start_with_status(StatusCode::INTERNAL_SERVER_ERROR).await;
        let cancellation = ProxyCancellationContext::new();
        let request = reqwest::Client::new().get(format!("{}/total-timeout", upstream.base_url));
        let timeouts = ProxyTimeoutConfig {
            request_send_seconds: 600,
            first_byte_seconds: 600,
            response_idle_seconds: 600,
            total_seconds: 300,
        };
        let send_cancellation = cancellation.clone();
        let send_timeouts = timeouts.clone();
        let send_task = tokio::spawn(async move {
            send_with_deadline(
                &send_cancellation,
                request,
                "controlled total timeout",
                &send_timeouts,
            )
            .await
        });

        upstream.wait_request().await;
        upstream.wait_headers_waiting().await;
        upstream.release_headers();
        let response = send_task
            .await
            .expect("total-timeout send task should join")
            .expect("error response headers should be available");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        pause();
        let error = match capture_error_response_with_deadline(
            response,
            &cancellation,
            &timeouts,
            &NonStreamResponseConfig::default(),
            1024,
        )
        .await
        {
            Ok(_) => panic!("total deadline should win before the later first-byte deadline"),
            Err(error) => error,
        };

        assert_timeout_contract(&error, TimeoutPhase::Total, ResponseVisibility::NotVisible);
        assert_eq!(upstream.request_seen_count(), 1);
        timeout(Duration::from_secs(1), upstream.wait_body_dropped())
            .await
            .expect("total-timeout response body should be dropped");
        assert_eq!(upstream.body_drop_count(), 1);
        drop(upstream);
    }

    #[test]
    fn send_deadline_prefers_request_send_at_equal_total_and_total_when_earlier() {
        let started_at = Instant::now();
        let equal =
            ProxySendDeadline::new(started_at, Duration::from_secs(5), Duration::from_secs(5));
        assert_eq!(
            equal.timeout_phase_at(started_at + Duration::from_secs(5)),
            Some(TimeoutPhase::RequestSend)
        );

        let total_first =
            ProxySendDeadline::new(started_at, Duration::from_secs(10), Duration::from_secs(5));
        assert_eq!(
            total_first.timeout_phase_at(started_at + Duration::from_secs(5)),
            Some(TimeoutPhase::Total)
        );
    }

    #[test]
    fn response_read_timeout_is_classified_as_response_idle() {
        let error = classify_response_read_error(
            "upstream response read",
            crate::service::upstream_response::UpstreamResponseReadError::Transport {
                kind: "timeout",
            },
        );
        assert_eq!(error.code(), ProxyErrorCode::UpstreamTimeoutError);
        assert_eq!(error.code().as_str(), "upstream_timeout_error");
        assert_eq!(error.status_code(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(error.stage(), ExecutionStage::UpstreamResponse);
        assert_eq!(error.response_visibility(), ResponseVisibility::NotVisible);
        assert_eq!(error.timeout_phase(), Some(TimeoutPhase::ResponseIdle));
    }

    #[tokio::test]
    async fn cancellation_drops_in_progress_bounded_response_read_and_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (socket_dropped_tx, socket_dropped_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n",
                )
                .await
                .unwrap();
            let mut byte = [0u8; 1];
            let closed = socket.read(&mut byte).await.unwrap() == 0;
            let _ = socket_dropped_tx.send(closed);
        });
        let response = reqwest::Client::new()
            .get(format!("http://{address}/hanging"))
            .send()
            .await
            .unwrap();
        let cancellation = ProxyCancellationContext::new();
        let cancellation_trigger = cancellation.clone();
        let timeouts = ProxyTimeoutConfig::default();
        assert!(cancellation.set_total_deadline(Instant::now() + timeouts.total()));
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            cancellation_trigger.cancel_now("client cancelled bounded read");
        });

        let error = read_complete_response_with_cancellation(
            response,
            &cancellation,
            &NonStreamResponseConfig {
                raw_body_limit_bytes: 1024,
                decoded_body_limit_bytes: 1024,
            },
            &timeouts,
        )
        .await
        .err()
        .expect("cancelled read must fail");
        assert_eq!(error.code(), ProxyErrorCode::ClientCancelledError);
        assert_eq!(error.stage(), ExecutionStage::UpstreamResponse);
        assert_eq!(error.response_visibility(), ResponseVisibility::NotVisible);
        assert!(
            timeout(Duration::from_secs(2), socket_dropped_rx)
                .await
                .expect("socket close should be observed")
                .expect("socket close signal should be sent")
        );
    }
}
