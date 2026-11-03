//! The game screen: opponents strip, board chain, your hand, and the
//! play / pass / play-again controls.
//!
//! Interaction model: tap a playable tile to PICK it up — nothing is played on
//! that tap. While a tile is picked, a placement bar above the hand offers one
//! button per end it fits, and the two pips shown at the ends of the board are
//! tap targets for the same move. Tapping the picked tile again puts it back.
//! An empty board has no ends to choose between, so the opening tile gets a
//! single confirm button (the server ignores the side there).

use wasm_bindgen::JsCast;
use yew::prelude::*;

use crate::legality::{fit, has_playable, open_ends};
use crate::protocol::{
    ClientToServer, EndReason, GameResult, PublicGameState, PublicRoomState, PublicTile, SEAT_COUNT,
    Side,
};
use crate::session::SocketState;
use crate::state::{Msg, ToastKind};
use crate::view::domino::{Domino, DominoSize};
use crate::view::TopBar;
use crate::Api;

#[derive(Properties, PartialEq)]
pub struct Props {
    pub api: Api,
}

/// One legal landing spot for the picked tile: which end of the chain, the pip
/// it matches, and how the tile will read once it is down.
#[derive(Clone)]
struct Placement {
    side: Side,
    end_pip: u8,
    reads: String,
}

