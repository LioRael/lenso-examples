use std::time::Duration;

use lenso_auth_sdk::ActorAssertionIssuer;
use lenso_kernel::{Kernel, NativeApp, ShutdownOutcome};
use lenso_native_adapter::NativeModuleRegistry;
use lenso_runner::TokioDriver;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task::LocalSet,
    time::{sleep, timeout},
};
use vnext_game_session::{
    AuthModuleFactory, ClientFrame, GameProtocolFactory, GameProviderFactory, ProtocolConfig,
    ProtocolVariant, ServerFrame, SessionMode, TerminalFrame, resolved_plan_with_variants,
};

const TEST_KEY: &[u8] = b"fixture-game-session-key";

fn config() -> ProtocolConfig {
    ProtocolConfig {
        bind: "127.0.0.1:0".to_owned(),
        max_frame_bytes: 1_024,
        max_connections: 4,
        idle_timeout_ms: 500,
        session_timeout_ms: 2_000,
    }
}

async fn start_app(mode: SessionMode, config: ProtocolConfig) -> (NativeApp, GameProtocolFactory) {
    start_app_with_variants(ProtocolVariant::Primary, mode, config).await
}

async fn start_app_with_variants(
    protocol_variant: ProtocolVariant,
    mode: SessionMode,
    config: ProtocolConfig,
) -> (NativeApp, GameProtocolFactory) {
    let issuer = ActorAssertionIssuer::new("fixture.auth", TEST_KEY);
    let protocol = GameProtocolFactory::with_variant(protocol_variant);
    let registry = NativeModuleRegistry::new()
        .with_factory(AuthModuleFactory::new(issuer.clone()))
        .with_factory(GameProviderFactory::new(issuer.verifier(), mode))
        .with_factory(protocol.clone());
    let plan = resolved_plan_with_variants(&config, protocol_variant, mode)
        .expect("fixture composition should resolve");
    let app = Kernel::start_native(plan, TokioDriver::new(), registry)
        .await
        .expect("fixture app should start");
    assert!(app.is_ready());
    (app, protocol)
}

async fn connect(protocol: &GameProtocolFactory) -> TcpStream {
    TcpStream::connect(protocol.local_addr().expect("protocol should bind"))
        .await
        .expect("client should connect")
}

async fn write_frame(stream: &mut TcpStream, frame: &ClientFrame) {
    let payload = serde_json::to_vec(frame).expect("client frame should serialize");
    let length = u32::try_from(payload.len()).expect("test frame should fit u32");
    stream
        .write_all(&length.to_be_bytes())
        .await
        .expect("frame length should write");
    stream
        .write_all(&payload)
        .await
        .expect("frame payload should write");
}

async fn write_raw_frame(stream: &mut TcpStream, payload: &[u8]) {
    let length = u32::try_from(payload.len()).expect("test frame should fit u32");
    stream
        .write_all(&length.to_be_bytes())
        .await
        .expect("raw frame length should write");
    stream
        .write_all(payload)
        .await
        .expect("raw frame payload should write");
}

