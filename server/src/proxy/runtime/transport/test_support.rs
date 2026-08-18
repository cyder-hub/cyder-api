use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
};

use axum::{
    body::{Body, Bytes},
    extract::Request,
    http::{StatusCode, header::CONTENT_TYPE},
    response::Response,
    routing::any,
    serve,
};
use futures::Stream;
use tokio::{
    net::TcpListener,
    sync::{Notify, mpsc, oneshot},
    task::JoinHandle,
};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

const BODY_CHANNEL_CAPACITY: usize = 8;

#[derive(Debug)]
pub enum BodyCommand {
    Chunk(Bytes),
    Eof,
}

#[derive(Debug, Default)]
struct Signal {
    notify: Notify,
    count: AtomicUsize,
}

impl Signal {
    fn notify(&self) {
        self.count.fetch_add(1, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    async fn wait(&self) {
        if self.count.load(Ordering::SeqCst) > 0 {
            return;
        }
        self.notify.notified().await;
    }
}

type ResolveError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolverBehavior {
    ImmediateFailure,
    Pending,
}

#[derive(Clone)]
pub struct ControlledResolver {
    behavior: ResolverBehavior,
    resolve_seen: Arc<Signal>,
    pending_dropped: Arc<Signal>,
}

impl ControlledResolver {
    pub fn immediate_failure() -> Self {
        Self::new(ResolverBehavior::ImmediateFailure)
    }

    pub fn pending() -> Self {
        Self::new(ResolverBehavior::Pending)
    }

    pub fn new(behavior: ResolverBehavior) -> Self {
        Self {
            behavior,
            resolve_seen: Arc::new(Signal::default()),
            pending_dropped: Arc::new(Signal::default()),
        }
    }

    pub fn resolve_call_count(&self) -> usize {
        self.resolve_seen.count.load(Ordering::SeqCst)
    }

    pub fn pending_drop_count(&self) -> usize {
        self.pending_dropped.count.load(Ordering::SeqCst)
    }

    pub async fn wait_resolve(&self) {
        self.resolve_seen.wait().await;
    }

    pub async fn wait_pending_dropped(&self) {
        self.pending_dropped.wait().await;
    }
}

struct PendingResolution {
    dropped: Arc<Signal>,
}

impl Future for PendingResolution {
    type Output = Result<Addrs, ResolveError>;

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for PendingResolution {
    fn drop(&mut self) {
        self.dropped.notify();
    }
}

impl Resolve for ControlledResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        if !host.starts_with("r3-") || !host.ends_with(".invalid") {
            return Box::pin(async move {
                let addrs = tokio::net::lookup_host((host, 0)).await?;
                Ok(Box::new(addrs) as Addrs)
            });
        }

        self.resolve_seen.notify();
        match self.behavior {
            ResolverBehavior::ImmediateFailure => Box::pin(std::future::ready(Err(Box::new(
                io::Error::new(io::ErrorKind::NotFound, "controlled DNS failure"),
            )
                as ResolveError))),
            ResolverBehavior::Pending => Box::pin(PendingResolution {
                dropped: Arc::clone(&self.pending_dropped),
            }),
        }
    }
}

struct ControlledBodyStream {
    receiver: mpsc::Receiver<BodyCommand>,
    dropped: Arc<Signal>,
    chunk_seen: Arc<Signal>,
}

impl Stream for ControlledBodyStream {
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.receiver.poll_recv(cx) {
            Poll::Ready(Some(BodyCommand::Chunk(bytes))) => {
                if !bytes.is_empty() {
                    this.chunk_seen.notify();
                }
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(BodyCommand::Eof)) | Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for ControlledBodyStream {
    fn drop(&mut self) {
        self.dropped.notify();
    }
}

pub struct ControlledUpstream {
    pub base_url: String,
    request_started: Arc<Signal>,
    request_seen: Arc<Signal>,
    headers_waiting: Arc<Signal>,
    headers_release: Arc<Notify>,
    body_sender: Option<oneshot::Receiver<mpsc::Sender<BodyCommand>>>,
    body_chunk_seen: Arc<Signal>,
    body_dropped: Arc<Signal>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl ControlledUpstream {
    pub async fn start() -> Self {
        Self::start_with_status(StatusCode::OK).await
    }

    pub async fn start_with_status(response_status: StatusCode) -> Self {
        let request_started = Arc::new(Signal::default());
        let request_seen = Arc::new(Signal::default());
        let headers_waiting = Arc::new(Signal::default());
        let headers_release = Arc::new(Notify::new());
        let body_chunk_seen = Arc::new(Signal::default());
        let body_dropped = Arc::new(Signal::default());
        let (body_sender_tx, body_sender) = oneshot::channel();
        let body_sender_tx = Arc::new(Mutex::new(Some(body_sender_tx)));

        let router = axum::Router::new().fallback(any({
            let request_started = Arc::clone(&request_started);
            let request_seen = Arc::clone(&request_seen);
            let headers_waiting = Arc::clone(&headers_waiting);
            let headers_release = Arc::clone(&headers_release);
            let body_chunk_seen = Arc::clone(&body_chunk_seen);
            let body_dropped = Arc::clone(&body_dropped);
            let body_sender_tx = Arc::clone(&body_sender_tx);
            move |request: Request<Body>| {
                let request_started = Arc::clone(&request_started);
                let request_seen = Arc::clone(&request_seen);
                let headers_waiting = Arc::clone(&headers_waiting);
                let headers_release = Arc::clone(&headers_release);
                let body_chunk_seen = Arc::clone(&body_chunk_seen);
                let body_dropped = Arc::clone(&body_dropped);
                let body_sender_tx = Arc::clone(&body_sender_tx);
                async move {
                    let (_parts, body) = request.into_parts();
                    request_started.notify();
                    if axum::body::to_bytes(body, usize::MAX).await.is_err() {
                        return Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(Body::empty())
                            .expect("controlled upstream rejected response should build");
                    }
                    request_seen.notify();
                    headers_waiting.notify();
                    headers_release.notified().await;

                    let (body_tx, body_rx) = mpsc::channel(BODY_CHANNEL_CAPACITY);
                    body_sender_tx
                        .lock()
                        .expect("controlled upstream sender lock should not be poisoned")
                        .take()
                        .expect("controlled upstream accepts one request")
                        .send(body_tx)
                        .expect("controlled upstream body sender should be observed");
                    let stream = ControlledBodyStream {
                        receiver: body_rx,
                        dropped: body_dropped,
                        chunk_seen: body_chunk_seen,
                    };
                    Response::builder()
                        .status(response_status)
                        .header(CONTENT_TYPE, "application/octet-stream")
                        .body(Body::from_stream(stream))
                        .expect("controlled upstream response should build")
                }
            }
        }));

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("controlled upstream should bind");
        let address = listener
            .local_addr()
            .expect("controlled upstream address should be available");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("controlled upstream should run");
        });

        Self {
            base_url: format!("http://{address}"),
            request_started,
            request_seen,
            headers_waiting,
            headers_release,
            body_sender: Some(body_sender),
            body_chunk_seen,
            body_dropped,
            shutdown_tx: Some(shutdown_tx),
            task,
        }
    }

    pub async fn wait_request_started(&self) {
        self.request_started.wait().await;
    }

    pub fn request_started_count(&self) -> usize {
        self.request_started.count.load(Ordering::SeqCst)
    }

    pub async fn wait_request(&self) {
        self.request_seen.wait().await;
    }

    pub async fn wait_headers_waiting(&self) {
        self.headers_waiting.wait().await;
    }

    pub fn request_seen_count(&self) -> usize {
        self.request_seen.count.load(Ordering::SeqCst)
    }

    pub fn release_headers(&self) {
        self.headers_release.notify_one();
    }

    pub async fn take_body(&mut self) -> mpsc::Sender<BodyCommand> {
        self.body_sender
            .take()
            .expect("controlled upstream body sender can only be taken once")
            .await
            .expect("controlled upstream body sender should be sent")
    }

    pub async fn wait_body_chunk_seen(&self) {
        self.body_chunk_seen.wait().await;
    }

    pub async fn wait_body_dropped(&self) {
        self.body_dropped.wait().await;
    }

    pub fn body_drop_count(&self) -> usize {
        self.body_dropped.count.load(Ordering::SeqCst)
    }

    pub async fn shutdown(mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        (&mut self.task)
            .await
            .expect("controlled upstream task should join");
    }
}

impl Drop for ControlledUpstream {
    fn drop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use futures::StreamExt;
    use reqwest::dns::Name;
    use reqwest::{Client, dns::Resolve};
    use tokio::time::timeout;