/// Landing spots for `tile`, in board order (left end first). Empty while the
/// board is empty: there is nothing to choose between before the opener.
fn placements(tile: PublicTile, game: &PublicGameState) -> Vec<Placement> {
    if game.board.is_empty() {
        return Vec::new();
    }
    let f = fit(tile, game);
    let mut spots = Vec::with_capacity(2);
    if let Some(pip) = game.left_end.filter(|_| f.left) {
        // On the left end the matching pip faces the chain, i.e. to its right.
        spots.push(Placement {
            side: Side::Left,
            end_pip: pip,
            reads: format!("{}|{}", tile.outer_pip_at(pip), pip),
        });
    }
    if let Some(pip) = game.right_end.filter(|_| f.right) {
        spots.push(Placement {
            side: Side::Right,
            end_pip: pip,
            reads: format!("{}|{}", pip, tile.outer_pip_at(pip)),
        });
    }
    spots
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
    let playable_exists = has_playable(&hand, &game);
    let must_pass = my_turn && !playable_exists;

    // Any new snapshot invalidates a pending pick.
    {
        let selected = selected.clone();
        let key = (game.turn, game.board.len(), game.result);
        use_effect_with(key, move |_| {
            selected.set(None);
        });
    }

    // A pick only counts while we still hold the tile and it is our turn, so a
    // snapshot that ends our turn (or retires the tile) cannot leave a dangling
    // placement bar pointing at a tile that is already on the table.
    let picked = (*selected).filter(|tile| my_turn && hand.contains(tile));
    let spots = picked.map(|tile| placements(tile, &game)).unwrap_or_default();
    let fit_left = spots.iter().any(|spot| spot.side == Side::Left);
    let fit_right = spots.iter().any(|spot| spot.side == Side::Right);

    let on_tile = {
        let api = api.clone();
        let game = game.clone();
        let selected = selected.clone();
        Callback::from(move |tile: PublicTile| {
            if !my_turn {
                return;
            }
            if !fit(tile, &game).any() {
                // A silent no-op reads as a broken screen: name the pips the
                // tile failed to match, and say whether anything can go down.
                let text = if playable_exists {
                    format!("{tile} fits neither {} — try another tile", open_ends(&game))
                } else {
                    format!("{tile} fits neither {} — you have to pass", open_ends(&game))
                };
                api.state.dispatch(Msg::ShowToast { kind: ToastKind::Warn, text });
                return;
            }
            // Tap to pick it up; tap the picked tile again to put it back.
            selected.set(if *selected == Some(tile) { None } else { Some(tile) });
        })
    };

    // Confirms the picked tile onto one end. The board ends fire the same
    // callback, so the bar is a shortcut, not the only way to place.
    let on_place = {
        let api = api.clone();
        let selected = selected.clone();
        Callback::from(move |side: Side| {
            if let Some(tile) = *selected {
                api.send_cmd(ClientToServer::PlayTile { tile, side });
                selected.set(None);
            }
        })
    };
    let on_cancel = {
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
        if game.result.is_some_and(|result| result.is_closed()) {
            "Game closed".to_owned()
        } else {
            "Game over".to_owned()
        }
    } else if !my_turn {
        let name = api.state.name_at(game.turn).unwrap_or("A player");
        format!("{name} is playing…")
    } else if picked.is_some() {
        "Now choose the end".to_owned()
    } else if must_pass {
        "Nothing fits — pass".to_owned()
    } else {
        "Your turn — pick a tile".to_owned()
    };

    let player_card = |seat: usize| -> Html {
        let occupant = room.seats.iter().find(|s| s.seat as usize == seat);
        let count = game.hand_counts.get(seat).copied().unwrap_or(0);
        let active = !over && game.turn == seat;
        // Seats are placed relative to you: play passes to the right (CCW),
        // so the next turn sits east, your partner sits across, and the seat
        // that played before you sits west.
        let side = match (seat + 4 - you) % 4 {
            0 => "You",
            1 => "Right",
            2 => "Partner",
            _ => "Left",
        };
        let team = match seat {
            0 | 2 => "team-north-south",
            _ => "team-east-west",
        };
        html! {
            <div class={classes!("player", team,
                active.then_some("active"),
                (seat == you).then_some("self"),
                occupant.is_none().then_some("gone"))}>
                <span class="player-side">{side}</span>
                <span class="player-name">
                    { occupant.map(|o| o.name.clone()).unwrap_or_else(|| "—".to_owned()) }
                </span>
                <span class="player-count">
                    { if occupant.is_some() { count.to_string() } else { "✕".to_owned() } }
                </span>
            </div>
        }
    };

    let end_button = |side: Side, pip: Option<u8>, fits: bool| -> Html {
        let on_place = on_place.clone();
        let label = match (side, pip) {
            (Side::Left, Some(pip)) => format!("◀ {pip}"),
            (Side::Left, None) => "◀ –".to_owned(),
            (Side::Right, Some(pip)) => format!("{pip} ▶"),
            (Side::Right, None) => "– ▶".to_owned(),
        };
        html! {
            <button class={classes!("end", "end-btn", fits.then_some("fit"))}
                    disabled={!fits}
                    onclick={Callback::from(move |_: MouseEvent| on_place.emit(side))}>
                { label }
            </button>
        }
    };

    let table = html! {
        <div class="table">
            <div class="table-north">{ player_card((you + 2) % 4) }</div>
            <div class="table-west">{ player_card((you + 3) % 4) }</div>
            <div class="table-board board card">
                <div class="ends">
                    { end_button(Side::Left, game.left_end, fit_left) }
                    <span class="status">{ &status }</span>
                    { end_button(Side::Right, game.right_end, fit_right) }
                </div>
                <div class="chain" ref={chain_ref}>
                    if game.board.is_empty() {
                        <div class="chain-empty">{"The table is empty — first tile opens."}</div>
                    } else {
                        { for game.board.iter().map(|tile| html! {
                            <Domino tile={*tile} size={DominoSize::Board}
                                    crosswise={tile.is_double()} />
                        }) }
                    }
                </div>
            </div>
            <div class="table-east">{ player_card((you + 1) % 4) }</div>
            <div class="table-south">{ player_card(you) }</div>
        </div>
    };

    // Placement bar: shown while a tile is picked, between board and hand so
    // the chain stays visible while the end is chosen.
    let place_bar = picked.map(|tile| {
        let open_table = {
            let on_place = on_place.clone();
            Callback::from(move |_: MouseEvent| on_place.emit(Side::Right))
        };
        html! {
            <div class="place-bar card">
                <div class="place-head">
                    <Domino tile={tile} size={DominoSize::Board} />
                    <span class="place-hint">{
                        if game.board.is_empty() { "Opens the table" } else { "Play it on…" }
                    }</span>
                    <button class="btn btn-ghost place-cancel" onclick={on_cancel.clone()}>
                        {"Cancel"}
                    </button>
                </div>
                <div class="place-row">
                    if game.board.is_empty() {
                        <button class="btn btn-primary" onclick={open_table}>
                            {"Open the table"}
                        </button>
                    } else {
                        { for spots.iter().map(|spot| {
                            let on_place = on_place.clone();
                            let side = spot.side;
                            let title = match side {
                                Side::Left => format!("◀ {}", spot.end_pip),
                                Side::Right => format!("{} ▶", spot.end_pip),
                            };
                            html! {
                                <button class="btn btn-primary place-end"
                                        onclick={Callback::from(move |_: MouseEvent| on_place.emit(side))}>
                                    <span class="place-end-pip">{ title }</span>
                                    <span class="place-end-reads">{ format!("reads {}", spot.reads) }</span>
                                </button>
                            }
                        }) }
                    }
                </div>
            </div>
        }
    });

    let hand_row = html! {
        <div class="hand">
            <div class="hand-label">{format!("Your hand · {} tiles", hand.len())}</div>
            <div class="hand-row">
                { for hand.iter().map(|t| {
                    let tile = *t;
                    let playable = my_turn && fit(tile, &game).any();
                    let picked_tile = picked == Some(tile);
                    let on = {
                        let on_tile = on_tile.clone();
                        Callback::from(move |_: MouseEvent| on_tile.emit(tile))
                    };
                    html! {
                        <button class={classes!("hand-tile",
                            playable.then_some("playable"),
                            picked_tile.then_some("picked"))}
                            onclick={on}>
                            <Domino tile={tile} size={DominoSize::Hand} />
                        </button>
                    }
                }) }
            </div>
        </div>
    };

    // Closed-game reveal: when nobody could play, the server broadcasts every
    // seat's remaining tiles. Show them face-up, grouped per player, so the
    // pips that decided the blocked winner are visible to everyone.
    let reveal_panel = match (&game.result, api.state.reveal.as_ref()) {
        (Some(result), Some(hands)) if result.is_closed() && !hands.is_empty() => {
            let rows = hands.iter().map(|entry| {
                let seat = entry.seat as usize;
                let occupant = room.seats.iter().find(|s| s.seat as usize == seat);
                let pips: u32 = entry.hand.iter().map(|t| t.left as u32 + t.right as u32).sum();
                html! {
                    <div class="reveal-row">
                        <span class="reveal-name">
                            { occupant.map(|o| o.name.clone()).unwrap_or_else(|| "—".to_owned()) }
                        </span>
                        <span class="reveal-pips">{ format!("{pips} pips") }</span>
                        <span class="reveal-tiles">
                            { for entry.hand.iter().map(|t| html! {
                                <Domino tile={*t} size={DominoSize::Board} />
                            }) }
                        </span>
                    </div>
                }
            });
            html! {
                <div class="reveal card">
                    <div class="reveal-head">{"All hands — the game is closed"}</div>
                    { for rows }
                </div>
            }
        }
        _ => Html::default(),
    };

    let actions = if let Some(result) = &game.result {
        let (title, subtitle) = result_text(result, &room, you);
        // Rematch readiness lives on the roster: tapping sends `PlayAgain`
        // and the server's echo marks this seat ready. The deal lands when
        // the whole table is ready — no socket cycling, no "seats taken".
        let i_am_ready = room.seats.iter().any(|s| s.seat as usize == you && s.ready);
        let ready_count = room.seats.iter().filter(|s| s.ready).count();
        let seats_here = room.seats.len();
        let hint = if seats_here < SEAT_COUNT {
            format!(
                "{seats_here} of {SEAT_COUNT} seats are at the table — the next game deals \
                 as soon as the empty seats are filled and everyone taps \u{201c}Play again\u{201d}.",
            )
        } else if i_am_ready {
            format!(
                "{ready_count} of {seats_here} seats are ready — dealing the moment the table is complete.",
            )
        } else {
            format!(
                "Tap \u{201c}Play again\u{201d} when you are ready \u{2014} {ready_count} of \
                 {seats_here} seats are already waiting.",
            )
        };
        html! {
            <div class="banner card">
                <h3>{ title }</h3>
                if let Some(subtitle) = subtitle { <p>{ subtitle }</p> }
                <p class="hint">{ hint }</p>
                <div class="banner-buttons">
                    if i_am_ready {
                        <button class="btn btn-primary" disabled={true}>{"Waiting for players…"}</button>
                    } else {
                        <button class="btn btn-primary" onclick={on_play_again}>{"Play again"}</button>
                    }
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

    let offline = matches!(api.state.socket, SocketState::Reconnecting)
        .then(|| html! { <div class="offline-bar">{"Connection lost — reconnecting…"}</div> });

    html! {
        <>
            <TopBar api={api.clone()} />
            { offline }
            <div class="page game">
                { table }
                { place_bar.unwrap_or_default() }
                { hand_row }
                { reveal_panel }
                { actions }
            </div>
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
                "Game closed — you win on pips! 🎉".to_owned()
            } else {
                format!("Game closed — {} wins on pips", winner.as_deref().unwrap_or("a player"))
            },
            Some("nobody could put a tile".to_owned()),
        ),
        (None, EndReason::Blocked) => (
            "Game closed — draw".to_owned(),
            Some("nobody could put a tile and pips were tied".to_owned()),
        ),
        (_, EndReason::Abandoned) => (
            "Game voided".to_owned(),
            Some("a player lost connection".to_owned()),
        ),
        (None, EndReason::HandEmpty) => ("Game over".to_owned(), None),
    }
}
