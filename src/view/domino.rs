//! A domino drawn as two pip squares, pure CSS (3×3 dot grids).

use yew::prelude::*;

use crate::protocol::PublicTile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DominoSize {
    /// Slightly smaller than the board tiles, in the player's hand.
    Hand,
    /// A tile on the table.
    Board,
}

#[derive(Properties, PartialEq)]
pub struct Props {
    pub tile: PublicTile,
    #[prop_or(DominoSize::Board)]
    pub size: DominoSize,
    #[prop_or_default]
    pub classes: Classes,
}

/// Pip layout on a 3×3 grid, cell indices 1..=9 (row-major).
fn pattern(pips: u8) -> &'static [u8] {
    match pips {
        0 => &[],
        1 => &[5],
        2 => &[1, 9],
        3 => &[1, 5, 9],
        4 => &[1, 3, 7, 9],
        5 => &[1, 3, 5, 7, 9],
        6 => &[1, 3, 4, 6, 7, 9],
        _ => &[],
    }
}

fn half(pips: u8) -> Html {
    let dots = pattern(pips);
    html! {
        <div class="domino-half">
            { for (1..=9).map(|cell| html! {
                <span class={classes!("pip-cell", dots.contains(&cell).then_some("pip-on"))} />
            }) }
        </div>
    }
}

#[function_component(Domino)]
pub fn domino(props: &Props) -> Html {
    let size_class = match props.size {
        DominoSize::Hand => "domino-hand",
        DominoSize::Board => "domino-board",
    };
    html! {
        <div class={classes!("domino", size_class, props.classes.clone())}>
            { half(props.tile.left) }
            <div class="domino-sep" />
            { half(props.tile.right) }
        </div>
    }
}
