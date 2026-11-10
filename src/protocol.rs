//! Wire types mirrored from `server/src/protocol.rs`.
//!
//! The server owns the contract; this module reproduces it exactly so the
//! client can speak it. Tags (`#[serde(tag = "type")]`) and casing
//! (`snake_case`) must stay in sync with the server's definitions.

use std::fmt;

use serde::{Deserialize, Serialize};

pub const SEAT_COUNT: usize = 4;
/// Partnerships at the table: seats 0 & 2 (North–South) versus seats 1 & 3
/// (West–East); teammates sit opposite each other.
pub const TEAM_COUNT: usize = 2;

/// The partnership a seat belongs to (even seats are team 0, odd seats 1).
pub const fn team_of(seat: usize) -> usize {
    seat % TEAM_COUNT
}

/// A partnership's readable name for the scoreboard.
pub const fn team_name(team: usize) -> &'static str {
    match team {
        0 => "Norte–Sur",
        _ => "Oeste–Este",
    }
}

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

    /// Whether both halves show the same pip. Doubles are the tiles that lie
    /// across the chain instead of in line with it.
    pub fn is_double(&self) -> bool {
        self.left == self.right
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

    /// The same tile in canonical order (`left <= right`), which is how hands
    /// are sent: board tiles are oriented, so comparing the two needs this.
    pub fn normalized(&self) -> PublicTile {
        PublicTile {
            left: self.left.min(self.right),
            right: self.left.max(self.right),
        }
    }
}

impl fmt::Display for PublicTile {
    /// A tile as the player reads it: `3|6`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}|{}", self.left, self.right)
    }
}

/// Inbound messages from a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientToServer {
    JoinRoom {
        room: String,
        name: String,
        /// Best-effort partnership preference (`0` or `1`). Absent or `null`
        /// means "no preference" — the seat lands wherever one is free, and
        /// `RoomState.seats[*].team` reports the actual side. A full team
        /// never refuses a join.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team: Option<u8>,
    },
    PlayTile {
        tile: PublicTile,
        side: Side,
    },
    PassTurn,
    /// "Play again": this seated player is ready for the next game. When a
    /// full table of four ready seats shares a finished (or absent) game,
    /// the room deals a fresh one in place — no socket cycling, so no seat
    /// can be held hostage by a stale connection.
    PlayAgain,
}

/// Outbound messages from the server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type")]
pub enum ServerToClient {
    RoomState {
        room: PublicRoomState,
    },
    HandDealt {
        seat: u8,
        hand: Vec<PublicTile>,
    },
    GameStateUpdate {
        state: PublicGameState,
    },
    /// The game closed: every seat passed in a row, so nobody can put a tile.
    /// Broadcast to all seats right after the terminating `GameStateUpdate`,
    /// with every player's remaining tiles revealed to everyone.
    GameClosed {
        hands: Vec<ClosedHand>,
    },
    MoveRejected {
        reason: String,
    },
    OpponentDisconnected {
        player: String,
        seat: u8,
    },
}

/// One seat's remaining tiles, made public when the game closes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClosedHand {
    pub seat: u8,
    pub hand: Vec<PublicTile>,
}

/// Lifecycle of a room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomPhase {
    Waiting,
    Playing,
    Finished,
}

/// A player's position around the square table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableSide {
    North,
    East,
    South,
    West,
}

/// One occupied seat. Vacant seats are simply absent from `seats`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicSeat {
    pub seat: u8,
    pub player_id: u64,
    pub name: String,
    /// Where this seat sits around the table (pinned to the seat, not to the
    /// occupant).
    pub table_side: TableSide,
    /// Partnership this seat belongs to: `seat % 2` (team 0 = seats 0 & 2,
    /// team 1 = seats 1 & 3).
    pub team: u8,
    /// Whether this player has tapped "Play again" since the last deal.
    pub ready: bool,
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

/// The final result of one hand. The hand is won by a **partnership**
/// (`Rules.md`), never by a lone seat: even when one player plays out, the
/// points are the pips the opposing team failed to shed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameResult {
    /// The seat that emptied its hand, if anybody did (`hand_empty` endings).
    pub out: Option<usize>,
    /// The winning partnership, or `None` for a draw or a voided game.
    pub winner_team: Option<u8>,
    /// Points awarded to `winner_team` for this hand: the pips left in the
    /// losing team's two hands.
    pub points: u8,
    /// Pips still held per absolute seat when the hand ended.
    pub hand_pips: [u8; SEAT_COUNT],
    pub reason: EndReason,
}

