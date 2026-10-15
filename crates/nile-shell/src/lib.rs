//! `nile` — opens an HTML file in a native window, or renders it to PNG.
//!
//!   nile page.html                      open a window (F5 / Ctrl-R reloads, wheel scrolls)
//!   nile page.html --png out.png        render headless
//!        [--width 900] [--height 640] [--scale 2]
//!        [--hover X,Y] [--press X,Y] [--click X,Y]   simulate the pointer (logical px)
//!        [--wheel X,Y,DY]              scroll at a point (negative DY scrolls content up)
//!        [--type TEXT]                 type into the focused input ({Backspace}, {Shift+ArrowLeft}, {Cmd+a}, ... for keys)
//!        [--compose TEXT]              show TEXT as an input method's composition at the caret
//!        [--page N]                    with --png: render page N (1-based) of the paginated document instead
//!   nile                                open main.html / index.html next to the program (or in the pack)
//!   nile folder                         open the folder's main.html / index.html
//!   nile --pack folder [-o app]         write `app`: a copy of this program with the folder's files appended
//!        [--icon icon.png|ico]          the executable's icon (macOS: makes `app.app`; Windows: in the .exe)
//!        [--name N] [--version 1.2.3] [--description D] [--company C] [--copyright T]
//!                                       what the OS shows about it (Windows: file properties; macOS: Info.plist)
//!        [--category Utility;]          Linux: writes `app.desktop` (+ `app.png`) next to it; `--install` copies them
//!                                       to ~/.local/share
//!   nile --brand program.exe -o out.exe  the same icon and details for any Windows program (release builds use it)
//!   nile page.html --pdf out.pdf        write every page as a PDF ([--wait MS] first)
//!        [--wait MS]                   let timers run this long before the snapshot

#[cfg(windows)]
mod alpha_win;
mod access;
mod app;
mod bundle;
mod dialogs;
mod material;
mod menu;

use std::{path::PathBuf, rc::Rc, time::{Duration, Instant}};

use nile_engine::{ModifiersState as Mods, Page, SystemClipboard};
use skia_safe::{AlphaType, ColorType, EncodedImageFormat, ImageInfo};
use winit::{event_loop::EventLoop, keyboard::ModifiersState};

/// winit's modifier state as the engine's.
fn mods_of(m: ModifiersState) -> Mods {
    let mut out = Mods::default();
    for (on, bit) in [(m.shift_key(), Mods::SHIFT), (m.control_key(), Mods::CONTROL), (m.alt_key(), Mods::ALT), (m.super_key(), Mods::SUPER)] {
        if on {
            out |= bit;
        }
    }
    out
}

/// Pointer simulation for headless renders: hover (and optionally press) at a logical point.
struct Pointer {
    x: f32,
    y: f32,
    press: bool,
    /// Release again after pressing (a full click).
    release: bool,
    /// A right click (the `contextmenu` event).
    context: bool,
    /// Wheel delta to apply at the point (positive scrolls content down, like wheel-up).
    wheel: f32,
}

/// Options for a headless render.
struct Snapshot {
    out: String,
    width: u32,
    height: u32,
    scale: f32,
    pointer: Option<Pointer>,
    typed: Option<String>,
    /// `--compose TEXT`: show TEXT as an input method's composition at the caret.
    compose: Option<String>,
    wait_ms: u64,
    settle_ms: u64,
    /// `--select X1,Y1,X2,Y2`: drag a text selection.
    select: Option<[f32; 4]>,
}

