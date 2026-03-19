use anyhow::{Context, Result, bail};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::message::Message;

const MAX_FRAME_SIZE: u32 = 16 * 1024 * 1024; // 16 MB max

/// Send a message over a stream using length-prefixed framing.
/// Format: [4 bytes big-endian length][JSON payload]
pub async fn send_message<W: AsyncWrite + Unpin>(stream: &mut W, msg: &Message) -> Result<()> {
    let json = serde_json::to_vec(msg).context("Failed to serialize message")?;
    let len = json.len() as u32;

    stream
        .write_all(&len.to_be_bytes())
        .await
        .context("Failed to write frame length")?;
    stream
        .write_all(&json)
        .await
        .context("Failed to write frame payload")?;
    stream.flush().await.context("Failed to flush stream")?;

    Ok(())
}

/// Receive a message from a stream using length-prefixed framing.
/// Returns None if the stream is closed cleanly.
pub async fn recv_message<R: AsyncRead + Unpin>(stream: &mut R) -> Result<Option<Message>> {
    let mut len_buf = [0u8; 4];
    match stream.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e).context("Failed to read frame length"),
    }

    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME_SIZE {
        bail!("Frame too large: {} bytes (max {})", len, MAX_FRAME_SIZE);
    }

    let mut payload = vec![0u8; len as usize];
    stream
        .read_exact(&mut payload)
        .await
        .context("Failed to read frame payload")?;

    let msg: Message = serde_json::from_slice(&payload).context("Failed to deserialize message")?;

    Ok(Some(msg))
}
