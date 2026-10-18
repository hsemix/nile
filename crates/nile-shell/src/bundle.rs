//! Making a packed program look like an application: an icon and version information for the
//! executable (Windows resources), an `.app` bundle on macOS and a `.desktop` entry on Linux.
//! (`nile --pack folder -o App --icon icon.png --version 1.2.0 --company "Acme"`.)

use std::path::{Path, PathBuf};

/// Nile's own icon (`assets/icon.html` rendered by Nile): the window icon of a program that names none, and
/// the icon `--pack` gives an executable or bundle when `--icon` isn't used.
pub const DEFAULT_ICON: &[u8] = include_bytes!("../../../assets/icon-512.png");

/// What the operating system shows about the program (Explorer's Details tab, Finder's Get Info,
/// the application menu of a Linux desktop).
#[derive(Debug, Clone, Default)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub company: String,
    pub copyright: String,
    /// Desktop-entry categories, `Utility;` style (Linux).
    pub category: String,
    /// URL schemes the app opens (`myapp` for `myapp://...`): declared in the macOS bundle and the Linux entry.
    pub protocols: Vec<String>,
    /// File extensions the app opens (`md`, `txt`), without the dot.
    pub file_types: Vec<String>,
}

pub use nile_script::mime_of;

impl AppInfo {
    /// `1.2.3.4` as four numbers (missing parts are 0).
    #[allow(dead_code)]
    pub fn numbers(&self) -> [u16; 4] {
        let mut n = [0u16; 4];
        for (slot, part) in n.iter_mut().zip(self.version.split('.')) {
            *slot = part.trim().chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0);
        }
        n
    }
}

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect()
}

fn align4(v: &mut Vec<u8>) {
    while !v.len().is_multiple_of(4) {
        v.push(0);
    }
}

/// One block of a `VS_VERSIONINFO`: length, value length, type, key, padding, value.
fn version_block(value_len: u16, text: bool, key: &str, value: &[u8]) -> Vec<u8> {
    let mut b = vec![0, 0];
    b.extend(value_len.to_le_bytes());
    b.extend(u16::from(text).to_le_bytes());
    b.extend(utf16z(key));
    align4(&mut b);
    b.extend_from_slice(value);
    let len = (b.len() as u16).to_le_bytes();
    b[..2].copy_from_slice(&len);
    b
}

/// The `RT_VERSION` resource for `info`: what Explorer shows under Properties > Details.
#[allow(dead_code)]
pub fn version_resource(info: &AppInfo) -> Vec<u8> {
    let [a, b, c, d] = info.numbers();
    let (ms, ls) = ((u32::from(a) << 16) | u32::from(b), (u32::from(c) << 16) | u32::from(d));
    let mut fixed = Vec::new();
    for v in [0xFEEF04BDu32, 0x0001_0000, ms, ls, ms, ls, 0x3F, 0, 0x0004_0004, 1, 0, 0, 0] {
        fixed.extend(v.to_le_bytes());
    }
    let version = if info.version.is_empty() { "1.0.0".to_string() } else { info.version.clone() };
    let exe = format!("{}.exe", info.name);
    let strings = [
        ("CompanyName", info.company.as_str()),
        ("FileDescription", if info.description.is_empty() { info.name.as_str() } else { info.description.as_str() }),
        ("FileVersion", version.as_str()),
        ("InternalName", info.name.as_str()),
        ("LegalCopyright", info.copyright.as_str()),
        ("OriginalFilename", exe.as_str()),
        ("ProductName", info.name.as_str()),
        ("ProductVersion", version.as_str()),
    ];
    let mut table = Vec::new();
    for (key, value) in strings {
        align4(&mut table);
        let text = utf16z(value);
        table.extend(version_block((text.len() / 2) as u16, true, key, &text));
    }
    // Language 0409 (US English), code page 04B0 (Unicode).
    let string_info = version_block(0, true, "StringFileInfo", &version_block(0, true, "040904B0", &table));
    let var_info = version_block(0, true, "VarFileInfo", &version_block(4, false, "Translation", &[0x09, 0x04, 0xB0, 0x04]));
    let mut body = fixed;
    align4(&mut body);
    body.extend(string_info);
    align4(&mut body);
    body.extend(var_info);
    version_block(52, false, "VS_VERSION_INFO", &body)
}

