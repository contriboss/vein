use rama::futures::stream::{unfold, Stream};
use tokio::sync::mpsc;

/// Convert a tokio mpsc::Receiver into a Stream
pub fn receiver_stream<T>(rx: mpsc::Receiver<T>) -> impl Stream<Item = T> {
    unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    })
}
