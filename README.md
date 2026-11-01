# fastPlay Client

A browser/PWA client for the **fastPlay 4‑player dominoes server**.  
Built with **Yew** (Rust) and **Trunk**, it runs as a single‑page app and can be installed on a phone as a progressive web app.

---

## Table of Contents

1. [Overview](#overview)
2. [Prerequisites](#prerequisites)
3. [Development (local setup)](#development-local-setup)
4. [Running the server](#running-the-server)
5. [Running the client](#running-the-client)
6. [Building for production](#building-for-production)
7. [Project structure](#project-structure)
8. [License](#license)

---

## Overview

- **Purpose**: Connects to a fastPlay dominoes server via WebSocket, provides a home screen, waiting room, and game board.
- **Tech stack**: Yew (Rust framework compiling to WASM), Trunk (dev server & build), gloo-net (WebSocket), serde (wire format), web-sys / js-sys (DOM/Web APIs).
- **Deployment**: `trunk build --release` → static files served by a reverse proxy (Caddy/NGINX) that also proxies `/ws` to the Rust server.

---

## Prerequisites

| Tool | Minimum version | Install command |
|------|-----------------|-----------------|
| **Rust** (including `cargo`) | 1.60+ (tested on 1.75) | `rustup update` |
| **Node.js** (for Trunk) | 14+ (18+ recommended) | `curl -fsSL https://deb.nodesource.com/setup_20.x \| sudo -E bash - && sudo apt-get install -y nodejs` (or use `nvm`, `fnm`, etc.) |
| **Trunk** | latest | `cargo install trunk` |
| **Optional**: A Caddy/NGINX reverse proxy for production | — | — |

---

## Development (local setup)

1. **Clone the repository** (or cd into the existing `client/` directory):

   ```bash
   cd /home/rngkll/git/fastPlay/client
   ```

2. **Install Rust dependencies**:

   ```bash
   cargo build       # first build (downloads crates, compiles)
   # or
   cargo run --release  # build + run if you also have a server binary
   ```

3. **Install Node.js dependencies** (Trunk uses some tiny JS deps):

   ```bash
   npm install -g trunk   # if you used `cargo install` you may skip this
   # or simply ensure `trunk` is on your PATH from cargo
   ```

4. **Start the development server** (see *Running the client* below).

---

## Running the server

The dominoes server listens on **`ws://0.0.0.0:8080/ws`** by default (see the server’s own configuration). Make sure the server is running before you start the client.

Typical start command (from the repository root, `server/` directory):

```bash
cargo run --bin fastplay-server   # or whatever the server binary is named
```

If you are using the provided server binary, check its `Cargo.toml` for the exact name.

> **Note**: The server expects the client to connect to that exact WS endpoint. The client’s auto‑detect logic (see *WebSocket URL* below) will produce that URL in the normal dev setup.

---

## Running the client

The client is meant to be served by **Trunk**, which also sets up the correct WebSocket URL automatically.

```bash
# From the client/ directory
trunk serve
```

### What happens

| Step | Detail |
|------|--------|
| **Port** | Trunk listens on `0.0.0.0:8081` (configured in `Trunk.toml`). |
| **WebSocket URL** | The function `roomcode::guess_ws_url()` examines the page’s origin. Since the page runs on port `8081`, it automatically constructs `ws://<host>:8080/ws` – exactly the server’s default. |
| **Browser** | Open `http://localhost:8081` (or `http://<LAN-IP>:8081` if you’re on a local network). |
| **First run** | Trunk compiles the WASM binary (cached after the first build) and serves an HTML page. The app shows a name input, “New game” / “Join” actions, and a share link. |

### Optional: Custom WS URL

If you need to point the client at a different WebSocket host (e.g., a remote server), you can:

1. **Via the UI** – On the home screen there is a “WS URL” text field. Edit it and the value is stored in `localStorage` under the key `fastplay.ws_url`.
2. **Manually** – Set the key from the browser console:

   ```javascript
   localStorage.setItem('fastplay.ws_url', 'ws://your‑host:8080/ws')
   ```

3. **Source edit** – Edit `src/roomcode.rs::guess_ws_url()` and rebuild.

---

## Building for production

```bash
trunk build --release
```

- Output goes to `client/dist/` (HTML, CSS, JS, and the WASM binary).
- The `Trunk.toml` `[pwa]` section generates a web‑app manifest and a service worker (`sw.js`) with your chosen name, icons, and colors.

### Serve the built files

A simple static‑file server (Caddy, NGINX, `python -m http.server`, etc.) can serve `dist/`.  
Configure your reverse proxy to forward `/ws` to the running fastPlay server.

Example Caddyfile snippet:

```
:80 {
    root * /home/rngkll/git/fastPlay/client/dist
    encode gzip
    tls /path/to/cert.pem   # optional

    # Proxy WebSocket upgrades to the game server
    fastplay ws://127.0.0.1:8080 {
        header_upstream -Origin *
    }
}
```

(Adjust the proxy syntax to match your chosen reverse‑proxy tool.)

---

## Project structure

```
client/
├─ Cargo.toml            # Rust dependencies (yew, gloo, serde, wasm-bindgen, …)
├─ Trunk.toml            # Build + PWA config, dev server port 8081
├─ index.html            # Shell HTML (generated by Trunk)
├─ styles.css            # Global CSS (mobile‑first, dark‑green table theme)
├─ manifest/             # PWA manifest and service‑worker template
├─ icons/                # PNG icons (192×192, 512×512)
├─ src/
│  ├─ main.rs            # Entry point – mounts <App />
│  ├─ protocol.rs        # Wire types (mirrors server protocol)
│  ├─ session.rs         # WebSocket lifecycle: connect, join, send cmds, reconnect
│  ├─ state.rs           # AppState + reducer (RoomState, HandDealt, GameUpdate, …)
│  ├─ roomcode.rs        # Room‑code generation, share‑link handling, guess_ws_url()
│  ├─ legality.rs        # Compute playable tiles from hand + board state
│  ├─ view/
│  │  ├─ mod.rs
│  │  ├─ home.rs         # Name field, new‑room / join UI, WS URL input
│  │  ├─ room.rs         # Waiting‑room UI with share link & copy button
│  │  ├─ game.rs         # Main game board, hand, tile selection, pass button
│  │  ├─ board.rs        # Horizontal scroll strip of the domino chain
│  │  ├─ hand.rs         # Domino graphics along the bottom
│  │  └─ domino.rs        # CSS‑based domino tile rendering
│  └─ … (other helpers)
└─ README.md             # (this file)
```

Key files explained:

- **`session.rs`** – Owns a single WebSocket, re‑joins the room on every connect/reconnect. Handles back‑off reconnection and forwards server messages to the Yew reducer.
- **`state.rs`** – Holds the app state (name, room code, WS URL, room state, hand, turn, etc.) and uses a reducer (`use_reducer`) to handle `Msg` events from the socket.
- **`roomcode.rs`** – `guess_ws_url()` auto‑detects the correct WS endpoint based on the page’s port. `generate()` creates short alphanumeric room codes. `share_link()` builds a URL like `http://host/?room=ABCDE`.
- **`view/home.rs`** – UI for the home screen: name input (persisted in `localStorage`), “New game” (generates a code and opens WS), “Join” (enters a code), and an optional manual WS‑URL override.
- **`view/room.rs`** – Waiting‑room UI showing the 4 seats, share‑link copy button, and “waiting for players” hint.
- **`view/game.rs`** – Game board, hand display, tile‑play logic, pass button, turn indicator, result banner.

---

## License

This project is open source. See the root `LICENSE` (or `server/LICENSE`) for full terms. Typically it’s either MIT or Apache‑2.0 – check the repository.

---

## Acknowledgments

- Inspired by classic 4‑player dominoes games.
- Uses the excellent **Yew** framework and **Trunk** for the dev/PWA workflow.
- WebSocket wire format mirrors the server’s `protocol.rs` (same `serde` structs, same tag names).

---

**Happy domino‑playing!** 🎲🚀