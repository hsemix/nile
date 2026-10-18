# Neon Breaker

A complete three-sector brick-breaker built with Nile's existing Canvas 2D,
animation, input, audio, window, and localStorage APIs. No engine extension or
external asset download is needed.

From the repository root:

```sh
cargo run -p nile-shell -- examples/neon-breaker/index.html
```

Move with the mouse, arrow keys, or A/D. Space launches the ball. Escape or P
pauses, M mutes, F toggles fullscreen, and R opens restart confirmation.
Window deactivation pauses automatically; resume explicitly when returning.

Break armored bricks with two hits. Consecutive brick breaks build a score
multiplier until the ball returns to the paddle. Catch capsules for a wide
paddle (12 seconds), multiball (up to three balls), or a one-use safety shield.
Three lost volleys end a run. High score and sound/display preferences survive
restarts. Settings include music, volume, reduced motion, and sharper rendering.

## Package

Build the shell, then use Nile's packager:

```sh
cargo build -p nile-shell
target/debug/nile --pack examples/neon-breaker -o target/NeonBreaker.app
open target/NeonBreaker.app
```

The command above creates a macOS app. On Windows use an `.exe` output; on Linux
use an executable output without `.app`. Package on each target platform with
that platform's shell. The assets are embedded, including sound; source files
are not needed at runtime. Desktop builds require the default audio feature for
audible playback.

## Implementation and checks

`game.js` is the seeded simulation, with fixed 1/120-second steps and swept
collisions to prevent tunneling. `render.js` draws into a reusable canvas with
bounded particles. `audio.js` reuses twelve effect voices and one music track.
`app.js` handles controls, menus, persistence, and focus. Rendering uses the CPU
Canvas backend; the frame readout describes observed frame intervals rather
than a guaranteed frame rate. Circle/brick contacts use expanded rectangles,
so corner contacts are an arcade approximation.

```sh
cargo test -p nile-shell --test neon_breaker -- --nocapture
```

These tests exercise physics, real UI input, focus, settings, saved records,
audio loading/playback, and a packaged app after removing its source directory.
Audio uses the silent clock (`NILE_AUDIO=off`) in automation; listening and
native fullscreen/window controls need a desktop check.

All graphics are drawn in code. The original PCM effects and eight-second
music loop are synthesized by the Rust example below; there are no third-party
sound assets or Python build steps. The generator is a standalone development
tool outside the engine workspace. It is never built or linked by Nile builds,
and running or packaging the game only uses the checked-in WAV files.
Regenerate them explicitly with:

```sh
cargo run --manifest-path tools/neon-sounds/Cargo.toml
```
