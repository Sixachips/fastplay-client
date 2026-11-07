// Browser smoke test for the fastPlay client (run with node ≥ 21 and chromium).
//
// Drives the REAL compiled WASM app in headless chromium over the DevTools
// protocol while three bot seats play the rest of the table on the raw
// WebSocket. It asserts the things a wire-level test cannot see:
//   • the home screen's partnership picker seats the player on the requested
//     team, in both the new-game flow and the join flow
//   • tapping a hand tile only PICKS it up (nothing is played yet)
//   • a picked tile raises the placement bar with one button per fitting end
//   • confirming grows the board by 1 and shrinks the hand by 1
//   • Cancel puts the tile back without playing it
//   • the hand stays in step with the server for the whole game
//   • Pass is reachable exactly when nothing fits
//   • a double lies across the chain while every other tile keeps its length
//     with it — class and the box it was drawn into, tile by tile
//
//   cd server && cargo run                 # ws://127.0.0.1:8080/ws
//   cd client && trunk build               # writes client/dist
//   node client/scripts/e2e-browser.mjs
//
// Set FASTPLAY_SHOT=/path.png to keep a screenshot of the finished table.
//
// Exit code 0 means a full game was played through the UI with no rejection on
// any seat and all 28 tiles accounted for.

import { spawn } from 'node:child_process';
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const CLIENT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const ROOT = process.env.FASTPLAY_DIST || path.join(CLIENT, 'dist');
const WS = process.env.FASTPLAY_WS || 'ws://127.0.0.1:8080/ws';
const PORT = Number(process.env.FASTPLAY_PORT || 8081);
const CHROME = process.env.FASTPLAY_CHROME || 'chromium';
const ROOM = 'cdp' + Math.random().toString(36).slice(2, 6);

const fits = (t, e) => t.left === e || t.right === e;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---------- static server for the built client ----------
const TYPES = {
  '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm',
  '.css': 'text/css', '.png': 'image/png', '.webmanifest': 'application/manifest+json',
};
if (!fs.existsSync(path.join(ROOT, 'index.html'))) {
  throw new Error(`no index.html in ${ROOT} — run \`trunk build\` in client/ first`);
}
const server = http.createServer((req, res) => {
  const file = req.url.split('?')[0];
  const p = path.join(ROOT, file === '/' ? 'index.html' : decodeURIComponent(file));
  fs.readFile(p, (err, buf) => {
    if (err) { res.writeHead(404); res.end('not found'); return; }
    // no-store: a cached index.html would point the browser at a previous
    // build's wasm hash, and the test would then drive bytes it did not build.
    res.writeHead(200, { 'content-type': TYPES[path.extname(p)] || 'application/octet-stream', 'cache-control': 'no-store' });
    res.end(buf);
  });
});
await new Promise((r) => server.listen(PORT, r));

// ---------- three bot seats on the raw wire protocol ----------
function bot(name, team = null) {
  const ws = new WebSocket(WS);
  // `team` is a partnership preference (0/1) that this bot requests on join;
  // null joins with no preference.
  const join = () => ws.send(JSON.stringify({
    type: 'JoinRoom', room: ROOM, name, ...(team !== null ? { team } : {}),
  }));
  const b = { name, hand: [], you: null, team: null, game: null, closed: null, rejects: [], join };
  ws.onopen = join;
  ws.onmessage = (ev) => {
    const m = JSON.parse(ev.data);
    if (m.type === 'RoomState') {
      b.you = m.room.you;
      b.roster = m.room.seats;
      b.team = m.room.seats.find((s) => s.seat === b.you)?.team ?? null;
    }
    if (m.type === 'HandDealt') b.hand = m.hand;
    if (m.type === 'GameStateUpdate') { b.game = m.state; playFor(b, ws); }
    if (m.type === 'GameClosed') b.closed = m.hands;
    if (m.type === 'MoveRejected') b.rejects.push(m.reason);
  };
  b.ws = ws;
  return b;
}

