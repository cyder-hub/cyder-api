use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, Proxy};

use crate::config::CONFIG;
#[cfg(test)]
use crate::database::TestDbContext;
use crate::proxy::logging::LogManager;

pub struct AppInfra {
    client: Client,
    proxy_client: Client,
    log_manager: Arc<LogManager>,
    #[cfg(test)]
    test_db_context: Option<TestDbContext>,
}

impl AppInfra {
    #[cfg(not(test))]
    pub(crate) async fn new() -> Self {
        Self::new_with_test_db_context().await
    }

    #[cfg(test)]
    pub(crate) async fn new() -> Self {
        Self::new_with_test_db_context(None).await
    }

    #[cfg(test)]
    pub(crate) async fn new_for_test(test_db_context: TestDbContext) -> Self {
        Self::new_with_test_db_context(Some(test_db_context)).await
    }

    async fn new_with_test_db_context(#[cfg(test)] test_db_context: Option<TestDbContext>) -> Self {
        let client = Self::build_http_client(false);
        let proxy_client = Self::build_http_client(true);
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
            client,
            proxy_client,
            log_manager,
            #[cfg(test)]
            test_db_context,
        }
    }

    fn build_http_client(use_proxy: bool) -> Client {
        let proxy_request_config = &CONFIG.proxy_request;
        let connect_timeout = proxy_request_config.connect_timeout();
        let total_timeout = proxy_request_config.total_timeout();

        let mut builder = Client::builder().connect_timeout(connect_timeout);

        if let Some(timeout) = total_timeout {
            builder = builder.timeout(timeout);
        }

        if use_proxy {
            if let Some(proxy_url) = &CONFIG.proxy {
                let proxy = Proxy::all(proxy_url).expect("Invalid proxy URL in configuration");
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

        builder.build().unwrap_or_else(|err| {
            panic!(
                "Failed to build {} reqwest client: {}",
                if use_proxy { "proxy" } else { "default" },
                err
            )
        })
    }

    pub(crate) fn client(&self) -> &Client {
        &self.client
    }

    pub(crate) fn proxy_client(&self) -> &Client {
        &self.proxy_client
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
