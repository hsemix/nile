//! Audio playback for the `Audio` class and `<audio>`: tracks decoded from bytes and played through the
//! system's default output device (rodio: CoreAudio, WASAPI, ALSA/PulseAudio).
//!
//! Everything lives on the calling thread (the one running scripts): the output stream is not `Send`.
//! With no output device, or `NILE_AUDIO=off`, tracks play silently against a clock, so a page behaves
//! the same (positions advance, `ended` fires) on a machine without sound.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rodio::{Decoder, Player, Source};

/// Longest a single page may keep loaded at once.
const MAX_TRACKS: usize = 256;

/// What a track reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct State {
    /// Seconds from the start.
    pub position: f64,
    pub playing: bool,
    pub ended: bool,
    /// Seconds, when known.
    pub duration: Option<f64>,
}

/// Something that happened to a track since the last [`take_events`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Ended,
}

struct Track {
    bytes: Arc<[u8]>,
    duration: Option<f64>,
    player: Option<Player>,
    playing: bool,
    ended: bool,
    volume: f32,
    muted: bool,
    looping: bool,
    rate: f32,
    /// The position when not running (seconds), or when the clock last restarted.
    base: f64,
    /// When the silent clock (no device) or the position estimate last restarted.
    since: Option<Instant>,
}

struct Engine {
    device: Option<rodio::MixerDeviceSink>,
    /// Opening the device failed (or it is switched off): play silently.
    silent: bool,
    tracks: HashMap<u32, Track>,
    next: u32,
    events: Vec<(u32, Event)>,
}

thread_local! {
    static ENGINE: RefCell<Engine> = RefCell::new(Engine { device: None, silent: false, tracks: HashMap::new(), next: 1, events: Vec::new() });
}

fn decode(bytes: &Arc<[u8]>) -> Result<Decoder<Cursor<Arc<[u8]>>>, String> {
    Decoder::try_from(Cursor::new(bytes.clone())).map_err(|e| e.to_string())
}

fn seconds(d: Duration) -> f64 {
    d.as_secs_f64()
}

impl Engine {
    fn mixer(&mut self) -> Option<&rodio::mixer::Mixer> {
        if self.device.is_none() && !self.silent {
            let off = std::env::var("NILE_AUDIO").is_ok_and(|v| v.eq_ignore_ascii_case("off"));
            match (!off).then(rodio::DeviceSinkBuilder::open_default_sink) {
                Some(Ok(mut sink)) => {
                    sink.log_on_drop(false);
                    self.device = Some(sink);
                }
                _ => self.silent = true,
            }
        }
        self.device.as_ref().map(|d| d.mixer())
    }
}

impl Track {
    fn effective_volume(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            self.volume
        }
    }

    /// Where playback is now, in seconds: where it last started plus the time since, at the playback rate.
    fn position(&self) -> f64 {
        let mut pos = match (self.playing, self.since) {
            (true, Some(t)) => self.base + t.elapsed().as_secs_f64() * f64::from(self.rate),
            _ => self.base,
        };
        if let Some(d) = self.duration.filter(|d| *d > 0.0) {
            pos = if self.looping && self.playing { pos % d } else { pos.min(d) };
        }
        pos.max(0.0)
    }
}

fn track<R>(id: u32, f: impl FnOnce(&mut Engine, u32) -> Result<R, String>) -> Result<R, String> {
    ENGINE.with(|e| {
        let mut e = e.borrow_mut();
        if !e.tracks.contains_key(&id) {
            return Err("no such audio".into());
        }
        f(&mut e, id)
    })
}

/// Decode `bytes` as audio and register it: its id, and its duration in seconds when known.
pub fn load(bytes: Vec<u8>) -> Result<(u32, Option<f64>), String> {
    let bytes: Arc<[u8]> = bytes.into();
    let decoder = decode(&bytes)?;
    let mut duration = decoder.total_duration().map(seconds);
    if duration.is_none() {
        // A stream without a length in its header (some MP3s): decode it once to count the samples.
        let (rate, channels) = (f64::from(decoder.sample_rate().get()), f64::from(decoder.channels().get()));
        let count = decoder.count() as f64;
        duration = (rate > 0.0 && channels > 0.0).then_some(count / channels / rate);
    }
    ENGINE.with(|e| {
        let mut e = e.borrow_mut();
        if e.tracks.len() >= MAX_TRACKS {
            return Err("too many audio files are loaded".to_string());
        }
        let id = e.next;
        e.next += 1;
        e.tracks.insert(id, Track { bytes, duration, player: None, playing: false, ended: false, volume: 1.0, muted: false, looping: false, rate: 1.0, base: 0.0, since: None });
        Ok((id, duration))
    })
}