/// A freedesktop `.desktop` entry that starts `exe` (Linux application menus and Wayland icons).
#[allow(dead_code)]
pub fn desktop_entry(info: &AppInfo, exe: &Path, icon: Option<&Path>) -> String {
    let quote = |p: &Path| format!("\"{}\"", p.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\""));
    let mut out = String::from("[Desktop Entry]\nType=Application\n");
    out += &format!("Name={}\n", info.name);
    if !info.description.is_empty() {
        out += &format!("Comment={}\n", info.description);
    }
    let opens = !info.protocols.is_empty() || !info.file_types.is_empty();
    // `%U` hands the URLs and files the system opens with the app to it as arguments.
    out += &format!("Exec={}{}\n", quote(exe), if opens { " %U" } else { "" });
    if let Some(icon) = icon {
        out += &format!("Icon={}\n", icon.to_string_lossy());
    }
    out += "Terminal=false\n";
    if opens {
        let mut mimes: Vec<String> = info.protocols.iter().map(|p| format!("x-scheme-handler/{}", p.to_ascii_lowercase())).collect();
        mimes.extend(info.file_types.iter().filter_map(|e| mime_of(e)).map(str::to_string));
        out += &format!("MimeType={};\n", mimes.join(";"));
    }
    out += &format!("Categories={}\n", if info.category.is_empty() { "Utility;" } else { info.category.as_str() });
    if let Some(stem) = exe.file_name() {
        out += &format!("StartupWMClass={}\n", stem.to_string_lossy());
    }
    out
}

