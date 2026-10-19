# Playing sound

Nile plays audio files through the system's default output device: CoreAudio on macOS,
WASAPI on Windows and ALSA (so PulseAudio or PipeWire through their ALSA layers) on Linux.
There are two ways to use it, and both control the same player.

Supported formats: MP3, WAV (PCM) and FLAC in every build, and Ogg Vorbis (`audio-vorbis`, about 0.7 MB) and AAC/M4A (`audio-aac`, about 0.25 MB) when the `nile-engine` features of those names are on. The whole feature is `audio` (about 0.8 MB with the three default decoders) and can be switched off for embedders that don't need sound. Files load from
the page's folder, a packed app, an absolute path or a `data:` URL. Remote (`http`) audio isn't
supported yet; `fetch` the bytes and make a `data:` URL or save them first.

```html
<audio id="click" src="click.wav"></audio>
<script>
  document.getElementById("save").onclick = () => click.play();
</script>
```

## Sciter's `Audio` class

Follows [docs.sciter.com/docs/JS.runtime/Audio](https://docs.sciter.com/docs/JS.runtime/Audio):

```js
const audio = await Audio.load(__DIR__ + "/sounds/hello.mp3");  // rejects if the file is missing
audio.volume = 0.5;      // 0.0 - 1.0
await audio.play();      // settles when the sound has ended
audio.pause(); audio.resume(); audio.stop();
audio.progress;          // 0.0 - 1.0
```

In this form `play()` returns a promise that settles at the end of the sound, as in Sciter.

## The web API

`new Audio(url)` and `<audio>` are `HTMLAudioElement`s:

| Member | Notes |
| --- | --- |
| `src`, `currentSrc`, `<source src>` | the first source that loads is used |
| `play()` | a promise that resolves once playback has started (web behaviour); rejects if the source can't be played |
| `pause()`, `load()` | |
| `currentTime`, `duration`, `progress` | seconds; setting `currentTime` seeks |
| `paused`, `ended`, `readyState`, `networkState`, `error` | |
| `volume`, `muted`, `loop`, `playbackRate` | `playbackRate` 0.25 - 4 (the pitch moves with it) |
| `autoplay` | starts when the page has loaded (there is no autoplay policy on a desktop) |
| events | `loadstart`, `durationchange`, `loadedmetadata`, `loadeddata`, `canplay`, `canplaythrough`, `play`, `playing`, `pause`, `ended`, `timeupdate` (about every 250 ms), `seeking`, `seeked`, `volumechange`, `ratechange`, `error` |

`<audio>` is hidden (`display: none`); `controls` draws nothing yet, so build your own buttons
(`examples/audio.html`).

## When there is no sound device

With no output device, or with `NILE_AUDIO=off`, tracks play silently against a clock: positions
advance and `ended` fires exactly as they would, which keeps tests and servers working.

## Limits

- No Web Audio (`AudioContext`), recording or `getUserMedia`, and no streaming from the network.
- Up to 256 loaded files at once; a file is decoded as it plays, from memory.
- A WAV or MP3 without a length in its header is decoded once on load to find the duration.
- On Linux the program links `libasound.so.2`, which every desktop distribution has.

`examples/audio.html` has buttons for both styles.
