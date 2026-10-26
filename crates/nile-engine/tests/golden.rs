//! Golden-image tests: text-free pages are painted and compared with the PNGs in `tests/golden/`.
//!
//! Text is left out on purpose (every system has its own fonts), so what is checked is layout, borders, gradients,
//! shadows, transforms, stacking, clipping and the like, which must look the same everywhere. A small tolerance absorbs
//! the last-bit differences between CPUs' raster paths. To accept a deliberate change, run
//! `NILE_UPDATE_GOLDEN=1 cargo test -p nile-engine --test golden` and look at the PNGs before committing them; a failing
//! comparison writes `<name>.actual.png` and `<name>.diff.png` next to the build output (see the message).
use nile_engine::Page;
use std::path::PathBuf;

const W: u32 = 360;
const H: u32 = 240;
/// Largest per-channel difference that still counts as the same pixel.
const CHANNEL_TOLERANCE: i16 = 6;
/// Pixels allowed to differ by more than that, as a fraction of the image.
const BAD_FRACTION: f64 = 0.002;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn render(html: &str, name: &str) -> Vec<u8> {
    // One neutral appearance whatever the machine's settings are.
    std::env::set_var("NILE_COLOR_SCHEME", "light");
    std::env::set_var("NILE_THEME", "macos");
    std::env::set_var("NILE_STORAGE_DIR", "off");
    let path = std::env::temp_dir().join(format!("nile-golden-{}-{name}.html", std::process::id()));
    std::fs::write(&path, html).unwrap();
    let mut page = Page::load(path.clone(), (W as f32, H as f32), 0.0);
    page.layout(W as f32, H as f32);
    let mut bgra = vec![0u8; (W * H * 4) as usize];
    page.paint(&mut bgra, W, H, 1.0);
    let _ = std::fs::remove_file(path);
    // PNG wants RGBA.
    let mut rgba = bgra;
    for i in 0..(W * H) as usize {
        rgba.swap(i * 4, i * 4 + 2);
    }
    rgba
}

fn write_png(path: &std::path::Path, rgba: &[u8]) {
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(rgba).unwrap();
}

fn read_png(path: &std::path::Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file)).read_info().ok()?;
    let mut out = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut out).ok()?;
    (info.width == W && info.height == H && info.color_type == png::ColorType::Rgba).then(|| out[..info.buffer_size()].to_vec())
}

fn check(name: &str, html: &str) {
    let actual = render(html, name);
    let golden = golden_dir().join(format!("{name}.png"));
    if std::env::var("NILE_UPDATE_GOLDEN").is_ok_and(|v| v == "1") || !golden.exists() {
        write_png(&golden, &actual);
        return;
    }
    let expected = read_png(&golden).unwrap_or_else(|| panic!("{} is not a {W}x{H} RGBA PNG", golden.display()));
    let mut bad = 0usize;
    let mut diff = vec![0u8; actual.len()];
    for i in 0..(W * H) as usize {
        let (a, e) = (&actual[i * 4..i * 4 + 4], &expected[i * 4..i * 4 + 4]);
        let off = a.iter().zip(e).map(|(x, y)| (*x as i16 - *y as i16).abs()).max().unwrap_or(0);
        if off > CHANNEL_TOLERANCE {
            bad += 1;
            diff[i * 4..i * 4 + 4].copy_from_slice(&[255, 0, 0, 255]);
        } else {
            diff[i * 4..i * 4 + 4].copy_from_slice(&[e[0] / 2 + 127, e[1] / 2 + 127, e[2] / 2 + 127, 255]);
        }
    }
    if bad as f64 > BAD_FRACTION * (W * H) as f64 {
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/golden-failures");
        std::fs::create_dir_all(&out).unwrap();
        write_png(&out.join(format!("{name}.actual.png")), &actual);
        write_png(&out.join(format!("{name}.diff.png")), &diff);
        panic!("{name}: {bad} pixels differ (more than {:.2}% allowed); see {}", BAD_FRACTION * 100.0, out.display());
    }
}

macro_rules! golden {
    ($name:ident, $html:expr) => {
        #[test]
        fn $name() {
            check(stringify!($name), $html);
        }
    };
}

