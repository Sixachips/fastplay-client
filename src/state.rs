//! Application state: a pure data reducer fed by socket events and UI
//! actions. The wire snapshots (`RoomState`, `GameStateUpdate`) are stored
//! verbatim — the server is the authority and every update is a full view,
//! so the client never has to merge partial state.
//!
//! One private slice is *not* carried by the snapshots: our own hand, which is
//! dealt once and only ever summarized as a tile count afterwards. [`sync_hand`]
//! retires the tiles we played from it, so the hand on screen stays what the
//! server actually holds for us.

use std::rc::Rc;

use gloo_storage::{LocalStorage, Storage};
use serde::{Deserialize, Serialize};
use yew::Reducible;

use crate::legality::same_tile;
use crate::protocol::{
    ClientToServer, ClosedHand, PublicGameState, PublicRoomState, PublicTile, ServerToClient,
};
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
    /// Every seat's remaining tiles, revealed when a game closes (nobody can
    /// put a tile). Kept until the next deal or leave so the board can show
    /// what each player was holding.
    pub reveal: Option<Vec<ClosedHand>>,

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
            state.reveal = None;
            state.socket = SocketState::Connecting;
        }

        Msg::LeaveRoom => {
            state.target = None;
            state.room = None;
            state.hand = Vec::new();
            state.game = None;
            state.reveal = None;
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
                    let you = state.room.as_ref().map(|room| room.you);
                    sync_hand(&mut state.hand, state.game.as_ref(), &game, you);
                    // A live snapshot (no result, or a result that is not a
                    // close) retires any earlier reveal; a closed game keeps
                    // its hands until the next deal.
                    if !game.result.is_some_and(|result| result.is_closed()) {
                        state.reveal = None;
                    }
                    state.game = Some(game)
                }
                ServerToClient::GameClosed { hands } => {
                    // Nobody can put a tile: keep every hand visible and say
                    // so out loud so the end of the game is not mistaken for a
                    // stall.
                    state.reveal = Some(hands);
                    state.set_toast(
                        ToastKind::Info,
                        "Game closed — nobody could play. All hands are shown.".to_owned(),
                    );
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

/// Keeps our private hand in step with the authoritative snapshots.
///
/// `HandDealt` arrives once per game and never again: after that the table
/// only publishes `hand_counts`. So every tile we play has to leave the hand
/// here, and it is retired on evidence rather than guessed — a tile is ours to
/// drop when the chain just grew by exactly that tile (chain tiles are stored
/// oriented, hand tiles canonical, hence [`same_tile`]).
///
/// Getting this wrong is not cosmetic: a phantom tile keeps `has_playable`
/// true, which keeps the **Pass** button disabled and strands the seat with no
/// move it can actually make.
fn sync_hand(
    hand: &mut Vec<PublicTile>,
    prev: Option<&PublicGameState>,
    next: &PublicGameState,
    you: Option<usize>,
) {
    // Our move: the seat we were told to play as, the game still live, and one
    // tile added to the chain. This also covers the winning tile (the game ends
    // in the very snapshot that lands it), which the count clamp alone would
    // have to guess at.
    if let (Some(seat), Some(prev)) = (you, prev) {
        if prev.turn == seat && prev.result.is_none() {
            if let Some(played) = tile_added(&prev.board, &next.board) {
                if let Some(pos) = hand.iter().position(|held| same_tile(*held, played)) {
                    hand.remove(pos);
                }
            }
        }
    }

    // Safety net for anything this pass could not reconstruct (a snapshot lost
    // to a reconnect, an opponent move we never saw): never hold more tiles
    // than the table says we do.
    let Some(expected) = you
        .and_then(|seat| next.hand_counts.get(seat))
        .copied()
        .map(usize::from)
    else {
        return;
    };
    while hand.len() > expected {
        // Prefer the tile the board is already showing (that is the one we
        // played); otherwise trim the oldest surplus.
        let surplus = hand
            .iter()
            .position(|held| next.board.iter().any(|placed| same_tile(*held, *placed)))
            .unwrap_or(0);
        hand.remove(surplus);
    }
}

/// The single tile a chain gained between two snapshots. The train only grows
/// at its two ends, so the newcomer is the extra head or the extra tail; any
/// other shape (a redeal, several missed updates) is not trusted.
fn tile_added(prev: &[PublicTile], next: &[PublicTile]) -> Option<PublicTile> {
    if next.len() != prev.len() + 1 {
        return None;
    }
    if &next[1..] == prev {
        Some(next[0])
    } else if &next[..next.len() - 1] == prev {
        Some(next[next.len() - 1])
    } else {
        None
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
        reveal: None,
        toast: None,
        toast_nonce: 0,
    }
}

/// Remember the form inputs once a join actually happens.
pub fn persist_inputs(name: &str, ws_url: &str) {
    let _ = LocalStorage::set(NAME_KEY, name);
    let _ = LocalStorage::set(WS_URL_KEY, ws_url);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(left: u8, right: u8) -> PublicTile {
        PublicTile { left, right }
    }

    /// A minimal seated app state for reducer tests. `initial()` touches
    /// browser storage, so host tests build the struct directly instead.
    fn seated() -> AppState {
        AppState {
            name: "Ada".into(),
            room_input: "a".into(),
            ws_url: "ws://h/ws".into(),
            target: None,
            socket: SocketState::Online,
            room: Some(PublicRoomState {
                room_id: "a".into(),
                you: 0,
                phase: crate::protocol::RoomPhase::Playing,
                seats: vec![],
            }),
            hand: Vec::new(),
            game: None,
            reveal: None,
            toast: None,
            toast_nonce: 0,
        }
    }

    fn closed_result() -> crate::protocol::GameResult {
        crate::protocol::GameResult {
            winner: Some(0),
            reason: crate::protocol::EndReason::Blocked,
        }
    }

    #[test]
    fn a_closed_game_reveals_every_hand_and_notifies() {
        let mut state = seated();
        let mut closing = view(&[], 0, [1, 1, 1, 1]);
        closing.result = Some(closed_result());
        reduce(
            &mut state,
            Msg::Socket(SocketEvent::Msg(ServerToClient::GameStateUpdate { state: closing })),
        );
        assert!(state.reveal.is_none(), "the reveal only lands with its own message");

        let hands = vec![
            ClosedHand { seat: 0, hand: vec![tile(0, 0)] },
            ClosedHand { seat: 1, hand: vec![tile(1, 1)] },
        ];
        reduce(
            &mut state,
            Msg::Socket(SocketEvent::Msg(ServerToClient::GameClosed { hands: hands.clone() })),
        );
        assert_eq!(state.reveal, Some(hands), "every hand is kept for the board");
        assert!(matches!(state.toast.as_ref().map(|t| t.kind), Some(ToastKind::Info)));
    }

    #[test]
    fn the_reveal_survives_a_stale_closed_snapshot_but_not_a_live_game() {
        let mut state = seated();
        let mut closing = view(&[], 0, [1, 1, 1, 1]);
        closing.result = Some(closed_result());
        reduce(
            &mut state,
            Msg::Socket(SocketEvent::Msg(ServerToClient::GameStateUpdate { state: closing.clone() })),
        );
        reduce(
            &mut state,
            Msg::Socket(SocketEvent::Msg(ServerToClient::GameClosed {
                hands: vec![ClosedHand { seat: 0, hand: vec![tile(0, 0)] }],
            })),
        );
        assert!(state.reveal.is_some());

        // The terminating snapshot is re-sent (a reconnect), still closed: the
        // hands stay revealed.
        reduce(
            &mut state,
            Msg::Socket(SocketEvent::Msg(ServerToClient::GameStateUpdate { state: closing })),
        );
        assert!(state.reveal.is_some(), "a closed snapshot does not retire its own reveal");

        // The next deal is live, so the reveal goes away.
        reduce(
            &mut state,
            Msg::Socket(SocketEvent::Msg(ServerToClient::GameStateUpdate {
                state: view(&[], 1, [7, 7, 7, 7]),
            })),
        );
        assert!(state.reveal.is_none(), "a fresh live game hides hands again");
    }

    /// A snapshot built the way the server builds it: the chain carries the
    /// ends, and `left`/`right` of the first/last tile are those ends.
    fn view(board: &[PublicTile], turn: usize, counts: [u8; 4]) -> PublicGameState {
        PublicGameState {
            turn,
            board: board.to_vec(),
            left_end: board.first().map(|t| t.left),
            right_end: board.last().map(|t| t.right),
            hand_counts: counts,
            opening_tile: None,
            consecutive_passes: 0,
            result: None,
        }
    }

    #[test]
    fn our_tile_leaves_the_hand_when_it_lands_on_the_right_end() {
        let prev = view(&[tile(3, 4)], 2, [7, 7, 3, 7]);
        // 4|6 appended right: the chain tile is oriented `4|6`, our hand tile
        // is canonical `4|6`.
        let next = view(&[tile(3, 4), tile(4, 6)], 3, [7, 7, 2, 7]);
        let mut hand = vec![tile(1, 6), tile(4, 6), tile(2, 2)];
        sync_hand(&mut hand, Some(&prev), &next, Some(2));
        assert_eq!(hand, vec![tile(1, 6), tile(2, 2)]);
    }

    #[test]
    fn our_tile_leaves_the_hand_when_it_lands_on_the_left_end() {
        let prev = view(&[tile(5, 3)], 1, [7, 2, 7, 7]);
        // Canonical 2|5 played left of a 5: it reads `2|5` on the chain.
        let next = view(&[tile(2, 5), tile(5, 3)], 2, [7, 1, 7, 7]);
        let mut hand = vec![tile(2, 5), tile(6, 6)];
        sync_hand(&mut hand, Some(&prev), &next, Some(1));
        assert_eq!(hand, vec![tile(6, 6)]);
    }

    #[test]
    fn an_opponent_move_never_touches_our_hand() {
        let prev = view(&[tile(1, 2)], 3, [6, 7, 7, 7]);
        // Seat 3 lands 2|6 — which we could never also hold, but a hand that
        // mentions those pips must still be left alone.
        let next = view(&[tile(1, 2), tile(2, 6)], 0, [6, 7, 7, 6]);
        let mut hand = vec![tile(2, 6), tile(3, 3)];
        sync_hand(&mut hand, Some(&prev), &next, Some(2));
        assert_eq!(hand, vec![tile(2, 6), tile(3, 3)]);
    }

    #[test]
    fn a_hand_too_long_for_the_table_is_trimmed_to_it() {
        // Two tiles arrived at once (a snapshot we never saw, e.g. across a
        // reconnect), so the exact match cannot fire and the count clamp must.
        let prev = view(&[tile(3, 4)], 2, [7, 7, 3, 7]);
        let next = view(&[tile(3, 4), tile(4, 6), tile(6, 6)], 0, [7, 7, 1, 7]);
        let mut hand = vec![tile(1, 6), tile(4, 6), tile(6, 6)];
        sync_hand(&mut hand, Some(&prev), &next, Some(2));
        assert_eq!(hand, vec![tile(1, 6)], "board-visible tiles are the ones dropped");
    }

    #[test]
    fn a_redeal_leaves_the_hand_to_hand_dealt() {
        let prev = view(&[tile(1, 2), tile(2, 3)], 0, [5, 7, 7, 7]);
        let next = view(&[], 1, [7, 7, 7, 7]);
        let mut hand = vec![tile(1, 2)];
        sync_hand(&mut hand, Some(&prev), &next, Some(0));
        assert_eq!(hand, vec![tile(1, 2)], "a short hand is not invented away");
    }

    #[test]
    fn tile_added_only_trusts_a_chain_grown_at_one_end() {
        let (a, b, c) = (tile(1, 2), tile(2, 3), tile(3, 4));
        assert_eq!(tile_added(&[a, b], &[a, b, c]), Some(c), "grew right");
        assert_eq!(tile_added(&[a, b], &[c, a, b]), Some(c), "grew left");
        assert_eq!(tile_added(&[a, b], &[a, c, b]), None, "never happens mid-chain");
        assert_eq!(tile_added(&[a], &[a, b, c]), None, "two tiles at once");
        assert_eq!(tile_added(&[a, b], &[a, b]), None, "a pass changes nothing");
    }
}
