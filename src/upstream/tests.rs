use super::*;

use crate::test_support::{install_rustls_provider, raw_response};

use crate::config::{RetryConfig, reliability::ReliabilityConfig};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn client(max_attempts: u32) -> UpstreamClient {
    install_rustls_provider();
    UpstreamClient::new(&UpstreamConfig {
        reliability: ReliabilityConfig {
            retry: RetryConfig {
                max_attempts,
                initial_backoff_ms: 1,
                ..RetryConfig::default()
            },
        },
        ..UpstreamConfig::default()
    })
    .unwrap()
}

/// Serves `responses` in order, repeating the last one forever. Returns
/// the base URL and a live count of requests served.
async fn spawn_server(responses: Vec<Vec<u8>>) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let served = hits.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0_u8; 4096];
            let mut request = Vec::new();
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let read = socket.read(&mut buf).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..read]);
            }
            let n = served.fetch_add(1, Ordering::SeqCst);
            let response = &responses[n.min(responses.len() - 1)];
            socket.write_all(response).await.unwrap();
        }
    });
    (format!("http://{addr}/"), hits)
}

fn uri(base: &str) -> Uri {
    Uri::parse(base).unwrap()
}

#[tokio::test]
async fn retries_server_errors_then_relays_the_last_one() {
    let (base, hits) =
        spawn_server(vec![raw_response("503 Service Unavailable", &[], b"ok")]).await;

    let response = client(3)
        .get_with_headers(uri(&base), &HeaderMap::new())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(hits.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn honors_retry_after_within_the_backoff_cap() {
    let (base, hits) = spawn_server(vec![
        raw_response("429 Too Many Requests", &[("Retry-After", "0")], b"ok"),
        raw_response("200 OK", &[], b"ok"),
    ])
    .await;

    let response = client(3)
        .get_with_headers(uri(&base), &HeaderMap::new())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn relays_rate_limit_at_once_when_retry_after_exceeds_the_cap() {
    let (base, hits) = spawn_server(vec![raw_response(
        "429 Too Many Requests",
        &[("Retry-After", "3600")],
        b"ok",
    )])
    .await;

    let response = client(3)
        .get_with_headers(uri(&base), &HeaderMap::new())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()[header::RETRY_AFTER], "3600");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn open_breaker_isolates_the_failing_upstream_only() {
    let (failing, failing_hits) =
        spawn_server(vec![raw_response("500 Internal Server Error", &[], b"ok")]).await;
    let (healthy, healthy_hits) = spawn_server(vec![raw_response("200 OK", &[], b"ok")]).await;
    let client = client(1);

    for _ in 0..5 {
        let response = client
            .get_with_headers(uri(&failing), &HeaderMap::new())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    let err = client
        .get_with_headers(uri(&failing), &HeaderMap::new())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("is open"), "{err}");
    assert_eq!(failing_hits.load(Ordering::SeqCst), 5);

    let response = client
        .get_with_headers(uri(&healthy), &HeaderMap::new())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(healthy_hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn open_breaker_recovers_once_upstream_does() {
    let mut responses = vec![raw_response("500 Internal Server Error", &[], b"ok"); 5];
    responses.push(raw_response("200 OK", &[], b"ok"));
    let (base, hits) = spawn_server(responses).await;
    let client = UpstreamClient {
        half_open_timeout_secs: 0.05,
        ..client(1)
    };

    for _ in 0..5 {
        client
            .get_with_headers(uri(&base), &HeaderMap::new())
            .await
            .unwrap();
    }
    assert!(
        client
            .get_with_headers(uri(&base), &HeaderMap::new())
            .await
            .is_err()
    );

    tokio::time::sleep(Duration::from_millis(100)).await;

    for _ in 0..3 {
        let response = client
            .get_with_headers(uri(&base), &HeaderMap::new())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert_eq!(hits.load(Ordering::SeqCst), 8);
}

#[tokio::test]
async fn buffer_response_keeps_repeated_headers() {
    let mut response = Response::new(Body::from("payload"));
    let headers = response.headers_mut();
    headers.append(header::VARY, HeaderValue::from_static("Accept"));
    headers.append(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    headers.insert(
        header::TRANSFER_ENCODING,
        HeaderValue::from_static("chunked"),
    );

    let buffered = buffer_response(response, true).await.unwrap();

    let vary: Vec<_> = buffered.headers().get_all(header::VARY).iter().collect();
    assert_eq!(vary, ["Accept", "Accept-Encoding"]);
    assert!(!buffered.headers().contains_key(header::TRANSFER_ENCODING));
    let body = buffered.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), b"payload");
}
