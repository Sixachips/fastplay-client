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
        <div class="team-picker" role="radiogroup" aria-label="Pareja">
            { for [
                option(api, None, "Sin preferencia", "Ocupa el primer asiento libre"),
                option(api, Some(0), team_name(0), "Asientos 1 y 3, uno frente al otro"),
                option(api, Some(1), team_name(1), "Asientos 2 y 4, uno frente al otro"),
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
                text: "Escribe tu nombre y un código de sala primero".to_owned(),
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
                <p class="tagline">{"Dominó para cuatro — comparte un enlace y juega en el navegador"}</p>
            </div>

            if invited {
                <div class="card invite">
                    {"¡Te invitaron a una mesa! Escribe tu nombre y únete abajo."}
                </div>
            }

            <div class="card form">
                <label class="field">
                    <span>{"Tu nombre"}</span>
                    <input type="text" value={api.state.name.clone()}
                        oninput={on_name} maxlength="32" placeholder="p. ej. Ada" />
                </label>
                <div class="field">
                    <span>{"Pareja"}</span>
                    { team_picker(&api) }
                    <span class="hint">{"Tú y tu pareja os sentáis frente a frente — elige tu lado o deja que la mesa te siente."}</span>
                </div>
                <label class="field">
                    <span>{"Código de sala"}</span>
                    <input type="text" value={api.state.room_input.clone()}
                        oninput={on_room} maxlength="16" placeholder="p. ej. k7mpa" />
                </label>
                <div class="form-buttons">
                    <button class="btn btn-primary" onclick={on_join}>{"Unirse a la mesa"}</button>
                    <button class="btn" onclick={on_new_game}>{"Nueva partida"}</button>
                </div>
            </div>

            <details class="advanced">
                <summary>{"Servidor (avanzado)"}</summary>
                <input type="text" value={api.state.ws_url.clone()}
                    oninput={on_url} placeholder="ws://localhost:8080/ws" />
                <p class="hint">{format!("Por defecto: {}", roomcode::guess_ws_url())}</p>
            </details>

            <div class="card rules">
                <h3>{"Cómo se juega"}</h3>
                <ul>
                    <li>{"4 jugadores · juego doble seis · 7 fichas cada uno."}</li>
                    <li>{"La mesa reparte sola cuando entra el cuarto jugador."}</li>
                    <li>{"Toca una ficha y luego ◀ o ▶ para elegir el extremo."}</li>
                    <li>{"¿Nada encaja? Pasa."}</li>
                    <li>{"Las parejas se sientan frente a frente; las pintas que quedan en manos de la pareja rival puntúan para los ganadores de la mano. Gana la primera pareja en llegar a 100."}</li>
                    <li>{"Quien tiene el 6–6 abre la primera mano; las siguientes las abre el ganador anterior."}</li>
                    <li>{"Gana quien se queda sin fichas primero; cuatro pases seguidos bloquean la partida y gana quien tenga menos pintas."}</li>
                </ul>
            </div>
        </div>
    }
}
