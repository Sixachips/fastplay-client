//! View modules plus the shared top bar.

pub mod domino;
pub mod game;
pub mod home;
pub mod room;
pub mod share;

use yew::prelude::*;

use crate::Api;

#[derive(Properties, PartialEq)]
pub struct TopBarProps {
    pub api: Api,
}

/// Thin app bar: brand, room chip, connection status dot.
#[function_component(TopBar)]
pub fn top_bar(props: &TopBarProps) -> Html {
    let state = &props.api.state;
    let room_code = state.room.as_ref().map(|room| room.room_id.clone());
    let (dot_class, dot_hint) = match state.socket {
        crate::session::SocketState::Online => ("dot dot-on", "connected"),
        crate::session::SocketState::Connecting => ("dot dot-warn", "connecting…"),
        crate::session::SocketState::Reconnecting => ("dot dot-warn", "reconnecting…"),
        crate::session::SocketState::Offline => ("dot dot-off", "offline"),
    };

    html! {
        <div class="topbar">
            <span class="brand">{"fastPlay"}</span>
            if let Some(code) = room_code {
                <span class="roomchip">{format!("#{code}")}</span>
            }
            <span class={dot_class} title={dot_hint.to_owned()} />
        </div>
    }
}
