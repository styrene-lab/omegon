//! Frame encode/decode for the IPC wire protocol.
//!
//! Framing: [u32 BE length][msgpack bytes]

use anyhow::{bail, Context as _};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use omegon_traits::{IpcEnvelope, IPC_MAX_FRAME_BYTES};

/// Read one framed message from the stream.
/// Returns `None` on clean EOF.
pub async fn read_frame<R>(reader: &mut R) -> anyhow::Result<Option<Vec<u8>>>
where
    R: AsyncReadExt + Unpin,
{
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > IPC_MAX_FRAME_BYTES {
        bail!("frame too large: {len} bytes (max {IPC_MAX_FRAME_BYTES})");
    }
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .await
        .context("reading frame body")?;
    Ok(Some(buf))
}

/// Write one framed message to the stream.
pub async fn write_frame<W>(writer: &mut W, data: &[u8]) -> anyhow::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    if data.len() > IPC_MAX_FRAME_BYTES {
        bail!("frame too large: {} bytes (max {IPC_MAX_FRAME_BYTES})", data.len());
    }
    let len = (data.len() as u32).to_be_bytes();
    writer.write_all(&len).await.context("writing frame length")?;
    writer.write_all(data).await.context("writing frame body")?;
    writer.flush().await.context("flushing frame")?;
    Ok(())
}

/// Encode an envelope as a framed msgpack message.
pub fn encode_envelope(env: &IpcEnvelope) -> anyhow::Result<Vec<u8>> {
    env.encode_msgpack().context("encoding IpcEnvelope as msgpack")
}

/// Decode a raw frame into an envelope.
pub fn decode_envelope(raw: &[u8]) -> anyhow::Result<IpcEnvelope> {
    IpcEnvelope::decode_msgpack(raw).context("decoding IpcEnvelope from msgpack")
}

#[cfg(test)]
mod tests {
    use super::*;
    use omegon_traits::IpcEnvelopeKind;

    #[tokio::test]
    async fn round_trip_frame() {
        let env = IpcEnvelope {
            protocol_version: omegon_traits::IPC_PROTOCOL_VERSION,
            kind: IpcEnvelopeKind::Request,
            request_id: Some(*b"abcdefghijklmnop"),
            method: Some("ping".into()),
            payload: None,
            error: None,
        };

        let encoded = encode_envelope(&env).unwrap();
        let mut buf = Vec::new();
        write_frame(&mut buf, &encoded).await.unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let raw = read_frame(&mut cursor).await.unwrap().unwrap();
        let decoded = decode_envelope(&raw).unwrap();

        assert_eq!(decoded.method, Some("ping".into()));
        assert_eq!(decoded.kind, IpcEnvelopeKind::Request);
    }

    #[tokio::test]
    async fn eof_returns_none() {
        let buf: &[u8] = &[];
        let mut cursor = std::io::Cursor::new(buf);
        let result = read_frame(&mut cursor).await.unwrap();
        assert!(result.is_none());
    }
}