fn render_png(page: &mut Page, snap: Snapshot) {
    let Snapshot { out, width, height, scale, pointer, typed, compose, wait_ms, settle_ms, select } = snap;
    let out = out.as_str();
    // Let timers run (animations, setTimeout) before the snapshot.
    let wait = |page: &mut Page, ms: u64| {
        let deadline = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < deadline {
            let (_, next) = page.tick();
            let Some(next) = next else { break };
            std::thread::sleep(next.min(deadline.saturating_duration_since(Instant::now())));
        }
        page.run_timers();
    };
    wait(page, wait_ms);
    if let Some(p) = pointer {
        // Lay out first so there is something to hit test.
        page.layout(width as f32, height as f32);
        page.pointer_moved(p.x, p.y);
        if p.press {
            page.press();
        }
        if p.release {
            page.release();
        }
        if p.context {
            page.context_menu();
        }
        if p.wheel != 0.0 {
            page.wheel(0.0, p.wheel);
        }
    }
    if let Some([x1, y1, x2, y2]) = select {
        page.layout(width as f32, height as f32);
        page.pointer_moved(x1, y1);
        page.press();
        page.pointer_moved(x2, y2);
        page.release();
    }
    if let Some(text) = typed {
        let mut rest = text.as_str();
        while !rest.is_empty() {
            let (key, tail) = match rest.strip_prefix('{').and_then(|r| r.split_once('}')) {
                Some((name, tail)) => (name.to_string(), tail),
                None => {
                    let c = rest.chars().next().unwrap();
                    (c.to_string(), &rest[c.len_utf8()..])
                }
            };
            rest = tail;
            // `{Shift+ArrowRight}`, `{Cmd+a}`, `{Ctrl+Shift+End}`: modifiers before the key.
            let mut parts: Vec<&str> = key.split('+').collect();
            let name = parts.pop().unwrap_or("");
            let key = if name == "Space" { " ".to_string() } else { name.to_string() };
            let mut mods = Mods::default();
            for m in parts {
                match m.to_ascii_lowercase().as_str() {
                    "shift" => mods |= Mods::SHIFT,
                    "ctrl" | "control" => mods |= Mods::CONTROL,
                    "alt" => mods |= Mods::ALT,
                    "cmd" | "super" | "meta" => mods |= Mods::SUPER,
                    _ => {}
                }
            }
            let (proceed, _) = page.key("keydown", key.clone(), mods);
            if proceed {
                page.default_key(&key, mods);
            }
            page.key("keyup", key, mods);
        }
    }
    if let Some(text) = compose {
        page.ime_preedit(&text, None);
    }
    // After the input: transitions it started, and anything it scheduled.
    wait(page, settle_ms);
    let (pw, ph) = ((width as f32 * scale) as u32, (height as f32 * scale) as u32);
    let mut pixels = vec![0u8; (pw * ph * 4) as usize];
    page.paint(&mut pixels, pw, ph, scale);

    let info = ImageInfo::new((pw as i32, ph as i32), ColorType::BGRA8888, AlphaType::Premul, None);
    let image = skia_safe::images::raster_from_data(
        &info,
        skia_safe::Data::new_copy(&pixels),
        pw as usize * 4,
    )
    .expect("pixel buffer matches image info");
    let png = image.encode(None, EncodedImageFormat::PNG, None).expect("PNG encoding");
    std::fs::write(out, png.as_bytes()).expect("writing PNG");
    println!("wrote {out} ({pw}x{ph})");
}

/// `--pdf` and `--png --page N`: the document cut into pages.
fn paged_output(page: &mut Page, pdf: Option<String>, png: Option<String>, page_no: Option<usize>, scale: f32, wait_ms: u64) {
    let deadline = Instant::now() + Duration::from_millis(wait_ms);
    while Instant::now() < deadline {
        let (_, next) = page.tick();
        let Some(next) = next else { break };
        std::thread::sleep(next.min(deadline.saturating_duration_since(Instant::now())));
    }
    page.run_timers();
    if let Some(out) = pdf {
        let mut bytes = Vec::new();
        let title = page.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let count = page.write_pdf(&mut bytes, &title);
        std::fs::write(&out, bytes).expect("writing PDF");
        println!("wrote {out} ({count} pages)");
    }
    if let (Some(out), Some(n)) = (png, page_no) {
        let count = page.paginate();
        let Some((pixels, w, h)) = n.checked_sub(1).filter(|i| *i < count).and_then(|i| page.render_page(i, scale)) else {
            eprintln!("page {n} doesn't exist: the document has {count} pages");
            std::process::exit(1);
        };
        let info = ImageInfo::new((w as i32, h as i32), ColorType::BGRA8888, AlphaType::Premul, None);
        let image = skia_safe::images::raster_from_data(&info, skia_safe::Data::new_copy(&pixels), w as usize * 4).expect("pixel buffer matches image info");
        let png = image.encode(None, EncodedImageFormat::PNG, None).expect("PNG encoding");
        std::fs::write(&out, png.as_bytes()).expect("writing PNG");
        println!("wrote {out} (page {n} of {count}, {w}x{h})");
    }
}

