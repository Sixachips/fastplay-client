//! Room-code generation and share-link handling.
//!
//! The server treats room codes as opaque strings, so the host player just
//! needs something short, typeable, and link-friendly. Codes are lowercase
//! alphanumerics that survive being spoken over the phone and pasted from a
//! link without case ambiguity.

use web_sys::UrlSearchParams;

/// Unambiguous alphabet: no `0/o`, no `1/l/i`.
const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
const CODE_LEN: usize = 5;

/// Random room code, e.g. "k7mpa".
pub fn generate() -> String {
    (0..CODE_LEN)
        .map(|_| {
            let idx = (js_sys::Math::random() * ALPHABET.len() as f64) as usize;
            char::from(ALPHABET[idx.min(ALPHABET.len() - 1)])
        })
        .collect()
}

/// The room code carried by `?room=CODE` in the current URL, if any. This is
/// what makes a shared link jump straight to the right table.
pub fn room_from_url() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    if search.is_empty() {
        return None;
    }
    let params = UrlSearchParams::new_with_str(&search).ok()?;
    let code = params.get("room")?.trim().to_owned();
    (!code.is_empty()).then_some(code)
}

/// Absolute share link for a room: current origin + path + `?room=CODE`.
pub fn share_link(code: &str) -> String {
    let location = web_sys::window().expect("no window").location();
    let origin = location.origin().expect("origin");
    let path = location.pathname().expect("pathname");
    format!("{origin}{path}?room={}", urlencode(code))
}

/// Percent-encode the few characters a room code could contain beyond alnum.
fn urlencode(input: &str) -> String {
    js_sys::encode_uri_component(input)
        .as_string()
        .unwrap_or_else(|| input.to_owned())
}

/// Best guess for the server's WebSocket endpoint given where the page is
/// served from:
///  * same-origin `/ws` when the page runs on a standard port (i.e. a reverse
///    proxy fronts both static files and the game server in production);
///  * port 8080 on the same host otherwise (the headless server's default),
///    which is exactly the `trunk serve` on :8081 + server on :8080 dev setup
///    and works for phones browsing the dev server over the LAN.
pub fn guess_ws_url() -> String {
    let location = web_sys::window().expect("no window").location();
    let secure = location.protocol().as_deref() == Ok("https:");
    let scheme = if secure { "wss" } else { "ws" };
    let host = location.hostname().expect("hostname");
    let port = location.port().unwrap_or_default();

    match port.as_str() {
        "" | "80" | "443" => format!("{scheme}://{host}/ws"),
        _ => format!("{scheme}://{host}:8080/ws"),
    }
}
