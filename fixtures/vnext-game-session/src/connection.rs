use std::time::{Duration, Instant};

use lenso_auth_sdk::{AuthOutcome, CredentialEvidence, authenticate_request, decode_auth_response};
use lenso_capability_auth::{AuthInvocationError, AuthenticateError};
use lenso_capability_game_session::{
    GameSessionInvocationError, PlayError, PlayRequest, PlayResponse,
};
use lenso_kernel::{CancellationToken, NativeStream, RuntimeFailure, StreamEvent};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};

use crate::{
    auth::GAME_CREDENTIAL_SCHEME,
    frame::{
        ClientFrame, FrameError, ServerFrame, TerminalFrame, read_client_frame, write_server_frame,
    },
    protocol::{ConnectionRuntime, ProtocolConfig},
};

pub(crate) async fn serve_connection(mut socket: TcpStream, connection: ConnectionRuntime) {
    let Some((stream, session_deadline, session_cancellation, room)) =
        establish_session(&mut socket, &connection).await
    else {
        return;
    };
    if send_frame(
        &mut socket,
        &connection.config,
        &ServerFrame::Ready { room },
    )
    .await
    .is_err()
    {
        stream.cancel();
        return;
    }
    let (reader, mut writer) = socket.into_split();
    run_session_loop(
        reader,
        &mut writer,
        &connection.config,
        stream,
        session_deadline,
        session_cancellation,
        connection.module_cancellation,
    )
    .await;
}

async fn establish_session(
    socket: &mut TcpStream,
    connection: &ConnectionRuntime,
) -> Option<(
    NativeStream<lenso_capability_game_session::GameSession>,
    Instant,
    CancellationToken,
    String,
)> {
    let config = &connection.config;
    let initial_deadline = Instant::now() + config.session_timeout();
    let hello = match read_frame_with_deadline(
        socket,
        config.max_frame_bytes(),
        config.idle_timeout(),
        initial_deadline,
    )
    .await
    {
        Ok(Some(frame)) => frame,
        Ok(None) => return None,
        Err(error) => {
            let _ = send_read_error(socket, config, error).await;
            return None;
        }
    };
    let ClientFrame::Hello {
        token,
        room,
        deadline_ms,
    } = hello
    else {
        let _ = send_runtime(socket, config, "protocol_violation").await;
        return None;
    };
    if room.trim().is_empty() {
        let _ = send_rejected(socket, config, "invalid_room").await;
        return None;
    }

    let requested_timeout =
        deadline_ms.map_or_else(|| config.session_timeout(), Duration::from_millis);
    let session_timeout = requested_timeout.min(config.session_timeout());
    if session_timeout.is_zero() {
        let _ = send_runtime(socket, config, "deadline_exceeded").await;
        return None;
    }
    let session_deadline = Instant::now() + session_timeout;
    let session_cancellation = CancellationToken::new();
    let Ok(context) = connection
        .dependencies
        .invocation_context_after(session_timeout, session_cancellation.clone())
    else {
        let _ = send_runtime(socket, config, "admission_closed").await;
        return None;
    };
    let evidence = token.map(|token| CredentialEvidence::new(GAME_CREDENTIAL_SCHEME, token));
    let auth_response = match connection
        .auth
        .authenticate_with_context(context.clone(), authenticate_request(evidence))
        .await
    {
        Ok(response) => response,
        Err(AuthInvocationError::Domain(error)) => {
            let _ = send_rejected(socket, config, auth_error_code(&error)).await;
            return None;
        }
        Err(AuthInvocationError::Runtime(error)) => {
            let _ = send_runtime(socket, config, &runtime_error_code(&error)).await;
            return None;
        }
    };
    let Ok(outcome) = decode_auth_response(auth_response) else {
        let _ = send_runtime(socket, config, "protocol_violation").await;
        return None;
    };
    let assertion = match outcome {
        AuthOutcome::Absent => {
            let _ = send_rejected(socket, config, "credential_required").await;
            return None;
        }
        AuthOutcome::Authenticated(assertion) => assertion,
    };
    let Ok(context) = assertion.attach(context) else {
        let _ = send_runtime(socket, config, "protocol_violation").await;
        return None;
    };
    let stream = match connection
        .game
        .play_with_context(context, PlayRequest { room: room.clone() })
        .await
    {
        Ok(stream) => stream,
        Err(GameSessionInvocationError::Domain(error)) => {
            let _ = send_rejected(socket, config, game_error_code(&error)).await;
            return None;
        }
        Err(GameSessionInvocationError::Runtime(error)) => {
            let _ = send_runtime(socket, config, &runtime_error_code(&error)).await;
            return None;
        }
    };
    Some((stream, session_deadline, session_cancellation, room))
}

