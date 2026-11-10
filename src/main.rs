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

use wasm_bindgen::JsValue;

use gloo_timers::future::TimeoutFuture;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use protocol::ClientToServer;
use session::{Session, SocketEvent};
use state::{AppState, Msg, Screen, ToastKind};
use view::game::GameView;
use view::home::HomeView;
use view::room::RoomView;

/// Holder for the live socket that survives re-renders.
///
/// `Api` is rebuilt on every render of `App`, so the `Rc` it points at must
/// NOT be: a fresh `Rc::new(RefCell::new(None))` per render would hand the
/// views an empty slot while the running `Session` stayed behind in the
/// previous one — every command would then vanish in `Api::send_cmd` with no
/// error, no toast and a green "connected" dot. Equality is pointer equality,
/// which keeps `Api: PartialEq` (and therefore re-render detection) honest.
#[derive(Clone, Default)]
struct SessionSlot(Rc<RefCell<Option<Session>>>);

impl PartialEq for SessionSlot {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

// Command handle shared with the views. The reducer is pure, so anything
// that touches the socket goes through this: it owns the current `Session`
// (assigning a new one drops the old connection, which stops its reconnect
// loop) and dispatches the matching state messages.
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
    fn new(state: UseReducerHandle<AppState>, session: Rc<RefCell<Option<Session>>>) -> Api {
        Api { state, session }
    }

    /// Open (or re-open) a connection and claim a seat in `room` as `name`.
    /// `team` is the partnership preference from the home picker (`0`
    /// North–South, `1` West–East, or `None` for "no preference").
    pub fn join(&self, room: String, name: String, team: Option<u8>) {
        let room = room.trim().to_owned();
        let name = name.trim().to_owned();
        // Close any previous socket *before* the fresh one starts joining:
        // the server frees a seat only when it sees the old connection close,
        // and a lingering stale seat would bounce this join with "all four
        // seats taken".
        if let Some(previous) = self.session.borrow_mut().as_mut() {
            previous.leave();
        }
        self.state.dispatch(Msg::JoinRoom {
            room: room.clone(),
            name: name.clone(),
            team,
        });
        state::persist_inputs(&name, &self.state.ws_url, team);
        sync_room_url(Some(&room));

        let on_event = {
            let state = self.state.clone();
            Callback::from(move |event: SocketEvent| state.dispatch(Msg::Socket(event)))
        };
        let session = Session::start(self.state.ws_url.clone(), room, name, team, on_event);
        // Assigning drops the previous session: its connection loop stops, and the socket closes.
        *self.session.borrow_mut() = Some(session);
    }

    /// New game: mint a room code, park it in the form, and sit down. The
    /// partnership the picker chose becomes the creator's request.
    pub fn new_game(&self) {
        if self.state.name.trim().is_empty() {
            self.state.dispatch(Msg::ShowToast {
                kind: ToastKind::Warn,
                text: "Escribe tu nombre primero".to_owned(),
            });
            return;
        }
        let code = roomcode::generate();
        self.state.dispatch(Msg::RoomChanged(code.clone()));
        self.join(code, self.state.name.clone(), self.state.team);
    }

    /// Send a game command over the live socket. With no session at all (never
    /// joined, or just left) the command cannot be queued, so say so out loud:
    /// a dropped move that leaves no trace is indistinguishable from a dead
    /// screen, which is exactly how this bug hid for so long.
    pub fn send_cmd(&self, cmd: ClientToServer) {
        match &mut *self.session.borrow_mut() {
            Some(session) => session.send(cmd),
            None => {
                gloo_console::warn!(format!("no socket to send {cmd:?} on"));
                self.state.dispatch(Msg::ShowToast {
                    kind: ToastKind::Warn,
                    text: "No estás conectado a la mesa".to_owned(),
                });
            }
        }
    }

    /// Play again: signal readiness on the *existing* socket. The room
    /// re-deals in place as soon as all four seated players have tapped —
    /// cycling the socket instead would race the server's seat release and
    /// make the fresh join bounce off "all four seats taken".
    pub fn play_again(&self) {
        self.send_cmd(ClientToServer::PlayAgain);
    }

    /// Leave the room: stop reconnecting and go home.
    pub fn leave(&self) {
        if let Some(session) = &mut *self.session.borrow_mut() {
            session.leave();
        }
        *self.session.borrow_mut() = None;
        self.state.dispatch(Msg::LeaveRoom);
        sync_room_url(None);
    }
}

/// Keep the browser address bar in sync with the room we are seated in, so
/// copying the page URL (or sharing it from the OS) carries the `?room=CODE`
/// query string that makes the link jump straight to the right table.
fn sync_room_url(code: Option<&str>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(history) = window.history() else {
        return;
    };
    let location = window.location();
    let origin = location.origin().unwrap_or_default();
    let path = location.pathname().unwrap_or_default();
    let url = match code {
        Some(code) => format!("{origin}{path}?room={}", roomcode::urlencode(code)),
        None => format!("{origin}{path}"),
    };
    let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&url));
}

#[function_component(App)]
fn app() -> Html {
    let state = use_reducer_eq(state::initial);
    // The socket slot has to come from a hook: this body runs again on every
    // render, and a slot built here would be a new, empty one each time.
    let slot = use_state(SessionSlot::default);
    let api = Api::new(state.clone(), slot.0.clone());

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
        session::SocketState::Reconnecting => "Conexión perdida — reconectando…",
        _ => "Conectando a la mesa…",
    };
    html! {
        <div class="page center">
            <div class="spinner" />
            <p>{ text }</p>
            <button class="btn btn-ghost"
                onclick={Callback::from(move |_: MouseEvent| a.leave())}>
                {"Cancelar"}
            </button>
        </div>
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
