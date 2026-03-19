//! WebSocket AsyncRead/AsyncWrite adapter.
//!
//! Wraps a WebSocket stream (split into sink/stream) as a pair that
//! implements `AsyncRead` + `AsyncWrite`, so `handle_connection` can
//! use it exactly like a TLS stream.

use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_tungstenite::tungstenite::Message as WsMessage;

/// Wraps a `futures_util::Stream<Item = Result<WsMessage, ...>>` as `AsyncRead`.
pub struct WsReader<S> {
    stream: S,
    buf: VecDeque<u8>,
}

impl<S> WsReader<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            buf: VecDeque::new(),
        }
    }
}

impl<S> AsyncRead for WsReader<S>
where
    S: StreamExt<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // Drain buffered bytes first
        if !self.buf.is_empty() {
            let n = std::cmp::min(buf.remaining(), self.buf.len());
            let bytes: Vec<u8> = self.buf.drain(..n).collect();
            buf.put_slice(&bytes);
            return Poll::Ready(Ok(()));
        }

        // Poll the underlying WebSocket stream for the next message
        match Pin::new(&mut self.stream).poll_next(cx) {
            Poll::Ready(Some(Ok(WsMessage::Binary(data)))) => {
                let n = std::cmp::min(buf.remaining(), data.len());
                buf.put_slice(&data[..n]);
                if n < data.len() {
                    self.buf.extend(&data[n..]);
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Some(Ok(WsMessage::Text(text)))) => {
                let data = text.as_bytes();
                let n = std::cmp::min(buf.remaining(), data.len());
                buf.put_slice(&data[..n]);
                if n < data.len() {
                    self.buf.extend(&data[n..]);
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Some(Ok(WsMessage::Close(_)))) | Poll::Ready(None) => {
                // EOF
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Some(Ok(WsMessage::Ping(_) | WsMessage::Pong(_)))) => {
                // Skip control frames, re-poll
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Poll::Ready(Some(Ok(WsMessage::Frame(_)))) => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e))),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Wraps a `futures_util::Sink<WsMessage>` as `AsyncWrite`.
/// Buffers bytes and flushes them as Binary frames.
pub struct WsWriter<K> {
    sink: K,
    buf: Vec<u8>,
}

impl<K> WsWriter<K> {
    pub fn new(sink: K) -> Self {
        Self {
            sink,
            buf: Vec::new(),
        }
    }
}

impl<K> AsyncWrite for WsWriter<K>
where
    K: SinkExt<WsMessage, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.buf.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.buf.is_empty() {
            let data = std::mem::take(&mut self.buf);
            let msg = WsMessage::Binary(data.into());
            match Pin::new(&mut self.sink).poll_ready(cx) {
                Poll::Ready(Ok(())) => {
                    if let Err(e) = Pin::new(&mut self.sink).start_send(msg) {
                        return Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e)));
                    }
                }
                Poll::Ready(Err(e)) => {
                    return Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e)));
                }
                Poll::Pending => {
                    // Put data back
                    if let WsMessage::Binary(d) = msg {
                        self.buf = d.to_vec();
                    }
                    return Poll::Pending;
                }
            }
        }
        Pin::new(&mut self.sink)
            .poll_flush(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.sink)
            .poll_close(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn test_ws_reader_binary() {
        let data = b"hello world";
        let messages = vec![Ok(WsMessage::Binary(data.to_vec().into()))];
        let stream = futures_util::stream::iter(messages);
        let mut reader = WsReader::new(stream);

        let mut buf = vec![0u8; 32];
        let n = reader.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], data);
    }

    #[tokio::test]
    async fn test_ws_reader_text() {
        let text = "hello text";
        let messages = vec![Ok(WsMessage::Text(text.to_string().into()))];
        let stream = futures_util::stream::iter(messages);
        let mut reader = WsReader::new(stream);

        let mut buf = vec![0u8; 32];
        let n = reader.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], text.as_bytes());
    }

    #[tokio::test]
    async fn test_ws_reader_eof_on_close() {
        let messages: Vec<Result<WsMessage, tokio_tungstenite::tungstenite::Error>> =
            vec![Ok(WsMessage::Close(None))];
        let stream = futures_util::stream::iter(messages);
        let mut reader = WsReader::new(stream);

        let mut buf = vec![0u8; 32];
        let n = reader.read(&mut buf).await.unwrap();
        assert_eq!(n, 0); // EOF
    }
}