/// Windows: `runtime` (a `nile.exe` without an archive) with `icon` (`.ico`, or a `.png`) as its
/// application icon. Returns the new bytes; the archive is appended after this.
#[cfg(target_os = "windows")]
pub fn with_resources(runtime: Vec<u8>, icon: Option<&Path>, info: &AppInfo) -> std::io::Result<Vec<u8>> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::{BeginUpdateResourceW, EndUpdateResourceW, UpdateResourceW};

    // The icon's images: `(width, height, bytes)`, each a bitmap or a PNG.
    let data = match icon {
        Some(icon) => std::fs::read(icon)?,
        None => Vec::new(),
    };
    let images: Vec<(u8, u8, Vec<u8>)> = if data.is_empty() {
        Vec::new()
    } else if data.starts_with(b"\x89PNG") && data.len() > 24 {
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
    if icon.is_some() && images.is_empty() {
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
            if !images.is_empty() {
                good &= UpdateResourceW(h, 14 as _, 1 as _, 0, group.as_ptr().cast(), group.len() as u32) != 0;
            }
            // RT_VERSION = 16, resource 1, language 0x0409.
            let version = version_resource(info);
            good &= UpdateResourceW(h, 16 as _, 1 as _, 0x0409, version.as_ptr().cast(), version.len() as u32) != 0;
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

/// Encode modern ICNS PNG representations directly, without sips/iconutil.
#[cfg(any(target_os = "macos", test))]
fn png_to_icns(bytes: &[u8]) -> std::io::Result<Vec<u8>> {
    use skia_safe::{Data, EncodedImageFormat, Image, Paint, Rect};
    let invalid = || std::io::Error::new(std::io::ErrorKind::InvalidData, "could not decode application icon");
    let image = Image::from_encoded(Data::new_copy(bytes)).ok_or_else(invalid)?;
    let mut result = b"icns\0\0\0\0".to_vec();
    // Standard sizes and their Retina representations. Each element includes
    // its four-byte type and big-endian length before the PNG payload.
    for (kind, size) in [
        (b"icp4", 16), (b"icp5", 32), (b"ic07", 128), (b"ic08", 256),
        (b"ic09", 512), (b"ic10", 1024), (b"ic11", 32), (b"ic12", 64),
        (b"ic13", 256), (b"ic14", 512),
    ] {
        let mut surface = skia_safe::surfaces::raster_n32_premul((size, size))
            .ok_or_else(|| std::io::Error::other("could not allocate application icon"))?;
        surface.canvas().clear(skia_safe::Color::TRANSPARENT);
        surface.canvas().draw_image_rect(&image, None, Rect::from_wh(size as f32, size as f32), &Paint::default());
        let png = surface.image_snapshot().encode(None, EncodedImageFormat::PNG, None)
            .ok_or_else(|| std::io::Error::other("could not encode application icon"))?;
        result.extend_from_slice(kind);
        result.extend_from_slice(&((png.size() + 8) as u32).to_be_bytes());
        result.extend_from_slice(png.as_bytes());
    }
    let length = result.len() as u32;
    result[4..8].copy_from_slice(&length.to_be_bytes());
    Ok(result)
}

/// macOS: turn the plain program at `exe` into `<name>.app` (`Contents/MacOS/<name>`, an `Info.plist`,
/// the icon as `.icns` and the archive at `Contents/Resources/app.nilepack`: a signed executable can't
/// have an archive appended) and return the bundle's path. `icon` is a `.png` (turned into an
/// `.icns` with the built-in image renderer) or an `.icns`.
#[cfg(target_os = "macos")]
pub fn app_bundle(exe: &Path, archive: &Path, name: &str, icon: Option<&Path>, info: &AppInfo) -> std::io::Result<PathBuf> {
    use std::process::Command;
    let app = exe.with_file_name(format!("{name}.app"));
    // `-o Name.app`: the program was written where the bundle goes. Move it aside first.
    let aside = exe.with_file_name(format!(".{name}.nile-program"));
    let exe = if exe == app {
        std::fs::rename(exe, &aside)?;
        aside.as_path()
    } else {
        exe
    };
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
            std::fs::write(&target, png_to_icns(&std::fs::read(icon)?)?)?;
        }
        icon_key = "  <key>CFBundleIconFile</key><string>AppIcon</string>\n".to_string();
    }
    let version = if info.version.is_empty() { "1.0".to_string() } else { info.version.clone() };
    let copyright = if info.copyright.is_empty() { String::new() } else { format!("  <key>NSHumanReadableCopyright</key><string>{}</string>\n", info.copyright) };
    let id: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let esc = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut opens = String::new();
    if !info.protocols.is_empty() {
        opens += &format!(
            "  <key>CFBundleURLTypes</key><array><dict><key>CFBundleURLName</key><string>app.nile.{id}</string><key>CFBundleURLSchemes</key><array>{}</array></dict></array>\n",
            info.protocols.iter().map(|p| format!("<string>{}</string>", esc(p))).collect::<String>()
        );
    }
    if !info.file_types.is_empty() {
        opens += &format!(
            "  <key>CFBundleDocumentTypes</key><array><dict><key>CFBundleTypeName</key><string>{} document</string><key>CFBundleTypeRole</key><string>Editor</string><key>CFBundleTypeExtensions</key><array>{}</array></dict></array>\n",
            esc(name),
            info.file_types.iter().map(|e| format!("<string>{}</string>", esc(e.trim_start_matches('.')))).collect::<String>()
        );
    }
    std::fs::write(
        app.join("Contents/Info.plist"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n  <key>CFBundleName</key><string>{name}</string>\n  <key>CFBundleDisplayName</key><string>{name}</string>\n  <key>CFBundleExecutable</key><string>{name}</string>\n  <key>CFBundleIdentifier</key><string>app.nile.{id}</string>\n  <key>CFBundlePackageType</key><string>APPL</string>\n  <key>CFBundleVersion</key><string>{version}</string>\n  <key>CFBundleShortVersionString</key><string>{version}</string>\n{copyright}{opens}  <key>NSHighResolutionCapable</key><true/>\n{icon_key}</dict></plist>\n"
        ),
    )?;
    // The executable was changed after it was built: sign the whole bundle again (ad hoc).
    let signed = Command::new("codesign").args(["--force", "--deep", "--sign", "-"]).arg(&app).output()?;
    if !signed.status.success() {
        return Err(std::io::Error::other(format!("could not sign {}: {}", app.display(), String::from_utf8_lossy(&signed.stderr))));
    }
    Ok(app)
}

