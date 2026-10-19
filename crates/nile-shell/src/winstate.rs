//! Remembered window geometry (`<html window-remember="main">`): the last normal size and position and
//! whether the window was maximized, kept per app in `<user data>/nile/windows/<app>.json` (one entry per
//! remember name, so an app's other windows keep their own).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Saved {
    /// Outer position in physical pixels.
    pub x: i32,
    pub y: i32,
    /// Inner size in logical pixels.
    pub w: f32,
    pub h: f32,
    pub maximized: bool,
}

/// What names the app: `nile.json`'s `application.id` / `id` next to the page, else the program's name
/// when packed, else the page's file name.
pub fn app_key(page: &Path) -> String {
    let from_config = page
        .parent()
        .and_then(|d| nile_dom::vfs::read_to_string(&d.join("nile.json")).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.pointer("/application/id").or_else(|| v.get("id")).and_then(|i| i.as_str().map(str::to_string)));
    from_config.unwrap_or_else(|| {
        let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().into_owned());
        if nile_dom::vfs::mounted() { std::env::current_exe().ok().and_then(|e| stem(&e)) } else { stem(page) }.unwrap_or_else(|| "nile-app".into())
    })
}

fn file(app: &str) -> Option<PathBuf> {
    let base = match std::env::var_os("NILE_WINDOW_STATE_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => crate::updates::data_base()?.join("nile/windows"),
    };
    let safe: String = app.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
    Some(base.join(format!("{safe}.json")))
}

fn read(app: &str) -> Value {
    file(app).and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}))
}

pub fn load(app: &str, slot: &str) -> Option<Saved> {
    let v = read(app);
    let e = v.get(slot)?;
    let num = |k: &str| e.get(k).and_then(Value::as_f64);
    let (w, h) = (num("w")?, num("h")?);
    (w >= 50.0 && h >= 50.0).then(|| Saved { x: num("x").unwrap_or(0.0) as i32, y: num("y").unwrap_or(0.0) as i32, w: w as f32, h: h as f32, maximized: e.get("maximized").and_then(Value::as_bool).unwrap_or(false) })
}

pub fn save(app: &str, slot: &str, s: &Saved) {
    let Some(path) = file(app) else { return };
    let mut all = read(app);
    all[slot] = json!({"x": s.x, "y": s.y, "w": s.w, "h": s.h, "maximized": s.maximized});
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, all.to_string());
}

/// Is a window at `(x, y)` physical, `w` x `h` physical, reachable on one of the `screens` (physical
/// `(x, y, w, h)`)? A strip of its title bar has to be on a screen, so a window saved on a monitor that is
/// gone opens where the system puts it instead of off screen.
pub fn on_screen(x: i32, y: i32, w: u32, screens: &[(i32, i32, u32, u32)]) -> bool {
    let (px, py) = (x + (w as i32) / 2, y + 16);
    screens.iter().any(|&(sx, sy, sw, sh)| px >= sx && px < sx + sw as i32 && py >= sy && py < sy + sh as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_geometry_round_trips_per_slot() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("NILE_WINDOW_STATE_DIR", dir.path());
        assert_eq!(load("app.one", "main"), None);
        let a = Saved { x: -20, y: 40, w: 800.0, h: 600.5, maximized: false };
        save("app.one", "main", &a);
        save("app.one", "tools", &Saved { maximized: true, ..a });
        assert_eq!(load("app.one", "main"), Some(a));
        assert!(load("app.one", "tools").unwrap().maximized);
        assert_eq!(load("app.two", "main"), None);
        std::fs::write(dir.path().join("app.bad.json"), "{ not json").unwrap();
        assert_eq!(load("app.bad", "main"), None);
        std::env::remove_var("NILE_WINDOW_STATE_DIR");
    }

    #[test]
    fn a_window_on_a_missing_monitor_is_not_restored_there() {
        let screens = [(0, 0, 1920, 1080)];
        assert!(on_screen(100, 100, 800, &screens));
        assert!(on_screen(-300, 50, 800, &screens), "the middle of the title bar is still on screen");
        assert!(!on_screen(2500, 100, 800, &screens), "saved on a second monitor that is gone");
        assert!(!on_screen(100, 2000, 800, &screens));
    }
}
