use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, Proxy, Url};
use tokio::sync::RwLock;

use crate::config::ProxyRequestConfig;
#[cfg(test)]
use crate::database::TestDbContext;
use crate::proxy::logging::LogManager;

#[derive(Clone)]
pub struct HttpClientBundle {
    pub version: u64,
    pub client: Arc<Client>,
    pub proxy_client: Arc<Client>,
    pub proxy_request: ProxyRequestConfig,
    pub proxy: Option<String>,
}

pub struct HttpClientManager {
    current: RwLock<Arc<HttpClientBundle>>,
}

impl HttpClientManager {
    pub fn new(
        version: u64,
        proxy_request: ProxyRequestConfig,
        proxy: Option<String>,
    ) -> Result<Self, String> {
        let bundle = Self::build_bundle(version, proxy_request, proxy)?;
        Ok(Self {
            current: RwLock::new(Arc::new(bundle)),
        })
    }

    pub fn build_bundle(
        version: u64,
        proxy_request: ProxyRequestConfig,
        proxy: Option<String>,
    ) -> Result<HttpClientBundle, String> {
        let client = Arc::new(build_http_client(false, &proxy_request, proxy.as_deref())?);
        let proxy_client = Arc::new(build_http_client(true, &proxy_request, proxy.as_deref())?);

        Ok(HttpClientBundle {
            version,
            client,
            proxy_client,
            proxy_request,
            proxy,
        })
    }

    pub async fn current(&self) -> Arc<HttpClientBundle> {
        let current = self.current.read().await;
        Arc::clone(&current)
    }

    pub async fn replace_bundle(&self, bundle: HttpClientBundle) -> Arc<HttpClientBundle> {
        let bundle = Arc::new(bundle);
        *self.current.write().await = Arc::clone(&bundle);
        bundle
    }
}

pub struct AppInfra {
    http_clients: Arc<HttpClientManager>,
    log_manager: Arc<LogManager>,
    #[cfg(test)]
    test_db_context: Option<TestDbContext>,
}

impl AppInfra {
    pub(crate) async fn new_with_config(
        version: u64,
        proxy_request: ProxyRequestConfig,
        proxy: Option<String>,
        #[cfg(test)] test_db_context: Option<TestDbContext>,
    ) -> Self {
        let http_clients = Arc::new(
            HttpClientManager::new(version, proxy_request, proxy)
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

    pub(crate) fn http_clients(&self) -> Arc<HttpClientManager> {
        Arc::clone(&self.http_clients)
    }

    pub(crate) async fn client_bundle(&self) -> Arc<HttpClientBundle> {
        self.http_clients.current().await
    }

    pub(crate) async fn client(&self) -> Arc<Client> {
        Arc::clone(&self.http_clients.current().await.client)
    }

    pub(crate) async fn proxy_client(&self) -> Arc<Client> {
        Arc::clone(&self.http_clients.current().await.proxy_client)
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
        let err = match HttpClientManager::build_bundle(
            1,
            ProxyRequestConfig::default(),
            Some("socks5://127.0.0.1:1080".to_string()),
        ) {
            Ok(_) => panic!("invalid proxy scheme should fail"),
            Err(err) => err,
        };

        assert!(err.contains("invalid proxy URL"));
    }

    #[tokio::test]
    async fn http_client_manager_replace_keeps_old_bundle_alive() {
        let manager =
            HttpClientManager::new(1, ProxyRequestConfig::default(), None).expect("manager");
        let old = manager.current().await;
        let mut config = ProxyRequestConfig::default();
        config.first_byte_timeout_seconds = Some(120);
        let replacement =
            HttpClientManager::build_bundle(2, config.clone(), None).expect("replacement bundle");

        manager.replace_bundle(replacement).await;

        let current = manager.current().await;
        assert_eq!(old.version, 1);
        assert_eq!(current.version, 2);
        assert_eq!(current.proxy_request.first_byte_timeout_seconds, Some(120));
    }
}