/// Not on this platform: the program stays a plain executable.
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
pub fn app_bundle(exe: &Path, _archive: &Path, _name: &str, _icon: Option<&Path>, _info: &AppInfo) -> std::io::Result<PathBuf> {
    Ok(exe.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> AppInfo {
        AppInfo { name: "Demo".into(), version: "1.2.3".into(), description: "A demo".into(), company: "Acme".into(), copyright: "(c) Acme".into(), category: String::new(), protocols: Vec::new(), file_types: Vec::new() }
    }

    #[test]
    fn the_linux_entry_lists_the_links_and_files_the_app_opens() {
        let with = AppInfo { protocols: vec!["Notes".into()], file_types: vec!["md".into(), "weird".into()], ..info() };
        let entry = desktop_entry(&with, Path::new("/opt/demo/demo"), None);
        assert!(entry.contains("Exec=\"/opt/demo/demo\" %U"), "{entry}");
        assert!(entry.contains("MimeType=x-scheme-handler/notes;text/markdown;"), "{entry}");
        let plain = desktop_entry(&info(), Path::new("/opt/demo/demo"), None);
        assert!(plain.contains("Exec=\"/opt/demo/demo\"\n") && !plain.contains("MimeType"), "{plain}");
    }

    #[test]
    fn version_numbers_fill_four_parts() {
        assert_eq!(info().numbers(), [1, 2, 3, 0]);
        assert_eq!(AppInfo { version: "2.0.1-beta".into(), ..info() }.numbers(), [2, 0, 1, 0]);
    }

    #[test]
    fn generated_icns_contains_decodable_standard_and_retina_images() {
        let bytes = png_to_icns(DEFAULT_ICON).unwrap();
        assert_eq!(&bytes[..4], b"icns");
        assert_eq!(u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize, bytes.len());
        let mut offset = 8;
        for (kind, size) in [(b"icp4", 16), (b"icp5", 32), (b"ic07", 128), (b"ic08", 256), (b"ic09", 512), (b"ic10", 1024), (b"ic11", 32), (b"ic12", 64), (b"ic13", 256), (b"ic14", 512)] {
            assert_eq!(&bytes[offset..offset + 4], kind);
            let length = u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
            let image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&bytes[offset + 8..offset + length])).unwrap();
            assert_eq!((image.width(), image.height()), (size, size));
            offset += length;
        }
        assert_eq!(offset, bytes.len());
        assert!(png_to_icns(b"invalid image").is_err());
    }

    #[test]
    fn the_version_resource_is_well_formed() {
        let v = version_resource(&info());
        let word = |at: usize| u16::from_le_bytes([v[at], v[at + 1]]) as usize;
        let dword = |at: usize| u32::from_le_bytes(v[at..at + 4].try_into().unwrap());
        // The root block covers everything and holds a 52-byte fixed part at the next 4-byte boundary after its key.
        assert_eq!(word(0), v.len());
        assert_eq!(word(2), 52);
        let key = utf16z("VS_VERSION_INFO");
        let fixed = (6 + key.len()).div_ceil(4) * 4;
        assert_eq!(dword(fixed), 0xFEEF04BD);
        assert_eq!(dword(fixed + 8), (1 << 16) | 2, "file version, most significant half");
        assert_eq!(dword(fixed + 12), 3 << 16, "file version, least significant half");
        // Every string is in there, as UTF-16.
        for needle in ["CompanyName", "Acme", "FileDescription", "A demo", "ProductVersion", "1.2.3", "Demo.exe", "040904B0", "Translation"] {
            let n = utf16z(needle);
            assert!(v.windows(n.len() - 2).any(|w| w == &n[..n.len() - 2]), "missing {needle}");
        }
        // Blocks start on 4-byte boundaries: the StringFileInfo block follows the fixed part.
        let string_info = fixed + 52;
        assert_eq!(string_info % 4, 0);
        assert_eq!(&v[string_info + 6..string_info + 6 + 28], &utf16z("StringFileInfo")[..28]);
    }

    #[test]
    fn desktop_entries_quote_the_program_path() {
        let text = desktop_entry(&info(), Path::new("/opt/my app/demo"), Some(Path::new("/opt/my app/demo.png")));
        assert!(text.starts_with("[Desktop Entry]\nType=Application\nName=Demo\n"));
        assert!(text.contains("Exec=\"/opt/my app/demo\"\n") && text.contains("Icon=/opt/my app/demo.png\n"));
        assert!(text.contains("Categories=Utility;\n") && text.contains("StartupWMClass=demo\n") && text.contains("Comment=A demo\n"));
    }
}
