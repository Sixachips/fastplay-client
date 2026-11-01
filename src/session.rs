//! WebSocket session: owns one socket, re-joins the room on every (re)connect,
//! and forwards parsed server events to the app reducer.
//!
//! Reconnect model: the server frees a seat the moment its socket dies and
//! voids any running game, so "reconnecting" means "joining as a brand new
//! connection". On socket loss we wait with capped exponential backoff, then
//! open a fresh socket and send `JoinRoom` again. The server's personalized
//! `RoomState` reports which seat we ended up in, so the app never assumes
//! seat stability across sockets.

use futures::channel::mpsc;
use futures::prelude::*;
use gloo_net::websocket::futures::WebSocket;
use gloo_net::websocket::Message;
use gloo_timers::future::TimeoutFuture;
use wasm_bindgen_futures::spawn_local;
use yew::Callback;

use crate::protocol::{ClientToServer, ServerToClient};

/// Events the socket layer reports upward.
#[derive(Debug, Clone, PartialEq)]
pub enum SocketEvent {
    /// The link came up and `JoinRoom` was sent.
    Joined,
    /// The link dropped; a reconnect is already scheduled (unless the user
    /// intentionally left). Carries a human-readable cause for the UI.
    Lost(String),
    /// A well-formed server message arrived.
    Msg(ServerToClient),
}

/// Connection lifecycle as the UI sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketState {
    Offline,
    Connecting,
    Online,
    /// Link lost; the session is retrying with backoff.
    Reconnecting,
}

enum Command {
    Send(ClientToServer),
    /// Tear the loop down for good (the user left the room).
    Leave,
}

/// Why one connection attempt ended.
enum Outcome {
    /// The socket broke: reconnect after a backoff wait.
    Reconnect(String),
    /// The user left or the `Session` was dropped: stop for good.
    Stop,
}

/// Handle to a running connection loop. Dropping it (or calling `leave`)
/// stops reconnection attempts.
pub struct Session {
    tx: mpsc::UnboundedSender<Command>,
}

impl Session {
    /// Starts the loop and returns immediately; the socket runs in a task.
    pub fn start(
        url: String,
        room: String,
        name: String,
        on_event: Callback<SocketEvent>,
    ) -> Session {
        let (tx, rx) = mpsc::unbounded();
        spawn_local(run(url, room, name, rx, on_event));
        Session { tx }
    }

    /// Queues one command. Commands issued while the link is down either
    /// trigger an immediate retry or are dropped: game moves need a live
    /// seat anyway, and the full snapshots after any reconnect re-sync the
    /// app without them.
    pub fn send(&mut self, cmd: ClientToServer) {
        let _ = self.tx.unbounded_send(Command::Send(cmd));
    }

    /// Closes the socket and stops reconnecting.
    pub fn leave(&mut self) {
        let _ = self.tx.unbounded_send(Command::Leave);
    }
}

async fn run(
    url: String,
    room: String,
    name: String,
    commands: mpsc::UnboundedReceiver<Command>,
    on_event: Callback<SocketEvent>,
) {
    // `fuse` gives us a stream whose `next()` is a `FusedFuture`, which
    // `select_biased!` requires.
    let mut commands = commands.fuse();
    let mut backoff_ms = 500u32;

    loop {
        let opened_at = js_sys::Date::now();

        // gloo-net's open is synchronous: the handshake completes when the
        // first frames flow.
        let outcome: Outcome = match WebSocket::open(&url) {
            Err(error) => Outcome::Reconnect(error.to_string()),
            Ok(socket) => {
                let (mut sink, stream) = socket.split();
                let mut stream = stream.fuse();

                // Every fresh connection starts by claiming its room again.
                let join = ClientToServer::JoinRoom {
                    room: room.clone(),
                    name: name.clone(),
                };
                let text = serde_json::to_string(&join).expect("join is serializable");
                if sink.send(Message::Text(text)).await.is_err() {
                    Outcome::Reconnect("link dropped while joining".to_string())
                } else {
                    on_event.emit(SocketEvent::Joined);

                    // Run this socket until it breaks or the user leaves.
                    loop {
                        futures::select_biased! {
                            command = commands.next() => {
                                match command {
                                    Some(Command::Send(msg)) => {
                                        let text = serde_json::to_string(&msg)
                                            .expect("client messages are serializable");
                                        if sink.send(Message::Text(text)).await.is_err() {
                                            break Outcome::Reconnect("send failed".to_string());
                                        }
                                    }
                                    None => {} // No command yet, continue waiting
                                    // Explicit Leave, or every `Session`
                                    // handle was dropped: stop for good.
                                    _ => break Outcome::Stop,
                                }
                            }
                            frame = stream.next() => {
                                match frame {
                                    Some(Ok(Message::Text(text))) => {
                                        match serde_json::from_str::<ServerToClient>(&text) {
                                            Ok(msg) => on_event.emit(SocketEvent::Msg(msg)),
                                            Err(error) => {
                                                gloo_console::warn!(format!(
                                                    "unparsable server message: {error}: {text}"
                                                ));
                                            }
                                        }
                                    }
                                    // The server only sends text frames.
                                    Some(Ok(_)) => {}
                                    Some(Err(error)) => {
                                        break Outcome::Reconnect(format!("socket error: {error}"));
                                    }
                                    None => {
                                        break Outcome::Reconnect(
                                            "connection closed by server".to_string(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        };

        match outcome {
            Outcome::Stop => return,
            Outcome::Reconnect(reason) => on_event.emit(SocketEvent::Lost(reason)),
        }

        // A socket that survived a while means the endpoint is healthy:
        // restart the schedule. Fast failures keep backing off.
        if js_sys::Date::now() - opened_at > 10_000.0 {
            backoff_ms = 500;
        }

        // Wait out the backoff, but wake early: a Leave (or the Session being
        // dropped) ends us, and any queued command means retry immediately.
        let mut retry_now = false;
        futures::select_biased! {
            command = commands.next() => {
                match command {
                    Some(Command::Send(_)) => retry_now = true,
                    _ => return,
                }
            }
            _ = TimeoutFuture::new(backoff_ms).fuse() => {}
        }
        if !retry_now {
            backoff_ms = (backoff_ms * 2).min(8_000);
        }
    }
}