impl GameResult {
    /// Whether the game closed: it ended blocked, with no seat able to put a
    /// tile. A closed game reveals every remaining hand to every player.
    pub fn is_closed(self) -> bool {
        self.reason == EndReason::Blocked
    }
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
    /// While the board is empty: the only tile the opener may lay. Set on the
    /// first hand of a session (the 6|6 must start the layout); `None` on
    /// every later hand, which opens with any tile.
    pub opening_tile: Option<PublicTile>,
    /// How many passes were recorded in a row.
    pub consecutive_passes: u8,
    /// Present once the game has ended.
    pub result: Option<GameResult>,
    /// Running cumulative score for the current match of hands: the first
    /// partnership to `score.target` (100) points wins the match.
    pub score: PublicMatchScore,
}

/// The cumulative partnership score riding on every game snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicMatchScore {
    /// Points banked per partnership (`points[0]` is team 0, North–South).
    pub points: [u16; TEAM_COUNT],
    /// The target that wins the match (`Rules.md`: 100).
    pub target: u16,
    /// The match-winning partnership, present only once the match is over.
    pub winner_team: Option<u8>,
    /// Whether a partnership has reached `target`.
    pub over: bool,
}

impl Default for PublicMatchScore {
    /// A match that has not started: both partnerships on zero.
    fn default() -> Self {
        Self {
            points: [0, 0],
            target: 100,
            winner_team: None,
            over: false,
        }
    }
}

impl PublicGameState {
    pub fn is_over(&self) -> bool {
        self.result.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The join preference mirrors the server's `JoinRoom` examples: an
    /// explicit `0`/`1` requests a partnership, while an absent or `null`
    /// `team` means "no preference".
    #[test]
    fn join_room_parses_the_optional_team_preference() {
        let join: ClientToServer = serde_json::from_str(
            r#"{"type": "JoinRoom", "room": "table-42", "name": "Ada", "team": 1}"#,
        )
        .expect("valid JoinRoom with team");
        if let ClientToServer::JoinRoom { room, name, team } = join {
            assert_eq!(
                (room.as_str(), name.as_str(), team),
                ("table-42", "Ada", Some(1))
            );
        } else {
            panic!("expected a JoinRoom message");
        }

        let join: ClientToServer =
            serde_json::from_str(r#"{"type": "JoinRoom", "room": "table-42", "name": "Ada"}"#)
                .expect("valid JoinRoom without team");
        if let ClientToServer::JoinRoom { team, .. } = join {
            assert_eq!(team, None, "absent team means no preference");
        }

        let join: ClientToServer = serde_json::from_str(
            r#"{"type": "JoinRoom", "room": "table-42", "name": "Ada", "team": null}"#,
        )
        .expect("valid JoinRoom with null team");
        if let ClientToServer::JoinRoom { team, .. } = join {
            assert_eq!(team, None, "null team means no preference");
        }
    }

    /// Outbound: a join without a preference leaves `team` off the wire
    /// entirely (the server's `skip_serializing_if` example shape), while a
    /// picked partnership travels as `0` or `1`.
    #[test]
    fn join_room_serializes_the_team_only_when_picked() {
        let join = ClientToServer::JoinRoom {
            room: "table-42".into(),
            name: "Ada".into(),
            team: None,
        };
        let json = serde_json::to_string(&join).expect("serializable");
        assert_eq!(
            json,
            r#"{"type":"JoinRoom","room":"table-42","name":"Ada"}"#
        );

        for (team, tag) in [(Some(0), "0"), (Some(1), "1")] {
            let join = ClientToServer::JoinRoom {
                room: "table-42".into(),
                name: "Ada".into(),
                team,
            };
            let json = serde_json::to_string(&join).expect("serializable");
            assert_eq!(
                json,
                format!(r#"{{"type":"JoinRoom","room":"table-42","name":"Ada","team":{tag}}}"#)
            );
        }
    }

    /// A seat reports its place at the table *and* its partnership, so the
    /// waiting room shows each team straight from the wire.
    #[test]
    fn a_seat_carries_its_table_side_and_partnership() {
        let seat: PublicSeat = serde_json::from_str(
            r#"{"seat":3,"player_id":7,"name":"Bob","table_side":"east","team":1,"ready":true}"#,
        )
        .expect("a full seat row");
        assert_eq!(seat.table_side, TableSide::East);
        assert_eq!(seat.team, 1);
        assert!(seat.ready);

        let json = serde_json::to_string(&seat).expect("serializable");
        assert_eq!(
            json,
            r#"{"seat":3,"player_id":7,"name":"Bob","table_side":"east","team":1,"ready":true}"#
        );
    }

    /// The board chain draws doubles across the row, so the check must follow
    /// the pips and not the orientation the tile was played in.
    #[test]
    fn doubles_are_recognised_whichever_way_they_lie() {
        for pip in 0..=6 {
            assert!(PublicTile {
                left: pip,
                right: pip
            }
            .is_double());
        }
        assert!(!PublicTile { left: 5, right: 6 }.is_double());
        assert!(!PublicTile { left: 6, right: 5 }.is_double());
    }
}