// Plays this seat's first legal placement, or passes when nothing fits.
function playFor(b, ws) {
  const g = b.game;
  if (!g || g.result || g.turn !== b.you) return;
  if (!g.board.length) {
    // First hand of a session: only the forced 6|6 may open; later hands
    // accept any tile.
    const forced = g.opening_tile;
    const tile = forced
      ? b.hand.find((t) => t.left === forced.left && t.right === forced.right)
      : b.hand[0];
    place(b, ws, tile, 'right');
    return;
  }
  for (const tile of b.hand) {
    if (g.right_end !== null && fits(tile, g.right_end)) { place(b, ws, tile, 'right'); return; }
    if (g.left_end !== null && fits(tile, g.left_end)) { place(b, ws, tile, 'left'); return; }
  }
  ws.send(JSON.stringify({ type: 'PassTurn' }));
}

function place(b, ws, tile, side) {
  ws.send(JSON.stringify({ type: 'PlayTile', tile, side }));
  const i = b.hand.indexOf(tile);
  if (i >= 0) b.hand.splice(i, 1);
}

const PLAYERS = ['Ada', 'Bob', 'Cleo'];
// Bot seats are connected lazily: they join only once the browser seat has
// claimed its own, so the table cannot fill (and deal) before the picker's
// effect is observable on the room screen.
let bots = [];

// ---------- chromium over CDP ----------
// A fresh port per run: attaching to a leftover headless instance from an
// earlier (possibly different-build) session would test the wrong bytes.
const debugPort = Number(process.env.FASTPLAY_CDP_PORT || 9300 + Math.floor(Math.random() * 200));
const chrome = spawn(CHROME, [
  '--headless=new', `--remote-debugging-port=${debugPort}`, '--no-sandbox',
  '--disable-gpu', '--disable-dev-shm-usage',
  `--user-data-dir=${path.join(CLIENT, 'target', 'e2e-browser-profile-' + debugPort)}`,
  'about:blank',
], { stdio: 'ignore' });

const finish = async (code, message) => {
  try { cdp.close(); } catch {}
  try { for (const b of bots) b.ws.close(); } catch {}
  chrome.kill();
  server.close();
  if (message) (code ? console.error : console.log)(message);
  process.exit(code);
};
// A thrown assertion must still tear the browser down, or the next run finds
// this corpse on the same port and plays the wrong build.
process.on('unhandledRejection', (err) => finish(1, 'E2E BROWSER FAIL: ' + (err?.message || err)));
process.on('uncaughtException', (err) => finish(1, 'E2E BROWSER FAIL: ' + (err?.message || err)));
process.on('exit', () => { try { chrome.kill(); } catch {} });

let target = null;
for (let i = 0; i < 80 && !target; i++) {
  await sleep(250);
  try {
    const list = await (await fetch(`http://127.0.0.1:${debugPort}/json/list`)).json();
    target = list.find((t) => t.type === 'page');
  } catch { /* chromium is still booting */ }
}
if (!target) throw new Error('chromium never exposed a CDP target');

const cdp = new WebSocket(target.webSocketDebuggerUrl);
const pending = new Map();
const consoleLog = [];
cdp.onmessage = (ev) => {
  const m = JSON.parse(ev.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
  if (m.method === 'Runtime.consoleAPICalled') {
    consoleLog.push('console: ' + m.params.args.map((a) => a.value).join(' '));
  }
  if (m.method === 'Runtime.exceptionThrown') {
    consoleLog.push('EXCEPTION: ' + (m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text));
  }
};
await new Promise((r) => (cdp.onopen = r));

let nextId = 0;
const call = (method, params = {}) =>
  new Promise((res) => { const id = ++nextId; pending.set(id, res); cdp.send(JSON.stringify({ id, method, params })); });

const evaluate = async (expression) => {
  const r = await call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) throw new Error('page eval: ' + JSON.stringify(r.result.exceptionDetails));
  return r.result?.result?.value;
};

