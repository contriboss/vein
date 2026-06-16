use rama::futures::StreamExt;
use rama::futures::stream::{Stream, unfold};
use rama::http::service::web::response::{Html, IntoResponse, Sse};
use rama::http::sse::Event;
use rama::http::sse::server::{KeepAlive, KeepAliveStream};
use tokio::sync::mpsc;

/// Renders an error as a minimal HTML page.
pub fn error_html(err: impl std::fmt::Display) -> Html<String> {
    Html(format!("<h1>Error: {err}</h1>"))
}

/// Convert a tokio mpsc::Receiver into a Stream
pub fn receiver_stream<T>(rx: mpsc::Receiver<T>) -> impl Stream<Item = T> {
    unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    })
}

/// Builds a datastar `patch-elements` SSE event carrying the given fragments.
pub fn datastar_patch_event(fragments: impl std::fmt::Display) -> Event<String> {
    Event::default()
        .try_with_event("datastar-patch-elements")
        .expect("valid event name")
        .with_data(format!("fragments {fragments}"))
}

/// Wraps an event receiver in a keep-alive SSE response.
pub fn sse_from_receiver(rx: mpsc::Receiver<Event<String>>) -> impl IntoResponse {
    let stream = receiver_stream(rx);
    Sse::new(KeepAliveStream::new(
        KeepAlive::new(),
        stream.map(Ok::<_, std::convert::Infallible>),
    ))
}
