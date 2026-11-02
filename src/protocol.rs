//! Wire types mirrored from `server/src/protocol.rs`.
//!
//! The server owns the contract; this module reproduces it exactly so the
//! client can speak it. Tags (`#[serde(tag = "type")]`) and casing
//! (`snake_case`) must stay in sync with the server's definitions.

use serde::{Deserialize, Serialize};

pub const SEAT_COUNT: usize = 4;


/// Which end of the linear board a tile is played on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
}

/// A domino pair as exposed on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicTile {
    pub left: u8,
    pub right: u8,
}

impl PublicTile {
    /// Whether the tile shows pip `end` on at least one half.
    pub fn matches(&self, end: u8) -> bool {
        self.left == end || self.right == end
    }

    /// The pip to expose outward when appending this tile to an end
    /// currently showing `end` (i.e. the half that does NOT touch the chain).
    pub fn outer_pip_at(&self, end: u8) -> u8 {
        if self.left == end {
            self.right
        } else {
            self.left
        }
    }
}

/// Inbound messages from a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientToServer {
    JoinRoom { room: String, name: String },
    PlayTile { tile: PublicTile, side: Side },
    PassTurn,
}

/// Outbound messages from the server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type")]
pub enum ServerToClient {
    RoomState { room: PublicRoomState },
    HandDealt { seat: u8, hand: Vec<PublicTile> },
    GameStateUpdate { state: PublicGameState },
    MoveRejected { reason: String },
    OpponentDisconnected { player: String, seat: u8 },
}

/// Lifecycle of a room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomPhase {
    Waiting,
    Playing,
    Finished,
}

/// One occupied seat. Vacant seats are simply absent from `seats`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicSeat {
    pub seat: u8,
    pub player_id: u64,
    pub name: String,
}

/// Personalized snapshot of a room (`you` = recipient's seat).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicRoomState {
    pub room_id: String,
    pub you: usize,
    pub phase: RoomPhase,
    pub seats: Vec<PublicSeat>,
}

/// How a game ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    /// A seat emptied its hand.
    HandEmpty,
    /// Every seat passed in a row; the game is blocked.
    Blocked,
    /// A player left and the game was voided.
    Abandoned,
}

/// The final result of a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameResult {
    /// Winning seat, or `None` for a draw.
    pub winner: Option<usize>,
    pub reason: EndReason,
}

/// Full public game view broadcast after every accepted move or pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicGameState {
    /// Seat whose turn it is.
    pub turn: usize,
    /// Played tiles, left to right, physically oriented.
    pub board: Vec<PublicTile>,
    /// Pip facing the outside of the board's left end.
    pub left_end: Option<u8>,
    /// Pip facing the outside of the board's right end.
    pub right_end: Option<u8>,
    /// Tile count per seat.
    pub hand_counts: [u8; SEAT_COUNT],
    /// How many passes were recorded in a row.
    pub consecutive_passes: u8,
    /// Present once the game has ended.
    pub result: Option<GameResult>,
}

impl PublicGameState {
    pub fn is_over(&self) -> bool {
        self.result.is_some()
    }
}