/// `nile --pack folder [-o out]`: this program with the folder's files appended, as one executable.
fn pack_command(args: &[String]) -> ! {
    let mut folder = None;
    let mut out = None;
    let mut icon: Option<PathBuf> = None;
    let mut info = bundle::AppInfo::default();
    let (mut install, mut desktop) = (false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" | "--out" => out = it.next().cloned(),
            "--icon" => icon = it.next().map(PathBuf::from),
            "--name" => info.name = it.next().cloned().unwrap_or_default(),
            "--version" => info.version = it.next().cloned().unwrap_or_default(),
            "--description" => info.description = it.next().cloned().unwrap_or_default(),
            "--company" => info.company = it.next().cloned().unwrap_or_default(),
            "--copyright" => info.copyright = it.next().cloned().unwrap_or_default(),
            "--category" => {
                info.category = it.next().cloned().unwrap_or_default();
                desktop = true;
            }
            "--install" => install = true,
            _ => folder = Some(PathBuf::from(a)),
        }
    }
    let Some(folder) = folder.filter(|f| f.is_dir()) else {
        eprintln!("usage: nile --pack <folder> [-o <output>]");
        std::process::exit(2);
    };
    if nile_dom::vfs::start_file(&folder).is_none() {
        eprintln!("warning: {} has no main.html / index.html to start from", folder.display());
    }
    let name = std::fs::canonicalize(&folder).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "app".into());
    let out = PathBuf::from(out.unwrap_or_else(|| if cfg!(windows) { format!("{name}.exe") } else { name.clone() }));
    if info.name.is_empty() {
        info.name = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name);
    }
    #[cfg(target_os = "windows")]
    let has_info = !(info.version.is_empty() && info.description.is_empty() && info.company.is_empty() && info.copyright.is_empty());
    // The program's own window icon (Linux and Windows title bars; Wayland needs the .desktop entry too):
    // a `.png` goes into the archive and `App::create` reads it back.
    let embedded: Vec<(&str, Vec<u8>)> = match icon.as_deref().filter(|i| i.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"))) {
        Some(png) => std::fs::read(png).map(|b| vec![(nile_dom::vfs::ICON_FILE, b)]).unwrap_or_default(),
        None => Vec::new(),
    };
    // No `--icon`: Nile's own icon, where the system shows one for the file (a Windows executable, a macOS bundle).
    let bundle_wanted = icon.is_some();
    let default_icon = std::env::temp_dir().join(format!("nile-default-icon-{}.png", std::process::id()));
    if icon.is_none() && (cfg!(windows) || out.extension().is_some_and(|e| e == "app")) && std::fs::write(&default_icon, bundle::DEFAULT_ICON).is_ok() {
        icon = Some(default_icon.clone());
    }
    // macOS: an `.app` bundle (what Finder and the Dock show an icon for) when asked for with an icon or an
    // output name ending in `.app`. The archive then goes in its own file inside it.
    let bundle_mode = cfg!(target_os = "macos") && (bundle_wanted || out.extension().is_some_and(|e| e == "app"));
    let archive = out.with_extension("nilepack.tmp");
    let result = nile_dom::vfs::runtime_bytes().and_then(|runtime| {
        if bundle_mode {
            std::fs::write(&out, &runtime)?;
            return nile_dom::vfs::pack_with(&folder, &embedded, &[], &archive);
        }
        // On Windows the icon goes into the executable before the archive is appended.
        #[cfg(target_os = "windows")]
        let runtime = if icon.is_some() || has_info { bundle::with_resources(runtime, icon.as_deref(), &info)? } else { runtime };
        nile_dom::vfs::pack_with(&folder, &embedded, &runtime, &out)
    });
    match result {
        Ok((files, stored)) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755));
            }
            // A modified Mach-O has to be signed again to start on Apple silicon.
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("codesign").args(["--force", "--sign", "-"]).arg(&out).output();
            #[cfg(target_os = "macos")]
            let out = if bundle_mode {
                let name = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "App".into());
                match bundle::app_bundle(&out, &archive, &name, icon.as_deref(), &info) {
                    Ok(app) => app,
                    Err(e) => {
                        eprintln!("could not make the app bundle: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                out
            };
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            {
                // A `.desktop` entry (and the icon beside it) makes the program show up in menus and docks.
                if icon.is_some() || desktop || install {
                    linux_entry(&out, icon.as_deref(), &info, install);
                }
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let _ = (desktop, install);
            println!("wrote {} ({files} files, {} KB packed)", out.display(), stored / 1024);
            let _ = std::fs::remove_file(&default_icon);
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("could not pack {}: {e}", folder.display());
            let _ = std::fs::remove_file(&default_icon);
            std::process::exit(1);
        }
    }
}

