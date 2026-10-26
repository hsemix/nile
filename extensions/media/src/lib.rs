//! Camera and microphone capture as an optional native package (`@media`). Apps that never record don't carry it.
//!
//! Operations (namespace `media.`):
//! - `devices`: `{ cameras: [{ id, name }], microphones: [{ id, name, default }], cameraAccess }`
//! - `permission` `{ kind: "camera" }`: asks the system (macOS shows its prompt) and answers `{ granted }`
//! - `camera.start` (subscription) `{ device, width, height, fps, previewPixels }`: acknowledges with
//!   `{ stream, width, height, previewWidth, previewHeight }`, then every frame is an event with one RGBA buffer scaled
//!   to at most `previewPixels` pixels (default 230,400; a reply is capped at 1 MiB) and `{ width, height, seq }`
//! - `camera.photo` `{ stream, quality }`: the latest full-size frame as a JPEG buffer, shrunk until it fits a reply
//! - `microphone.start` (subscription) `{ device, sampleRate }`: acknowledges with `{ stream, sampleRate, channels: 1 }`,
//!   then events with one buffer of mono signed 16-bit little-endian samples and `{ level }` (peak, 0 to 1)
//!
//! Capture runs on the package's own threads; a camera or microphone object lives on the thread that created it
//! (CoreAudio and AVFoundation need that), and stopping a subscription joins the thread.
use nile_extension_sdk::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use nokhwa::pixel_format::{RgbAFormat, RgbFormat};
use nokhwa::utils::{ApiBackend, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType};
use nokhwa::{Buffer, Camera};

const REPLY_LIMIT: usize = 900_000;
const DEFAULT_PREVIEW_PIXELS: usize = 230_400;

struct Stream {
    context: u64,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// The newest camera frame at full size, for `camera.photo`.
    latest: Option<Arc<Mutex<Option<Buffer>>>>,
    /// Preview frames the page may still be sent: it returns one with `camera.ack` after drawing a frame.
    credits: Arc<AtomicI32>,
}

struct State {
    client: Client,
    streams: Mutex<HashMap<u64, Stream>>,
}

fn fail(client: Client, ctx: u64, call: u64, code: &str, message: &str) {
    client.reply(ctx, call, Err((code, message)), &[], false);
}

fn stop_stream(stream: &mut Stream) {
    stream.stop.store(true, Ordering::Release);
    if let Some(thread) = stream.thread.take() {
        let _ = thread.join();
    }
}

// ---- camera -------------------------------------------------------------------------------------------------------

/// macOS asks once; elsewhere there is nothing to ask and this is true.
fn camera_permission(wait: Duration) -> bool {
    if nokhwa::nokhwa_check() {
        return true;
    }
    let (tx, rx) = channel();
    nokhwa::nokhwa_initialize(move |granted| {
        let _ = tx.send(granted);
    });
    rx.recv_timeout(wait).unwrap_or(false) || nokhwa::nokhwa_check()
}

fn camera_index(device: &Value) -> CameraIndex {
    match device {
        Value::String(s) => s.parse::<u32>().map(CameraIndex::Index).unwrap_or_else(|_| CameraIndex::String(s.clone())),
        Value::Number(n) => CameraIndex::Index(n.as_u64().unwrap_or(0) as u32),
        _ => CameraIndex::Index(0),
    }
}

/// Nearest-neighbour shrink to at most `max_pixels` (a preview doesn't need better, and it is cheap).
fn shrink(rgba: &[u8], w: usize, h: usize, max_pixels: usize) -> (Vec<u8>, usize, usize) {
    if w * h <= max_pixels {
        return (rgba.to_vec(), w, h);
    }
    let scale = (max_pixels as f64 / (w * h) as f64).sqrt();
    let (nw, nh) = (((w as f64 * scale) as usize).max(1), ((h as f64 * scale) as usize).max(1));
    let mut out = Vec::with_capacity(nw * nh * 4);
    for y in 0..nh {
        let sy = y * h / nh;
        for x in 0..nw {
            let sx = x * w / nw;
            let i = (sy * w + sx) * 4;
            out.extend_from_slice(&rgba[i..i + 4]);
        }
    }
    (out, nw, nh)
}

