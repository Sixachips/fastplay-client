//! Application state: a pure data reducer fed by socket events and UI
//! actions. The wire snapshots (`RoomState`, `GameStateUpdate`) are stored
//! verbatim — the server is the authority and every update is a full view,
//! so the client never has to merge partial state.

use std::rc::Rc;

use gloo_storage::{LocalStorage, Storage};
use serde::{Deserialize, Serialize};
use yew::Reducible;

use crate::protocol::{ClientToServer, PublicGameState, PublicRoomState, PublicTile, ServerToClient};
use crate::session::{SocketEvent, SocketState};

pub const MAX_ROOM_LEN: usize = 16; // server: MAX_ROOM_CODE_LEN
pub const MAX_NAME_LEN: usize = 32; // server: MAX_PLAYER_NAME_LEN

/// Which top-level view to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Connecting,
    Room,
    Game,
}

/// A transient notification shown at the bottom of the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub nonce: u32,
    pub kind: ToastKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Warn,
    Info,
}

/// The room + name the current (or intended) connection is seated in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinTarget {
    pub room: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    // ---- home form ----
    pub name: String,
    pub room_input: String,
    pub ws_url: String,

    // ---- connection ----
    pub target: Option<JoinTarget>,
    pub socket: SocketState,

    // ---- wire snapshots ----
    pub room: Option<PublicRoomState>,
    pub hand: Vec<PublicTile>,
    pub game: Option<PublicGameState>,

    // ---- transient UI ----
    pub toast: Option<Toast>,
    toast_nonce: u32,
}

/// App-level messages.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    NameChanged(String),
    RoomChanged(String),
    WsUrlChanged(String),
    JoinRoom { room: String, name: String },
    LeaveRoom,
    Socket(SocketEvent),
    ShowToast { kind: ToastKind, text: String },
    DismissToast,
}

impl AppState {
    pub fn screen(&self) -> Screen {
        if self.target.is_none() {
            Screen::Home
        } else if self.room.is_none() {
            Screen::Connecting
        } else if self.game.is_none() {
            Screen::Room
        } else {
            Screen::Game
        }
    }

    /// Seat occupant lookup by seat index.
    pub fn name_at(&self, seat: usize) -> Option<&str> {
        self.room
            .as_ref()
            .and_then(|room| room.seats.iter().find(|s| s.seat as usize == seat))
            .map(|seat| seat.name.as_str())
    }

    fn set_toast(&mut self, kind: ToastKind, text: String) {
        self.toast_nonce += 1;
        self.toast = Some(Toast { nonce: self.toast_nonce, kind, text });
    }

    /// Client-side validation mirroring the server's wire constraints, so
    /// users get instant feedback instead of a network round-trip.
    pub fn join_request(&self) -> Option<ClientToServer> {
        let name = self.name.trim();
        let room = self.room_input.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME_LEN {
            return None;
        }
        if room.is_empty() || room.chars().count() > MAX_ROOM_LEN {
            return None;
        }
        Some(ClientToServer::JoinRoom {
            room: room.to_owned(),
            name: name.to_owned(),
        })
    }

    /// Check if we should show the connecting screen based on current state.
    /// Returns true if the UI should indicate connection is in progress.
    pub fn is_connecting(&self) -> bool {
        self.target.is_some() && self.room.is_none()
    }
}

impl Reducible for AppState {
    type Action = Msg;

    fn reduce(self: Rc<Self>, action: Self::Action) -> Rc<Self> {
        let mut next = (*self).clone();
        reduce(&mut next, action);
        Rc::new(next)
    }
}

fn reduce(state: &mut AppState, msg: Msg) {
    match msg {
        Msg::NameChanged(name) => state.name = name,
        Msg::RoomChanged(room) => state.room_input = room,
        Msg::WsUrlChanged(url) => state.ws_url = url,

        Msg::JoinRoom { room, name } => {
            // A fresh socket claims a fresh seat: drop all game data; the
            // server re-sends everything it thinks we should see.
            state.target = Some(JoinTarget { room, name });
            state.room = None;
            state.hand = Vec::new();
            state.game = None;
            state.socket = SocketState::Connecting;
        }

        Msg::LeaveRoom => {
            state.target = None;
            state.room = None;
            state.hand = Vec::new();
            state.game = None;
            state.socket = SocketState::Offline;
            state.toast = None;
        }

        Msg::Socket(event) => match event {
            SocketEvent::Joined => state.socket = SocketState::Online,
            SocketEvent::Lost(reason) => {
                state.socket = SocketState::Reconnecting;
                state.set_toast(ToastKind::Warn, format!("Connection lost: {reason}"));
            }
            SocketEvent::Msg(server) => match server {
                ServerToClient::RoomState { room } => state.room = Some(room),
                ServerToClient::HandDealt { seat, hand } => {
                    // Hands are private; ignore one addressed to a different
                    // seat (possible if our seat index shifted after a
                    // reconnect).
                    if state.room.as_ref().is_some_and(|room| room.you == seat as usize) {
                        state.hand = hand;
                    }
                }
                ServerToClient::GameStateUpdate { state: game } => {
                    state.game = Some(game)
                }
                ServerToClient::MoveRejected { reason } => {
                    state.set_toast(ToastKind::Warn, reason)
                }
                ServerToClient::OpponentDisconnected { player, seat } => {
                    state.set_toast(
                        ToastKind::Warn,
                        format!("{player} (seat {}) lost connection", seat + 1),
                    );
                }
            },
        },

        Msg::ShowToast { kind, text } => state.set_toast(kind, text),
        Msg::DismissToast => state.toast = None,
    }
}

const NAME_KEY: &str = "fastplay.name";
const WS_URL_KEY: &str = "fastplay.ws_url";

/// Build the initial state from remembered inputs plus the share link, so
/// `https://host/?room=CODE` pre-fills the table and returning players only
/// have to tap.
pub fn initial() -> AppState {
    AppState {
        name: LocalStorage::get::<String>(NAME_KEY).unwrap_or_default(),
        room_input: crate::roomcode::room_from_url().unwrap_or_default(),
        ws_url: LocalStorage::get::<String>(WS_URL_KEY)
            .unwrap_or_else(|_| crate::roomcode::guess_ws_url()),
        target: None,
        socket: SocketState::Offline,
        room: None,
        hand: Vec::new(),
        game: None,
        toast: None,
        toast_nonce: 0,
    }
}

/// Remember the form inputs once a join actually happens.
pub fn persist_inputs(name: &str, ws_url: &str) {
    let _ = LocalStorage::set(NAME_KEY, name);
    let _ = LocalStorage::set(WS_URL_KEY, ws_url);
}