golden!(boxes_borders_radius_and_shadows, r##"<body style="margin:0;background:#f4f1ea">
  <div style="position:absolute;left:20px;top:20px;width:100px;height:70px;background:#3b82f6;border:6px solid #1e3a8a;border-radius:14px"></div>
  <div style="position:absolute;left:140px;top:20px;width:100px;height:70px;background:#fff;border:3px dashed #ef4444;border-radius:35px 8px 35px 8px;box-shadow:6px 8px 12px rgba(0,0,0,.35)"></div>
  <div style="position:absolute;left:260px;top:20px;width:80px;height:80px;background:#22c55e;border-radius:50%;box-shadow:inset 0 0 14px #064e3b"></div>
  <div style="position:absolute;left:20px;top:120px;width:100px;height:70px;border-style:solid;border-width:8px;border-color:#f97316 #84cc16 #06b6d4 #a855f7"></div>
  <div style="position:absolute;left:140px;top:120px;width:200px;height:70px;background:#fde68a;outline:4px dotted #92400e;outline-offset:6px;opacity:.7"></div>
</body>"##);

golden!(gradients, r##"<body style="margin:0;background:#fff">
  <div style="position:absolute;left:10px;top:10px;width:160px;height:100px;background:linear-gradient(135deg,#ef4444,#f59e0b 40%,#10b981 70%,#3b82f6)"></div>
  <div style="position:absolute;left:190px;top:10px;width:160px;height:100px;background:radial-gradient(circle at 30% 30%,#fff,#6366f1 60%,#1e1b4b)"></div>
  <div style="position:absolute;left:10px;top:130px;width:160px;height:100px;background:repeating-linear-gradient(45deg,#111 0 10px,#eee 10px 20px)"></div>
  <div style="position:absolute;left:190px;top:130px;width:160px;height:100px;background:linear-gradient(0deg,#f43f5e 25%,#eab308 25% 50%,#22c55e 50% 75%,#06b6d4 75%);border-radius:12px"></div>
</body>"##);

golden!(flex_layout, r##"<body style="margin:0;background:#e5e7eb;height:240px;overflow:hidden">
  <div style="display:flex;gap:10px;padding:10px;height:80px">
    <div style="flex:1;background:#2563eb"></div>
    <div style="flex:2;background:#16a34a"></div>
    <div style="width:60px;background:#dc2626"></div>
  </div>
  <div style="display:flex;flex-wrap:wrap;gap:8px;padding:10px;justify-content:space-between;align-items:flex-start">
    <div style="width:70px;height:30px;background:#7c3aed"></div><div style="width:90px;height:40px;background:#db2777"></div>
    <div style="width:50px;height:30px;background:#0891b2"></div><div style="width:110px;height:36px;background:#ca8a04"></div>
    <div style="width:80px;height:26px;background:#475569"></div>
  </div>
  <div style="display:flex;flex-direction:column;align-items:center;justify-content:center;height:50px;background:#fff;margin:10px">
    <div style="width:80px;height:10px;background:#0f766e;margin-bottom:6px"></div><div style="width:140px;height:10px;background:#b45309"></div>
  </div>
</body>"##);

golden!(grid_layout, r##"<body style="margin:0;background:#fafaf9">
  <div style="display:grid;grid-template-columns:80px 1fr 2fr;grid-template-rows:60px 60px 1fr;gap:8px;padding:10px;height:200px;box-sizing:border-box;width:340px;
              grid-template-areas:'a a b' 'c d b' 'c e e'">
    <div style="grid-area:a;background:#0ea5e9"></div><div style="grid-area:b;background:#f43f5e"></div>
    <div style="grid-area:c;background:#84cc16"></div><div style="grid-area:d;background:#f59e0b"></div>
    <div style="grid-area:e;background:#8b5cf6"></div>
  </div>
</body>"##);

golden!(positioning_and_stacking, r##"<body style="margin:0;background:#fff">
  <div style="position:relative;width:200px;height:140px;margin:20px;background:#e2e8f0">
    <div style="position:absolute;left:10px;top:10px;width:100px;height:100px;background:#ef4444;z-index:1"></div>
    <div style="position:absolute;left:50px;top:40px;width:100px;height:90px;background:#3b82f6;z-index:2;opacity:.8"></div>
    <div style="position:absolute;right:0;bottom:0;width:60px;height:60px;background:#22c55e;z-index:0"></div>
  </div>
  <div style="position:fixed;right:12px;top:12px;width:70px;height:40px;background:#f59e0b"></div>
  <div style="position:absolute;left:250px;top:90px;width:90px;height:120px;background:#1e293b;clip-path:polygon(50% 0,100% 100%,0 100%)"></div>
</body>"##);

golden!(transforms_2d_and_3d, r##"<body style="margin:0;background:#fff">
  <div style="position:absolute;left:30px;top:30px;width:80px;height:60px;background:#6366f1;transform:rotate(20deg) scale(1.2)"></div>
  <div style="position:absolute;left:150px;top:30px;width:80px;height:60px;background:#ec4899;transform:skew(20deg,5deg) translate(10px,6px)"></div>
  <div style="position:absolute;left:250px;top:20px;width:90px;height:90px;perspective:200px"><div style="width:90px;height:90px;background:#14b8a6;transform:rotateY(45deg)"></div></div>
  <div style="position:absolute;left:30px;top:140px;width:120px;height:70px;background:#f97316;transform-origin:0 0;transform:rotate(-12deg)"></div>
  <div style="position:absolute;left:190px;top:140px;width:140px;height:70px;perspective:300px"><div style="width:140px;height:70px;background:#a3e635;transform:rotateX(50deg)"></div></div>
</body>"##);

golden!(filters_blend_and_masks, r##"<body style="margin:0;background:#fef3c7">
  <div style="position:absolute;left:20px;top:20px;width:90px;height:70px;background:#2563eb;filter:blur(3px)"></div>
  <div style="position:absolute;left:130px;top:20px;width:90px;height:70px;background:#16a34a;filter:drop-shadow(6px 6px 4px rgba(0,0,0,.5)) saturate(1.5)"></div>
  <div style="position:absolute;left:240px;top:20px;width:90px;height:70px;background:#dc2626;mix-blend-mode:multiply"><div style="position:absolute;left:-20px;top:20px;width:60px;height:60px;background:#facc15"></div></div>
  <div style="position:absolute;left:20px;top:130px;width:120px;height:80px;background:#7c3aed;-webkit-mask-image:linear-gradient(90deg,#000,transparent);mask-image:linear-gradient(90deg,#000,transparent)"></div>
  <div style="position:absolute;left:170px;top:130px;width:100px;height:80px;background:#0f172a;clip-path:circle(40px at 50% 50%)"></div>
  <div style="position:absolute;left:290px;top:130px;width:50px;height:80px;background:#ef4444;filter:grayscale(1) contrast(1.4) brightness(1.1)"></div>
</body>"##);

golden!(overflow_clipping_and_floats, r##"<body style="margin:0;background:#fff">
  <div style="width:140px;height:60px;margin:10px;overflow:hidden;border:2px solid #334155;border-radius:16px;background:#e2e8f0">
    <div style="width:220px;height:120px;margin:-20px 0 0 -20px;background:linear-gradient(90deg,#fb923c,#e11d48)"></div>
  </div>
  <div style="margin:10px;width:300px;overflow:hidden">
    <div style="float:left;width:60px;height:50px;background:#0ea5e9;margin-right:10px"></div>
    <div style="float:right;width:60px;height:60px;background:#f59e0b;margin-left:10px"></div>
    <div style="clear:both;height:14px;background:#a3a3a3;margin-top:6px"></div>
  </div>
  <div style="margin:10px;width:300px;line-height:0"><span style="display:inline-block;width:40px;height:30px;background:#16a34a;vertical-align:bottom"></span><span style="display:inline-block;width:60px;height:50px;background:#9333ea"></span><span style="display:inline-block;width:30px;height:20px;background:#db2777;vertical-align:top"></span></div>
</body>"##);

golden!(table_without_text, r##"<body style="margin:0;background:#fff">
  <table style="border-collapse:collapse;margin:14px"><tr>
    <td style="width:60px;height:40px;background:#fecaca;border:2px solid #7f1d1d"></td><td style="width:80px;background:#bbf7d0;border:2px solid #14532d"></td><td rowspan="2" style="width:50px;background:#bfdbfe;border:2px solid #1e3a8a"></td></tr>
    <tr><td colspan="2" style="height:50px;background:#fde68a;border:2px solid #78350f"></td></tr></table>
  <table style="border-spacing:6px;margin:14px;background:#475569"><tr><td style="width:50px;height:30px;background:#e2e8f0"></td><td style="width:50px;background:#f1f5f9"></td></tr><tr><td style="height:30px;background:#cbd5e1"></td><td style="background:#94a3b8"></td></tr></table>
</body>"##);

golden!(canvas_and_inline_svg, r##"<body style="margin:0;background:#fff">
  <canvas id="c" width="160" height="100" style="position:absolute;left:10px;top:10px"></canvas>
  <svg width="160" height="100" viewBox="0 0 160 100" style="position:absolute;left:190px;top:10px">
    <rect x="5" y="5" width="150" height="90" rx="12" fill="#e0f2fe" stroke="#0369a1" stroke-width="3"/>
    <circle cx="50" cy="50" r="28" fill="#f43f5e"/><path d="M90 80 L120 20 L150 80 Z" fill="#22c55e" stroke="#14532d" stroke-width="2"/>
  </svg>
  <div style="position:absolute;left:10px;top:130px;width:340px;height:90px;background:#f8fafc;border:1px solid #cbd5e1"></div>
  <script>
    const g = document.getElementById("c").getContext("2d");
    g.fillStyle = "#fde68a"; g.fillRect(0, 0, 160, 100);
    g.fillStyle = "#b91c1c"; g.beginPath(); g.arc(50, 50, 30, 0, Math.PI * 2); g.fill();
    g.strokeStyle = "#1d4ed8"; g.lineWidth = 6; g.beginPath(); g.moveTo(80, 90); g.quadraticCurveTo(110, 0, 150, 70); g.stroke();
    const grad = g.createLinearGradient(0, 80, 160, 100); grad.addColorStop(0, "#16a34a"); grad.addColorStop(1, "#0f172a"); g.fillStyle = grad; g.fillRect(0, 84, 160, 16);
  </script>
</body>"##);
