use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, Proxy, Url, redirect};

use crate::config::{OutboundHttpConfig, ProxyRequestConfig};
#[cfg(test)]
use crate::database::TestDbContext;
use crate::proxy::logging::LogManager;
use crate::service::provider_http::parse_proxy_url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProviderHttpClientError {
    ProxyNotConfigured,
}

impl fmt::Display for ProviderHttpClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProxyNotConfigured => formatter
                .write_str("provider requires the global proxy, but no proxy is configured"),
        }
    }
}

#[derive(Clone)]
pub struct HttpClientBundle {
    pub client: Arc<Client>,
    proxy_client: Option<Arc<Client>>,
    pub outbound_http: OutboundHttpConfig,
    pub proxy_request: ProxyRequestConfig,
}

impl HttpClientBundle {
    pub fn build(
        outbound_http: OutboundHttpConfig,
        proxy_request: ProxyRequestConfig,
        proxy: Option<String>,
    ) -> Result<HttpClientBundle, String> {
        outbound_http.validate()?;
        proxy_request.validate()?;
        let proxy_url = proxy
            .as_deref()
            .map(parse_proxy_url)
            .transpose()
            .map_err(|error| format!("invalid proxy URL in configuration: {error}"))?;
        let client = Arc::new(build_http_client(
            "default",
            &outbound_http,
            &proxy_request,
            None,
        )?);
        let proxy_client = proxy_url
            .as_ref()
            .map(|proxy_url| {
                build_http_client("proxy", &outbound_http, &proxy_request, Some(proxy_url))
            })
            .transpose()?
            .map(Arc::new);

        Ok(HttpClientBundle {
            client,
            proxy_client,
            outbound_http,
            proxy_request,
        })
    }

    #[cfg(test)]
    pub(crate) fn build_with_test_resolver(
        outbound_http: OutboundHttpConfig,
        proxy_request: ProxyRequestConfig,
        proxy: Option<String>,
        resolver: Arc<dyn reqwest::dns::Resolve>,
    ) -> Result<HttpClientBundle, String> {
        outbound_http.validate()?;
        proxy_request.validate()?;
        let proxy_url = proxy
            .as_deref()
            .map(parse_proxy_url)
            .transpose()
            .map_err(|error| format!("invalid proxy URL in configuration: {error}"))?;
        let client = Arc::new(build_http_client_with_resolver(
            "default",
            &outbound_http,
            &proxy_request,
            None,
            Arc::clone(&resolver),
        )?);
        let proxy_client = proxy_url
            .as_ref()
            .map(|proxy_url| {
                build_http_client_with_resolver(
                    "proxy",
                    &outbound_http,
                    &proxy_request,
                    Some(proxy_url),
                    Arc::clone(&resolver),
                )
            })
            .transpose()?
            .map(Arc::new);

        Ok(HttpClientBundle {
            client,
            proxy_client,
            outbound_http,
            proxy_request,
        })
    }

    pub(crate) fn provider_client(
        &self,
        use_proxy: bool,
    ) -> Result<Arc<Client>, ProviderHttpClientError> {
        if !use_proxy {
            return Ok(Arc::clone(&self.client));
        }

        self.proxy_client
            .as_ref()
            .map(Arc::clone)
            .ok_or(ProviderHttpClientError::ProxyNotConfigured)
    }
}

pub struct AppInfra {
    http_clients: Arc<HttpClientBundle>,
    log_manager: Arc<LogManager>,
    #[cfg(test)]
    test_db_context: Option<TestDbContext>,
}

impl AppInfra {
    pub(crate) async fn new_with_config(
        outbound_http: OutboundHttpConfig,
        proxy_request: ProxyRequestConfig,
        proxy: Option<String>,
        #[cfg(test)] test_db_context: Option<TestDbContext>,
    ) -> Self {
        let http_clients = Arc::new(
            HttpClientBundle::build(outbound_http, proxy_request, proxy)
                .expect("failed to build initial HTTP client bundle"),
        );
        let log_manager = Arc::new({
            #[cfg(test)]
            {
                match test_db_context.clone() {
                    Some(test_db_context) => LogManager::new_for_test(test_db_context),
                    None => LogManager::new(),
                }
            }

            #[cfg(not(test))]
            {
                LogManager::new()
            }
        });

        Self {
            http_clients,
            log_manager,
            #[cfg(test)]
            test_db_context,
        }
    }

