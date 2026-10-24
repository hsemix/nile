//! Paged output: cut the document into pages (`@page` size and margins, `break-*`), paint a page,
//! or write every page to a PDF.

use std::io::Write;

use nile_render::Overlay;
use nile_style::{MarginPlace, PageStyle};
use skia_safe::{surfaces, AlphaType, Canvas, Color, ColorType, Font, FontMgr, FontStyle, ImageInfo, Paint, Rect};

use crate::Page;

/// CSS px are 96 per inch, PDF user units 72.
const PDF_SCALE: f32 = 0.75;

impl Page {
    /// Lay the document out on paper (the first page's `@page` size and margins) and find where
    /// the pages break. Returns the page count. The window's own layout is redone on the next paint.
    // TODO: pages with their own `size`/margins (`@page :first`) are painted with them but flow at the first page's content size.
    pub fn paginate(&mut self) -> usize {
        let style = self.view.borrow().styles.page_style(0);
        let (w, h) = style.content_size();
        self.layout(w, h);
        let frags = {
            let mut view = self.view.borrow_mut();
            view.scale = 1.0;
            view.layout();
            let tree = &view.layout.as_ref().expect("laid out above").2;
            nile_layout::paginate_tree(tree, &self.doc.borrow(), &view.styles, h)
        };
        self.page_starts = frags.starts;
        self.page_repeats = frags.repeats;
        self.page_feet = frags.feet;
        self.page_starts.len()
    }

    /// The `@page` style of page `index` (0-based).
    pub fn page_style(&self, index: usize) -> PageStyle {
        self.view.borrow().styles.page_style(index)
    }

    /// Draw page `index` onto `canvas`, whose unit is one CSS px: paper, content inside the margins,
    /// then the margin boxes. Call [`paginate`](Self::paginate) first.
    pub fn draw_page(&mut self, canvas: &Canvas, index: usize) {
        let Some(&start) = self.page_starts.get(index) else { return };
        let count = self.page_starts.len();
        let style = self.page_style(index);
        let (cw, ch) = style.content_size();
        let end = self.page_starts.get(index + 1).copied().unwrap_or(f32::INFINITY);
        canvas.draw_rect(Rect::from_wh(style.size.0, style.size.1), &Paint::default().set_color(Color::WHITE).clone());
        let bg = self.canvas_background();
        {
            let mut view = self.view.borrow_mut();
            view.layout();
            let page_selection = Vec::new();
            let view = &*view;
            let tree = &view.layout.as_ref().expect("laid out above").2;
            canvas.save();
            canvas.translate((style.margin[3], style.margin[0]));
            // A table continuing from the page before repeats its header; the content ends at the
            // next break, or at the content box.
            let head = self.page_repeats.get(index).cloned().flatten();
            let head_h = head.as_ref().map_or(0.0, |r| r.height);
            let foot = self.page_feet.get(index).cloned().flatten();
            let foot_h = foot.as_ref().map_or(0.0, |r| r.height);
            let height = (end - start).min(ch - head_h - foot_h);
            if bg != Color::WHITE {
                canvas.draw_rect(Rect::from_xywh(0.0, 0.0, cw, head_h + height), &Paint::default().set_color(bg).clone());
            }
            let overlay = |scroll_y: f32, only: (usize, usize)| Overlay {
                scroll_y,
                offsets: &view.scroll_offsets,
                caret: None,
                selection: None,
                viewport: (cw, ch),
                page_selection: &page_selection,
                dom_caret: None,
                preedit: None,
                only: Some(only),
                skip: &[],
                selected_image: None,
                bar: None,
            };
            if let Some(repeat) = &head {
                canvas.save();
                canvas.clip_rect(Rect::from_xywh(0.0, 0.0, cw, head_h), None, true);
                for &range in &repeat.ranges {
                    self.renderer.paint(canvas, tree, &view.styles, &overlay(repeat.y, range));
                }
                canvas.restore();
            }
            canvas.save();
            canvas.translate((0.0, head_h));
            canvas.clip_rect(Rect::from_xywh(0.0, 0.0, cw, height), None, true);
            self.renderer.paint(canvas, tree, &view.styles, &overlay(start, (0, tree.boxes.len())));
            canvas.restore();
            if let Some(foot) = &foot {
                canvas.save();
                canvas.translate((0.0, ch - foot_h));
                canvas.clip_rect(Rect::from_xywh(0.0, 0.0, cw, foot_h), None, true);
                for &range in &foot.ranges {
                    self.renderer.paint(canvas, tree, &view.styles, &overlay(foot.y, range));
                }
                canvas.restore();
            }
            canvas.restore();
        }
        let fonts = FontMgr::new();
        let typeface = ["Helvetica", "Arial", "DejaVu Sans", "Liberation Sans", "sans-serif"]
            .iter()
            .find_map(|f| fonts.match_family_style(f, FontStyle::normal()))
            .or_else(|| fonts.legacy_make_typeface(None, FontStyle::normal()));
        for mb in &style.boxes {
            let text = mb.text(index + 1, count);
            let mut font = Font::default();
            font.set_size(mb.font_size);
            if let Some(tf) = &typeface {
                font.set_typeface(tf.clone());
            }
            let (width, _) = font.measure_str(&text, None);
            let (left, right) = (style.margin[3], style.size.0 - style.margin[1]);
            let x = match mb.place {
                MarginPlace::TopLeft | MarginPlace::BottomLeft => left,
                MarginPlace::TopCenter | MarginPlace::BottomCenter => (left + right - width) / 2.0,
                MarginPlace::TopRight | MarginPlace::BottomRight => right - width,
            };
            let y = match mb.place {
                MarginPlace::TopLeft | MarginPlace::TopCenter | MarginPlace::TopRight => (style.margin[0] + mb.font_size * 0.7) / 2.0,
                _ => style.size.1 - (style.margin[2] - mb.font_size * 0.7) / 2.0,
            };
            let c = mb.color;
            canvas.draw_str(&text, (x, y), &font, &Paint::default().set_color(Color::from_argb(c.a, c.r, c.g, c.b)).clone());
        }
    }

