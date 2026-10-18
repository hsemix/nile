import { createGame, start, launch, pause, resume, nextLevel, update, drainEvents, WIDTH, HEIGHT, STEP, LEVELS, clamp } from './game.js';
import { createRenderer } from './render.js';
import { createSound } from './audio.js';
const $ = id => document.getElementById(id);
const canvas = $('game');
const game = createGame(20261018);
const renderer = createRenderer(canvas);
const keys = new Set();
const actionKeys = new Set();
const input = { direction: 0, target: null };
const storageKey = 'neon-breaker-v1';
let preferences = { muted: false, music: true, volume: .55, sharp: false, reduced: false };
let best = 0;
try {
  const saved = JSON.parse(localStorage.getItem(storageKey) || '{}');
  if (Number.isSafeInteger(saved.best) && saved.best >= 0 && saved.best < 1e9) best = saved.best;
  for (const key of ['muted', 'music', 'sharp', 'reduced']) if (typeof saved[key] === 'boolean') preferences[key] = saved[key];
  if (Number.isFinite(saved.volume)) preferences.volume = clamp(saved.volume, 0, 1);
} catch (_) { /* A damaged preference file must not prevent play. */ }
let panel = null;
let overlayKey = '';
let lastTime = null;
let accumulator = 0;
let needsDraw = true;
let savedBest = best;
let saveTime = 0;
let statsTime = 0;
let frameCount = 0;
let frameSum = 0;
let stopped = false;
let rafId;
let fullscreen = false;
let frames = 0;
let simulationSteps = 0;
function message(text) { $('message').textContent = text; }
const sound = createSound(message);
function persist() {
  try { localStorage.setItem(storageKey, JSON.stringify({ ...preferences, best })); savedBest = best; }
  catch (_) { message('Your record could not be saved. This run can continue.'); }
}
function configure() {
  sound.configure(preferences); renderer.configure(preferences.sharp, preferences.reduced);
  $('sound').textContent = preferences.muted ? 'Sound off' : 'Sound on';
  $('sound').setAttribute('aria-pressed', String(!preferences.muted));
  sound.sync(game.phase); needsDraw = true;
}
function fitArena() {
  const box = $('arena-wrap').getBoundingClientRect();
  const scale = Math.max(.1, Math.min((box.width - 4) / WIDTH, (box.height - 4) / HEIGHT, 1.4));
  const width = Math.floor(WIDTH * scale), height = Math.floor(HEIGHT * scale);
  canvas.style.width = width + 'px'; canvas.style.height = height + 'px';
  $('arena').style.width = (width + 2) + 'px'; $('arena').style.height = (height + 2) + 'px'; needsDraw = true;
}
function text(id, value) { if ($(id).textContent !== value) $(id).textContent = value; }
function updateHud() {
  text('score', String(game.score).padStart(6, '0'));
  text('best', String(Math.max(best, game.score)).padStart(6, '0'));
  text('sector', '0' + (game.level + 1) + ' / 0' + LEVELS);
  text('lives', Array(Math.max(0, game.lives)).fill('●').join(' ') || '—');
  const effects = [];
  if (game.wideUntil > game.time) effects.push('WIDE ' + Math.ceil(game.wideUntil - game.time) + 's');
  if (game.balls.length > 1) effects.push(game.balls.length + ' BALLS');
  if (game.shield) effects.push('SHIELD');
  text('effect', effects.join(' · ') || (game.phase === 'playing' ? 'KEEP THE MOMENTUM' : game.phase === 'ready' ? 'READY TO LAUNCH' : 'NILE ARCADE'));
  $('pause').disabled = !['ready', 'playing', 'paused'].includes(game.phase) || !!panel;
  text('pause', game.phase === 'paused' ? '▶ Resume' : 'Ⅱ Pause');
}
function renderOverlay() {
  const key = (panel || game.phase) + ':' + (['over', 'won', 'clear'].includes(game.phase) ? game.score : '');
  if (key === overlayKey) return;
  overlayKey = key;
  const overlay = $('overlay');
  if (!panel && ['ready', 'playing'].includes(game.phase)) { overlay.style.display = 'none'; overlay.innerHTML = ''; return; }
  overlay.style.display = 'flex';
  let body = '';
  if (panel === 'settings') {
    body = '<div class="tag">MAKE IT YOURS</div><h2>Sound & display</h2><label class="settings-field"><span>Master volume</span><input id="volume" type="range" min="0" max="1" step="0.05" value="' + preferences.volume + '"></label><label class="settings-check"><input id="music" type="checkbox"' + (preferences.music ? ' checked' : '') + '>Background music</label><label class="settings-check"><input id="reduced" type="checkbox"' + (preferences.reduced ? ' checked' : '') + '>Reduce motion and particles</label><label class="settings-check"><input id="sharp" type="checkbox"' + (preferences.sharp ? ' checked' : '') + '>Sharper canvas (uses more processing)</label><button class="primary" data-game-action="back">Done</button>';
  } else if (panel === 'help') {
    body = '<div class="tag">FIND YOUR FLOW</div><h2>Keep it in play.</h2><p>Steer with your mouse, ← →, or A / D.<br>Space or click launches the ball.<br>P / Escape pauses. M mutes. F toggles fullscreen.<br><br>Hit the edges of the paddle to change the angle. Break bricks without touching the paddle to build a combo.<br><br>Catch W for a wider paddle, 3 for multiball, and S for a one-use safety shield.</p><button class="primary" data-game-action="back">Got it</button>';
  } else if (panel === 'restart') {
    body = '<div class="tag">A FRESH START</div><h2>Restart this run?</h2><p>Your best score is kept. This run will start again at sector one.</p><button class="primary" data-game-action="restart">Restart run</button><button class="secondary" data-game-action="back">Keep this run</button>';
  } else if (game.phase === 'title') {
    body = '<div class="tag">WELCOME TO NILE ARCADE</div><h2>Break through.<br><span>Find your rhythm.</span></h2><p>Three sectors of light, motion, and momentum.<br>Clear every brick. Keep the ball alive.</p><button class="primary" data-game-action="primary">Start a run <span>↗</span></button><button class="secondary" data-game-action="help">How to play</button><div class="power-legend"><span><b>W</b> WIDE</span><span><b class="mint">3</b> MULTIBALL</span><span><b class="gold">S</b> SHIELD</span></div>';
  } else if (game.phase === 'paused') {
    body = '<div class="tag">TAKE A BREATH</div><h2>Momentum can wait.</h2><p>Your run is paused. Pick up exactly where you left off.</p><button class="primary" data-game-action="primary">Resume run</button><button class="secondary" data-game-action="restart-request">Restart run</button>';
  } else if (game.phase === 'clear') {
    body = '<div class="tag">SECTOR 0' + (game.level + 1) + ' COMPLETE</div><h2>A little further.</h2><p>The next sector brings a new pattern and a faster ball.</p><div class="result-number">' + String(game.score).padStart(6, '0') + '</div><button class="primary" data-game-action="primary">Enter sector 0' + (game.level + 2) + '</button>';
  } else {
    body = '<div class="tag">' + (game.phase === 'won' ? 'ALL THREE SECTORS CLEARED' : 'RUN COMPLETE') + '</div><h2>' + (game.phase === 'won' ? 'You found your flow.' : 'One more run?') + '</h2><p>' + (game.phase === 'won' ? 'Every brick. Every sector. A beautiful finish.' : 'Every run is a chance to find a new rhythm.') + '</p><div class="result-number">' + String(game.score).padStart(6, '0') + '</div><button class="primary" data-game-action="primary">Play again</button><button class="secondary" data-game-action="help">Controls & power-ups</button>';
  }
  overlay.innerHTML = '<section class="overlay-card" role="dialog" aria-label="' + (panel || game.phase) + '">' + body + '</section>';
  overlay.querySelector('button')?.focus();
}
function sync() {
  if (game.score > best) best = game.score;
  updateHud(); renderOverlay(); sound.sync(game.phase); needsDraw = true;
}
function primary() {
  if (panel || !['title', 'over', 'won', 'ready', 'paused', 'clear'].includes(game.phase)) return;
  if (['title', 'over', 'won'].includes(game.phase)) { persist(); sound.stop(); start(game); renderer.clear(); launch(game); }
  else if (game.phase === 'ready') launch(game);
  else if (game.phase === 'paused') resume(game);
  else if (game.phase === 'clear') { nextLevel(game); renderer.clear(); launch(game); }
  accumulator = 0; lastTime = null; sync();
  if (['ready', 'playing'].includes(game.phase)) canvas.focus();
}
function togglePause() {
  if (panel) { panel = null; sync(); return; }
  if (game.phase === 'paused') resume(game); else pause(game);
  keys.clear(); input.direction = 0; input.target = null; accumulator = 0; lastTime = null; sync();
  if (game.phase !== 'paused') canvas.focus();
}
function showPanel(kind) { pause(game); keys.clear(); input.direction = 0; input.target = null; accumulator = 0; panel = kind; sync(); }
function mute() { preferences.muted = !preferences.muted; configure(); persist(); message(preferences.muted ? 'Sound muted. Press M to bring it back.' : 'Sound on.'); }
function toggleFullscreen() {
  if (fullscreen) document.exitFullscreen(); else document.documentElement.requestFullscreen();
  fullscreen = !fullscreen;
}
function processEvents() {
  const events = drainEvents(game); renderer.events(events);
  const played = new Set();
  for (const event of events) {
    if (!played.has(event.kind)) { sound.effect(event.kind); played.add(event.kind); }
    if (event.kind === 'power') message(({ wide: 'Wide paddle for 12 seconds.', multi: 'Three balls. Keep them in play.', shield: 'Safety shield armed. One save when you need it.' })[event.power]);
    if (event.kind === 'loss') message(game.phase === 'over' ? 'Run complete. Your best score is kept.' : 'Energy lost. Press Space or click to launch again.');
    if (event.kind === 'level') message('Sector cleared. Ready for the next pattern?');
    if (event.kind === 'victory') message('All sectors cleared. You found your flow.');
  }
}
function frame(timestamp) {
  if (stopped) return;
  frames++;
  let delta = lastTime === null ? 0 : Math.max(0, (timestamp - lastTime) / 1000); lastTime = timestamp;
  if (delta > .5 && ['ready', 'playing'].includes(game.phase)) { pause(game); keys.clear(); input.direction = 0; input.target = null; message('Paused after an interruption. Resume when you are ready.'); sync(); }
  delta = Math.min(delta, .15);
  const previousPhase = game.phase;
  if (!panel && ['ready', 'playing'].includes(game.phase)) {
    input.direction = (keys.has('arrowright') || keys.has('d') ? 1 : 0) - (keys.has('arrowleft') || keys.has('a') ? 1 : 0);
    accumulator += delta;
    let steps = 0;
    while (accumulator >= STEP && steps < 18) { update(game, STEP, input); accumulator -= STEP; steps++; simulationSteps++; }
    if (steps === 18) accumulator = 0;
  } else accumulator = 0;
  processEvents();
  if (game.score > best) best = game.score;
  if (best !== savedBest && timestamp - saveTime > 1000) { persist(); saveTime = timestamp; }
  if (game.phase !== previousPhase) { persist(); sync(); }
  const animate = ['ready', 'playing'].includes(game.phase) && !panel;
  if (animate || needsDraw) { renderer.draw(game, delta, animate); needsDraw = false; }
  updateHud();
  if (delta > 0) { frameCount++; frameSum += delta; }
  if (timestamp - statsTime > 500 && frameCount) {
    text('frame-info', Math.round(frameCount / frameSum) + ' FPS · ' + (frameSum / frameCount * 1000).toFixed(1) + ' ms');
    statsTime = timestamp; frameCount = 0; frameSum = 0;
  }
  rafId = requestAnimationFrame(frame);
}
canvas.addEventListener('mousemove', event => {
  const rectangle = canvas.getBoundingClientRect();
  if (rectangle.width > 0) input.target = clamp((event.clientX - rectangle.x) * WIDTH / rectangle.width, 14, WIDTH - 14);
});
canvas.addEventListener('mousedown', event => { if (event.button === 0) { event.preventDefault(); canvas.focus(); if (game.phase === 'ready') primary(); } });
$('pause').addEventListener('click', togglePause); $('sound').addEventListener('click', mute);
$('settings').addEventListener('click', () => showPanel('settings')); $('fullscreen').addEventListener('click', toggleFullscreen);
$('overlay').addEventListener('click', event => {
  const target = event.target.closest('[data-game-action]'); if (!target) return;
  const action = target.dataset.gameAction;
  if (action === 'primary') primary();
  else if (action === 'help') showPanel('help');
  else if (action === 'back') { panel = null; sync(); }
  else if (action === 'restart-request') showPanel('restart');
  else if (action === 'restart') { panel = null; game.phase = 'over'; primary(); }
});
$('overlay').addEventListener('input', event => {
  if (event.target.id !== 'volume') return;
  preferences.volume = clamp(Number(event.target.value), 0, 1); configure(); persist();
});
$('overlay').addEventListener('change', event => {
  if (!['music', 'reduced', 'sharp'].includes(event.target.id)) return;
  preferences[event.target.id] = !!event.target.checked; configure(); persist();
});
function controlKey(event) { const key = event.key.toLowerCase(); return key === 'arrowleft' || key === 'a' ? 'left' : key === 'arrowright' || key === 'd' ? 'right' : null; }
document.addEventListener('keydown', event => {
  if (event.ctrlKey || event.metaKey || event.altKey) return;
  const key = event.key.toLowerCase();
  if (['input', 'select', 'textarea'].includes(event.target.localName) && key !== 'escape') return;
  if ([' ', 'enter'].includes(key) && event.target.localName === 'button') return;
  if (['escape', 'p', 'm', 'f', 'r', ' ', 'enter'].includes(key)) {
    event.preventDefault(); if (actionKeys.has(key)) return; actionKeys.add(key);
  }
  if (key === 'escape') { event.preventDefault(); togglePause(); return; }
  if (['input', 'select', 'textarea'].includes(event.target.localName)) return;
  const control = controlKey(event);
  if (control) { event.preventDefault(); keys.add(key); input.target = null; return; }
  if (key === 'm') { event.preventDefault(); mute(); }
  else if (key === 'f') { event.preventDefault(); toggleFullscreen(); }
  else if (key === 'p') { event.preventDefault(); togglePause(); }
  else if (key === 'r' && game.phase !== 'title') { event.preventDefault(); showPanel('restart'); }
  else if ((key === ' ' || key === 'enter') && event.target.localName !== 'button') { event.preventDefault(); primary(); }
});
document.addEventListener('keyup', event => { actionKeys.delete(event.key.toLowerCase()); const control = controlKey(event); if (control) { event.preventDefault(); keys.delete(event.key.toLowerCase()); } });
function loseFocus() {
  keys.clear(); actionKeys.clear(); input.direction = 0; input.target = null;
  if (pause(game)) { accumulator = 0; lastTime = null; persist(); message('Paused while the window is inactive.'); sync(); }
}
window.addEventListener('blur', loseFocus);
Window.this.on('activate', event => { if (!event.reason) loseFocus(); });
window.addEventListener('resize', fitArena);
Window.this.on('closerequest', () => { best = Math.max(best, game.score); persist(); stopped = true; cancelAnimationFrame(rafId); sound.stop(); });
configure(); fitArena(); sync(); rafId = requestAnimationFrame(timestamp => { fitArena(); frame(timestamp); });
console.log('neon breaker ready');