    use super::{BodyCommand, ControlledResolver, ControlledUpstream};

    #[tokio::test]
    async fn headers_are_released_before_the_first_body_chunk() {
        let mut upstream = ControlledUpstream::start().await;
        let client = Client::new();
        let request = client.get(format!("{}/controlled", upstream.base_url));
        let response_task = tokio::spawn(async move { request.send().await });

        upstream.wait_request().await;
        upstream.wait_headers_waiting().await;
        upstream.release_headers();
        let response = timeout(Duration::from_secs(1), response_task)
            .await
            .expect("headers should be returned without a body chunk")
            .expect("request task should join")
            .expect("controlled response should succeed");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let body = upstream.take_body().await;
        body.send(BodyCommand::Chunk("first".into()))
            .await
            .expect("first body chunk should be accepted");
        body.send(BodyCommand::Eof)
            .await
            .expect("body EOF should be accepted");

        let chunks = response.bytes_stream().collect::<Vec<_>>().await;
        assert_eq!(chunks.len(), 1);
        assert_eq!(
            chunks[0]
                .as_ref()
                .expect("body chunk should be valid")
                .as_ref(),
            b"first".as_slice()
        );
        drop(body);
        upstream.shutdown().await;
    }

    #[tokio::test]
    async fn immediate_dns_failure_is_reported_without_connecting() {
        let resolver = Arc::new(ControlledResolver::immediate_failure());
        let client = Client::builder()
            .dns_resolver2(Arc::clone(&resolver))
            .build()
            .expect("resolver test client should build");

        let error = client
            .get("http://r3-dns-failure.invalid/")
            .send()
            .await
            .expect_err("immediate resolver failure should fail the request");

        assert!(error.is_connect());
        assert_eq!(resolver.resolve_call_count(), 1);
    }

