use std::{
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

struct ControlledBodyStream {
    receiver: mpsc::Receiver<BodyCommand>,
    dropped: Arc<Signal>,
}

impl Stream for ControlledBodyStream {
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.receiver.poll_recv(cx) {
            Poll::Ready(Some(BodyCommand::Chunk(bytes))) => Poll::Ready(Some(Ok(bytes))),
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
    request_seen: Arc<Signal>,
    headers_release: Arc<Notify>,
    body_sender: Option<oneshot::Receiver<mpsc::Sender<BodyCommand>>>,
    body_dropped: Arc<Signal>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl ControlledUpstream {
    pub async fn start() -> Self {
        let request_seen = Arc::new(Signal::default());
        let headers_release = Arc::new(Notify::new());
        let body_dropped = Arc::new(Signal::default());
        let (body_sender_tx, body_sender) = oneshot::channel();
        let body_sender_tx = Arc::new(Mutex::new(Some(body_sender_tx)));

        let router = axum::Router::new().fallback(any({
            let request_seen = Arc::clone(&request_seen);
            let headers_release = Arc::clone(&headers_release);
            let body_dropped = Arc::clone(&body_dropped);
            let body_sender_tx = Arc::clone(&body_sender_tx);
            move |request: Request<Body>| {
                let request_seen = Arc::clone(&request_seen);
                let headers_release = Arc::clone(&headers_release);
                let body_dropped = Arc::clone(&body_dropped);
                let body_sender_tx = Arc::clone(&body_sender_tx);
                async move {
                    let (_parts, body) = request.into_parts();
                    let _ = axum::body::to_bytes(body, usize::MAX)
                        .await
                        .expect("controlled upstream request body should be readable");
                    request_seen.notify();
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
                    };
                    Response::builder()
                        .status(StatusCode::OK)
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
            request_seen,
            headers_release,
            body_sender: Some(body_sender),
            body_dropped,
            shutdown_tx: Some(shutdown_tx),
            task,
        }
    }

    pub async fn wait_request(&self) {
        self.request_seen.wait().await;
    }

    pub fn release_headers(&self) {
        self.headers_release.notify_waiters();
    }

    pub async fn take_body(&mut self) -> mpsc::Sender<BodyCommand> {
        self.body_sender
            .take()
            .expect("controlled upstream body sender can only be taken once")
            .await
            .expect("controlled upstream body sender should be sent")
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
    use std::time::Duration;

    use futures::StreamExt;
    use reqwest::Client;
    use tokio::time::timeout;

    use super::{BodyCommand, ControlledUpstream};

    #[tokio::test]
    async fn headers_are_released_before_the_first_body_chunk() {
        let mut upstream = ControlledUpstream::start().await;
        let client = Client::new();
        let request = client.get(format!("{}/controlled", upstream.base_url));
        let response_task = tokio::spawn(async move { request.send().await });

        upstream.wait_request().await;
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
    async fn dropping_an_unpolled_response_closes_the_controlled_body_once() {
        let mut upstream = ControlledUpstream::start().await;
        let response_task = tokio::spawn({
            let url = format!("{}/held", upstream.base_url);
            async move { Client::new().get(url).send().await }
        });
        upstream.wait_request().await;
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
