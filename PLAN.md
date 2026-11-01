# fastPlay client — plan

Rust→WASM single-page app that plays the server's 4-player dominoes over
WebSocket. Works in any browser and installs on a phone as a PWA
("Add to Home Screen" → standalone fullscreen app).

## Fixed constraints (server is NOT modified)

* Single endpoint: `ws://host:8080/ws`. Only JSON text frames.
* Inbound (`ClientToServer`, `{"type": ...}` tagged):
  - `JoinRoom { room, name }` — room code ≤16 chars, name ≤32; one seat per connection, no rejoin on the same socket.
  - `PlayTile { tile: {left,right}, side: "left"|"right" }` — first tile sets both ends, side ignored.
  - `PassTurn`
* Outbound (`ServerToClient`):
  - `RoomState { room: { room_id, you, phase, seats } }` — personalized, broadcast on membership changes. `phase`: `waiting | playing | finished`.
  - `HandDealt { seat, hand }` — private, only to each seat, sent when the 4th player joins (game auto-deals).
  - `GameStateUpdate { state }` — full snapshot after every accepted move: `turn, board, left_end, right_end, hand_counts, consecutive_passes, result`. Opponent hands are hidden (counts only).
  - `MoveRejected { reason }`
  - `OpponentDisconnected { player, seat }`
* The server serves **no static files** → the client is built and served
  separately (Trunk dev server locally; static files + reverse proxy in prod).
* Disconnect = seat freed and any running game voided. Reconnect means joining
  as a new seat; the game restarts when the table refills.

## Stack

* **Yew** (function components + `use_reducer`) + **Trunk** build.
* **gloo-net** WebSocket (or web-sys) + serde for the wire types.
* `client/src/protocol.rs` mirrors `server/src/protocol.rs` (serde structs —
  the tags must match exactly).
* **PWA via Trunk's built-in `[pwa]` section** → generates manifest + `sw.js`
  with one config block. One icon + colors.
* Single CSS file (plain or scss), mobile-first: big touch targets, portrait
  layout, dark green table look.

## UX / screens (one SPA, state machine — no router needed)

1. **Home** — name field (remembered in localStorage) + two actions:
   * *New game* → generate a short room code (e.g. `fast-7f3a`), open WS, join.
   * *Join* → room code field. Shared links open as `https://host/?room=CODE`
     → auto-fills code, asks only for the name.
2. **Room (waiting)** — the 4 seats with names / empty slots, the **share link**
   with a copy button (and Web Share API on phone), "waiting for players" hint.
   Auto-advances when `HandDealt` + `GameStateUpdate` arrive.
3. **Game** —
   * Your hand along the bottom as real domino graphics (CSS pips).
   * Board = horizontal scroll strip of the chain, ends labeled ◀ ▶.
   * Playable tiles are highlighted (client computes legality from
     `left_end`/`right_end` + hand; server remains the authority).
   * Tap tile → tap an end (or one tap if it fits only one end). First move:
     pick any tile, no side choice needed.
   * **Pass** button enabled only when it's your turn and nothing is playable.
   * Top bar: turn indicator (whose turn + opponent tile counts), 4-seat badges.
   * End of game: result banner — winner name, draw, blocked, or abandoned.
   * `MoveRejected` → toast with the reason; `OpponentDisconnected` → notice.
4. **Disconnected** — socket closed → auto-reconnect with backoff, then
   `JoinRoom` again; visible "reconnecting…" state.

## Client architecture

```
client/
  Trunk.toml          # build + [pwa] config, dev server
  index.html          # shell, WS target note for dev
  manifest + icons/
  styles.css
  Cargo.toml          # yew, gloo, serde, web-sys, js-sys
  src/
    main.rs           # mount <App/>
    protocol.rs       # wire types (mirror of server protocol)
    session.rs        # WS lifecycle: connect, join, send cmds, incoming events
    state.rs          # AppState + reducer (RoomState/HandDealt/GameUpdate/…)
    roomcode.rs       # generate + parse share links (?room=CODE)
    legality.rs       # playable-tile computation for the hand
    view/
      mod.rs, home.rs, room.rs, game.rs, board.rs, hand.rs, domino.rs
```

* One `WebSocket` owned by a task; parsed `ServerToClient` events are sent to
  the Yew reducer via a channel/callback. Commands are plain serialized JSON.
* WS URL: same-host `wss://` in prod (reverse-proxied `/ws`);
  `ws://<host>:8080/ws` in dev, configurable so a phone on the LAN can test it.

## Dev / deploy workflow

* Dev: `cargo run` in `server/` (port 8080) + `trunk serve` in `client/`
  (port 8081, WS pointed at 8080). Phone: browse to LAN IP:8081.
* Prod: `trunk build --release` → serve `client/dist` statically; reverse proxy
  (caddy/nginx) sends `/ws` to the Rust server, everything else to static files.

## Milestones

1. Scaffold: trunk app boots as a PWA, WS connects, `JoinRoom` round-trip
   (roster shows up in debug view).
2. Home + share link + waiting room.
3. Board + hand rendering, tile selection, play/pass, live updates.
4. Reconnect + rejection toasts + result banner + mobile polish.
