use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, Proxy, Url};

use crate::config::ProxyRequestConfig;
#[cfg(test)]
use crate::database::TestDbContext;
use crate::proxy::logging::LogManager;

#[derive(Clone)]
pub struct HttpClientBundle {
    pub client: Arc<Client>,
    pub proxy_client: Arc<Client>,
    pub proxy_request: ProxyRequestConfig,
    pub proxy: Option<String>,
}

impl HttpClientBundle {
    pub fn build(
        proxy_request: ProxyRequestConfig,
        proxy: Option<String>,
    ) -> Result<HttpClientBundle, String> {
        let client = Arc::new(build_http_client(false, &proxy_request, proxy.as_deref())?);
        let proxy_client = Arc::new(build_http_client(true, &proxy_request, proxy.as_deref())?);

        Ok(HttpClientBundle {
            client,
            proxy_client,
            proxy_request,
            proxy,
        })
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
        proxy_request: ProxyRequestConfig,
        proxy: Option<String>,
        #[cfg(test)] test_db_context: Option<TestDbContext>,
    ) -> Self {
        let http_clients = Arc::new(
            HttpClientBundle::build(proxy_request, proxy)
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

    pub(crate) async fn client_bundle(&self) -> Arc<HttpClientBundle> {
        Arc::clone(&self.http_clients)
    }

    pub(crate) async fn client(&self) -> Arc<Client> {
        Arc::clone(&self.http_clients.client)
    }

    pub(crate) async fn proxy_client(&self) -> Arc<Client> {
        Arc::clone(&self.http_clients.proxy_client)
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

fn optional_duration_to_millis(duration: Option<Duration>) -> Option<u64> {
    duration.map(duration_to_millis)
}

fn build_http_client(
    use_proxy: bool,
    proxy_request_config: &ProxyRequestConfig,
    proxy_url: Option<&str>,
) -> Result<Client, String> {
    let connect_timeout = proxy_request_config.connect_timeout();
    let total_timeout = proxy_request_config.total_timeout();

    let mut builder = Client::builder().connect_timeout(connect_timeout);

    if let Some(timeout) = total_timeout {
        builder = builder.timeout(timeout);
    }

    if use_proxy {
        if let Some(proxy_url) = proxy_url {
            let parsed = Url::parse(proxy_url)
                .map_err(|err| format!("invalid proxy URL in configuration: {err}"))?;
            match parsed.scheme() {
                "http" | "https" => {}
                scheme => {
                    return Err(format!(
                        "invalid proxy URL in configuration: only http and https are supported, got {scheme}"
                    ));
                }
            }
            let proxy = Proxy::all(proxy_url)
                .map_err(|err| format!("invalid proxy URL in configuration: {err}"))?;
            builder = builder.proxy(proxy);
        }
    }

    crate::info_event!(
        "startup.http_client_built",
        client_kind = if use_proxy { "proxy" } else { "default" },
        use_proxy = use_proxy,
        connect_timeout_ms = duration_to_millis(connect_timeout),
        first_byte_timeout_ms =
            optional_duration_to_millis(proxy_request_config.first_byte_timeout()),
        total_timeout_ms = optional_duration_to_millis(total_timeout),
    );

    builder.build().map_err(|err| {
        format!(
            "failed to build {} reqwest client: {}",
            if use_proxy { "proxy" } else { "default" },
            err
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_client_bundle_rejects_invalid_proxy_url() {
        let err = match HttpClientBundle::build(
            ProxyRequestConfig::default(),
            Some("socks5://127.0.0.1:1080".to_string()),
        ) {
            Ok(_) => panic!("invalid proxy scheme should fail"),
            Err(err) => err,
        };

        assert!(err.contains("invalid proxy URL"));
    }
}
