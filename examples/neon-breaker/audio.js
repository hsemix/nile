// A bounded voice pool avoids reloading a sound for each collision.
export function createSound(message) {
  const pools = {};
  const cursors = {};
  let muted = false, volume = .55, musicEnabled = true, failed = false;
  const music = new Audio('sounds/music.wav'); music.loop = true;
  for (const [name, count] of [['brick', 4], ['paddle', 3], ['launch', 1], ['power', 2], ['loss', 1], ['level', 1]]) {
    pools[name] = Array.from({ length: count }, () => new Audio('sounds/' + name + '.wav'));
    cursors[name] = 0;
  }
  function unavailable(error) { if (!failed) { failed = true; message('Audio unavailable — you can keep playing. ' + (error.message || error)); } }
  function configure(settings) {
    muted = !!settings.muted; volume = settings.volume; musicEnabled = !!settings.music;
    music.muted = muted || !musicEnabled; music.volume = volume * .3;
    for (const voices of Object.values(pools)) for (const voice of voices) { voice.muted = muted; voice.volume = volume * .7; }
  }
  function effect(name) {
    if (muted || failed) return;
    const key = name === 'wall' || name === 'shield' ? 'paddle' : name === 'victory' ? 'level' : name;
    const pool = pools[key]; if (!pool) return;
    const voice = pool[cursors[key]++ % pool.length];
    try { voice.pause(); voice.currentTime = 0; voice.play().catch(unavailable); } catch (error) { unavailable(error); }
  }
  function sync(phase) {
    if (failed) return;
    if (phase === 'playing' && musicEnabled && !muted) {
      if (music.paused) music.play().catch(unavailable);
    } else music.pause();
    if (phase === 'paused') for (const voices of Object.values(pools)) for (const voice of voices) voice.pause();
  }
  function stop() { music.pause(); music.currentTime = 0; for (const voices of Object.values(pools)) for (const voice of voices) { voice.pause(); voice.currentTime = 0; } }
  return { configure, effect, sync, stop, music, voiceCount: () => 1 + Object.values(pools).reduce((sum, pool) => sum + pool.length, 0) };
}
