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
pub fn urlencode(input: &str) -> String {
    js_sys::encode_uri_component(input)
        .as_string()
        .unwrap_or_else(|| input.to_owned())
}

/// WebSocket endpoint of the production game server — the default for hosts
/// that serve this client as static pages but cannot proxy `/ws` themselves.
const DOMINO_SERVER_WS: &str = "wss://domino.sixachips.com/ws";

/// Hosts that serve this client from GitHub Pages.
const PAGES_HOSTS: &[&str] = &["sixachips.com", "www.sixachips.com", "sixachips.github.io"];

/// Best guess for the server's WebSocket endpoint given where the page is
/// served from:
///  * the dedicated game server (`wss://domino.sixachips.com/ws`) when the
///    page is served from GitHub Pages, which cannot proxy `/ws` itself;
///  * same-origin `/ws` when the page runs on a standard port (i.e. a reverse
///    proxy fronts both static files and the game server in production);
///  * port 8080 on the same host otherwise (the headless server's default),
///    which is exactly the `trunk serve` on :8081 + server on :8080 dev setup
///    and works for phones browsing the dev server over the LAN.
pub fn guess_ws_url() -> String {
    let location = web_sys::window().expect("no window").location();
    ws_url_for(
        &location.protocol().unwrap_or_default(),
        &location.hostname().expect("hostname"),
        &location.port().unwrap_or_default(),
    )
}

/// Pure half of `guess_ws_url`, so the guessing rules are unit-testable on
/// the host without a browser.
fn ws_url_for(protocol: &str, hostname: &str, port: &str) -> String {
    // GitHub Pages cannot proxy WebSocket upgrades, so its hosts get the
    // dedicated game server rather than a same-origin guess.
    if PAGES_HOSTS.contains(&hostname) {
        return DOMINO_SERVER_WS.to_owned();
    }

    let scheme = if protocol == "https:" { "wss" } else { "ws" };

    match port {
        "" | "80" | "443" => format!("{scheme}://{hostname}/ws"),
        _ => format!("{scheme}://{hostname}:8080/ws"),
    }
}

#[cfg(test)]
mod tests {
    use super::ws_url_for;

    #[test]
    fn a_standard_port_defaults_to_the_same_origin() {
        assert_eq!(
            ws_url_for("https:", "example.com", ""),
            "wss://example.com/ws"
        );
        assert_eq!(
            ws_url_for("http:", "example.com", "80"),
            "ws://example.com/ws"
        );
        assert_eq!(
            ws_url_for("https:", "domino.sixachips.com", "443"),
            "wss://domino.sixachips.com/ws"
        );
    }

    #[test]
    fn github_pages_hosts_default_to_the_domino_server() {
        for hostname in ["sixachips.com", "www.sixachips.com", "sixachips.github.io"] {
            assert_eq!(
                ws_url_for("https:", hostname, ""),
                "wss://domino.sixachips.com/ws",
                "unexpected default for {hostname}"
            );
        }
    }

    #[test]
    fn any_other_port_defaults_to_the_dev_server_on_8080() {
        assert_eq!(
            ws_url_for("http:", "192.168.1.20", "8081"),
            "ws://192.168.1.20:8080/ws"
        );
    }
}