/// Open the camera with the format closest to the wanted size and rate. Cameras offer different pixel formats (a Mac's
/// built-in one has no MJPEG) and a listed format is sometimes refused when it is set, so each candidate is tried on a
/// fresh camera and the device's own default is the last resort.
fn open_camera(device: &Value, w: u32, h: u32, fps: u32) -> Result<Camera, String> {
    let index = camera_index(device);
    let mut formats = {
        let mut probe = Camera::new(index.clone(), RequestedFormat::new::<RgbAFormat>(RequestedFormatType::AbsoluteHighestFrameRate)).map_err(|e| e.to_string())?;
        probe.compatible_camera_formats().unwrap_or_default()
    };
    formats.retain(|f| f.width() > 0 && f.height() > 0);
    // Closest area first, then the rate, preferring formats that need no heavy decoding.
    formats.sort_by_key(|f| {
        let area = (f.width() as i64 * f.height() as i64 - w as i64 * h as i64).abs();
        let rate = (f.frame_rate() as i64 - fps as i64).abs();
        let cost = match f.format() {
            FrameFormat::NV12 | FrameFormat::YUYV | FrameFormat::RAWRGB | FrameFormat::RAWBGR => 0,
            _ => 1,
        };
        (area, rate, cost)
    });
    let mut last = String::from("the camera offers no usable format");
    let requests = formats.iter().take(6).map(|f| RequestedFormatType::Exact(*f)).chain(std::iter::once(RequestedFormatType::AbsoluteHighestFrameRate));
    for request in requests {
        match Camera::new(index.clone(), RequestedFormat::new::<RgbAFormat>(request)).and_then(|mut c| c.open_stream().map(|()| c)) {
            Ok(camera) => return Ok(camera),
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("{last} (offered: {})", formats.iter().take(12).map(|f| format!("{}x{}@{} {:?}", f.width(), f.height(), f.frame_rate(), f.format())).collect::<Vec<_>>().join(", ")))
}

fn camera_thread(client: Client, ctx: u64, call: u64, args: Value, stop: Arc<AtomicBool>, latest: Arc<Mutex<Option<Buffer>>>, credits: Arc<AtomicI32>) {
    if !camera_permission(Duration::from_secs(120)) {
        return fail(client, ctx, call, "MEDIA_PERMISSION", "camera access was not granted");
    }
    let (w, h) = (args["width"].as_u64().unwrap_or(640) as u32, args["height"].as_u64().unwrap_or(480) as u32);
    let fps = args["fps"].as_u64().unwrap_or(30) as u32;
    let preview = args["previewPixels"].as_u64().map(|p| p as usize).unwrap_or(DEFAULT_PREVIEW_PIXELS).clamp(1024, 240_000);
    let mut camera = match open_camera(&args["device"], w, h, fps) {
        Ok(c) => c,
        Err(e) => return fail(client, ctx, call, "MEDIA_NO_DEVICE", &e),
    };
    let size = camera.resolution();
    let (pw, ph) = {
        let scale = ((preview as f64) / (size.width() as f64 * size.height() as f64)).sqrt().min(1.0);
        (((size.width() as f64 * scale) as usize).max(1), ((size.height() as f64 * scale) as usize).max(1))
    };
    client.reply(
        ctx,
        call,
        Ok(&json!({ "stream": call, "width": size.width(), "height": size.height(), "previewWidth": pw, "previewHeight": ph, "format": format!("{:?} {}x{}@{}", camera.frame_format(), size.width(), size.height(), camera.frame_rate()) }).to_string()),
        &[],
        false,
    );
    let mut seq = 0u64;
    while !stop.load(Ordering::Acquire) {
        let buffer = match camera.frame() {
            Ok(b) => b,
            Err(e) => {
                if !stop.load(Ordering::Acquire) {
                    client.reply(ctx, call, Ok(&json!({ "ended": true, "error": e.to_string() }).to_string()), &[], true);
                }
                break;
            }
        };
        *latest.lock().unwrap() = Some(buffer.clone());
        // A page that is behind gets no more frames than it can use, or its photo and stop replies would queue behind a
        // backlog of pictures: the frame is skipped before it is even converted.
        if credits.load(Ordering::Acquire) <= 0 {
            continue;
        }
        let Ok(image) = buffer.decode_image::<RgbAFormat>() else { continue };
        let (full_w, full_h) = (image.width() as usize, image.height() as usize);
        let (pixels, sw, sh) = shrink(image.as_raw(), full_w, full_h, preview);
        seq += 1;
        // A busy page may skip frames: video doesn't wait.
        if client.reply(ctx, call, Ok(&json!({ "width": sw, "height": sh, "seq": seq }).to_string()), &[pixels], true) == NILE_OK {
            credits.fetch_sub(1, Ordering::AcqRel);
        }
    }
    let _ = camera.stop_stream();
}

fn photo(latest: &Mutex<Option<Buffer>>, quality: u8) -> Result<Vec<u8>, (&'static str, String)> {
    let buffer = latest.lock().unwrap().clone().ok_or(("MEDIA_NO_FRAME", "no frame has arrived yet".to_string()))?;
    let image = buffer.decode_image::<RgbFormat>().map_err(|e| ("MEDIA_IO", e.to_string()))?;
    let (mut w, mut h) = (image.width() as usize, image.height() as usize);
    let mut rgb = image.into_raw();
    let mut q = quality.clamp(10, 100);
    loop {
        let mut out = Vec::new();
        jpeg_encoder::Encoder::new(&mut out, q)
            .encode(&rgb, w as u16, h as u16, jpeg_encoder::ColorType::Rgb)
            .map_err(|e| ("MEDIA_IO", e.to_string()))?;
        if out.len() <= REPLY_LIMIT {
            return Ok(out);
        }
        if q > 35 {
            q -= 15;
        } else if w > 160 {
            // Halve the picture and try again at a middling quality.
            let mut half = Vec::with_capacity(rgb.len() / 4);
            for y in 0..h / 2 {
                for x in 0..w / 2 {
                    let i = (y * 2 * w + x * 2) * 3;
                    half.extend_from_slice(&rgb[i..i + 3]);
                }
            }
            (w, h, rgb, q) = (w / 2, h / 2, half, 70);
        } else {
            return Err(("MEDIA_RESULT_LIMIT", "the picture does not fit one reply".into()));
        }
    }
}

// ---- microphone ---------------------------------------------------------------------------------------------------

fn input_device(host: &cpal::Host, wanted: &Value) -> Option<cpal::Device> {
    match wanted.as_str() {
        Some(id) if !id.is_empty() => host.input_devices().ok()?.find(|d| device_name(d) == id),
        _ => host.default_input_device(),
    }
}

fn device_name(device: &cpal::Device) -> String {
    device.description().map(|d| d.name().to_string()).unwrap_or_default()
}

fn to_i16_mono<T: cpal::Sample>(data: &[T], channels: usize, to_f32: impl Fn(T) -> f32) -> Vec<i16> {
    data.chunks(channels.max(1))
        .map(|frame| {
            let mean = frame.iter().map(|s| to_f32(*s)).sum::<f32>() / frame.len() as f32;
            (mean.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
        })
        .collect()
}

fn microphone_thread(client: Client, ctx: u64, call: u64, args: Value, stop: Arc<AtomicBool>) {
    let host = cpal::default_host();
    let Some(device) = input_device(&host, &args["device"]) else {
        return fail(client, ctx, call, "MEDIA_NO_DEVICE", "no such microphone");
    };
    let config = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => return fail(client, ctx, call, "MEDIA_IO", &e.to_string()),
    };
    let (rate, channels) = (config.sample_rate(), config.channels() as usize);
    let (tx, rx) = channel::<Vec<i16>>();
    let failed = Arc::new(AtomicBool::new(false));
    let error_flag = failed.clone();
    let on_error = move |_e: cpal::Error| error_flag.store(true, Ordering::Release);
    let built = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(config.config(), move |d: &[f32], _: &_| drop(tx.send(to_i16_mono(d, channels, |s| s))), on_error, None),
        cpal::SampleFormat::I16 => device.build_input_stream(config.config(), move |d: &[i16], _: &_| drop(tx.send(to_i16_mono(d, channels, |s| s as f32 / 32768.0))), on_error, None),
        cpal::SampleFormat::U16 => device.build_input_stream(config.config(), move |d: &[u16], _: &_| drop(tx.send(to_i16_mono(d, channels, |s| (s as f32 - 32768.0) / 32768.0))), on_error, None),
        other => return fail(client, ctx, call, "MEDIA_IO", &format!("unsupported sample format {other:?}")),
    };
    let stream = match built.and_then(|s| s.play().map(|()| s)) {
        Ok(s) => s,
        Err(e) => return fail(client, ctx, call, "MEDIA_PERMISSION", &format!("could not start recording (is microphone access allowed?): {e}")),
    };
    client.reply(ctx, call, Ok(&json!({ "stream": call, "sampleRate": rate, "channels": 1 }).to_string()), &[], false);
    // Gather about 50 ms per event; audio must not be dropped, so a full queue waits.
    let per_event = (rate as usize / 20).max(256);
    let mut pending: Vec<i16> = Vec::new();
    let flush = |pending: &mut Vec<i16>| -> bool {
        if pending.is_empty() {
            return true;
        }
        let level = pending.iter().map(|s| (*s as i32).unsigned_abs()).max().unwrap_or(0) as f64 / 32768.0;
        let bytes: Vec<u8> = pending.iter().flat_map(|s| s.to_le_bytes()).collect();
        pending.clear();
        loop {
            match client.reply(ctx, call, Ok(&json!({ "level": level }).to_string()), std::slice::from_ref(&bytes), true) {
                NILE_OK => return true,
                NILE_OVERLOADED if !stop.load(Ordering::Acquire) => std::thread::sleep(Duration::from_millis(5)),
                _ => return false,
            }
        }
    };
    while !stop.load(Ordering::Acquire) {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(samples) => {
                pending.extend(samples);
                if pending.len() >= per_event && !flush(&mut pending) {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if failed.load(Ordering::Acquire) {
            client.reply(ctx, call, Ok("{\"ended\":true,\"error\":\"the microphone stopped\"}"), &[], true);
            break;
        }
    }
    drop(stream);
}

// ---- dispatch -----------------------------------------------------------------------------------------------------

fn devices() -> Value {
    let access = nokhwa::nokhwa_check();
    let cameras: Vec<Value> = if access {
        nokhwa::query(ApiBackend::Auto)
            .map(|list| list.into_iter().map(|c| json!({ "id": c.index().to_string(), "name": c.human_name() })).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let host = cpal::default_host();
    let default = host.default_input_device().map(|d| device_name(&d));
    let microphones: Vec<Value> = host
        .input_devices()
        .map(|list| list.map(|d| device_name(&d)).map(|name| json!({ "id": name, "name": name, "default": Some(&name) == default.as_ref() })).collect())
        .unwrap_or_default();
    json!({ "cameras": cameras, "microphones": microphones, "cameraAccess": access })
}

unsafe extern "C" fn dispatch(user: u64, request: *const NileRequest) -> NileStatus {
    guard(|| {
        let state = &*(user as *const State);
        let request = &*request;
        let client = state.client;
        let (ctx, call) = (request.context, request.call);
        let op = std::str::from_utf8(request.operation.bytes()).unwrap_or("");
        let args: Value = serde_json::from_slice(request.json.bytes()).unwrap_or(Value::Null);
        let subscribe = request.flags == NILE_SUBSCRIBE;
        let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| std::thread::Builder::new().name(name.into()).spawn(f);
        match (op, subscribe) {
            ("media.devices", false) => {
                let _ = spawn("nile-media-devices", Box::new(move || {
                    client.reply(ctx, call, Ok(&devices().to_string()), &[], false);
                }));
                NILE_OK
            }
            ("media.permission", false) => {
                let _ = spawn(
                    "nile-media-permission",
                    Box::new(move || {
                        let granted = args["kind"].as_str() != Some("camera") || camera_permission(Duration::from_secs(120));
                        client.reply(ctx, call, Ok(&json!({ "granted": granted }).to_string()), &[], false);
                    }),
                );
                NILE_OK
            }
            ("media.camera.start", true) | ("media.microphone.start", true) => {
                let camera = op == "media.camera.start";
                let stop = Arc::new(AtomicBool::new(false));
                let latest = camera.then(|| Arc::new(Mutex::new(None)));
                let credits = Arc::new(AtomicI32::new(2));
                let (flag, slot, tokens) = (stop.clone(), latest.clone(), credits.clone());
                let body: Box<dyn FnOnce() + Send> = if camera {
                    Box::new(move || camera_thread(client, ctx, call, args, flag, slot.expect("camera slot"), tokens))
                } else {
                    Box::new(move || microphone_thread(client, ctx, call, args, flag))
                };
                let mut streams = state.streams.lock().unwrap();
                match spawn(if camera { "nile-camera" } else { "nile-microphone" }, body) {
                    Ok(thread) => {
                        streams.insert(call, Stream { context: ctx, stop, thread: Some(thread), latest, credits });
                        NILE_OK
                    }
                    Err(_) => NILE_FAILED,
                }
            }
            ("media.camera.ack", false) => {
                let id = args["stream"].as_u64().unwrap_or(0);
                if let Some(stream) = state.streams.lock().unwrap().get(&id).filter(|s| s.context == ctx) {
                    // Never more than three outstanding, however many acknowledgments arrive.
                    if stream.credits.load(Ordering::Acquire) < 3 {
                        stream.credits.fetch_add(1, Ordering::AcqRel);
                    }
                }
                client.reply(ctx, call, Ok("{}"), &[], false);
                NILE_OK
            }
            ("media.camera.photo", false) => {
                let id = args["stream"].as_u64().unwrap_or(0);
                let latest = state.streams.lock().unwrap().get(&id).filter(|s| s.context == ctx).and_then(|s| s.latest.clone());
                let Some(latest) = latest else {
                    fail(client, ctx, call, "MEDIA_HANDLE", "no such camera stream");
                    return NILE_OK;
                };
                let quality = args["quality"].as_u64().unwrap_or(85) as u8;
                let _ = spawn(
                    "nile-photo",
                    Box::new(move || match photo(&latest, quality) {
                        Ok(jpeg) => {
                            client.reply(ctx, call, Ok("{}"), &[jpeg], false);
                        }
                        Err((code, message)) => fail(client, ctx, call, code, &message),
                    }),
                );
                NILE_OK
            }
            _ => NILE_INVALID,
        }
    })
}

unsafe extern "C" fn cancel(user: u64, _context: u64, call: u64) {
    guard(|| {
        let state = &*(user as *const State);
        let stream = state.streams.lock().unwrap().remove(&call);
        if let Some(mut stream) = stream {
            stop_stream(&mut stream);
        }
        NILE_OK
    });
}

unsafe extern "C" fn dispose(user: u64, context: u64) -> NileStatus {
    guard(|| {
        let state = &*(user as *const State);
        let mine: Vec<Stream> = {
            let mut streams = state.streams.lock().unwrap();
            let keys: Vec<u64> = streams.iter().filter(|(_, s)| context == 0 || s.context == context).map(|(k, _)| *k).collect();
            keys.into_iter().filter_map(|k| streams.remove(&k)).collect()
        };
        for mut stream in mine {
            stop_stream(&mut stream);
        }
        NILE_OK
    })
}

unsafe extern "C" fn shutdown(user: u64) -> NileStatus {
    guard(|| {
        let status = dispose(user, 0);
        if status != NILE_OK {
            return status;
        }
        drop(Box::from_raw(user as *mut State));
        NILE_OK
    })
}

/// Initialize one extension instance from the versioned public host table.
///
/// # Safety
/// `api` must point to a readable ABI table and `out` to writable storage for a full `NileExtension`.
/// The host must obey the SDK callback/lifetime contract.
#[no_mangle]
pub unsafe extern "C" fn nile_extension_init(api: *const NileExtensionApi, out: *mut NileExtension) -> NileStatus {
    guard(|| {
        if out.is_null() {
            return NILE_INVALID;
        }
        let client = match Client::from_api(api) {
            Ok(c) => c,
            Err(e) => return e,
        };
        let need = NILE_CAP_ASYNC | NILE_CAP_BUFFERS | NILE_CAP_EVENTS | NILE_CAP_CANCEL;
        if client.0.capabilities & need != need {
            return NILE_INVALID;
        }
        let user = Box::into_raw(Box::new(State { client, streams: Mutex::default() })) as u64;
        *out = NileExtension {
            user,
            required_capabilities: need,
            cancel: Some(cancel),
            context_dispose: Some(dispose),
            shutdown: Some(shutdown),
            ..NileExtension::default()
        };
        for name in ["media.devices", "media.permission", "media.camera.start", "media.camera.photo", "media.camera.ack", "media.microphone.start"] {
            let status = (client.0.register_operation.unwrap())(client.0.host, NileSlice::new(name.as_bytes()), dispatch, user);
            if status != NILE_OK {
                return status;
            }
        }
        NILE_OK
    })
}