const waitFor = async (expression, ms, what) => {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if (await evaluate(expression)) return true;
    await sleep(120);
  }
  throw new Error(`timeout waiting for ${what}\nrecent page log:\n  ${consoleLog.slice(-6).join('\n  ')}`);
};

await call('Runtime.enable');
await call('Page.enable');

// Type into the home form like a finger would. The DOM may catch up to the
// click a frame later, so fall back to polling before judging a tap.
const selectByClick = async (label) => {
  const deadline = Date.now() + 3000;
  while (Date.now() < deadline) {
    const cls = await evaluate(`(() => {
      const b = [...document.querySelectorAll('.team-picker .btn')]
        .find((x) => x.textContent.trim() === ${JSON.stringify(label)});
      if (!b) return null;
      b.click();
      return b.className;
    })()`);
    if (cls?.includes('picked')) return cls;
    await sleep(120);
  }
  throw new Error('tapping a team did not mark it picked: ' + label);
};

// Phase 1 — new-game flow: create the room from the home screen and check the
// picker's choice reached the seat claim.
await call('Page.navigate', { url: `http://127.0.0.1:${PORT}/` });
await waitFor(`!!document.querySelector('.home input')`, 25000, 'the home form');
await evaluate(`(() => {
  const set = (el, v) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, v);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  };
  set(document.querySelectorAll('.home input')[0], 'Rita');
  return true;
})()`);
const northSouth = await selectByClick('North–South');
if (!northSouth.includes('picked')) throw new Error('tapping a team did not mark it picked');
await evaluate(`(() => {
  [...document.querySelectorAll('.home .form-buttons .btn')]
    .find((b) => b.textContent.includes('New game')).click();
  return true;
})()`);
await waitFor(`!!document.querySelector('.seat.you .seat-team')`, 25000, 'the waiting room');
{
  const you = await evaluate(
    `(() => {
      const seat = document.querySelector('.seat.you');
      return { team: seat.querySelector('.seat-team')?.textContent, cls: seat.className, num: seat.querySelector('.seat-num')?.textContent };
    })()`);
  if (!you.team || !you.team.includes('North–South')) throw new Error('the new-game join ignored the North–South preference: ' + JSON.stringify(you));
  if (!you.cls.includes('team-north-south')) throw new Error('the room seat missed its team class: ' + you.cls);
  console.log('OK: new game — the creator sat on', you.team, '(seat', you.num + ')');
}
await evaluate(`(() => { document.querySelector('.leave')?.click(); return true; })()`);
await sleep(300);

// Phase 2 — join flow: arrive via the shared link, pick West–East, join first,
// then fill the table with bot seats (a full team never refuses a join; the
// room screen must show the team that actually seated us).
await call('Page.navigate', { url: `http://127.0.0.1:${PORT}/?room=${ROOM}` });
await waitFor(`!!document.querySelector('.home input')`, 25000, 'the home form');
await evaluate(`(() => {
  const set = (el, v) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, v);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  };
  set(document.querySelectorAll('.home input')[0], 'Rita');
  return true;
})()`);
{
  const westEast = await selectByClick('West–East');
  if (!westEast.includes('picked')) throw new Error('tapping a team did not mark it picked');
}
await evaluate(`(() => {
  [...document.querySelectorAll('.home .form-buttons .btn')]
    .find((b) => b.textContent.includes('Join table')).click();
  return true;
})()`);
await waitFor(`!!document.querySelector('.seat.you .seat-team')`, 25000, 'the waiting room');
{
  const you = await evaluate(
    `(() => {
      const seat = document.querySelector('.seat.you');
      return { team: seat.querySelector('.seat-team')?.textContent, cls: seat.className, num: seat.querySelector('.seat-num')?.textContent };
    })()`);
  if (!you.team || !you.team.includes('West–East')) {
    throw new Error('the preference join ignored the West–East preference: ' + JSON.stringify(you));
  }
  if (!you.cls.includes('team-east-west')) throw new Error('the room seat missed its team class: ' + you.cls);
  console.log('OK: join flow — the picker sat me on West–East (seat', you.num + ')');
}

