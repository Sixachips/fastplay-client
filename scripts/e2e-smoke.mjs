// Headless smoke test for the fastPlay wire protocol (run with node ≥ 21).
// Joins 4 players, plays a full game to the end, then checks the
// disconnect / rejoin / redeal flows — the same paths the WASM client uses.
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
    rejections: [],
    open: new Promise((r) => (ws.onopen = r)),
  };
  ws.onmessage = (ev) => {
    const m = JSON.parse(ev.data);
    if (m.type === 'RoomState') p.roomState = m.room;
    if (m.type === 'HandDealt') p.hand = m.hand;
    if (m.type === 'GameStateUpdate') p.game = m.state;
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

  // Find this actor's first legal placement (empty board opens with anything).
  let mv = null;
  for (const tile of actor.hand) {
    if (g.board.length === 0) {
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