/// `nile --brand program.exe -o out.exe [--icon x.png] [--name N] [--version V] ...`: a copy of any Windows
/// program with an icon and version information written into it (how the release builds get Nile's own).
/// Elsewhere it only copies the file.
fn brand_command(args: &[String]) -> ! {
    let (mut input, mut out, mut icon) = (None, None, None::<PathBuf>);
    let mut info = bundle::AppInfo::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" | "--out" => out = it.next().map(PathBuf::from),
            "--icon" => icon = it.next().map(PathBuf::from),
            "--name" => info.name = it.next().cloned().unwrap_or_default(),
            "--version" => info.version = it.next().cloned().unwrap_or_default(),
            "--description" => info.description = it.next().cloned().unwrap_or_default(),
            "--company" => info.company = it.next().cloned().unwrap_or_default(),
            "--copyright" => info.copyright = it.next().cloned().unwrap_or_default(),
            _ => input = Some(PathBuf::from(a)),
        }
    }
    let (Some(input), Some(out)) = (input, out) else {
        eprintln!("usage: nile --brand <program> -o <output> [--icon icon.png] [--name N] [--version 1.2.3] [--description D] [--company C] [--copyright T]");
        std::process::exit(2);
    };
    if info.name.is_empty() {
        info.name = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    }
    let result = std::fs::read(&input).and_then(|bytes| {
        #[cfg(target_os = "windows")]
        let bytes = {
            let default_icon = std::env::temp_dir().join(format!("nile-default-icon-{}.png", std::process::id()));
            let icon_file = match &icon {
                Some(i) => i.clone(),
                None => {
                    std::fs::write(&default_icon, bundle::DEFAULT_ICON)?;
                    default_icon.clone()
                }
            };
            let branded = bundle::with_resources(bytes, Some(&icon_file), &info);
            let _ = std::fs::remove_file(&default_icon);
            branded?
        };
        #[cfg(not(target_os = "windows"))]
        let _ = (&icon, &info);
        std::fs::write(&out, bytes)
    });
    match result {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755));
            }
            println!("wrote {}", out.display());
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("could not brand {}: {e}", input.display());
            std::process::exit(1);
        }
    }
}

/// Linux: `<out>.desktop` (and `<out>.png`) next to the program; with `install`, in `~/.local/share` too.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn linux_entry(out: &std::path::Path, icon: Option<&std::path::Path>, info: &bundle::AppInfo, install: bool) {
    let exe = std::fs::canonicalize(out).unwrap_or_else(|_| out.to_path_buf());
    let png = icon.filter(|i| i.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"))).map(|i| {
        let target = exe.with_extension("png");
        let _ = std::fs::copy(i, &target);
        target
    });
    let entry = exe.with_extension("desktop");
    match std::fs::write(&entry, bundle::desktop_entry(info, &exe, png.as_deref())) {
        Ok(()) => println!("wrote {}", entry.display()),
        Err(e) => eprintln!("could not write {}: {e}", entry.display()),
    }
    if install {
        let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
        if let (Some(data), Some(file)) = (data, entry.file_name()) {
            let dir = data.join("applications");
            let _ = std::fs::create_dir_all(&dir);
            match std::fs::copy(&entry, dir.join(file)) {
                Ok(_) => println!("installed {}", dir.join(file).display()),
                Err(e) => eprintln!("could not install the entry: {e}"),
            }
        }
    }
}

