/// fastPlay client root: mounts the app, owns the WebSocket session, and
/// routes between the home / room / game screens.

mod legality;
mod protocol;
mod roomcode;
mod session;
mod state;
mod view;

use std::cell::RefCell;
use std::rc::Rc;

use gloo_timers::future::TimeoutFuture;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use protocol::ClientToServer;
use session::{Session, SocketEvent};
use state::{AppState, Msg, Screen, ToastKind};
use view::game::GameView;
use view::home::HomeView;
use view::room::RoomView;

// Command handle shared with the views. The reducer is pure, so anything
// that touches the socket goes through this: it owns the current `Session`
/// (assigning a new one drops the old connection, which stops its reconnect
/// loop) and dispatches the matching state messages.
#[derive(Clone)]
pub struct Api {
    pub state: UseReducerHandle<AppState>,
    session: Rc<RefCell<Option<Session>>>,

}

impl PartialEq for Api {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.session, &other.session) && self.state == other.state
    }
}

impl Api {
    fn new(state: UseReducerHandle<AppState>) -> Api {
        Api { state, session: Rc::new(RefCell::new(None)) }
    }

    /// Open (or re-open) a connection and claim a seat in `room` as `name`.
    pub fn join(&self, room: String, name: String) {
        let room = room.trim().to_owned();
        let name = name.trim().to_owned();
        self.state.dispatch(Msg::JoinRoom { room: room.clone(), name: name.clone() });
        state::persist_inputs(&name, &self.state.ws_url);

        let on_event = {
            let state = self.state.clone();
            Callback::from(move |event: SocketEvent| state.dispatch(Msg::Socket(event)))
        };
        let session = Session::start(self.state.ws_url.clone(), room, name, on_event);
        // Assigning drops the previous session: its connection loop stops, and the socket closes.
        *self.session.borrow_mut() = Some(session);
    }

    /// New game: mint a room code, park it in the form, and sit down.
    pub fn new_game(&self) {
        if self.state.name.trim().is_empty() {
            self.state.dispatch(Msg::ShowToast {
                kind: ToastKind::Warn,
                text: "Enter your name first".to_owned(),
            });
            return;
        }
        let code = roomcode::generate();
        self.state.dispatch(Msg::RoomChanged(code.clone()));
        self.join(code, self.state.name.clone());
    }

    /// Send a game command over the live socket. Commands issued while the
    /// link is down are dropped; the post-reconnect snapshots re-sync us.
    pub fn send_cmd(&self, cmd: ClientToServer) {
        if let Some(session) = &mut *self.session.borrow_mut() {
            session.send(cmd);
        }
    }

    /// Play again: cycle the socket to re-claim our seat. The server deals a
    /// fresh game to the whole table the moment the fourth seat lands.
    pub fn play_again(&self) {
        if let Some(target) = self.state.target.clone() {
            self.join(target.room, target.name);
        }
    }

    /// Leave the room: stop reconnecting and go home.
    pub fn leave(&self) {
        if let Some(session) = &mut *self.session.borrow_mut() {
            session.leave();
        }
        *self.session.borrow_mut() = None;
        self.state.dispatch(Msg::LeaveRoom);
    }
}

#[function_component(App)]
fn app() -> Html {
    let state = use_reducer_eq(state::initial);
    let api = Api::new(state.clone());

    // Toasts fade out on their own.
    {
        let state = state.clone();
        let nonce = state.toast.as_ref().map(|toast| toast.nonce);
        use_effect_with(nonce, move |nonce| {
            if nonce.is_some() {
                let state = state.clone();
                spawn_local(async move {
                    TimeoutFuture::new(4_500).await;
                    state.dispatch(Msg::DismissToast);
                });
            }
            || ()
        });
    }

    let api = api.clone();

    html! {
        <div class="app">
            {
                match api.state.screen() {
                    Screen::Home => html! { <HomeView api={api.clone()} /> },
                    Screen::Connecting => html! { <ConnectingView api={api.clone()} /> },
                    Screen::Room => html! { <RoomView api={api.clone()} /> },
                    Screen::Game => html! { <GameView api={api.clone()} /> },
                }
            }
            {
                api.state.toast.as_ref().map(|toast| {
                    let a = api.clone();
                    let class = match toast.kind {
                        ToastKind::Warn => "toast toast-warn",
                        ToastKind::Info => "toast toast-info",
                    };
                    html! {
                        <div class={class}
                            onclick={Callback::from(move |_: MouseEvent| a.state.dispatch(Msg::DismissToast))}>
                            { toast.text.clone() }
                        </div>
                    }
                }).unwrap_or_default()
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct ConnectingProps {
    api: Api,
}

/// Shown between "join" and the first `RoomState`: the socket is still
/// connecting, or the link dropped and the session is retrying.
#[function_component(ConnectingView)]
fn connecting_view(props: &ConnectingProps) -> Html {
    let a = props.api.clone();
    let text = match props.api.state.socket {
        session::SocketState::Reconnecting => "Connection lost — reconnecting…",
        _ => "Connecting to the table…",
    };
    html! {
        <div class="page center">
            <div class="spinner" />
            <p>{ text }</p>
            <button class="btn btn-ghost"
                onclick={Callback::from(move |_: MouseEvent| a.leave())}>
                {"Cancel"}
            </button>
        </div>
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}