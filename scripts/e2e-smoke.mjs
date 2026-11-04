// Headless smoke test for the fastPlay wire protocol (run with node ≥ 21).
// Joins 4 players, plays a full game to the end, runs the in-place
// PlayAgain rematch, then checks the disconnect / rejoin / redeal flows —
// the same paths the WASM client uses.
//
//   cd server && cargo run              # terminal 1
//   node client/scripts/e2e-smoke.mjs   # terminal 2

const WS = process.env.FASTPLAY_WS || 'ws://127.0.0.1:8080/ws';

function connect(name) {
  const ws = new WebSocket(WS);
  const p = {
    ws, name,
    roomState: null,
    hand: [],
    game: null,
    closed: null,
    rejections: [],
    open: new Promise((r) => (ws.onopen = r)),
  };
  ws.onmessage = (ev) => {
    const m = JSON.parse(ev.data);
    if (m.type === 'RoomState') p.roomState = m.room;
    if (m.type === 'HandDealt') p.hand = m.hand;
    if (m.type === 'GameStateUpdate') p.game = m.state;
    if (m.type === 'GameClosed') p.closed = m.hands;
    if (m.type === 'MoveRejected') p.rejections.push(m.reason);
  };
  p.send = (o) => ws.send(JSON.stringify(o));
  return p;
}

// Wait until pred(p) holds for every listed player, or throw after ms.
async function waitAll(ps, pred, ms, what) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if (ps.every(pred)) return;
    await new Promise((r) => setTimeout(r, 20));
  }
  throw new Error('timeout waiting for ' + what);
}

const fits = (tile, end) => tile.left === end || tile.right === end;

const room = 'smoke' + Math.random().toString(36).slice(2, 7);
const players = ['Ada', 'Bob', 'Cleo', 'Dan'].map(connect);
await Promise.all(players.map((p) => p.open));
for (const p of players) p.send({ type: 'JoinRoom', room, name: p.name });

// Let the whole deal settle before commanding anything.
await waitAll(players, (p) => p.hand.length === 7 && p.game !== null, 8000, 'initial deal');
console.log('OK: four private hands of 7 tiles dealt');

// First hand of the session (rules.md): the 6|6 holder opens, and the 6|6 is
// the only tile that may start the layout.
const opening = players[0].game;
if (opening.opening_tile?.left !== 6 || opening.opening_tile?.right !== 6) {
  throw new Error('first hand must force the 6|6, got ' + JSON.stringify(opening.opening_tile));
}
const opener = players.find((p) => p.roomState.you === opening.turn);
if (!opener?.hand.some((t) => t.left === 6 && t.right === 6)) {
  throw new Error('the 6|6 holder must open the first hand');
}
console.log('OK: the 6|6 holder opens the first hand and must lay the 6|6');

const unique = new Set(
  players.flatMap((p) => p.hand.map((t) => [t.left, t.right].sort((a, b) => a - b).join('-')))
);
if (unique.size !== 28) throw new Error('hands overlap or wrong count: ' + unique.size);
console.log('OK: 28 unique tiles across hands, board starts empty');

let moves = 0,
  passes = 0;
const deadline = Date.now() + 60_000;
while (Date.now() < deadline && !players[0].game.result) {
  const g = players[0].game;
  const actor = players.find((p) => p.roomState.you === g.turn);
  if (!actor) throw new Error('no actor for turn ' + g.turn);

  // Find this actor's first legal placement. An empty board accepts any tile
  // except on the session's first hand, where only the forced 6|6 may open.
  let mv = null;
  for (const tile of actor.hand) {
    if (g.board.length === 0) {
      const forced = g.opening_tile;
      if (forced && !(tile.left === forced.left && tile.right === forced.right)) continue;
      mv = { tile, side: 'right' };
      break;
    }
    if (g.right_end !== null && fits(tile, g.right_end)) {
      mv = { tile, side: 'right' };
      break;
    }
    if (g.left_end !== null && fits(tile, g.left_end)) {
      mv = { tile, side: 'left' };
      break;
    }
  }

  const before = g.board.length + '|' + g.turn + '|' + g.consecutive_passes;
  if (mv) {
    actor.send({ type: 'PlayTile', tile: mv.tile, side: mv.side });
    actor.hand.splice(actor.hand.indexOf(mv.tile), 1);
    moves++;
  } else {
    actor.send({ type: 'PassTurn' });
    passes++;
  }
  await waitAll(
    [players[0]],
    (p) => p.game.board.length + '|' + p.game.turn + '|' + p.game.consecutive_passes !== before,
    4000,
    'game update'
  );
}

