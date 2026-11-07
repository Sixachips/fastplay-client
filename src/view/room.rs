//! The waiting room: roster, the share link, and the "deals when full"
//! notice. Shown whenever we hold a seat but no game snapshot has arrived
//! yet (phase waiting, or a voided game awaiting players).

use yew::prelude::*;

use crate::protocol::{team_name, team_of, SEAT_COUNT};
use crate::roomcode;
use crate::state::{Msg, ToastKind};
use crate::view::share;
use crate::view::TopBar;
use crate::Api;

#[derive(Properties, PartialEq)]
pub struct Props {
    pub api: Api,
}

#[function_component(RoomView)]
pub fn room_view(props: &Props) -> Html {
    let api = props.api.clone();
    let Some(room) = api.state.room.clone() else {
        return Html::default();
    };

    let link = roomcode::share_link(&room.room_id);
    let filled = room.seats.len();

    let on_leave = {
        let api = api.clone();
        Callback::from(move |_: MouseEvent| api.leave())
    };
    let on_copy = {
        let api = api.clone();
        let link = link.clone();
        Callback::from(move |_: MouseEvent| {
            if share::copy_text(&link) {
                api.state.dispatch(Msg::ShowToast {
                    kind: ToastKind::Info,
                    text: "Link copied".to_owned(),
                });
            } else {
                api.state.dispatch(Msg::ShowToast {
                    kind: ToastKind::Info,
                    text: "Tap the link box and copy manually".to_owned(),
                });
            }
        })
    };
    let on_share = {
        let api = api.clone();
        let link = link.clone();
        Callback::from(move |_: MouseEvent| {
            // No share sheet (mostly desktop): fall back to clipboard.
            if !share::share_link(&link) && !share::copy_text(&link) {
                api.state.dispatch(Msg::ShowToast {
                    kind: ToastKind::Info,
                    text: "Tap the link box and copy manually".to_owned(),
                });
            }
        })
    };
    // Tapping the readonly link box selects its contents for manual copying.
    let on_select = Callback::from(move |event: MouseEvent| {
        if let Some(input) = event.target_dyn_into::<web_sys::HtmlInputElement>() {
            input.select();
        }
    });

    let seat_cards = (0..SEAT_COUNT).map(|seat| {
        let occupant = room.seats.iter().find(|s| s.seat as usize == seat);
        let is_you = room.you == seat;
        let side = match seat {
            0 => "north",
            1 => "west",
            2 => "south",
            3 => "east",
            _ => "north",
        };
        // A full team never refuses a join (the seeker falls back to any free
        // seat), so the partnership is read from the wire, not from seat
        // parity. Vacant seats inherit their seat's side — the only pair of
        // partners that can fill them.
        let (team, team_label) = match occupant {
            Some(occupant) => (occupant.team, team_name(occupant.team as usize)),
            None => (team_of(seat) as u8, team_name(team_of(seat))),
        };
        let team_class = if team == 0 {
            "team-north-south"
        } else {
            "team-east-west"
        };
        html! {
            <div class={classes!("seat", side, team_class,
                occupant.is_some().then_some("taken"),
                is_you.then_some("you"))}>
                <span class="seat-num">{seat + 1}</span>
                <div class="seat-body">
                    { match occupant {
                        Some(occupant) => html! {
                            <>
                                <span class="seat-name">{ &occupant.name }</span>
                                if is_you { <span class="seat-you">{"you"}</span> }
                                <span class="seat-team">{ team_label }</span>
                            </>
                        },
                        None => html! { <span class="seat-empty">{ team_label }</span> },
                    } }
                </div>
            </div>
        }
    });

    html! {
        <>
            <TopBar api={api.clone()} />
            <div class="page room">
                <h2 class="room-title">{ format!("Room {}", room.room_id) }</h2>

                <div class="seats">{ for seat_cards }</div>

                <div class="card share">
                    <h3>{"Invite friends"}</h3>
                    <div class="share-row">
                        <input class="linkbox" type="text" readonly=true
                            value={link} onclick={on_select} />
                        <button class="btn" onclick={on_copy}>{"Copy"}</button>
                        <button class="btn btn-primary" onclick={on_share}>{"Share"}</button>
                    </div>
                    <p class="hint">
                        { format!("{filled} of 4 seats taken — the table deals automatically once it's full.") }
                    </p>
                </div>

                <button class="btn btn-ghost leave" onclick={on_leave}>{"Leave"}</button>
            </div>
        </>
    }
}