    /// Page `index` as BGRA pixels (`scale` physical px per CSS px), with its size. Call
    /// [`paginate`](Self::paginate) first.
    pub fn render_page(&mut self, index: usize, scale: f32) -> Option<(Vec<u8>, u32, u32)> {
        let style = self.page_style(index);
        let (w, h) = ((style.size.0 * scale).ceil() as u32, (style.size.1 * scale).ceil() as u32);
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        let info = ImageInfo::new((w as i32, h as i32), ColorType::BGRA8888, AlphaType::Premul, None);
        let mut surface = surfaces::wrap_pixels(&info, &mut pixels, w as usize * 4, None)?;
        surface.canvas().scale((scale, scale));
        self.draw_page(surface.canvas(), index);
        drop(surface);
        Some((pixels, w, h))
    }

    /// Write the page as a PDF for `Window.this.print()` / `exportPdf()` (an empty `path` means a temp file) and tell the script the file name, or the error, through `__printReady`. The window's own viewport and
    /// scale are put back afterwards, so showing paper doesn't change what is on screen.
    pub(crate) fn run_print_job(&mut self, id: u32, wanted: &str, title: &str) {
        let path = if wanted.is_empty() { std::env::temp_dir().join(format!("nile-print-{}-{id}.pdf", std::process::id())) } else { std::path::PathBuf::from(wanted) };
        let (viewport, scale) = {
            let v = self.view.borrow();
            (v.viewport, v.scale)
        };
        let outcome = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display())).and_then(|file| {
            let mut out = std::io::BufWriter::new(file);
            self.write_pdf(&mut out, title);
            out.flush().map_err(|e| e.to_string())
        });
        {
            let mut v = self.view.borrow_mut();
            v.set_viewport(viewport.0, viewport.1);
            v.scale = scale;
        }
        let (file, error) = match outcome {
            Ok(()) => (path.to_string_lossy().into_owned(), String::new()),
            Err(e) => (String::new(), e),
        };
        let args = format!("[{id},{},{}]", crate::editing::json_string(&file), crate::editing::json_string(&error));
        let _ = self.call("__printReady", &args);
    }

    /// Write every page to `out` as a PDF (vector text and shapes). Returns the page count.
    pub fn write_pdf(&mut self, out: &mut impl Write, title: &str) -> usize {
        let count = self.paginate();
        let meta = skia_safe::pdf::Metadata { title: title.to_string(), creator: "Nile".to_string(), ..Default::default() };
        let mut doc = skia_safe::pdf::new_document(out, Some(&meta));
        for i in 0..count {
            let style = self.page_style(i);
            let mut on_page = doc.begin_page((style.size.0 * PDF_SCALE, style.size.1 * PDF_SCALE), None);
            {
                let canvas = on_page.canvas();
                canvas.scale((PDF_SCALE, PDF_SCALE));
                self.draw_page(canvas, i);
            }
            doc = on_page.end_page();
        }
        doc.close();
        count
    }
}