async fn run_session_loop<R, W>(
    reader: R,
    writer: &mut W,
    config: &ProtocolConfig,
    stream: NativeStream<lenso_capability_game_session::GameSession>,
    session_deadline: Instant,
    session_cancellation: CancellationToken,
    module_cancellation: CancellationToken,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut client_send_open = true;
    let mut read = Box::pin(read_next_frame(reader, config.max_frame_bytes()));
    let mut receive = Box::pin(stream.receive());
    loop {
        let remaining = session_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            stream.cancel();
            let _ = send_runtime(writer, config, "deadline_exceeded").await;
            return;
        }
        let activity = futures::future::select(read, receive);
        let activity = tokio::select! {
            () = module_cancellation.cancelled() => {
                session_cancellation.cancel();
                let _ = send_runtime(writer, config, "cancelled").await;
                return;
            }
            activity = tokio::time::timeout(config.idle_timeout().min(remaining), activity) => activity,
        };
        let Ok(activity) = activity else {
            stream.cancel();
            let code = if Instant::now() >= session_deadline {
                "deadline_exceeded"
            } else {
                "idle_timeout"
            };
            let _ = send_runtime(writer, config, code).await;
            return;
        };
        match activity {
            futures::future::Either::Left(((reader, frame), pending_receive)) => {
                receive = pending_receive;
                let frame = match frame {
                    Ok(Some(frame)) => frame,
                    Ok(None) => {
                        stream.cancel();
                        return;
                    }
                    Err(error) => {
                        stream.cancel();
                        let _ = send_read_error(writer, config, ReadFrameError::Frame(error)).await;
                        return;
                    }
                };
                match frame {
                    ClientFrame::Message { action } if client_send_open && !action.is_empty() => {
                        if let Err(error) = stream.send(PlayResponse { action }).await {
                            let _ = send_runtime(writer, config, &runtime_error_code(&error)).await;
                            return;
                        }
                    }
                    ClientFrame::CloseSend if client_send_open => {
                        if let Err(error) = stream.close_send().await {
                            let _ = send_runtime(writer, config, &runtime_error_code(&error)).await;
                            return;
                        }
                        client_send_open = false;
                    }
                    ClientFrame::Cancel => {
                        stream.cancel();
                        let _ = send_runtime(writer, config, "cancelled").await;
                        return;
                    }
                    ClientFrame::Message { .. }
                    | ClientFrame::Hello { .. }
                    | ClientFrame::CloseSend => {
                        stream.cancel();
                        let _ = send_runtime(writer, config, "protocol_violation").await;
                        return;
                    }
                }
                read = Box::pin(read_next_frame(reader, config.max_frame_bytes()));
            }
            futures::future::Either::Right((event, pending_read)) => {
                read = pending_read;
                let event = match event {
                    Ok(event) => event,
                    Err(error) => {
                        let _ = send_runtime(writer, config, &runtime_error_code(&error)).await;
                        return;
                    }
                };
                match send_stream_event(writer, config, event).await {
                    Ok(StreamEventOutcome::Continue) => {}
                    Ok(StreamEventOutcome::Terminal) => return,
                    Err(_) => {
                        stream.cancel();
                        return;
                    }
                }
                receive = Box::pin(stream.receive());
            }
        }
    }
}

