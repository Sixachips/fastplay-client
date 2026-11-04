# fastPlay Client (`client/`) — AGENTS.md

Browser/PWA client (Yew 0.21 CSR + Trunk, compiled to `wasm32-unknown-unknown`) for the 4-player fastPlay dominoes server (`../server`). `client/` and `../server/` are separate Cargo packages (not a workspace).

## Commands

Run from `client/` unless noted:

| Task | Command |
|---|---|
| Host unit tests (reducer + legality) | `cargo test` |
| Run a single unit test | `cargo test <test_name>` |
| Typecheck WASM target | `cargo check --target wasm32-unknown-unknown` |
| Dev server (`0.0.0.0:8081`) | `trunk serve` |
| Dev bundle (`dist/`) | `trunk build` |
| Production bundle (`dist/`, `opt-level = "s"`, LTO) | `trunk build --release` |
| Wire E2E smoke (needs server on `:8080`, Node ≥ 21) | `node scripts/e2e-smoke.mjs` |
| Browser E2E (needs `trunk build` + server on `:8080` + `chromium`, Node ≥ 21) | `node scripts/e2e-browser.mjs` |

### E2E script environment overrides

- `scripts/e2e-smoke.mjs`: `FASTPLAY_WS` (default `ws://127.0.0.1:8080/ws`).
- `scripts/e2e-browser.mjs`:
  - Does **not** run `trunk build` itself — always run `trunk build` before running `node scripts/e2e-browser.mjs` so `dist/` has fresh WASM/JS bytes.
  - Env vars: `FASTPLAY_DIST` (default `./dist`), `FASTPLAY_WS` (default `ws://127.0.0.1:8080/ws`), `FASTPLAY_PORT` (default `8081`), `FASTPLAY_CHROME` (default `chromium`), `FASTPLAY_CDP_PORT` (random `9300..9500`), `FASTPLAY_SHOT=/path/to/shot.png` (captures final table screenshot).

## Architecture & Non-Obvious Invariants

- **Note on `README.md` drift**: `README.md` mentions `src/view/board.rs`, `src/view/hand.rs`, and a `[pwa]` section in `Trunk.toml` — none of those exist. The actual views are `home.rs`, `room.rs`, `game.rs`, `domino.rs`, and `share.rs` under `src/view/`, and PWA assets (`manifest.webmanifest`, `sw.js`, `icons/`) are copied via `<link data-trunk rel="copy-...">` directives in `index.html`.
- **Wire contract (`src/protocol.rs`)**: Mirrors `../server/src/protocol.rs` by hand (no shared crate). Serde enum tags (`#[serde(tag = "type")]`) and `snake_case` field/variant names must match the server exactly. Max lengths (`MAX_ROOM_LEN = 16`, `MAX_NAME_LEN = 32` in `src/state.rs`) mirror server constants.
- **Socket slot across Yew renders (`src/main.rs`)**: `App` rebuilds `Api` on every render. The `Rc<RefCell<Option<Session>>>` inside `Api` **must** come from `use_state(SessionSlot::default)` with pointer-equality (`Rc::ptr_eq`). Creating a new `Rc` per render silently orphans the active `Session`, causing outbound moves to drop while inbound snapshots keep arriving.
- **Hand synchronization (`sync_hand` in `src/state.rs`)**: The server sends `HandDealt` **once** per deal and only broadcasts `hand_counts` on subsequent `GameStateUpdate`s. Also, hand tiles are canonical (`left <= right`) whereas board tiles are oriented as played. `sync_hand` uses `legality::same_tile` (which compares `.normalized()`) to remove the tile we just placed when the chain grows by 1, with a count-clamp fallback across missed snapshots. Breaking this leaves phantom tiles in `state.hand`, keeping `has_playable` true and disabling the **Pass** button on a blocked turn.
- **Forced opening (`opening_tile`)**: A session's first hand must be opened with the 6|6, so `PublicGameState.opening_tile` is `Some(6|6)` while the board is empty. `legality::fit` honours it (only the forced tile fits an empty board) and `view/game.rs` words the pick/placement hints around it. Later hands carry `None` and accept any opening tile. Ignoring this field makes the first pick of a game look broken (server `WrongOpeningTile` rejections).
- **Two-step tile placement (`src/view/game.rs`)**: Tapping a `.hand-tile.playable` only **picks** the tile (sets `selected` state and opens `.place-bar`); it never sends `PlayTile` directly. The move is sent only when the user clicks an end button (`.place-end`, `.end-btn.fit`, or `"Open the table"` on an empty board). `scripts/e2e-browser.mjs` explicitly asserts this two-step interaction and key CSS selectors (`.hand-tile.playable`, `.hand-tile.picked`, `.place-bar`, `.place-end`, `.chain .domino.domino-cross`, `.reveal`, `.reveal-row`, `.banner-buttons .btn`).
- **Crosswise doubles (`src/view/domino.rs`, `styles.css`)**: On the board chain, doubles (`tile.is_double()`) render with `crosswise={true}` (`.domino-cross`), which stacks halves vertically so bounding-box `height > width` while non-doubles have `width > height`. `scripts/e2e-browser.mjs` verifies bounding-box dimensions on every board tile.
- **Rematches (`PlayAgain`) & socket lifecycle (`src/session.rs`)**:
  - Do **not** cycle the WebSocket for "Play again". Call `Api::play_again()`, which sends `ClientToServer::PlayAgain` on the existing socket; the server re-deals in place once all 4 seats mark `ready: true`.
  - When joining a new room or leaving, `previous.leave()` must close the old socket before opening a new one, and `session::run` must exit (`Outcome::Stop`) when the command channel yields `None` (dropped `Session`) so stale WASM sockets never hold seats hostage on the server.
- **WebSocket URL resolution (`src/roomcode.rs::guess_ws_url`)**: Uses same-origin `ws(s)://<host>/ws` on ports `""`, `80`, or `443` (production reverse proxy), and `ws(s)://<host>:8080/ws` on any other port (matching `trunk serve` on `:8081` talking to the server on `:8080`, including over LAN). Overridable via `localStorage['fastplay.ws_url']`.
- **Insecure-context fallbacks (`src/view/share.rs`, `src/state.rs`)**: `navigator.clipboard` and `navigator.share` are undefined over plain HTTP on a LAN IP; always go through `js_sys::Reflect::get` feature detection in `share.rs` (calling `web_sys::Navigator::clipboard()` directly panics when undefined). Similarly, `state::initial()` touches `LocalStorage` and `window.location`, so host `cargo test` tests must construct `AppState` directly rather than calling `initial()`.