    #[tokio::test]
    async fn pending_dns_resolution_is_dropped_when_request_is_cancelled() {
        let resolver = Arc::new(ControlledResolver::pending());
        let client = Client::builder()
            .dns_resolver2(Arc::clone(&resolver))
            .build()
            .expect("resolver test client should build");
        let request_task =
            tokio::spawn(async move { client.get("http://r3-dns-pending.invalid/").send().await });

        resolver.wait_resolve().await;
        request_task.abort();
        let _ = request_task.await;
        let _: () = timeout(Duration::from_secs(1), resolver.wait_pending_dropped())
            .await
            .expect("pending DNS future should be released after cancellation");
        assert_eq!(resolver.resolve_call_count(), 1);
        assert_eq!(resolver.pending_drop_count(), 1);
    }

    #[tokio::test]
    async fn resolver_delegates_non_fixture_hosts_to_system_lookup() {
        let resolver = ControlledResolver::immediate_failure();
        let name = "localhost"
            .parse::<Name>()
            .expect("localhost should be a valid DNS name");

        let addrs = resolver
            .resolve(name)
            .await
            .expect("non-fixture host should use normal DNS resolution")
            .collect::<Vec<_>>();

        assert!(!addrs.is_empty());
        assert_eq!(resolver.resolve_call_count(), 0);
    }

    #[tokio::test]
    async fn dropping_an_unpolled_response_closes_the_controlled_body_once() {
        let mut upstream = ControlledUpstream::start().await;
        let response_task = tokio::spawn({
            let url = format!("{}/held", upstream.base_url);
            async move { Client::new().get(url).send().await }
        });
        upstream.wait_request().await;
        upstream.wait_headers_waiting().await;
        upstream.release_headers();
        let response = response_task
            .await
            .expect("request task should join")
            .expect("controlled response should succeed");
        let body = upstream.take_body().await;
        drop(response);
        drop(body);
        timeout(Duration::from_secs(1), upstream.wait_body_dropped())
            .await
            .expect("body drop should not deadlock");
        assert_eq!(upstream.body_drop_count(), 1);
        upstream.shutdown().await;
    }
}