const final = players[0].game;
if (!final.result) throw new Error('game did not finish in time');
for (const p of players)
  if (p.rejections.length) throw new Error(p.name + ' got rejected: ' + p.rejections.join('; '));

const inHands = players.reduce((n, p) => n + p.hand.length, 0);
if (final.board.length + inHands !== 28) {
  throw new Error(`tiles vanished: board ${final.board.length} + hands ${inHands}`);
}
console.log(`OK: finished after ${moves} moves / ${passes} passes, result=${JSON.stringify(final.result)}`);
console.log(`OK: tile conservation: board ${final.board.length} + hands ${inHands} = 28`);

// A closed (blocked) game notifies every seat and reveals all hands; a game
// won by playing out reveals nothing, so no seat may have a GameClosed.
const tileKey = (t) => [t.left, t.right].sort((a, b) => a - b).join('-');
if (final.result.reason === 'blocked') {
  // Delivery to the other sockets is still in flight when the loop ends: give
  // every seat a moment to receive the closing update and its reveal.
  await waitAll(players, (p) => p.game.result && p.closed, 6000, 'GameClosed at every seat');
  for (const p of players) {
    if (!p.closed) throw new Error(p.name + ' was not told the game closed');
    if (p.closed.length !== 4) throw new Error(p.name + ' saw ' + p.closed.length + ' revealed hands, expected 4');
    const mine = p.closed.find((h) => h.seat === p.roomState.you);
    const shown = (mine?.hand || []).map(tileKey).sort().join(',');
    const held = p.hand.map(tileKey).sort().join(',');
    if (shown !== held) throw new Error(p.name + ': revealed hand does not match its own tiles');
  }
  const totalRevealed = players[0].closed.reduce((n, h) => n + h.hand.length, 0);
  if (totalRevealed !== inHands) {
    throw new Error('revealed tiles (' + totalRevealed + ') differ from tiles still held (' + inHands + ')');
  }
  console.log(`OK: blocked game notified all four seats and revealed ${totalRevealed} held tiles`);
} else if (players.some((p) => p.closed)) {
  throw new Error('a played-out game must not reveal hands');
}

// Every seat must have observed the finished game before the rematch phase.
await waitAll(players, (p) => p.game && p.game.result, 5000, 'final result at every seat');

// Play again: all four signal readiness on their *live* sockets — no socket
// cycling — and the room must redeal the same seats in place.
players[0].send({ type: 'PlayAgain' });
await waitAll(players, (p) => p.roomState.seats.some((s) => s.ready), 5000, 'first PlayAgain flag');
if (players.some((p) => p.game.result === null)) {
  throw new Error('the table redealt before all four seats were ready');
}
for (const p of players.slice(1)) p.send({ type: 'PlayAgain' });
await waitAll(
  players,
  (p) =>
    p.game && p.game.result === null && p.game.board.length === 0 &&
    p.hand.length === 7 && p.roomState.seats.every((s) => !s.ready),
  8000,
  'in-place redeal after four PlayAgain taps'
);
console.log('OK: four PlayAgain taps re-dealt a fresh game to the same four seats');

// Rematch opening policy (rules.md): the previous hand's winner opens — the
// pip-minimum seat after a blocked hand — and later hands have no forced tile.
const redealt = players[0].game;
if (redealt.opening_tile) throw new Error('only the first hand may force an opening tile');
if (final.result.winner !== null) {
  if (redealt.turn !== final.result.winner) {
    throw new Error(
      'rematch must be opened by the previous winner (turn ' +
        redealt.turn + ', winner ' + final.result.winner + ')'
    );
  }
  console.log('OK: the rematch is opened by the previous hand winner (seat ' + final.result.winner + ')');
} else {
  console.log('OK: a drawn/voided hand rotates the open with no forced tile (seat ' + redealt.turn + ')');
}

// Disconnect frees the seat and everyone sees it.
const gone = players.pop();
gone.ws.close();
await waitAll(players, (p) => p.roomState.seats.length === 3, 6000, 'seat release');
console.log('OK: disconnect frees the seat (3 seats remain)');

// Rejoin completes the table; the finished game must redeal for everyone.
const fifth = connect('Eve');
await fifth.open;
fifth.send({ type: 'JoinRoom', room, name: 'Eve' });
await waitAll(
  [...players, fifth],
  (p) => p.hand.length === 7 && p.game && !p.game.result && p.game.board.length === 0,
  8000,
  'fresh deal after rejoin'
);
console.log('OK: rejoin completes the table and deals a fresh game');

for (const p of [...players, fifth]) p.ws.close();
console.log('\nE2E PASS');
