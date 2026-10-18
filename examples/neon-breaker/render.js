import { WIDTH, HEIGHT, random } from './game.js';
const powerColors = { wide: '#a78bfa', multi: '#2dd4bf', shield: '#fbbf24' };
export function createRenderer(canvas) {
  const ctx = canvas.getContext('2d');
  const background = new OffscreenCanvas(WIDTH, HEIGHT);
  const bg = background.getContext('2d');
  const gradient = bg.createLinearGradient(0, 0, WIDTH, HEIGHT);
  gradient.addColorStop(0, '#101c33'); gradient.addColorStop(.55, '#0b1425'); gradient.addColorStop(1, '#07121e');
  bg.fillStyle = gradient; bg.fillRect(0, 0, WIDTH, HEIGHT);
  bg.strokeStyle = '#15243a'; bg.lineWidth = 1;
  for (let x = 40; x < WIDTH; x += 40) { bg.beginPath(); bg.moveTo(x, 0); bg.lineTo(x, HEIGHT); bg.stroke(); }
  for (let y = 40; y < HEIGHT; y += 40) { bg.beginPath(); bg.moveTo(0, y); bg.lineTo(WIDTH, y); bg.stroke(); }
  const rng = random(1987);
  for (let i = 0; i < 65; i++) { bg.fillStyle = i % 4 ? '#344d6d' : '#6b83a4'; bg.fillRect(rng() * WIDTH, rng() * HEIGHT, i % 4 ? 1 : 2, 1); }
  const particles = [];
  let quality = 1;
  let reduced = false;
  let flash = 0;
  function configure(sharp, reduceMotion) {
    const next = sharp ? 1.5 : 1;
    if (quality !== next) { quality = next; canvas.width = WIDTH * quality; canvas.height = HEIGHT * quality; }
    reduced = reduceMotion;
    if (reduced) particles.length = 0;
  }
  function burst(x, y, color, count = 12) {
    if (reduced) return;
    for (let i = 0; i < count && particles.length < 140; i++) {
      const angle = Math.random() * Math.PI * 2, speed = 45 + Math.random() * 140;
      particles.push({ x, y, vx: Math.cos(angle) * speed, vy: Math.sin(angle) * speed, life: .3 + Math.random() * .3, age: 0, color });
    }
  }
  function events(events) {
    for (const event of events) {
      if (event.kind === 'brick') burst(event.x, event.y, event.color, event.broken ? 13 : 5);
      if (event.kind === 'paddle') burst(event.x, event.y, '#67e8f9', 5);
      if (event.kind === 'power' || event.kind === 'shield') burst(event.x, event.y, powerColors[event.power] || '#fbbf24', 22);
      if (event.kind === 'loss') flash = .2;
    }
  }
  function clear() { particles.length = 0; flash = 0; }
  function draw(game, dt, animate) {
    ctx.setTransform(quality, 0, 0, quality, 0, 0);
    ctx.globalAlpha = 1; ctx.drawImage(background, 0, 0);
    ctx.strokeStyle = '#2c425f'; ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(14, HEIGHT - 20); ctx.lineTo(14, 28); ctx.quadraticCurveTo(14, 14, 28, 14); ctx.lineTo(WIDTH - 28, 14); ctx.quadraticCurveTo(WIDTH - 14, 14, WIDTH - 14, 28); ctx.lineTo(WIDTH - 14, HEIGHT - 20); ctx.stroke();
    ctx.fillStyle = '#67e8f9'; ctx.fillRect(13, 45, 2, 55); ctx.fillRect(WIDTH - 15, 45, 2, 55);
    ctx.font = '10px sans-serif'; ctx.fillStyle = '#57718f'; ctx.textAlign = 'left'; ctx.fillText('SECTOR 0' + (game.level + 1), 44, 49);
    ctx.textAlign = 'right'; ctx.fillText(['THE APPROACH', 'NEGATIVE SPACE', 'LAST LIGHT'][game.level], WIDTH - 44, 49);
    for (const brick of game.bricks) if (brick.hp > 0) {
      ctx.globalAlpha = brick.hp < brick.maxHp ? .6 : .95;
      ctx.fillStyle = brick.color; ctx.fillRect(brick.x, brick.y, brick.w, brick.h);
      ctx.globalAlpha = .28; ctx.fillStyle = '#ffffff'; ctx.fillRect(brick.x + 1, brick.y + 1, brick.w - 2, 3);
      ctx.globalAlpha = 1;
      if (brick.hp > 1) { ctx.fillStyle = '#14213f'; ctx.fillRect(brick.x + brick.w / 2 - 8, brick.y + brick.h / 2 - 1, 5, 2); ctx.fillRect(brick.x + brick.w / 2 + 3, brick.y + brick.h / 2 - 1, 5, 2); }
      if (brick.power) { ctx.fillStyle = '#092035'; ctx.fillRect(brick.x + brick.w - 8, brick.y + brick.h - 7, 3, 3); }
    }
    for (const drop of game.drops) {
      ctx.fillStyle = powerColors[drop.kind]; ctx.fillRect(drop.x - 11, drop.y - 9, 22, 18);
      ctx.fillStyle = '#102036'; ctx.font = 'bold 10px sans-serif'; ctx.textAlign = 'center'; ctx.fillText(({ wide: 'W', multi: '3', shield: 'S' })[drop.kind], drop.x, drop.y + 3);
    }
    if (game.shield) {
      ctx.globalAlpha = .25; ctx.fillStyle = '#fbbf24'; ctx.fillRect(16, HEIGHT - 25, WIDTH - 32, 5);
      ctx.globalAlpha = .9; ctx.fillRect(16, HEIGHT - 24, WIDTH - 32, 1); ctx.globalAlpha = 1;
    }
    for (const ball of game.balls) {
      if (animate && !reduced) { ball.trail.push({ x: ball.x, y: ball.y }); if (ball.trail.length > 9) ball.trail.shift(); }
      if (!reduced) ball.trail.forEach((point, i) => {
        ctx.globalAlpha = i / 25; ctx.fillStyle = '#67e8f9'; ctx.beginPath(); ctx.arc(point.x, point.y, 2 + i * .4, 0, Math.PI * 2); ctx.fill();
      });
      ctx.globalAlpha = .1; ctx.fillStyle = '#67e8f9'; ctx.beginPath(); ctx.arc(ball.x, ball.y, 15, 0, Math.PI * 2); ctx.fill();
      ctx.globalAlpha = 1; ctx.fillStyle = '#ecfeff'; ctx.beginPath(); ctx.arc(ball.x, ball.y, ball.r, 0, Math.PI * 2); ctx.fill();
      ctx.fillStyle = '#67e8f9'; ctx.beginPath(); ctx.arc(ball.x - 2, ball.y - 2, 2, 0, Math.PI * 2); ctx.fill();
    }
    const paddle = game.paddle;
    ctx.globalAlpha = .12; ctx.fillStyle = game.wideUntil ? '#a78bfa' : '#67e8f9'; ctx.fillRect(paddle.x - paddle.w / 2 - 4, paddle.y - 4, paddle.w + 8, paddle.h + 8);
    ctx.globalAlpha = 1; ctx.fillStyle = game.wideUntil ? '#a78bfa' : '#67e8f9';
    const left = paddle.x - paddle.w / 2, right = paddle.x + paddle.w / 2, top = paddle.y, bottom = paddle.y + paddle.h;
    ctx.beginPath(); ctx.moveTo(left + 5, top); ctx.lineTo(right - 5, top); ctx.quadraticCurveTo(right, top, right, top + 5);
    ctx.lineTo(right, bottom - 5); ctx.quadraticCurveTo(right, bottom, right - 5, bottom); ctx.lineTo(left + 5, bottom);
    ctx.quadraticCurveTo(left, bottom, left, bottom - 5); ctx.lineTo(left, top + 5); ctx.quadraticCurveTo(left, top, left + 5, top); ctx.fill();
    ctx.fillStyle = '#d2fbff'; ctx.fillRect(paddle.x - paddle.w / 2 + 9, paddle.y + 2, paddle.w - 18, 2);
    if (animate) {
      for (let i = particles.length - 1; i >= 0; i--) {
        const particle = particles[i]; particle.age += dt; particle.x += particle.vx * dt; particle.y += particle.vy * dt; particle.vy += 100 * dt;
        if (particle.age >= particle.life) particles.splice(i, 1);
      }
      flash = Math.max(0, flash - dt);
    }
    for (const particle of particles) { ctx.globalAlpha = 1 - particle.age / particle.life; ctx.fillStyle = particle.color; ctx.fillRect(particle.x, particle.y, 3, 3); }
    ctx.globalAlpha = 1;
    if (game.phase === 'ready') {
      ctx.textAlign = 'center'; ctx.font = '11px sans-serif'; ctx.fillStyle = '#8ba9c8'; ctx.fillText('SPACE OR CLICK TO LAUNCH', WIDTH / 2, HEIGHT - 100);
    }
    if (game.combo > 1 && game.phase === 'playing') {
      ctx.textAlign = 'center'; ctx.font = 'bold 15px sans-serif'; ctx.fillStyle = '#67e8f9'; ctx.fillText('×' + game.combo + ' COMBO', WIDTH / 2, HEIGHT - 115);
    }
    if (flash > 0 && !reduced) { ctx.globalAlpha = flash * .5; ctx.fillStyle = '#f472b6'; ctx.fillRect(0, 0, WIDTH, HEIGHT); ctx.globalAlpha = 1; }
  }
  return { draw, events, configure, clear, particleCount: () => particles.length };
}
