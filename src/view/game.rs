//! The game screen: opponents strip, board chain, your hand, and the
//! play / pass / play-again controls.
//!
//! Interaction model: tap a playable tile. If it fits exactly one end it
//! plays there immediately; if it fits both, an end picker appears; on an
//! empty board the tile plays directly (the server ignores the side).

use wasm_bindgen::JsCast;
use yew::prelude::*;

use crate::legality::{fit, has_playable};
use crate::protocol::{
    ClientToServer, EndReason, GameResult, PublicRoomState, PublicTile, Side,
    SEAT_COUNT,
};
use crate::session::SocketState;
use crate::view::domino::{Domino, DominoSize};
use crate::view::TopBar;
use crate::Api;

#[derive(Properties, PartialEq)]
pub struct Props {
    pub api: Api,
}

#[function_component(GameView)]
pub fn game_view(props: &Props) -> Html {
    let api = props.api.clone();
    let Some(room) = api.state.room.clone() else {
        return Html::default();
    };
    let Some(game) = api.state.game.clone() else {
        return Html::default();
    };
    let hand = api.state.hand.clone();
    let selected = use_state(|| Option::<PublicTile>::None);

    let you = room.you;
    let over = game.result.is_some();
    let my_turn = !over && game.turn == you;
    let must_pass = my_turn && !game.board.is_empty() && !has_playable(&hand, &game);

    // Any new snapshot invalidates a pending end pick.
    {
        let selected = selected.clone();
        let key = (game.turn, game.board.len(), game.result);
        use_effect_with(key, move |_| {
            selected.set(None);
        });
    }

    let on_tile = {
        let api = api.clone();
        let game = game.clone();
        let selected = selected.clone();
        Callback::from(move |tile: PublicTile| {
            if !my_turn {
                return;
            }
            let f = fit(tile, &game);
            if game.board.is_empty() {
                api.send_cmd(ClientToServer::PlayTile { tile, side: Side::Right });
            } else {
                match (f.left, f.right) {
                    (true, true) => {
                        // Toggle: tapping the already-picked tile cancels.
                        selected.set(if *selected == Some(tile) { None } else { Some(tile) });
                    }
                    (true, false) => {
                        api.send_cmd(ClientToServer::PlayTile { tile, side: Side::Left });
                    }
                    (false, true) => {
                        api.send_cmd(ClientToServer::PlayTile { tile, side: Side::Right });
                    }
                    (false, false) => {}
                }
            }
        })
    };

    let pick_end = {
        let api = api.clone();
        let selected = selected.clone();
        Callback::from(move |side: Side| {
            if let Some(tile) = *selected {
                api.send_cmd(ClientToServer::PlayTile { tile, side });
                selected.set(None);
            }
        })
    };
    let cancel_pick = {
        let selected = selected.clone();
        Callback::from(move |_: MouseEvent| selected.set(None))
    };

    let on_pass = {
        let api = api.clone();
        Callback::from(move |_: MouseEvent| api.send_cmd(ClientToServer::PassTurn))
    };
    let on_play_again = {
        let api = api.clone();
        Callback::from(move |_: MouseEvent| api.play_again())
    };
    let on_leave = {
        let api = api.clone();
        Callback::from(move |_: MouseEvent| api.leave())
    };

    // Keep the chain scrolled to the newest end as tiles land.
    let chain_ref = use_node_ref();
    {
        let chain_ref = chain_ref.clone();
        let len = game.board.len();
        use_effect_with(len, move |_| {
            if let Some(node) = chain_ref.get() {
                let el: web_sys::Element = node.unchecked_into();
                el.set_scroll_left(el.scroll_width());
            }
        });
    }

    let status = if over {
        "Game over".to_owned()
    } else if my_turn {
        "Your turn — pick a tile".to_owned()
    } else {
        let name = api.state.name_at(game.turn).unwrap_or("A player");
        format!("{name} is playing…")
    };

    let players = html! {
        <div class="players">
            { for (0..SEAT_COUNT).map(|seat| {
                let occupant = room.seats.iter().find(|s| s.seat as usize == seat);
                let count = game.hand_counts.get(seat).copied().unwrap_or(0);
                let active = !over && game.turn == seat;
                html! {
                    <div class={classes!("player",
                        active.then_some("active"),
                        (seat == you).then_some("self"),
                        occupant.is_none().then_some("gone"))}>
                        <span class="player-name">
                            { occupant.map(|o| o.name.clone()).unwrap_or_else(|| "—".to_owned()) }
                        </span>
                        <span class="player-count">
                            { if occupant.is_some() { count.to_string() } else { "✕".to_owned() } }
                        </span>
                    </div>
                }
            }) }
        </div>
    };

    let board = html! {
        <div class="board card">
            <div class="ends">
                <span class="end">
                    { game.left_end.map(|p| format!("◀ {p}")).unwrap_or_else(|| "◀ –".to_owned()) }
                </span>
                <span class="status">{ &status }</span>
                <span class="end">
                    { game.right_end.map(|p| format!("{p} ▶")).unwrap_or_else(|| "– ▶".to_owned()) }
                </span>
            </div>
            <div class="chain" ref={chain_ref}>
                if game.board.is_empty() {
                    <div class="chain-empty">{"The table is empty — first tile opens."}</div>
                } else {
                    { for game.board.iter().map(|tile| html! {
                        <Domino tile={*tile} size={DominoSize::Board} />
                    }) }
                }
            </div>
        </div>
    };

    let hand_row = html! {
        <div class="hand">
            <div class="hand-label">{format!("Your hand · {} tiles", hand.len())}</div>
            <div class="hand-row">
                { for hand.iter().map(|t| {
                    let tile = *t;
                    let f = fit(tile, &game);
                    let playable = my_turn && f.any();
                    let picked = *selected == Some(tile);
                    let on = {
                        let on_tile = on_tile.clone();
                        Callback::from(move |_: MouseEvent| on_tile.emit(tile))
                    };
                    html! {
                        <button class={classes!("hand-tile",
                            playable.then_some("playable"),
                            picked.then_some("picked"))}
                            onclick={on}>
                            <Domino tile={tile} size={DominoSize::Hand} />
                        </button>
                    }
                }) }
            </div>
        </div>
    };

    let actions = if let Some(result) = &game.result {
        let (title, subtitle) = result_text(result, &room, you);
        html! {
            <div class="banner card">
                <h3>{ title }</h3>
                if let Some(subtitle) = subtitle { <p>{ subtitle }</p> }
                <p class="hint">
                    { format!(
                        "{} of 4 seats are back at the table — everyone taps \u{201c}Play again\u{201d}, and the next game deals as soon as all four seats are filled.",
                        room.seats.len()
                    ) }
                </p>
                <div class="banner-buttons">
                    <button class="btn btn-primary" onclick={on_play_again}>{"Play again"}</button>
                    <button class="btn btn-ghost" onclick={on_leave}>{"Leave"}</button>
                </div>
            </div>
        }
    } else {
        html! {
            <div class="actions">
                <button class="btn" disabled={!must_pass} onclick={on_pass}>{"Pass"}</button>
                <button class="btn btn-ghost" onclick={on_leave.clone()}>{"Leave"}</button>
            </div>
        }
    };

    // End picker: shown only when the picked tile fits both ends.
    let picker = selected.as_ref().map(|tile| {
        let on_left = {
            let pick_end = pick_end.clone();
            Callback::from(move |_: MouseEvent| pick_end.emit(Side::Left))
        };
        let on_right = {
            let pick_end = pick_end.clone();
            Callback::from(move |_: MouseEvent| pick_end.emit(Side::Right))
        };
        html! {
            <div class="scrim">
                <div class="picker card">
                    <p>{"Play this tile on which end?"}</p>
                    <div class="picker-tile">
                        <Domino tile={*tile} size={DominoSize::Board} />
                    </div>
                    <div class="picker-row">
                        <button class="btn btn-primary" onclick={on_left}>
                            { game.left_end.map(|p| format!("◀ {p}")).unwrap_or_default() }
                        </button>
                        <button class="btn btn-primary" onclick={on_right}>
                            { game.right_end.map(|p| format!("{p} ▶")).unwrap_or_default() }
                        </button>
                    </div>
                    <button class="btn btn-ghost" onclick={cancel_pick.clone()}>{"Cancel"}</button>
                </div>
            </div>
        }
    });

    let offline = matches!(api.state.socket, SocketState::Reconnecting)
        .then(|| html! { <div class="offline-bar">{"Connection lost — reconnecting…"}</div> });

    html! {
        <>
            <TopBar api={api.clone()} />
            { offline }
            <div class="page game">
                { players }
                { board }
                { hand_row }
                { actions }
            </div>
            { picker.unwrap_or_default() }
        </>
    }
}

