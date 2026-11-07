//! The landing screen: name + room code, "join table" or "new game", plus a
//! collapsed server-address override.

use yew::prelude::*;

use crate::protocol::{team_name, ClientToServer};
use crate::roomcode;
use crate::state::{Msg, ToastKind};
use crate::Api;

#[derive(Properties, PartialEq)]
pub struct Props {
    pub api: Api,
}

/// The partnership picker: one row of three options, the picked one lit.
/// Team 0 (North–South) is seats 1 & 3 on screen, team 1 (West–East) seats 2
/// & 4 — partners always sit opposite each other.
fn team_picker(api: &Api) -> Html {
    let picked = api.state.team;
    let option = |api: &Api, team: Option<u8>, label: &str, hint: &'static str| {
        let a = api.clone();
        let is_picked = picked == team;
        html! {
            <button class={classes!("btn", is_picked.then_some("picked"))}
                title={hint}
                onclick={Callback::from(move |_: MouseEvent| {
                    a.state.dispatch(Msg::TeamChanged(team));
                })}>
                {label}
            </button>
        }
    };
    html! {
        <div class="team-picker" role="radiogroup" aria-label="Partnership">
            { for [
                option(api, None, "No preference", "Grab the first free seat"),
                option(api, Some(0), team_name(0), "Seats 1 and 3, opposite each other"),
                option(api, Some(1), team_name(1), "Seats 2 and 4, opposite each other"),
            ] }
        </div>
    }
}

#[function_component(HomeView)]
pub fn home_view(props: &Props) -> Html {
    let api = props.api.clone();

    let on_name = {
        let api = api.clone();
        Callback::from(move |event: InputEvent| {
            if let Some(input) = event.target_dyn_into::<web_sys::HtmlInputElement>() {
                api.state.dispatch(Msg::NameChanged(input.value()));
            }
        })
    };
    let on_room = {
        let api = api.clone();
        Callback::from(move |event: InputEvent| {
            if let Some(input) = event.target_dyn_into::<web_sys::HtmlInputElement>() {
                api.state.dispatch(Msg::RoomChanged(input.value()));
            }
        })
    };
    let on_url = {
        let api = api.clone();
        Callback::from(move |event: InputEvent| {
            if let Some(input) = event.target_dyn_into::<web_sys::HtmlInputElement>() {
                api.state.dispatch(Msg::WsUrlChanged(input.value()));
            }
        })
    };

    let on_join = {
        let api = api.clone();
        Callback::from(move |_: MouseEvent| match api.state.join_request() {
            Some(ClientToServer::JoinRoom { room, name, team }) => api.join(room, name, team),
            _ => api.state.dispatch(Msg::ShowToast {
                kind: ToastKind::Warn,
                text: "Enter your name and a room code first".to_owned(),
            }),
        })
    };
    let on_new_game = {
        let api = api.clone();
        Callback::from(move |_: MouseEvent| api.new_game())
    };

    let invited = roomcode::room_from_url().is_some();

    html! {
        <div class="home">
            <div class="hero">
                <h1>{"fastPlay"}</h1>
                <p class="tagline">{"Dominoes for four — share a link, play in the browser"}</p>
            </div>

            if invited {
                <div class="card invite">
                    {"You've been invited to a table! Enter your name and join below."}
                </div>
            }

            <div class="card form">
                <label class="field">
                    <span>{"Your name"}</span>
                    <input type="text" value={api.state.name.clone()}
                        oninput={on_name} maxlength="32" placeholder="e.g. Ada" />
                </label>
                <div class="field">
                    <span>{"Partnership"}</span>
                    { team_picker(&api) }
                    <span class="hint">{"You and your partner sit opposite each other — pick your side, or let the table seat you."}</span>
                </div>
                <label class="field">
                    <span>{"Room code"}</span>
                    <input type="text" value={api.state.room_input.clone()}
                        oninput={on_room} maxlength="16" placeholder="e.g. k7mpa" />
                </label>
                <div class="form-buttons">
                    <button class="btn btn-primary" onclick={on_join}>{"Join table"}</button>
                    <button class="btn" onclick={on_new_game}>{"New game"}</button>
                </div>
            </div>

            <details class="advanced">
                <summary>{"Server (advanced)"}</summary>
                <input type="text" value={api.state.ws_url.clone()}
                    oninput={on_url} placeholder="ws://localhost:8080/ws" />
                <p class="hint">{format!("Default: {}", roomcode::guess_ws_url())}</p>
            </details>

            <div class="card rules">
                <h3>{"How it plays"}</h3>
                <ul>
                    <li>{"4 players · double-six set · 7 tiles each."}</li>
                    <li>{"The table deals automatically when the fourth player joins."}</li>
                    <li>{"Tap a tile — then ◀ or ▶ to choose the end."}</li>
                    <li>{"Nothing fits? Pass."}</li>
                    <li>{"Partners sit opposite each other; the pips left with the rival pair score for the hand's winners, first partnership to 100 wins."}</li>
                    <li>{"The 6–6 holder opens the first hand with it; later hands go to the previous winner."}</li>
                    <li>{"First hand played out wins; four straight passes block the game and the fewest pips wins."}</li>
                </ul>
            </div>
        </div>
    }
}