    #[cfg(test)]
    pub(crate) async fn new_with_config_and_test_resolver(
        outbound_http: OutboundHttpConfig,
        proxy_request: ProxyRequestConfig,
        proxy: Option<String>,
        resolver: Arc<dyn reqwest::dns::Resolve>,
        test_db_context: Option<TestDbContext>,
    ) -> Self {
        let http_clients = Arc::new(
            HttpClientBundle::build_with_test_resolver(
                outbound_http,
                proxy_request,
                proxy,
                resolver,
            )
            .expect("failed to build initial test HTTP client bundle"),
        );
        let log_manager = Arc::new(match test_db_context.clone() {
            Some(test_db_context) => LogManager::new_for_test(test_db_context),
            None => LogManager::new(),
        });

        Self {
            http_clients,
            log_manager,
            test_db_context,
        }
    }

    pub(crate) async fn client_bundle(&self) -> Arc<HttpClientBundle> {
        Arc::clone(&self.http_clients)
    }

    pub(crate) async fn provider_client(
        &self,
        use_proxy: bool,
    ) -> Result<Arc<Client>, ProviderHttpClientError> {
        self.http_clients.provider_client(use_proxy)
    }

    pub(crate) async fn auxiliary_client(
        &self,
        use_proxy: bool,
    ) -> Result<Arc<Client>, ProviderHttpClientError> {
        // Auxiliary requests share the same direct/proxy transport selection,
        // while their total lifetime is enforced by auxiliary_http helpers.
        self.http_clients.provider_client(use_proxy)
    }

    pub(crate) fn auxiliary_total_timeout(&self) -> Duration {
        self.http_clients.outbound_http.auxiliary_total_timeout()
    }

    pub(crate) fn proxy_request_config(&self) -> &ProxyRequestConfig {
        &self.http_clients.proxy_request
    }

    pub(crate) fn log_manager(&self) -> &LogManager {
        self.log_manager.as_ref()
    }

    pub async fn flush_proxy_logs(&self) {
        self.log_manager.flush().await;
    }

    pub(crate) fn spawn_background_task<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        #[cfg(test)]
        if let Some(test_db_context) = &self.test_db_context {
            return test_db_context.spawn(future);
        }

        tokio::spawn(future)
    }
}

fn duration_to_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

fn configured_http_client_builder(
    client_kind: &'static str,
    outbound_http: &OutboundHttpConfig,
    proxy_request_config: &ProxyRequestConfig,
    proxy_url: Option<&Url>,
) -> Result<reqwest::ClientBuilder, String> {
    let connect_timeout = outbound_http.connect_timeout();

    let mut builder = Client::builder()
        .connect_timeout(connect_timeout)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .redirect(redirect::Policy::none());

    if let Some(proxy_url) = proxy_url {
        let proxy = Proxy::all(proxy_url.clone())
            .map_err(|_| "invalid proxy URL in configuration".to_string())?;
        builder = builder.proxy(proxy);
    }

    crate::info_event!(
        "startup.http_client_built",
        client_kind = client_kind,
        use_proxy = proxy_url.is_some(),
        connect_timeout_ms = duration_to_millis(connect_timeout),
        auxiliary_total_timeout_ms = duration_to_millis(outbound_http.auxiliary_total_timeout()),
        request_send_timeout_ms = duration_to_millis(proxy_request_config.timeouts.request_send()),
        first_byte_timeout_ms = duration_to_millis(proxy_request_config.timeouts.first_byte()),
        response_idle_timeout_ms =
            duration_to_millis(proxy_request_config.timeouts.response_idle()),
        total_timeout_ms = duration_to_millis(proxy_request_config.timeouts.total()),
    );

    Ok(builder)
}

fn build_http_client(
    client_kind: &'static str,
    outbound_http: &OutboundHttpConfig,
    proxy_request_config: &ProxyRequestConfig,
    proxy_url: Option<&Url>,
) -> Result<Client, String> {
    configured_http_client_builder(client_kind, outbound_http, proxy_request_config, proxy_url)?
        .build()
        .map_err(|_| {
            if proxy_url.is_some() {
                "failed to build proxy reqwest client".to_string()
            } else {
                "failed to build default reqwest client".to_string()
            }
        })
}