/// Start (or resume, or restart after the end) playback.
pub fn play(id: u32) -> Result<(), String> {
    track(id, |e, id| {
        let t = e.tracks.get_mut(&id).ok_or("no such audio")?;
        if t.playing {
            return Ok(());
        }
        if t.ended {
            t.base = 0.0;
            t.ended = false;
            t.player = None;
        }
        // A paused player just resumes; anything else starts a new source at the current position.
        let resume = t.player.as_ref().is_some_and(|p| !p.empty());
        if resume {
            if let Some(p) = &t.player {
                p.play();
            }
            t.playing = true;
            t.since = Some(Instant::now());
            return Ok(());
        }
        let (bytes, base, looping, rate, volume) = (t.bytes.clone(), t.base, t.looping, t.rate, t.effective_volume());
        t.player = None;
        let player = match e.mixer() {
            Some(mixer) => {
                let player = Player::connect_new(mixer);
                player.set_volume(volume);
                player.set_speed(rate);
                if looping {
                    player.append(Decoder::new_looped(Cursor::new(bytes)).map_err(|x| x.to_string())?);
                } else {
                    player.append(decode(&bytes)?);
                }
                if base > 0.0 {
                    let _ = player.try_seek(Duration::from_secs_f64(base));
                }
                Some(player)
            }
            None => None,
        };
        let t = e.tracks.get_mut(&id).ok_or("no such audio")?;
        t.player = player;
        t.playing = true;
        t.since = Some(Instant::now());
        Ok(())
    })
}

pub fn pause(id: u32) -> Result<(), String> {
    track(id, |e, id| {
        let t = e.tracks.get_mut(&id).ok_or("no such audio")?;
        if !t.playing {
            return Ok(());
        }
        t.base = t.position();
        if let Some(p) = &t.player {
            p.pause();
        }
        t.playing = false;
        t.since = None;
        Ok(())
    })
}

/// Stop and rewind to the start.
pub fn stop(id: u32) -> Result<(), String> {
    track(id, |e, id| {
        let t = e.tracks.get_mut(&id).ok_or("no such audio")?;
        t.player = None;
        t.playing = false;
        t.ended = false;
        t.base = 0.0;
        t.since = None;
        Ok(())
    })
}

/// Move to `secs`, playing or not.
pub fn seek(id: u32, secs: f64) -> Result<(), String> {
    let was_playing = state(id).is_some_and(|s| s.playing);
    track(id, |e, id| {
        let t = e.tracks.get_mut(&id).ok_or("no such audio")?;
        let max = t.duration.unwrap_or(f64::MAX);
        t.base = secs.clamp(0.0, max);
        t.player = None;
        t.ended = false;
        t.playing = false;
        t.since = None;
        Ok(())
    })?;
    if was_playing {
        play(id)?;
    }
    Ok(())
}

pub fn set_volume(id: u32, volume: f32) -> Result<(), String> {
    track(id, |e, id| {
        let t = e.tracks.get_mut(&id).ok_or("no such audio")?;
        t.volume = volume.clamp(0.0, 1.0);
        if let Some(p) = &t.player {
            p.set_volume(t.effective_volume());
        }
        Ok(())
    })
}

pub fn set_muted(id: u32, muted: bool) -> Result<(), String> {
    track(id, |e, id| {
        let t = e.tracks.get_mut(&id).ok_or("no such audio")?;
        t.muted = muted;
        if let Some(p) = &t.player {
            p.set_volume(t.effective_volume());
        }
        Ok(())
    })
}

pub fn set_loop(id: u32, looping: bool) -> Result<(), String> {
    let (changed, was_playing) = ENGINE.with(|e| {
        let mut e = e.borrow_mut();
        match e.tracks.get_mut(&id) {
            Some(t) => {
                let changed = t.looping != looping;
                t.looping = looping;
                (changed, t.playing)
            }
            None => (false, false),
        }
    });
    // A running source was built for the old setting: rebuild it at the current position.
    if changed && was_playing {
        let pos = state(id).map_or(0.0, |s| s.position);
        seek(id, pos)?;
        play(id)?;
    }
    Ok(())
}