// Fill the table; the fourth join deals hands automatically.
bots = PLAYERS.map((name) => bot(name));
await waitFor(`!!document.querySelector('.hand .hand-tile')`, 25000, 'a dealt hand in the browser');

// The seat the picker produced is on the wire too: every bot roster lists
// Rita at an odd (West–East) seat with team 1, and every seat's team is its
// seat parity.
{
  const deadline = Date.now() + 8000;
  while (true) {
    const rosters = bots.filter((b) => Array.isArray(b.roster) && b.roster.length === 4);
    if (rosters.length === bots.length) {
      for (const b of rosters) {
        const rita = b.roster.find((s) => s.name === 'Rita');
        if (!rita) throw new Error(b.name + "'s roster lost Rita");
        if (rita.team !== 1 || rita.seat % 2 !== 1) {
          throw new Error('Rita was seated at seat ' + rita.seat + ' with team ' + rita.team +
            ', not the requested West–East partnership');
        }
        for (const s of b.roster) {
          if (s.team !== s.seat % 2) throw new Error('seat ' + s.seat + ' reports team ' + s.team);
        }
      }
      console.log('OK: the wire confirms Rita joined West–East (seat ' +
        rosters[0].roster.find((s) => s.name === 'Rita').seat + ')');
      break;
    }
    if (Date.now() > deadline) throw new Error('timeout waiting for full rosters at every bot');
    await sleep(120);
  }
}

// Everything the test can observe about the game screen, in one round-trip.
const readUi = () => evaluate(`(() => ({
  tiles: document.querySelectorAll('.hand-tile').length,
  playable: document.querySelectorAll('.hand-tile.playable').length,
  picked: document.querySelectorAll('.hand-tile.picked').length,
  bar: !!document.querySelector('.place-bar'),
  barEnds: document.querySelectorAll('.place-end').length,
  openTable: [...document.querySelectorAll('.place-row .btn')].some((b) => b.textContent.includes('Open the table')),
  board: document.querySelectorAll('.chain .domino').length,
  status: document.querySelector('.status')?.textContent,
  over: !!document.querySelector('.banner'),
  reveal: !!document.querySelector('.reveal'),
  revealRows: document.querySelectorAll('.reveal-row').length,
  passEnabled: (() => { const p = document.querySelector('.actions .btn:not(.btn-ghost)'); return p ? !p.disabled : null; })(),
  toast: document.querySelector('.toast')?.textContent || null,
  toastWarn: !!document.querySelector('.toast-warn'),
}))()`);

let picks = 0, plays = 0, passes = 0, nudges = 0;
let cancelTested = false;
const toasts = [];

// How the page actually drew the chain: which tiles were stood up, and the box
// each one ended with.
const readChain = () => evaluate(`(() =>
  [...document.querySelectorAll('.chain .domino')].map((el) => {
    const r = el.getBoundingClientRect();
    return { cross: el.classList.contains('domino-cross'), w: Math.round(r.width), h: Math.round(r.height) };
  })
)()`);

// A double lies across the chain; every other tile keeps its length with it.
// The bot seats hold the same snapshot the page rendered, so once both show the
// same number of tiles they have to agree tile by tile — a chain that is still
// catching up with the wire is skipped, not failed.
let sawDouble = false;
const chainProblems = new Set();
async function checkChain() {
  const board = bots.map((b) => b.game).filter(Boolean)
    .sort((a, b) => b.board.length - a.board.length)[0]?.board;
  if (!board?.length) return;
  const drawn = await readChain();
  if (drawn.length !== board.length) return;
  board.forEach((tile, i) => {
    const cross = tile.left === tile.right;
    if (cross) sawDouble = true;
    const spot = `tile ${i} (${tile.left}|${tile.right})`;
    if (drawn[i].cross !== cross) {
      chainProblems.add(`${spot} drawn ${drawn[i].cross ? 'crosswise' : 'lengthwise'}`);
    } else if (cross ? drawn[i].h <= drawn[i].w : drawn[i].w <= drawn[i].h) {
      chainProblems.add(`${spot} drawn ${drawn[i].cross ? 'crosswise' : 'lengthwise'} but its box is ${drawn[i].w}×${drawn[i].h}`);
    }
  });
}
const deadline = Date.now() + 150000;

