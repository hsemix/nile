//! Recreate the game's original procedural PCM assets; no external tools needed.
use std::{f64::consts::TAU, fs, io, path::Path};
const RATE: u32 = 22050;
fn wav(path: &Path, seconds: f64, sample: impl Fn(f64) -> f64) -> io::Result<()> {
    let count = (seconds * RATE as f64).round() as u32;
    let mut bytes = Vec::with_capacity(44 + count as usize * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + count * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(count * 2).to_le_bytes());
    for index in 0..count {
        let value = (sample(index as f64 / RATE as f64).clamp(-1., 1.) * i16::MAX as f64) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fs::write(path, bytes)
}
fn main() -> io::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/neon-breaker/sounds");
    fs::create_dir_all(&root)?;
    for (name, frequency, end, duration) in [
        ("brick", 880., 1320., 0.095),
        ("paddle", 330., 440., 0.085),
        ("launch", 220., 880., 0.24),
        ("power", 523.25, 1046.5, 0.48),
        ("loss", 330., 82.5, 0.55),
        ("level", 440., 1320., 0.7),
    ] {
        wav(&root.join(format!("{name}.wav")), duration, |t| {
            let envelope = (t / 0.008).min(1.) * (1. - t / duration).max(0.).powf(1.8);
            let phase = TAU * (frequency * t + (end - frequency) * t * t / (2. * duration));
            (phase.sin() + 0.2 * (phase * 2.).sin()) * envelope * 0.45
        })?;
    }
    let notes = [
        220., 329.63, 440., 523.25, 196., 293.66, 392., 493.88, 174.61, 261.63, 349.23, 440., 196.,
        293.66, 392., 493.88,
    ];
    wav(&root.join("music.wav"), 8., |t| {
        let beat = t / 0.5;
        let index = beat.floor() as usize % notes.len();
        let local = t % 0.5;
        let envelope = (local / 0.015).min(1.) * (1. - local / 0.5).powf(2.2);
        let melody = (TAU * notes[index] * local).sin() * envelope * 0.16;
        let bass = (TAU * notes[(index / 4) * 4] * 0.5 * t).sin()
            * (std::f64::consts::PI * (t % 2.) / 2.).sin().powi(2)
            * 0.08;
        (melody + bass) * (t / 0.03).min(1.) * ((8. - t) / 0.03).min(1.)
    })?;
    println!("Generated seven PCM WAV assets in {}", root.display());
    Ok(())
}
