// @media: camera and microphone capture. An optional native package: include it only in apps that record.
//
//   import media from "@media";
//   const { cameras, microphones, cameraAccess } = await media.devices();
//   const cam = await media.camera({ device: cameras[0].id, width: 1280, height: 720, fps: 30 });
//   cam.drawTo(document.getElementById("preview"));            // a <canvas>, resized to the preview; updates every frame
//   cam.onFrame((image) => ...);                               // an ImageData (RGBA) per frame instead
//   const jpeg = await cam.photo({ quality: 85 });             // Uint8Array: the latest full-size frame as a JPEG
//   cam.stop();
//
//   const mic = await media.microphone({ sampleRate: 16000 }); // the device's own rate is used: read mic.sampleRate
//   mic.onData((samples, level) => ...);                       // Int16Array mono samples, level 0..1 (peak)
//   mic.startRecording(); ... const wav = mic.stopRecording(); // Uint8Array holding a WAV file (new Audio() plays it)
//   mic.stop();
//
// The preview is scaled to at most previewPixels (230,400 by default, e.g. 640x360) because a native reply holds 1 MiB;
// photo() is always full size. media.permission("camera") asks the system for camera access up front (macOS shows
// its prompt the first time); the first camera() call asks too. The microphone prompt comes when recording starts (a denied microphone delivers silence, level 0).
// Errors carry a code: MEDIA_PERMISSION, MEDIA_NO_DEVICE, MEDIA_IO, MEDIA_NO_FRAME (no picture yet), MEDIA_HANDLE.
// On macOS a packed .app needs NSCameraUsageDescription / NSMicrophoneUsageDescription (nile --pack --camera / --microphone).
import native from '@native';

export async function devices() {
  return (await native.call('media.devices', {})).value;
}

export async function permission(kind = 'camera') {
  return (await native.call('media.permission', { kind })).value.granted;
}

function wav(chunks, sampleRate) {
  let length = 0;
  for (const c of chunks) length += c.length;
  const out = new Uint8Array(44 + length * 2);
  const view = new DataView(out.buffer);
  const text = (at, s) => { for (let i = 0; i < s.length; i++) out[at + i] = s.charCodeAt(i); };
  text(0, 'RIFF'); view.setUint32(4, 36 + length * 2, true); text(8, 'WAVEfmt ');
  view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true); view.setUint32(28, sampleRate * 2, true); view.setUint16(32, 2, true); view.setUint16(34, 16, true);
  text(36, 'data'); view.setUint32(40, length * 2, true);
  let at = 44;
  for (const c of chunks) for (let i = 0; i < c.length; i++, at += 2) view.setInt16(at, c[i], true);
  return out;
}

export async function camera(options = {}) {
  const frames = new Set();
  const ends = new Set();
  let info = null;
  let owed = 0;
  const ack = () => {
    if (!info) { owed++; return; }
    native.call('media.camera.ack', { stream: info.stream }).catch(() => {});
  };
  let stopped = false;
  const sub = native.subscribe('media.camera.start', options, (packet) => {
    const v = packet.value;
    if (v.ended) { stopped = true; for (const cb of ends) { try { cb(v.error || ''); } catch (e) { console.error(e); } } return; }
    if (!packet.buffers.length) return;
    // Hand the frame's credit back so the next one may be sent, whether or not anything draws it.
    if (!frames.size) { ack(); return; }
    const image = new ImageData(new Uint8ClampedArray(packet.buffers[0].buffer, packet.buffers[0].byteOffset, packet.buffers[0].length), v.width, v.height);
    for (const cb of frames) { try { cb(image); } catch (e) { console.error(e); } }
    ack();
  });
  info = (await sub.ready).value;
  for (; owed > 0; owed--) ack();
  const need = () => { if (stopped) throw Object.assign(new Error('the camera is stopped'), { code: 'MEDIA_IO' }); };
  return Object.freeze({
    width: info.width, height: info.height, previewWidth: info.previewWidth, previewHeight: info.previewHeight,
    onFrame(cb) { frames.add(cb); return () => frames.delete(cb); },
    onEnd(cb) { ends.add(cb); return () => ends.delete(cb); },
    drawTo(canvas) {
      canvas.width = info.previewWidth; canvas.height = info.previewHeight;
      const ctx = canvas.getContext('2d');
      return this.onFrame((image) => {
        if (canvas.width !== image.width || canvas.height !== image.height) { canvas.width = image.width; canvas.height = image.height; }
        ctx.putImageData(image, 0, 0);
      });
    },
    async photo({ quality = 85 } = {}) {
      need();
      return (await native.call('media.camera.photo', { stream: info.stream, quality })).buffers[0];
    },
    stop() { if (!stopped) { stopped = true; sub.unsubscribe(); } },
  });
}

export async function microphone(options = {}) {
  const listeners = new Set();
  const ends = new Set();
  let recording = null;
  let stopped = false;
  const sub = native.subscribe('media.microphone.start', options, (packet) => {
    const v = packet.value;
    if (v.ended) { stopped = true; for (const cb of ends) { try { cb(v.error || ''); } catch (e) { console.error(e); } } return; }
    for (const b of packet.buffers) {
      const samples = new Int16Array(b.buffer.slice(b.byteOffset, b.byteOffset + b.length));
      if (recording) recording.push(samples);
      for (const cb of listeners) { try { cb(samples, v.level); } catch (e) { console.error(e); } }
    }
  });
  const info = (await sub.ready).value;
  return Object.freeze({
    sampleRate: info.sampleRate, channels: 1,
    onData(cb) { listeners.add(cb); return () => listeners.delete(cb); },
    onEnd(cb) { ends.add(cb); return () => ends.delete(cb); },
    startRecording() { recording = []; },
    stopRecording() { const chunks = recording || []; recording = null; return wav(chunks, info.sampleRate); },
    stop() { if (!stopped) { stopped = true; sub.unsubscribe(); } },
  });
}

export default Object.freeze({ devices, permission, camera, microphone });
