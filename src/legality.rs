//! Client-side legality hints.
//!
//! The server is always the authority; this module only predicts which of
//! OUR tiles can play so the UI can highlight them and know when to offer
//! Pass. A wrong guess is harmless: it only leads to a `MoveRejected` toast
//! (or a disabled button), never to invalid state.

use crate::protocol::{PublicGameState, PublicTile};

/// Which ends of the board a tile could attach to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fit {
    pub left: bool,
    pub right: bool,
}

impl Fit {
    pub fn any(&self) -> bool {
        self.left || self.right
    }
}

/// Whether two tiles are the same physical domino, ignoring orientation.
///
/// Hand tiles arrive canonical (`left <= right`) while board tiles carry the
/// orientation they were played in, so `3|6` and `6|3` are one and the same —
/// which is what lets hand bookkeeping be matched against the chain.
pub fn same_tile(a: PublicTile, b: PublicTile) -> bool {
    a.normalized() == b.normalized()
}

/// The pips a tile would have to match, worded for a "that fits nowhere"
/// message: one number when both ends show the same pip.
pub fn open_ends(game: &PublicGameState) -> String {
    match (game.left_end, game.right_end) {
        (Some(left), Some(right)) if left == right => left.to_string(),
        (Some(left), Some(right)) => format!("{left} or {right}"),
        _ => "no tile yet".to_owned(),
    }
}

/// Fit of `tile` against the current board ends. On an empty board any tile
/// opens, so both sides are reported (the server ignores the side anyway).
pub fn fit(tile: PublicTile, game: &PublicGameState) -> Fit {
    if game.board.is_empty() {
        return Fit { left: true, right: true };
    }
    Fit {
        left: game.left_end.is_some_and(|end| tile.matches(end)),
        right: game.right_end.is_some_and(|end| tile.matches(end)),
    }
}

/// Whether the hand has at least one legal move (i.e. must not pass).
pub fn has_playable(hand: &[PublicTile], game: &PublicGameState) -> bool {
    hand.iter().any(|tile| fit(*tile, game).any())
}