async fn next_frame(stream: &mut TcpStream) -> std::io::Result<ServerFrame> {
    let mut header = [0_u8; 4];
    stream.read_exact(&mut header).await?;
    let length = usize::try_from(u32::from_be_bytes(header)).expect("u32 fits usize");
    let mut payload = vec![0_u8; length];
    stream.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

async fn establish(stream: &mut TcpStream, token: Option<&str>, room: &str) {
    write_frame(
        stream,
        &ClientFrame::Hello {
            token: token.map(ToOwned::to_owned),
            room: room.to_owned(),
            deadline_ms: None,
        },
    )
    .await;
    assert_eq!(
        next_frame(stream).await.expect("ready frame"),
        ServerFrame::Ready {
            room: room.to_owned()
        }
    );
    assert_eq!(
        next_frame(stream).await.expect("welcome frame"),
        ServerFrame::Message {
            action: format!("welcome:{room}:player-123")
        }
    );
}

async fn clean_shutdown(app: &NativeApp) {
    assert!(matches!(
        app.shutdown(Duration::from_secs(1)).await,
        ShutdownOutcome::Clean
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn authenticated_full_duplex_session_has_clean_terminal_teardown() {
    LocalSet::new()
        .run_until(async {
            let (app, protocol) = start_app(SessionMode::Echo, config()).await;
            let mut stream = connect(&protocol).await;
            establish(&mut stream, Some("good-token"), "arena").await;

            write_frame(
                &mut stream,
                &ClientFrame::Message {
                    action: "emit-two".to_owned(),
                },
            )
            .await;
            assert_eq!(
                next_frame(&mut stream).await.expect("ack frame"),
                ServerFrame::Message {
                    action: "ack:emit-two".to_owned()
                }
            );
            assert_eq!(
                timeout(Duration::from_millis(200), next_frame(&mut stream))
                    .await
                    .expect("provider push should not wait for another client frame")
                    .expect("provider push frame"),
                ServerFrame::Message {
                    action: "push:emit-two".to_owned()
                }
            );

            write_frame(&mut stream, &ClientFrame::CloseSend).await;
            assert_eq!(
                next_frame(&mut stream).await.expect("peer half-close"),
                ServerFrame::PeerHalfClosed
            );
            assert_eq!(
                next_frame(&mut stream).await.expect("terminal frame"),
                ServerFrame::Terminal {
                    outcome: TerminalFrame::Success
                }
            );
            clean_shutdown(&app).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn auth_and_game_provider_rejections_are_bounded() {
    LocalSet::new()
        .run_until(async {
            let (app, protocol) = start_app(SessionMode::Echo, config()).await;
            for (token, room, expected) in [
                (None, "arena", "credential_required"),
                (Some("bad-token"), "arena", "invalid"),
                (Some("expired-token"), "arena", "expired"),
                (Some("forbidden-token"), "arena", "not_allowed"),
                (Some("good-token"), "closed", "room_closed"),
            ] {
                let mut stream = connect(&protocol).await;
                write_frame(
                    &mut stream,
                    &ClientFrame::Hello {
                        token: token.map(str::to_owned),
                        room: room.to_owned(),
                        deadline_ms: None,
                    },
                )
                .await;
                assert_eq!(
                    next_frame(&mut stream).await.expect("rejection frame"),
                    ServerFrame::Rejected {
                        code: expected.to_owned()
                    }
                );
            }

            let mut cancelled = connect(&protocol).await;
            establish(&mut cancelled, Some("good-token"), "arena").await;
            write_frame(&mut cancelled, &ClientFrame::Cancel).await;
            assert_eq!(
                next_frame(&mut cancelled).await.expect("cancel outcome"),
                ServerFrame::Runtime {
                    code: "cancelled".to_owned()
                }
            );
            clean_shutdown(&app).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn provider_terminal_domain_errors_cross_the_public_stream_seam() {
    LocalSet::new()
        .run_until(async {
            let (app, protocol) = start_app(SessionMode::Echo, config()).await;
            let mut stream = connect(&protocol).await;
            establish(&mut stream, Some("good-token"), "arena").await;
            write_frame(
                &mut stream,
                &ClientFrame::Message {
                    action: "quit".to_owned(),
                },
            )
            .await;
            assert_eq!(
                next_frame(&mut stream)
                    .await
                    .expect("terminal domain frame"),
                ServerFrame::Terminal {
                    outcome: TerminalFrame::Domain {
                        code: "room_closed".to_owned()
                    }
                }
            );
            clean_shutdown(&app).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn malformed_oversized_and_idle_frames_have_explicit_runtime_outcomes() {
    LocalSet::new()
        .run_until(async {
            let mut small_config = config();
            small_config.max_frame_bytes = 256;
            small_config.idle_timeout_ms = 40;
            small_config.session_timeout_ms = 250;
            let (app, protocol) = start_app(SessionMode::Echo, small_config).await;

            let mut malformed = connect(&protocol).await;
            write_raw_frame(&mut malformed, br#"{"kind":"hello"#).await;
            assert_eq!(
                next_frame(&mut malformed).await.expect("malformed outcome"),
                ServerFrame::Runtime {
                    code: "protocol_violation".to_owned()
                }
            );

            let mut oversized = connect(&protocol).await;
            write_raw_frame(&mut oversized, &vec![b'x'; 300]).await;
            assert_eq!(
                next_frame(&mut oversized).await.expect("size outcome"),
                ServerFrame::Runtime {
                    code: "frame_too_large".to_owned()
                }
            );

            let mut idle = connect(&protocol).await;
            establish(&mut idle, Some("good-token"), "arena").await;
            assert_eq!(
                timeout(Duration::from_millis(200), next_frame(&mut idle))
                    .await
                    .expect("idle outcome should arrive")
                    .expect("idle frame"),
                ServerFrame::Runtime {
                    code: "idle_timeout".to_owned()
                }
            );

            let mut deadline = connect(&protocol).await;
            write_frame(
                &mut deadline,
                &ClientFrame::Hello {
                    token: Some("good-token".to_owned()),
                    room: "arena".to_owned(),
                    deadline_ms: Some(20),
                },
            )
            .await;
            assert!(matches!(
                next_frame(&mut deadline).await.expect("deadline ready"),
                ServerFrame::Ready { .. }
            ));
            assert!(matches!(
                next_frame(&mut deadline).await.expect("deadline welcome"),
                ServerFrame::Message { .. }
            ));
            assert_eq!(
                timeout(Duration::from_millis(200), next_frame(&mut deadline))
                    .await
                    .expect("deadline outcome should arrive")
                    .expect("deadline frame"),
                ServerFrame::Runtime {
                    code: "deadline_exceeded".to_owned()
                }
            );
            clean_shutdown(&app).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn active_connection_admission_is_bounded() {
    LocalSet::new()
        .run_until(async {
            let mut limited = config();
            limited.max_connections = 1;
            let (app, protocol) = start_app(SessionMode::Echo, limited).await;
            let mut first = connect(&protocol).await;
            establish(&mut first, Some("good-token"), "arena").await;

            let mut second = connect(&protocol).await;
            assert_eq!(
                next_frame(&mut second).await.expect("capacity outcome"),
                ServerFrame::Runtime {
                    code: "resource_exhausted".to_owned()
                }
            );
            clean_shutdown(&app).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn client_disconnect_cancels_the_session_and_releases_admission() {
    LocalSet::new()
        .run_until(async {
            let mut limited = config();
            limited.max_connections = 1;
            let (app, protocol) = start_app(SessionMode::Echo, limited).await;
            let mut first = connect(&protocol).await;
            establish(&mut first, Some("good-token"), "arena").await;
            drop(first);

            timeout(Duration::from_secs(1), async {
                loop {
                    let mut candidate = connect(&protocol).await;
                    write_frame(
                        &mut candidate,
                        &ClientFrame::Hello {
                            token: Some("good-token".to_owned()),
                            room: "arena".to_owned(),
                            deadline_ms: None,
                        },
                    )
                    .await;
                    match next_frame(&mut candidate)
                        .await
                        .expect("bounded disconnect outcome")
                    {
                        ServerFrame::Ready { .. } => {
                            assert!(matches!(
                                next_frame(&mut candidate)
                                    .await
                                    .expect("welcome after disconnect"),
                                ServerFrame::Message { .. }
                            ));
                            break;
                        }
                        ServerFrame::Runtime { code } if code == "resource_exhausted" => {
                            sleep(Duration::from_millis(10)).await;
                        }
                        frame => panic!("unexpected frame after disconnect: {frame:?}"),
                    }
                }
            })
            .await
            .expect("disconnect should release its connection slot");
            clean_shutdown(&app).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn provider_failure_restarts_and_composition_selects_a_replacement() {
    LocalSet::new()
        .run_until(async {
            let (app, protocol) = start_app(SessionMode::Echo, config()).await;
            let mut crashing = connect(&protocol).await;
            establish(&mut crashing, Some("good-token"), "arena").await;
            write_frame(
                &mut crashing,
                &ClientFrame::Message {
                    action: "crash".to_owned(),
                },
            )
            .await;
            assert_eq!(
                next_frame(&mut crashing).await.expect("failure outcome"),
                ServerFrame::Runtime {
                    code: "module_failure".to_owned()
                }
            );
            timeout(Duration::from_secs(1), async {
                loop {
                    if app.module_generation("game") == Some(2) {
                        break;
                    }
                    sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("game provider should restart");
            clean_shutdown(&app).await;

            let (replacement_app, replacement_protocol) =
                start_app(SessionMode::Replacement, config()).await;
            let mut replacement = connect(&replacement_protocol).await;
            write_frame(
                &mut replacement,
                &ClientFrame::Hello {
                    token: Some("good-token".to_owned()),
                    room: "arena".to_owned(),
                    deadline_ms: None,
                },
            )
            .await;
            assert_eq!(
                next_frame(&mut replacement)
                    .await
                    .expect("replacement ready"),
                ServerFrame::Ready {
                    room: "arena".to_owned()
                }
            );
            assert_eq!(
                next_frame(&mut replacement)
                    .await
                    .expect("replacement welcome"),
                ServerFrame::Message {
                    action: "replacement-welcome:arena:player-123".to_owned()
                }
            );
            write_frame(
                &mut replacement,
                &ClientFrame::Message {
                    action: "move:right".to_owned(),
                },
            )
            .await;
            assert_eq!(
                next_frame(&mut replacement).await.expect("replacement ack"),
                ServerFrame::Message {
                    action: "replacement-ack:move:right".to_owned()
                }
            );
            clean_shutdown(&replacement_app).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn composition_selects_a_replacement_protocol_package() {
    LocalSet::new()
        .run_until(async {
            let (app, protocol) =
                start_app_with_variants(ProtocolVariant::Replacement, SessionMode::Echo, config())
                    .await;
            let mut stream = connect(&protocol).await;
            establish(&mut stream, Some("good-token"), "arena").await;
            write_frame(&mut stream, &ClientFrame::CloseSend).await;
            assert!(matches!(
                next_frame(&mut stream)
                    .await
                    .expect("replacement half-close"),
                ServerFrame::PeerHalfClosed
            ));
            assert!(matches!(
                next_frame(&mut stream).await.expect("replacement terminal"),
                ServerFrame::Terminal { .. }
            ));
            clean_shutdown(&app).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn managed_shutdown_closes_an_active_protocol_connection() {
    LocalSet::new()
        .run_until(async {
            let (app, protocol) = start_app(SessionMode::Echo, config()).await;
            let mut stream = connect(&protocol).await;
            establish(&mut stream, Some("good-token"), "arena").await;
            clean_shutdown(&app).await;
            let outcome = timeout(Duration::from_secs(1), next_frame(&mut stream))
                .await
                .expect("shutdown should close the socket")
                .expect("shutdown should produce a bounded outcome");
            assert!(matches!(
                outcome,
                ServerFrame::Runtime { code }
                    if code == "cancelled" || code == "unavailable"
            ));
        })
        .await;
}
