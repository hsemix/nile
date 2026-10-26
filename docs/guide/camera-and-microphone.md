# Camera and microphone: the `@media` extension

Capturing from a camera or microphone is an optional [native extension](../extensions.md), like SQLite: it lives in
`extensions/media`, and an app that never records doesn't carry it (or the macOS/Windows/Linux capture libraries it uses:
`nokhwa` for cameras, `cpal` for audio, `jpeg-encoder` for photos).

```sh
cargo run -p nile-extensions --example build-extension -- media --dest myapp/extensions/media [--release]
```

```json
{"extensions": {"@media": "./extensions/media"}}
```

```js
import media from "@media";

const { cameras, microphones, cameraAccess } = await media.devices();

// Camera
await media.permission("camera");                       // asks the system up front (macOS shows its prompt once)
const cam = await media.camera({ device: cameras[0].id, width: 1280, height: 720, fps: 30 });
cam.drawTo(document.getElementById("preview"));         // a <canvas>: putImageData for each frame
cam.onFrame((image) => { /* an ImageData (RGBA) */ });
const jpeg = await cam.photo({ quality: 85 });          // Uint8Array, the latest full-size frame
cam.stop();

// Microphone
const mic = await media.microphone({ sampleRate: 16000 });
mic.onData((samples, level) => { /* Int16Array mono, level 0..1 */ });
mic.startRecording();  /* ... */  const wav = mic.stopRecording();   // a WAV file as a Uint8Array
mic.stop();
```

- **Preview size:** a native reply holds 1 MiB, so the live preview is scaled to at most `previewPixels` pixels (230,400 by
  default, 640x360 for a 16:9 camera); `photo()` is always full size, as a JPEG shrunk until it fits a reply (about 900 KB).
- **Microphone:** the device's own rate is used (`mic.sampleRate`); there is no resampling yet, and several channels are
  mixed down to mono. Audio is never dropped when the page is slow: the capture waits for room.
- **Frames can be skipped** when the page is busy; video doesn't wait.
- **Permissions:** the camera is asked for by `media.permission("camera")` and by the first `camera()`. The microphone prompt
  appears when recording starts. A packed macOS app needs `nile --pack ... --camera "reason" --microphone "reason"`
  (the `Info.plist` keys), plus the camera/audio-input entitlements when it is signed for notarization. Running
  from a terminal, macOS attributes the request to the terminal program.
- **Errors** carry a `code`: `MEDIA_PERMISSION`, `MEDIA_NO_DEVICE`, `MEDIA_IO`, `MEDIA_NO_FRAME`, `MEDIA_HANDLE`.
- Nothing is saved for you: write the JPEG or WAV with `@sys`'s `fs.writeFileSync`.

`examples/media/` is a window with both: build the extension into it first (the command is printed in its log).

**Checked:** on a MacBook it lists the devices, starts the built-in camera (1080p, about 24 preview frames a second
with the page keeping up), takes a 190 KB full-size JPEG in tens of milliseconds, and the microphone delivers 48 kHz
chunks and a well-formed WAV; it type-checks for Windows; an engine test loads the library, lists devices and rejects an
unknown stream. **Not checked:** the microphone's level with sound (the test machine's microphone permission was not
granted, so it delivered silence), the Windows (Media Foundation) and Linux (V4L2, ALSA/PulseAudio) paths.
**How frames travel:** the page returns one credit per drawn frame (`camera.ack`, done by the wrapper), so a slow page
gets fewer frames instead of a growing backlog; the host hands buffers to scripts as `Uint8Array`s. Not included: video
recording to a file, camera controls (focus, exposure), audio output selection, echo cancellation.