async fn read_next_frame<R>(
    mut reader: R,
    max_frame_bytes: usize,
) -> (R, Result<Option<ClientFrame>, FrameError>)
where
    R: AsyncRead + Unpin,
{
    let frame = read_client_frame(&mut reader, max_frame_bytes).await;
    (reader, frame)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamEventOutcome {
    Continue,
    Terminal,
}

async fn send_stream_event<W>(
    writer: &mut W,
    config: &ProtocolConfig,
    event: StreamEvent<PlayResponse, PlayError>,
) -> Result<StreamEventOutcome, SendFrameError>
where
    W: AsyncWrite + Unpin,
{
    match event {
        StreamEvent::Message(message) => send_frame(
            writer,
            config,
            &ServerFrame::Message {
                action: message.action,
            },
        )
        .await
        .map(|()| StreamEventOutcome::Continue),
        StreamEvent::PeerHalfClosed => send_frame(writer, config, &ServerFrame::PeerHalfClosed)
            .await
            .map(|()| StreamEventOutcome::Continue),
        StreamEvent::Terminal(Ok(())) => send_frame(
            writer,
            config,
            &ServerFrame::Terminal {
                outcome: TerminalFrame::Success,
            },
        )
        .await
        .map(|()| StreamEventOutcome::Terminal),
        StreamEvent::Terminal(Err(error)) => send_frame(
            writer,
            config,
            &ServerFrame::Terminal {
                outcome: TerminalFrame::Domain {
                    code: game_error_code(&error),
                },
            },
        )
        .await
        .map(|()| StreamEventOutcome::Terminal),
    }
}

#[derive(Debug)]
enum ReadFrameError {
    Frame(FrameError),
    Timeout(&'static str),
}

async fn read_frame_with_deadline<R>(
    reader: &mut R,
    max_frame_bytes: usize,
    idle_timeout: Duration,
    deadline: Instant,
) -> Result<Option<ClientFrame>, ReadFrameError>
where
    R: AsyncRead + Unpin,
{
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(ReadFrameError::Timeout("deadline_exceeded"));
    }
    match tokio::time::timeout(
        idle_timeout.min(remaining),
        read_client_frame(reader, max_frame_bytes),
    )
    .await
    {
        Ok(result) => result.map_err(ReadFrameError::Frame),
        Err(_) if Instant::now() >= deadline => Err(ReadFrameError::Timeout("deadline_exceeded")),
        Err(_) => Err(ReadFrameError::Timeout("idle_timeout")),
    }
}

#[derive(Debug)]
pub(crate) enum SendFrameError {
    Frame,
    Timeout,
}

pub(crate) async fn send_frame<W>(
    writer: &mut W,
    config: &ProtocolConfig,
    frame: &ServerFrame,
) -> Result<(), SendFrameError>
where
    W: AsyncWrite + Unpin,
{
    match tokio::time::timeout(
        config.idle_timeout(),
        write_server_frame(writer, frame, config.max_frame_bytes()),
    )
    .await
    {
        Ok(result) => result.map_err(|_| SendFrameError::Frame),
        Err(_) => Err(SendFrameError::Timeout),
    }
}

async fn send_read_error<W>(
    writer: &mut W,
    config: &ProtocolConfig,
    error: ReadFrameError,
) -> Result<(), SendFrameError>
where
    W: AsyncWrite + Unpin,
{
    match error {
        ReadFrameError::Timeout(code) => send_runtime(writer, config, code).await,
        ReadFrameError::Frame(FrameError::TooLarge) => {
            send_runtime(writer, config, "frame_too_large").await
        }
        ReadFrameError::Frame(FrameError::Malformed | FrameError::Truncated) => {
            send_runtime(writer, config, "protocol_violation").await
        }
        ReadFrameError::Frame(FrameError::Io(error)) => {
            let _ = error.kind();
            send_runtime(writer, config, "transport_error").await
        }
    }
}

async fn send_runtime<W>(
    writer: &mut W,
    config: &ProtocolConfig,
    code: &str,
) -> Result<(), SendFrameError>
where
    W: AsyncWrite + Unpin,
{
    send_frame(
        writer,
        config,
        &ServerFrame::Runtime {
            code: code.to_owned(),
        },
    )
    .await
}

async fn send_rejected<W>(
    writer: &mut W,
    config: &ProtocolConfig,
    code: impl Into<String>,
) -> Result<(), SendFrameError>
where
    W: AsyncWrite + Unpin,
{
    send_frame(writer, config, &ServerFrame::Rejected { code: code.into() }).await
}

fn auth_error_code(error: &AuthenticateError) -> String {
    match error {
        AuthenticateError::Expired => "expired".to_owned(),
        AuthenticateError::Invalid => "invalid".to_owned(),
        AuthenticateError::Revoked => "revoked".to_owned(),
        AuthenticateError::Unsupported => "unsupported".to_owned(),
        AuthenticateError::Unknown(error) => error.code.clone(),
    }
}

fn game_error_code(error: &PlayError) -> String {
    match error {
        PlayError::ActorRequired => "actor_required".to_owned(),
        PlayError::InvalidAction => "invalid_action".to_owned(),
        PlayError::NotAllowed => "not_allowed".to_owned(),
        PlayError::RoomClosed => "room_closed".to_owned(),
        PlayError::Unknown(error) => error.code.clone(),
    }
}

fn runtime_error_code(error: &RuntimeFailure) -> String {
    match error {
        RuntimeFailure::Unavailable { .. } => "unavailable".to_owned(),
        RuntimeFailure::UnknownOperation { .. } => "unknown_operation".to_owned(),
        RuntimeFailure::AmbiguousBinding { .. } => "ambiguous_binding".to_owned(),
        RuntimeFailure::ProtocolViolation { .. } => "protocol_violation".to_owned(),
        RuntimeFailure::MissingModuleFactory { .. } => "missing_module_factory".to_owned(),
        RuntimeFailure::UnavailableExecutionClass { .. } => {
            "unavailable_execution_class".to_owned()
        }
        RuntimeFailure::InvalidResolvedPlan { .. } => "invalid_resolved_plan".to_owned(),
        RuntimeFailure::AdmissionClosed => "admission_closed".to_owned(),
        RuntimeFailure::ResourceExhausted { .. } => "resource_exhausted".to_owned(),
        RuntimeFailure::DeadlineExceeded { .. } => "deadline_exceeded".to_owned(),
        RuntimeFailure::Cancelled { .. } => "cancelled".to_owned(),
        RuntimeFailure::Internal { .. } => "internal".to_owned(),
        RuntimeFailure::ModuleFailure { .. } => "module_failure".to_owned(),
        RuntimeFailure::ModuleRestartExhausted { .. } => "module_restart_exhausted".to_owned(),
    }
}