#[cfg(test)]
fn build_http_client_with_resolver(
    client_kind: &'static str,
    outbound_http: &OutboundHttpConfig,
    proxy_request_config: &ProxyRequestConfig,
    proxy_url: Option<&Url>,
    resolver: Arc<dyn reqwest::dns::Resolve>,
) -> Result<Client, String> {
    configured_http_client_builder(client_kind, outbound_http, proxy_request_config, proxy_url)?
        .dns_resolver2(resolver)
        .build()
        .map_err(|_| {
            if proxy_url.is_some() {
                "failed to build proxy reqwest client".to_string()
            } else {
                "failed to build default reqwest client".to_string()
            }
        })
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::{
        Router,
        body::Body,
        extract::{Path, State},
        http::{
            StatusCode,
            header::{CONTENT_ENCODING, LOCATION},
        },
        response::Response,
        routing::{any, get},
    };
    use flate2::{Compression, write::GzEncoder};
    use tokio::net::TcpListener;

    use super::*;
    use crate::proxy::runtime::transport::test_support::ControlledResolver;

    #[test]
    fn http_client_bundle_rejects_invalid_proxy_url() {
        let err = match HttpClientBundle::build(
            OutboundHttpConfig::default(),
            ProxyRequestConfig::default(),
            Some("socks5://127.0.0.1:1080".to_string()),
        ) {
            Ok(_) => panic!("invalid proxy scheme should fail"),
            Err(err) => err,
        };

        assert!(err.contains("invalid proxy URL"));
        assert!(!err.contains("127.0.0.1"));
    }

    #[test]
    fn http_client_bundle_rejects_proxy_paths_queries_and_fragments_without_echoing_credentials() {
        for proxy in [
            "http://user:secret@proxy.example/gateway",
            "http://user:secret@proxy.example?region=cn",
            "http://user:secret@proxy.example#internal",
        ] {
            let err = match HttpClientBundle::build(
                OutboundHttpConfig::default(),
                ProxyRequestConfig::default(),
                Some(proxy.to_string()),
            ) {
                Ok(_) => panic!("ambiguous proxy URL should fail"),
                Err(error) => error,
            };
            assert!(err.contains("invalid proxy URL"));
            assert!(!err.contains("user"));
            assert!(!err.contains("secret"));
            assert!(!err.contains("proxy.example"));
        }
    }

    #[test]
    fn provider_client_fails_closed_when_proxy_is_required_but_unconfigured() {
        let bundle = HttpClientBundle::build(
            OutboundHttpConfig::default(),
            ProxyRequestConfig::default(),
            None,
        )
        .expect("default client bundle should build");

        assert!(bundle.provider_client(false).is_ok());
        assert_eq!(
            bundle
                .provider_client(true)
                .expect_err("proxy requirement must not fall back to direct"),
            ProviderHttpClientError::ProxyNotConfigured
        );
    }

    #[tokio::test]
    async fn test_only_dns_resolver_is_injected_without_changing_client_policy() {
        let resolver = Arc::new(ControlledResolver::immediate_failure());
        let bundle = HttpClientBundle::build_with_test_resolver(
            OutboundHttpConfig::default(),
            ProxyRequestConfig::default(),
            None,
            Arc::clone(&resolver) as Arc<dyn reqwest::dns::Resolve>,
        )
        .expect("test resolver client bundle should build");

        let error = bundle
            .client
            .get("http://r3-dns-failure.invalid/")
            .send()
            .await
            .expect_err("controlled DNS failure should fail before connecting");

        assert!(error.is_connect());
        assert_eq!(resolver.resolve_call_count(), 1);
    }

    async fn redirect_response(
        Path(status): Path<u16>,
        State(target): State<String>,
    ) -> Response<Body> {
        Response::builder()
            .status(StatusCode::from_u16(status).expect("test redirect status"))
            .header(LOCATION, target)
            .body(Body::from("redirect"))
            .expect("redirect response should build")
    }

    async fn target_response(State(hits): State<Arc<AtomicUsize>>) -> StatusCode {
        hits.fetch_add(1, Ordering::SeqCst);
        StatusCode::NO_CONTENT
    }

    async fn gzip_response(State(encoded): State<Vec<u8>>) -> Response<Body> {
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_ENCODING, "gzip")
            .body(Body::from(encoded))
            .expect("gzip fixture response should build")
    }

    #[tokio::test]
    async fn shared_http_client_never_automatically_decodes_gzip() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"encoded fixture").unwrap();
        let encoded = encoder.finish().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let expected = encoded.clone();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/gzip", get(gzip_response))
                    .with_state(encoded),
            )
            .await
            .unwrap();
        });

        let bundle = HttpClientBundle::build(
            OutboundHttpConfig::default(),
            ProxyRequestConfig::default(),
            None,
        )
        .unwrap();
        let response = bundle
            .client
            .get(format!("http://{address}/gzip"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.headers().get(CONTENT_ENCODING).unwrap(), "gzip");
        assert_eq!(
            response.bytes().await.unwrap().as_ref(),
            expected.as_slice()
        );
        task.abort();
    }

    #[tokio::test]
    async fn shared_http_client_never_follows_any_redirect_status() {
        let target_hits = Arc::new(AtomicUsize::new(0));
        let target_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("target listener should bind");
        let target_addr = target_listener.local_addr().expect("target address");
        let target_hits_server = Arc::clone(&target_hits);
        let target_task = tokio::spawn(async move {
            axum::serve(
                target_listener,
                Router::new()
                    .fallback(any(target_response))
                    .with_state(target_hits_server),
            )
            .await
            .expect("target server should run");
        });

        let redirect_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("redirect listener should bind");
        let redirect_addr = redirect_listener.local_addr().expect("redirect address");
        let redirect_task = tokio::spawn(async move {
            axum::serve(
                redirect_listener,
                Router::new()
                    .route("/{status}", any(redirect_response))
                    .with_state(format!("http://{target_addr}/credential-capture")),
            )
            .await
            .expect("redirect server should run");
        });

        let bundle = HttpClientBundle::build(
            OutboundHttpConfig::default(),
            ProxyRequestConfig::default(),
            None,
        )
        .expect("client bundle should build");
        for status in [301, 302, 303, 307, 308] {
            let response = bundle
                .client
                .post(format!("http://{redirect_addr}/{status}"))
                .header("authorization", "Bearer provider-secret")
                .header("x-api-key", "provider-secret")
                .header("x-goog-api-key", "provider-secret")
                .body("provider-request-body")
                .send()
                .await
                .expect("redirect response should be returned without following");
            assert_eq!(response.status().as_u16(), status);
        }

        assert_eq!(target_hits.load(Ordering::SeqCst), 0);
        redirect_task.abort();
        target_task.abort();
    }

    #[tokio::test]
    async fn unavailable_configured_proxy_never_falls_back_to_direct_access() {
        let target_hits = Arc::new(AtomicUsize::new(0));
        let target_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("target listener should bind");
        let target_addr = target_listener.local_addr().expect("target address");
        let target_hits_server = Arc::clone(&target_hits);
        let target_task = tokio::spawn(async move {
            axum::serve(
                target_listener,
                Router::new()
                    .fallback(any(target_response))
                    .with_state(target_hits_server),
            )
            .await
            .expect("target server should run");
        });

        let unavailable_proxy_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("temporary proxy listener should bind");
        let unavailable_proxy_addr = unavailable_proxy_listener
            .local_addr()
            .expect("temporary proxy address");
        drop(unavailable_proxy_listener);

        let bundle = HttpClientBundle::build(
            OutboundHttpConfig::default(),
            ProxyRequestConfig::default(),
            Some(format!("http://{unavailable_proxy_addr}")),
        )
        .expect("proxy client bundle should build");
        let error = bundle
            .provider_client(true)
            .expect("configured proxy client should resolve")
            .get(format!("http://{target_addr}/must-not-be-reached"))
            .send()
            .await
            .expect_err("unavailable proxy should fail the request");

        assert!(error.is_connect());
        assert_eq!(target_hits.load(Ordering::SeqCst), 0);
        target_task.abort();
    }
}
