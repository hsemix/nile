// Deterministic simulation. No DOM, audio, or wall-clock dependencies.
export const WIDTH = 960;
export const HEIGHT = 600;
export const STEP = 1 / 120;
export const LEVELS = 3;
export const clamp = (n, lo, hi) => Math.max(lo, Math.min(hi, n));
export function random(seed) {
  let value = seed >>> 0 || 1;
  return () => { value ^= value << 13; value ^= value >>> 17; value ^= value << 5; return (value >>> 0) / 4294967296; };
}
const colors = ['#a78bfa', '#818cf8', '#60a5fa', '#22d3ee', '#2dd4bf', '#fbbf24'];
function emit(game, kind, extra = {}) { if (game.events.length < 512) game.events.push({ kind, ...extra }); }
function ballAtPaddle(game) { return { x: game.paddle.x, y: game.paddle.y - 9, vx: 0, vy: 0, r: 7, trail: [] }; }
function makeBricks(level) {
  const result = [];
  for (let row = 0; row < 6; row++) for (let col = 0; col < 12; col++) {
    if (level === 1 && ((row === 2 || row === 3) && col >= 4 && col <= 7)) continue;
    if (level === 2 && (col + row) % 5 === 0) continue;
    const id = row * 12 + col;
    const hp = level > 0 && (row + col) % (level === 1 ? 4 : 3) === 0 ? 2 : 1;
    result.push({ id, x: 44 + col * 73, y: 95 + row * 34, w: 68, h: 24, hp, maxHp: hp, color: colors[row], power: id % 19 === 4 ? 'wide' : id % 23 === 12 ? 'multi' : id % 29 === 20 ? 'shield' : null });
  }
  return result;
}
function prepareLevel(game) {
  game.bricks = game.layouts ? game.layouts[game.level].map(brick => ({ ...brick })) : makeBricks(game.level); game.drops = []; game.combo = 0;
  game.wideUntil = 0; game.shield = 0; game.paddle.w = 112;
  game.balls = [ballAtPaddle(game)]; game.phase = 'ready';
}
export function createGame(seed = 1) {
  const game = { seed, rng: random(seed), phase: 'title', previousPhase: 'ready', level: 0, score: 0, lives: 3, combo: 0, time: 0, elapsed: 0, wideUntil: 0, shield: 0, paddle: { x: WIDTH / 2, y: HEIGHT - 49, w: 112, h: 12 }, balls: [], bricks: [], drops: [], events: [] };
  prepareLevel(game); game.phase = 'title'; return game;
}
export function start(game) {
  game.rng = random(game.seed); game.level = 0; game.score = 0; game.lives = 3; game.time = 0; game.elapsed = 0;
  game.paddle.x = WIDTH / 2; game.events = []; prepareLevel(game);
}
export function launch(game) {
  if (game.phase !== 'ready') return false;
  const speed = 400 + game.level * 45;
  game.balls[0].vx = (game.rng() > .5 ? 1 : -1) * 115;
  game.balls[0].vy = -Math.sqrt(speed * speed - 115 * 115);
  game.phase = 'playing'; emit(game, 'launch'); return true;
}
export function pause(game) {
  if (!['ready', 'playing'].includes(game.phase)) return false;
  game.previousPhase = game.phase; game.phase = 'paused'; return true;
}
export function resume(game) { if (game.phase === 'paused') { game.phase = game.previousPhase; return true; } return false; }
export function nextLevel(game) {
  if (game.phase !== 'clear') return false;
  game.level++; prepareLevel(game); return true;
}
export function drainEvents(game) { const result = game.events; game.events = []; return result; }
export function applyPower(game, kind) {
  if (kind === 'wide') { game.wideUntil = game.time + 12; game.paddle.w = 174; game.paddle.x = clamp(game.paddle.x, game.paddle.w / 2 + 14, WIDTH - game.paddle.w / 2 - 14); }
  if (kind === 'shield') game.shield = 1;
  if (kind === 'multi' && game.balls.length) {
    const source = game.balls[0]; const speed = Math.max(400, Math.hypot(source.vx, source.vy));
    for (const sign of [-1, 1]) if (game.balls.length < 3) game.balls.push({ ...source, vx: sign * speed * .64, vy: -speed * .7684, trail: [] });
  }
  emit(game, 'power', { power: kind, x: game.paddle.x, y: game.paddle.y });
}
// Ray against a box expanded by the ball radius. This gives the earliest contact
// during a step, rather than testing only the ball's final (possibly tunneled) position.
export function sweepBox(ball, box, duration) {
  let enter = -Infinity, leave = Infinity, nx = 0, ny = 0;
  for (const axis of ['x', 'y']) {
    const velocity = axis === 'x' ? ball.vx : ball.vy;
    const min = box[axis] - ball.r, max = box[axis] + (axis === 'x' ? box.w : box.h) + ball.r;
    if (Math.abs(velocity) < 1e-9) { if (ball[axis] < min || ball[axis] > max) return null; continue; }
    const a = (min - ball[axis]) / velocity, b = (max - ball[axis]) / velocity;
    const near = Math.min(a, b), far = Math.max(a, b);
    if (near > enter) { enter = near; nx = axis === 'x' ? (velocity > 0 ? -1 : 1) : 0; ny = axis === 'y' ? (velocity > 0 ? -1 : 1) : 0; }
    leave = Math.min(leave, far);
    if (enter > leave) return null;
  }
  return enter >= -1e-8 && enter <= duration && leave >= 0 ? { t: Math.max(0, enter), nx, ny } : null;
}
function moveBall(game, ball, dt) {
  let remaining = dt;
  for (let iteration = 0; remaining > 1e-7 && iteration < 7 && game.phase === 'playing'; iteration++) {
    let hit = null;
    const consider = candidate => { if (candidate && candidate.t >= 0 && candidate.t <= remaining && (!hit || candidate.t < hit.t - 1e-8)) hit = candidate; };
    if (ball.vx < 0) consider({ t: (14 + ball.r - ball.x) / ball.vx, nx: 1, ny: 0, kind: 'wall' });
    if (ball.vx > 0) consider({ t: (WIDTH - 14 - ball.r - ball.x) / ball.vx, nx: -1, ny: 0, kind: 'wall' });
    if (ball.vy < 0) consider({ t: (14 + ball.r - ball.y) / ball.vy, nx: 0, ny: 1, kind: 'wall' });
    if (ball.vy > 0 && ball.y <= game.paddle.y - ball.r + .01) {
      const contact = sweepBox(ball, { x: game.paddle.x - game.paddle.w / 2, y: game.paddle.y, w: game.paddle.w, h: game.paddle.h }, remaining);
      if (contact) consider({ ...contact, kind: 'paddle' });
    }
    if (ball.vy > 0 && game.shield && ball.y < HEIGHT - 23 - ball.r) consider({ t: (HEIGHT - 23 - ball.r - ball.y) / ball.vy, nx: 0, ny: -1, kind: 'shield' });
    for (const brick of game.bricks) if (brick.hp > 0) {
      const contact = sweepBox(ball, brick, remaining);
      if (contact) consider({ ...contact, kind: 'brick', brick });
    }
    if (!hit) { ball.x += ball.vx * remaining; ball.y += ball.vy * remaining; break; }
    ball.x += ball.vx * hit.t; ball.y += ball.vy * hit.t; remaining -= hit.t;
    if (hit.kind === 'paddle') {
      const speed = Math.min(650, Math.hypot(ball.vx, ball.vy) + 4);
      const offset = clamp((ball.x - game.paddle.x) / (game.paddle.w / 2), -1, 1);
      const angle = offset * 1.08;
      ball.vx = Math.sin(angle) * speed;
      if (Math.abs(ball.vx) < 45) ball.vx = (ball.vx < 0 ? -1 : 1) * 45;
      ball.vy = -Math.sqrt(Math.max(1, speed * speed - ball.vx * ball.vx));
      ball.y = game.paddle.y - ball.r - .03; game.combo = 0;
      emit(game, 'paddle', { x: ball.x, y: ball.y });
    } else {
      if (hit.nx) ball.vx = -ball.vx;
      if (hit.ny) ball.vy = -ball.vy;
      ball.x += hit.nx * .03; ball.y += hit.ny * .03;
      if (hit.kind === 'brick') {
        const brick = hit.brick; brick.hp--;
        if (!brick.hp) {
          game.combo = Math.min(8, game.combo + 1); game.score += (35 + game.level * 10) * game.combo;
          if (brick.power) game.drops.push({ x: brick.x + brick.w / 2, y: brick.y + brick.h / 2, kind: brick.power });
        } else game.score += 10;
        emit(game, 'brick', { x: ball.x, y: ball.y, color: brick.color, broken: !brick.hp });
        if (game.bricks.every(row => !row.hp)) {
          game.phase = game.level === LEVELS - 1 ? 'won' : 'clear';
          emit(game, game.phase === 'won' ? 'victory' : 'level');
        }
      } else if (hit.kind === 'shield') {
        game.shield = 0; emit(game, 'shield', { x: ball.x, y: ball.y });
      } else emit(game, 'wall', { x: ball.x, y: ball.y });
    }
  }
}
export function update(game, dt, input = {}) {
  if (!['ready', 'playing'].includes(game.phase) || !Number.isFinite(dt) || dt <= 0) return;
  dt = Math.min(dt, .025);
  const half = game.paddle.w / 2;
  if (input.direction) game.paddle.x += clamp(input.direction, -1, 1) * 640 * dt;
  else if (Number.isFinite(input.target)) game.paddle.x += clamp(input.target - game.paddle.x, -950 * dt, 950 * dt);
  game.paddle.x = clamp(game.paddle.x, half + 14, WIDTH - half - 14);
  if (game.phase === 'ready') { game.balls[0].x = game.paddle.x; game.balls[0].y = game.paddle.y - 9; return; }
  game.time += dt; game.elapsed += dt;
  if (game.wideUntil && game.time >= game.wideUntil) { game.wideUntil = 0; game.paddle.w = 112; }
  for (const ball of game.balls) { moveBall(game, ball, dt); if (game.phase !== 'playing') break; }
  if (game.phase !== 'playing') return;
  game.balls = game.balls.filter(ball => ball.y - ball.r <= HEIGHT + 4);
  if (!game.balls.length) {
    game.lives--; game.combo = 0; game.drops = []; game.wideUntil = 0; game.paddle.w = 112; game.shield = 0;
    emit(game, 'loss');
    if (game.lives <= 0) game.phase = 'over';
    else { game.phase = 'ready'; game.balls = [ballAtPaddle(game)]; }
    return;
  }
  const retained = [];
  for (const drop of game.drops) {
    drop.y += 120 * dt;
    if (Math.abs(drop.x - game.paddle.x) <= game.paddle.w / 2 + 12 && drop.y >= game.paddle.y - 12 && drop.y <= game.paddle.y + 20) applyPower(game, drop.kind);
    else if (drop.y < HEIGHT + 20) retained.push(drop);
  }
  game.drops = retained;
}
