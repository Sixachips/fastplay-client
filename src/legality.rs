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