while (Date.now() < deadline) {
  const ui = await readUi();
  await checkChain();
  if (ui.toastWarn) toasts.push(ui.toast);
  if (ui.over) break;

  const mine = ui.status === 'Your turn — pick a tile' || ui.status === 'Now choose the end';

  if (mine) {
    // Put a picked tile back: the bar's Cancel must not play anything.
    if (!cancelTested && ui.picked === 1) {
      const board = ui.board;
      await evaluate(`(() => {
        [...document.querySelectorAll('.place-bar .btn')].find((b) => b.textContent.includes('Cancel')).click();
        return true;
      })()`);
      await sleep(300);
      const after = await readUi();
      if (after.board !== board) throw new Error('Cancel played the tile anyway');
      if (after.picked !== 0) throw new Error('Cancel left the tile picked');
      cancelTested = true;
      continue;
    }

    if (ui.picked === 0) {
      if (ui.playable === 0) {
        throw new Error(`my turn, ${ui.tiles} tiles in hand, none playable: ${JSON.stringify(ui)}`);
      }
      await evaluate(`(() => { document.querySelector('.hand-tile.playable').click(); return true; })()`);
      await sleep(250);
      const mid = await readUi();
      if (mid.picked === 1) {
        picks++;
        if (mid.board !== ui.board) throw new Error('picking a tile already played it');
        if (!mid.bar) throw new Error('a picked tile showed no placement bar');
        if (mid.barEnds === 0 && !mid.openTable) throw new Error('the placement bar had no way to place');
      } else if (mid.toastWarn) {
        nudges++; // a tile that fits nowhere explains itself instead of dying silently
      } else {
        throw new Error('the tap did not pick the tile: ' + JSON.stringify(mid));
      }
      continue;
    }

    // Confirm the pick and check the two things that must happen together.
    await evaluate(`(() => {
      const b = document.querySelector('.place-end')
        || [...document.querySelectorAll('.place-row .btn')].find((x) => x.textContent.includes('Open the table'));
      b.click();
      return true;
    })()`);
    await sleep(400);
    const after = await readUi();
    // The board may have grown by more than one: the bot seats play the moment
    // their turn lands, and they are not waiting on this assertion.
    const note = () => `bar=${JSON.stringify(ui)}\n  after=${JSON.stringify(after)}\n  page log:\n  ${consoleLog.slice(-8).join('\n  ')}`;
    if (after.board <= ui.board) throw new Error(`board did not grow after the play (${ui.board} → ${after.board})\n  ${note()}`);
    if (after.tiles !== ui.tiles - 1) throw new Error(`hand went ${ui.tiles} → ${after.tiles}, expected −1\n  ${note()}`);
    if (after.picked !== 0 || after.bar) throw new Error('the pick survived the play');
    plays++;
    continue;
  }

  if (ui.status === 'Nothing fits — pass') {
    if (ui.passEnabled !== true) throw new Error('the table says pass but the Pass button is disabled');
    await evaluate(`(() => { document.querySelector('.actions .btn:not(.btn-ghost)').click(); return true; })()`);
    await sleep(400);
    passes++;
    continue;
  }

  await sleep(200);
}

// The banner lands with the terminating snapshot; the closed reveal follows
// on its own message a beat later, so let the flow settle before reading.
await sleep(600);

const ui = await readUi();
await checkChain();
const botTiles = bots.reduce((n, b) => n + b.hand.length, 0);
const accounted = ui.board + botTiles + ui.tiles;

// An optional PNG of the finished table, for eyeballing how it was drawn.
if (process.env.FASTPLAY_SHOT) {
  const shot = await call('Page.captureScreenshot', { format: 'png' });
  fs.writeFileSync(process.env.FASTPLAY_SHOT, Buffer.from(shot.result?.data || '', 'base64'));
  console.log('screenshot:', process.env.FASTPLAY_SHOT);
}