/// Banner copy for each way a game can end.
fn result_text(
    result: &GameResult,
    room: &PublicRoomState,
    you: usize,
) -> (String, Option<String>) {
    let winner = result
        .winner
        .and_then(|seat| room.seats.iter().find(|s| s.seat as usize == seat))
        .map(|s| s.name.clone());
    match (result.winner, result.reason) {
        (Some(seat), EndReason::HandEmpty) => (
            if seat == you {
                "You win! 🎉".to_owned()
            } else {
                format!("{} wins! 🎉", winner.as_deref().unwrap_or("A player"))
            },
            Some("played the last tile".to_owned()),
        ),
        (Some(seat), EndReason::Blocked) => (
            if seat == you {
                "Blocked — you win on pips! 🎉".to_owned()
            } else {
                format!("Blocked — {} wins on pips", winner.as_deref().unwrap_or("a player"))
            },
            Some("nobody could play".to_owned()),
        ),
        (None, EndReason::Blocked) => (
            "Blocked — draw".to_owned(),
            Some("nobody could play and pips were tied".to_owned()),
        ),
        (_, EndReason::Abandoned) => (
            "Game voided".to_owned(),
            Some("a player lost connection".to_owned()),
        ),
        (None, EndReason::HandEmpty) => ("Game over".to_owned(), None),
    }
}