fn main() {
    // A packed app finds its files behind its own executable.
    nile_dom::vfs::mount_self();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().is_some_and(|a| a == "--brand") {
        brand_command(&argv[1..]);
    }
    if argv.first().is_some_and(|a| a == "--pack") {
        pack_command(&argv[1..]);
    }
    let mut args = argv.into_iter();
    let mut file = None;
    let mut png = None;
    let mut pointer = None;
    let mut wait_ms = 0u64;
    let mut settle_ms = 0u64;
    let mut select = None;
    let mut typed = None;
    let mut compose = None;
    let mut pdf = None;
    let mut page_no = None;
    let (mut width, mut height, mut scale) = (900u32, 640u32, 1.0f32);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--png" => png = args.next(),
            "--pdf" => pdf = args.next(),
            "--page" => page_no = args.next().and_then(|v| v.parse::<usize>().ok()),
            "--width" => width = args.next().and_then(|v| v.parse().ok()).unwrap_or(width),
            "--height" => height = args.next().and_then(|v| v.parse().ok()).unwrap_or(height),
            "--hover" | "--press" | "--click" | "--rclick" | "--wheel" => {
                // X,Y (or X,Y,DY for --wheel), in logical px.
                let nums: Vec<f32> = args.next().map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect()).unwrap_or_default();
                if let [x, y, rest @ ..] = nums.as_slice() {
                    pointer = Some(Pointer {
                        x: *x,
                        y: *y,
                        press: arg == "--press" || arg == "--click",
                        release: arg == "--click",
                        context: arg == "--rclick",
                        wheel: if arg == "--wheel" { rest.first().copied().unwrap_or(0.0) } else { 0.0 },
                    });
                }
            }
            "--type" => typed = args.next(),
            "--compose" => compose = args.next(),
            "--select" => {
                let v: Vec<f32> = args.next().unwrap_or_default().split(',').filter_map(|n| n.parse().ok()).collect();
                if let [a, b, c, d] = v[..] {
                    select = Some([a, b, c, d]);
                }
            }
            "--settle" => settle_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--wait" => wait_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--scale" => scale = args.next().and_then(|v| v.parse().ok()).unwrap_or(scale),
            _ => file = Some(PathBuf::from(arg)),
        }
    }
    // No file (or a folder): the start file of the pack, the program's folder, or the folder given.
    let file = match file {
        Some(f) if f.is_dir() => nile_dom::vfs::start_file(&f),
        Some(f) => Some(f),
        None if nile_dom::vfs::mounted() => nile_dom::vfs::start_file(&nile_dom::vfs::mount_path()),
        None => std::env::current_exe().ok().and_then(|e| e.parent().and_then(nile_dom::vfs::start_file)).or_else(|| nile_dom::vfs::start_file(std::path::Path::new("."))),
    };
    let Some(file) = file else {
        eprintln!("usage: nile [<file.html> | <folder>] [--png out.png] [--width N] [--height N] [--scale N] [--hover X,Y] [--press X,Y] [--click X,Y] [--wheel X,Y,DY] [--type TEXT] [--wait MS]");
        std::process::exit(2);
    };

    // A window knows its id before its scripts run, so `new Window()` at startup can name its opener.
    let info = nile_script::WindowInfo { id: nile_script::next_window_id(), ..Default::default() };
    let headless = png.is_some() || pdf.is_some();
    // A `.js` start file is a bootstrap script (Sciter's `run.js`): it runs in a hidden window and opens the real ones.
    let bootstrap = !headless && file.extension().is_some_and(|e| e == "js");
    let mut page = if bootstrap {
        let dir = file.parent().map(PathBuf::from).unwrap_or_default();
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Page::html_window(&format!("<html window-state=\"hidden\"><body><script type=\"module\" src=\"{name}\"></script></body></html>"), dir, (width as f32, height as f32), info)
    } else if !headless { Page::load_window(file, (width as f32, height as f32), info) } else { Page::load(file, (width as f32, height as f32), 0.0) };
    if !headless {
        page.clipboard = Box::new(SystemClipboard::default());
        page.set_dialogs(Rc::new(dialogs::NativeDialogs));
    }
    if pdf.is_some() || (png.is_some() && page_no.is_some()) {
        paged_output(&mut page, pdf, png, page_no, scale, wait_ms);
        return;
    }
    if let Some(out) = png {
        render_png(&mut page, Snapshot { out, width, height, scale, pointer, typed, compose, wait_ms, settle_ms, select });
        return;
    }

    let mut builder = EventLoop::<app::UserEvent>::with_user_event();
    #[cfg(target_os = "windows")]
    menu::install_hook(&mut builder);
    let event_loop = builder.build().expect("event loop");
    let mut app = app::App::new(page, event_loop.create_proxy());
    app.bootstrap = bootstrap;
    event_loop.run_app(&mut app).expect("run app");
    if app.exit_code != 0 {
        std::process::exit(app.exit_code);
    }
}
