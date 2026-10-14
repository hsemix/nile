//! Making a packed program look like an application: an icon for the executable, and on macOS an
//! `.app` bundle. (`nile --pack folder -o App --icon icon.png`.)

use std::path::{Path, PathBuf};

/// Windows: `runtime` (a `nile.exe` without an archive) with `icon` (`.ico`, or a `.png`) as its
/// application icon. Returns the new bytes; the archive is appended after this.
#[cfg(target_os = "windows")]
pub fn with_icon(runtime: Vec<u8>, icon: &Path) -> std::io::Result<Vec<u8>> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::{BeginUpdateResourceW, EndUpdateResourceW, UpdateResourceW};

    // The icon's images: `(width, height, bytes)`, each a bitmap or a PNG.
    let data = std::fs::read(icon)?;
    let images: Vec<(u8, u8, Vec<u8>)> = if data.starts_with(b"\x89PNG") && data.len() > 24 {
        let dim = |at: usize| u32::from_be_bytes(data[at..at + 4].try_into().unwrap_or_default());
        // 256 is stored as 0.
        vec![(dim(16).min(256) as u8, dim(20).min(256) as u8, data)]
    } else if data.len() >= 6 && data[2] == 1 {
        let count = u16::from_le_bytes([data[4], data[5]]) as usize;
        (0..count)
            .filter_map(|i| {
                let e = data.get(6 + i * 16..6 + i * 16 + 16)?;
                let (size, offset) = (u32::from_le_bytes(e[8..12].try_into().ok()?) as usize, u32::from_le_bytes(e[12..16].try_into().ok()?) as usize);
                Some((e[0], e[1], data.get(offset..offset + size)?.to_vec()))
            })
            .collect()
    } else {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "the icon must be a .ico or .png file"));
    };
    if images.is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "the icon has no images"));
    }
    let tmp = std::env::temp_dir().join(format!("nile-icon-{}.exe", std::process::id()));
    std::fs::write(&tmp, &runtime)?;
    let wide: Vec<u16> = tmp.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // RT_ICON = 3, RT_GROUP_ICON = 14; language 0 (neutral).
    let ok = unsafe {
        let h = BeginUpdateResourceW(wide.as_ptr(), 0);
        if h == 0 {
            false
        } else {
            let mut group = vec![0u8, 0, 1, 0];
            group.extend_from_slice(&(images.len() as u16).to_le_bytes());
            let mut good = true;
            for (i, (w, h_, bytes)) in images.iter().enumerate() {
                let id = (i + 1) as u16;
                good &= UpdateResourceW(h, 3 as _, id as usize as _, 0, bytes.as_ptr().cast(), bytes.len() as u32) != 0;
                // GRPICONDIR entry: size, planes, bit count, bytes, id.
                group.extend_from_slice(&[*w, *h_, 0, 0, 1, 0, 32, 0]);
                group.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                group.extend_from_slice(&id.to_le_bytes());
            }
            good &= UpdateResourceW(h, 14 as _, 1 as _, 0, group.as_ptr().cast(), group.len() as u32) != 0;
            EndUpdateResourceW(h, i32::from(!good)) != 0 && good
        }
    };
    let out = std::fs::read(&tmp);
    let _ = std::fs::remove_file(&tmp);
    if !ok {
        return Err(std::io::Error::other("could not write the icon resource"));
    }
    out
}

/// macOS: turn the plain program at `exe` into `<name>.app` (`Contents/MacOS/<name>`, an `Info.plist`,
/// the icon as `.icns` and the archive at `Contents/Resources/app.nilepack`: a signed executable can't
/// have an archive appended) and return the bundle's path. `icon` is a `.png` (turned into an
/// `.icns` with `sips` and `iconutil`) or an `.icns`.
#[cfg(target_os = "macos")]
pub fn app_bundle(exe: &Path, archive: &Path, name: &str, icon: Option<&Path>) -> std::io::Result<PathBuf> {
    use std::process::Command;
    let app = exe.with_file_name(format!("{name}.app"));
    let _ = std::fs::remove_dir_all(&app);
    let (macos, resources) = (app.join("Contents/MacOS"), app.join("Contents/Resources"));
    std::fs::create_dir_all(&macos)?;
    std::fs::create_dir_all(&resources)?;
    std::fs::rename(archive, resources.join("app.nilepack")).or_else(|_| std::fs::copy(archive, resources.join("app.nilepack")).and_then(|_| std::fs::remove_file(archive)))?;
    std::fs::rename(exe, macos.join(name)).or_else(|_| std::fs::copy(exe, macos.join(name)).and_then(|_| std::fs::remove_file(exe)))?;
    let mut icon_key = String::new();
    if let Some(icon) = icon {
        let target = resources.join("AppIcon.icns");
        if icon.extension().is_some_and(|e| e == "icns") {
            std::fs::copy(icon, &target)?;
        } else {
            let set = std::env::temp_dir().join(format!("nile-iconset-{}.iconset", std::process::id()));
            let _ = std::fs::remove_dir_all(&set);
            std::fs::create_dir_all(&set)?;
            for (size, file) in [(16, "icon_16x16"), (32, "icon_16x16@2x"), (32, "icon_32x32"), (64, "icon_32x32@2x"), (128, "icon_128x128"), (256, "icon_128x128@2x"), (256, "icon_256x256"), (512, "icon_256x256@2x"), (512, "icon_512x512"), (1024, "icon_512x512@2x")] {
                Command::new("sips").args(["-z", &size.to_string(), &size.to_string()]).arg(icon).arg("--out").arg(set.join(format!("{file}.png"))).output()?;
            }
            let made = Command::new("iconutil").args(["-c", "icns"]).arg(&set).arg("-o").arg(&target).output()?;
            let _ = std::fs::remove_dir_all(&set);
            if !made.status.success() {
                return Err(std::io::Error::other(String::from_utf8_lossy(&made.stderr).into_owned()));
            }
        }
        icon_key = "  <key>CFBundleIconFile</key><string>AppIcon</string>\n".to_string();
    }
    let id: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    std::fs::write(
        app.join("Contents/Info.plist"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n  <key>CFBundleName</key><string>{name}</string>\n  <key>CFBundleDisplayName</key><string>{name}</string>\n  <key>CFBundleExecutable</key><string>{name}</string>\n  <key>CFBundleIdentifier</key><string>app.nile.{id}</string>\n  <key>CFBundlePackageType</key><string>APPL</string>\n  <key>CFBundleVersion</key><string>1.0</string>\n  <key>NSHighResolutionCapable</key><true/>\n{icon_key}</dict></plist>\n"
        ),
    )?;
    // The executable was changed after it was built: sign the whole bundle again (ad hoc).
    let _ = Command::new("codesign").args(["--force", "--deep", "--sign", "-"]).arg(&app).output();
    Ok(app)
}

/// Not on this platform: the program stays a plain executable.
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
pub fn app_bundle(exe: &Path, _archive: &Path, _name: &str, _icon: Option<&Path>) -> std::io::Result<PathBuf> {
    Ok(exe.to_path_buf())
}
