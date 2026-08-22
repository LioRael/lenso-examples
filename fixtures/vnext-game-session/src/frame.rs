use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_DISCARD_BYTES: usize = 64 * 1024;

/// The first frame on a game connection selects one credential and one room.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClientFrame {
    /// Starts one authenticated game session.
    Hello {
        #[serde(default)]
        token: Option<String>,
        room: String,
        #[serde(default)]
        deadline_ms: Option<u64>,
    },
    /// Sends one game action through the portable Stream Operation.
    Message { action: String },
    /// Half-closes the client sending direction.
    CloseSend,
    /// Cancels the game session.
    Cancel,
}

/// One explicit terminal result for an established game session.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TerminalFrame {
    /// The provider completed the session normally.
    Success,
    /// The game provider returned a declared Domain Error.
    Domain { code: String },
    /// The Runtime or protocol boundary terminated the session.
    Runtime { code: String },
}

/// Frames emitted by the protocol Module.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerFrame {
    /// The authenticated stream is open.
    Ready { room: String },
    /// One provider-to-client game message.
    Message { action: String },
    /// The provider closed only its sending direction.
    PeerHalfClosed,
    /// The stream's one terminal outcome.
    Terminal { outcome: TerminalFrame },
    /// Authentication or stream-open Domain Error before establishment.
    Rejected { code: String },
    /// A bounded Runtime Failure at the protocol boundary.
    Runtime { code: String },
}

#[derive(Debug)]
pub(crate) enum FrameError {
    Io(std::io::Error),
    Malformed,
    TooLarge,
    Truncated,
}

pub(crate) async fn read_client_frame<R>(
    stream: &mut R,
    max_frame_bytes: usize,
) -> Result<Option<ClientFrame>, FrameError>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0_u8; 4];
    let first = stream.read(&mut header).await.map_err(FrameError::Io)?;
    if first == 0 {
        return Ok(None);
    }
    if first != header.len() {
        stream
            .read_exact(&mut header[first..])
            .await
            .map_err(|_| FrameError::Truncated)?;
    }
    let length = usize::try_from(u32::from_be_bytes(header)).unwrap_or(usize::MAX);
    if length == 0 || length > max_frame_bytes {
        if length <= MAX_DISCARD_BYTES {
            let mut remaining = length;
            let mut scratch = [0_u8; 4 * 1024];
            while remaining > 0 {
                let chunk = remaining.min(scratch.len());
                stream
                    .read_exact(&mut scratch[..chunk])
                    .await
                    .map_err(|_| FrameError::Truncated)?;
                remaining -= chunk;
            }
        }
        return Err(FrameError::TooLarge);
    }
    let mut payload = vec![0_u8; length];
    stream
        .read_exact(&mut payload)
        .await
        .map_err(|_| FrameError::Truncated)?;
    serde_json::from_slice(&payload).map_err(|_| FrameError::Malformed)
}

pub(crate) async fn write_server_frame<W>(
    stream: &mut W,
    frame: &ServerFrame,
    max_frame_bytes: usize,
) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
{
    let payload = serde_json::to_vec(frame).map_err(|_| FrameError::Malformed)?;
    if payload.is_empty() || payload.len() > max_frame_bytes || payload.len() > u32::MAX as usize {
        return Err(FrameError::TooLarge);
    }
    let length = u32::try_from(payload.len()).map_err(|_| FrameError::TooLarge)?;
    stream
        .write_all(&length.to_be_bytes())
        .await
        .map_err(FrameError::Io)?;
    stream.write_all(&payload).await.map_err(FrameError::Io)
}