console.log('room:', ROOM);
console.log('browser seat — picks:', picks, 'plays:', plays, 'passes:', passes, 'unplayable taps:', nudges, 'cancel tested:', cancelTested);
console.log('final screen:', JSON.stringify(ui));
for (const b of bots) console.log(`  ${b.name} (seat ${b.you}): hand ${b.hand.length}, rejections ${JSON.stringify(b.rejects)}`);
console.log(`tile conservation: board ${ui.board} + bots ${botTiles} + browser ${ui.tiles} = ${accounted}`);
if (toasts.length) console.log('toasts seen:', toasts.join(' | '));
console.log(sawDouble
  ? 'OK: every double on the table lay across the chain, other tiles in line with it'
  : 'note: no double reached the table this game, so the crosswise rule went unexercised');

const failures = [];
if (!ui.over) failures.push('the game never reached its end');
if (accounted !== 28) failures.push(`tiles vanished or duplicated (expected 28, got ${accounted})`);
for (const b of bots) if (b.rejects.length) failures.push(`${b.name} was rejected: ${b.rejects.join('; ')}`);
if (toasts.length) failures.push(`the browser seat got ${toasts.length} rejection toast(s)`);
if (!plays) failures.push('no tile was ever played from the browser');
if (chainProblems.size) failures.push('the chain drew a tile the wrong way: ' + [...chainProblems].join('; '));

// A blocked game must notify and reveal on both sides of the wire; a game
// won by playing out must not.
const key = (t) => [t.left, t.right].sort((a, b) => a - b).join('-');
const blocked = bots.some((b) => b.game?.result?.reason === 'blocked');
if (blocked) {
  if (!ui.reveal) failures.push('the closed game showed no reveal panel in the UI');
  for (const b of bots) {
    if (!b.closed) { failures.push(`${b.name} never received the GameClosed reveal`); continue; }
    const mine = b.closed.find((h) => h.seat === b.you);
    const shown = (mine?.hand || []).map(key).sort().join(',');
    const held = b.hand.map(key).sort().join(',');
    if (shown !== held) failures.push(`${b.name}'s revealed hand differs from its own tiles`);
  }
  if (ui.reveal && ui.revealRows !== 4) failures.push(`the reveal panel showed ${ui.revealRows} rows, expected one per seat`);
  console.log('closed game: reveal panel', ui.reveal, 'rows', ui.revealRows);
} else if (bots.some((b) => b.closed)) {
  failures.push('a played-out game revealed hands it should have kept hidden');
}

// Rematch through the real UI: three bots tap PlayAgain on the wire and the
// browser seat clicks its banner button — the table must redeal in place.
if (ui.over) {
  try {
    await evaluate(`(() => {
      const b = [...document.querySelectorAll('.banner-buttons .btn')].find((x) => x.textContent.includes('Play again'));
      if (!b) throw new Error('no Play again button in the banner');
      b.click();
      return true;
    })()`);
    for (const b of bots) b.ws.send(JSON.stringify({ type: 'PlayAgain' }));
    await waitFor(
      `!document.querySelector('.banner') && document.querySelectorAll('.hand-tile').length === 7`,
      15000,
      'a fresh 7-tile hand after the table rematched'
    );
    console.log('OK: rematch — four PlayAgain taps re-dealt in place, browser seat has a fresh hand');
  } catch (err) {
    failures.push('rematch failed: ' + (err?.message || err));
  }
}

cdp.close();
chrome.kill();
server.close();
for (const b of bots) b.ws.close();

if (failures.length) {
  console.error('\nE2E BROWSER FAIL:\n  ' + failures.join('\n  '));
  if (consoleLog.length) console.error('page log:\n  ' + consoleLog.slice(-12).join('\n  '));
  process.exit(1);
}
console.log('\nE2E BROWSER PASS — a full game played through the UI, hand kept in sync, zero rejections');
process.exit(0);