pub fn set_rate(id: u32, rate: f32) -> Result<(), String> {
    track(id, |e, id| {
        let t = e.tracks.get_mut(&id).ok_or("no such audio")?;
        let rate = rate.clamp(0.25, 4.0);
        if t.playing {
            // The clock keeps what has played so far at the old rate.
            t.base = t.position();
            t.since = Some(Instant::now());
        }
        t.rate = rate;
        if let Some(p) = &t.player {
            p.set_speed(rate);
        }
        Ok(())
    })
}

pub fn state(id: u32) -> Option<State> {
    ENGINE.with(|e| {
        let e = e.borrow();
        let t = e.tracks.get(&id)?;
        Some(State { position: t.position(), playing: t.playing, ended: t.ended, duration: t.duration })
    })
}

/// Free a track (its decoded source and player).
pub fn unload(id: u32) {
    ENGINE.with(|e| {
        e.borrow_mut().tracks.remove(&id);
    });
}

/// Notice tracks that finished; call regularly while [`busy`].
pub fn poll() {
    ENGINE.with(|e| {
        let mut e = e.borrow_mut();
        let Engine { tracks, events, .. } = &mut *e;
        for (id, t) in tracks.iter_mut().filter(|(_, t)| t.playing) {
            let done = match &t.player {
                Some(p) => p.empty(),
                None => t.duration.is_some_and(|d| !t.looping && t.position() >= d - 0.0005),
            };
            if done {
                t.base = t.duration.unwrap_or_else(|| t.position());
                t.playing = false;
                t.ended = true;
                t.since = None;
                t.player = None;
                events.push((*id, Event::Ended));
            }
        }
    });
}

pub fn take_events() -> Vec<(u32, Event)> {
    ENGINE.with(|e| std::mem::take(&mut e.borrow_mut().events))
}

/// Whether something is playing (so the caller should poll again soon).
pub fn busy() -> bool {
    ENGINE.with(|e| e.borrow().tracks.values().any(|t| t.playing))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mono 8 kHz, 16-bit WAV of `ms` milliseconds of silence.
    fn wav(ms: u32) -> Vec<u8> {
        let samples = 8 * ms;
        let data = samples * 2;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes()); // PCM
        v.extend_from_slice(&1u16.to_le_bytes()); // mono
        v.extend_from_slice(&8000u32.to_le_bytes());
        v.extend_from_slice(&16000u32.to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data.to_le_bytes());
        v.resize(v.len() + data as usize, 0);
        v
    }

    #[test]
    fn a_silent_track_plays_pauses_seeks_and_ends() {
        std::env::set_var("NILE_AUDIO", "off");
        let (id, duration) = load(wav(300)).unwrap();
        assert!((duration.unwrap() - 0.3).abs() < 0.01);
        assert_eq!(state(id).unwrap().position, 0.0);
        play(id).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let s = state(id).unwrap();
        assert!(s.playing && s.position > 0.05 && s.position < 0.2, "{s:?}");
        pause(id).unwrap();
        let held = state(id).unwrap().position;
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(state(id).unwrap().position, held, "paused time stands still");
        seek(id, 0.25).unwrap();
        assert!((state(id).unwrap().position - 0.25).abs() < 1e-6);
        play(id).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        poll();
        assert_eq!(take_events(), vec![(id, Event::Ended)]);
        let s = state(id).unwrap();
        assert!(s.ended && !s.playing && (s.position - 0.3).abs() < 0.01);
        play(id).unwrap();
        assert!(state(id).unwrap().playing && !state(id).unwrap().ended, "play after the end starts over");
        stop(id).unwrap();
        assert_eq!(state(id).unwrap().position, 0.0);
        unload(id);
        assert!(state(id).is_none());
    }

    #[test]
    fn looping_never_ends_and_junk_is_refused() {
        std::env::set_var("NILE_AUDIO", "off");
        let (id, _) = load(wav(100)).unwrap();
        set_loop(id, true).unwrap();
        play(id).unwrap();
        std::thread::sleep(Duration::from_millis(250));
        poll();
        assert!(take_events().is_empty() && state(id).unwrap().playing);
        assert!(state(id).unwrap().position < 0.1);
        assert!(load(b"not audio at all".to_vec()).is_err());
    }
}
